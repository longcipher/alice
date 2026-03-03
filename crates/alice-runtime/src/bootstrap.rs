//! Runtime/bootstrap wiring for Alice.

use std::sync::Arc;

use alice_adapters::memory::sqlite_store::SqliteMemoryStore;
use alice_core::memory::{domain::HybridWeights, service::MemoryService};
use bob_adapters::{
    llm_genai::GenAiLlmAdapter, mcp_rmcp::McpToolAdapter, observe::TracingEventSink,
    store_memory::InMemorySessionStore, tape_memory::InMemoryTapeStore,
};
use bob_core::{
    ports::{EventSink, LlmPort, SessionStore, TapeStorePort, ToolPort},
    types::TurnPolicy,
};
use bob_runtime::{
    AgentBootstrap, DispatchMode, NoOpToolPort, RuntimeBuilder, TimeoutToolLayer, ToolLayer,
    agent_loop::AgentLoop, composite::CompositeToolPort,
};

use crate::{
    config::{AliceConfig, DispatchMode as ConfigDispatchMode, McpServerConfig},
    context::AliceRuntimeContext,
};

const DEFAULT_MAX_STEPS: u32 = 12;
const DEFAULT_TURN_TIMEOUT_MS: u64 = 90_000;
const DEFAULT_TOOL_TIMEOUT_MS: u64 = 15_000;

/// Build runtime context from configuration.
///
/// # Errors
///
/// Returns an error if any adapter fails to initialize.
pub async fn build_runtime(cfg: &AliceConfig) -> eyre::Result<AliceRuntimeContext> {
    let llm: Arc<dyn LlmPort> = Arc::new(GenAiLlmAdapter::new(Default::default()));
    let tools = build_tool_port(cfg).await?;
    let tools_ref = tools.clone();
    let store: Arc<dyn SessionStore> = Arc::new(InMemorySessionStore::new());
    let events: Arc<dyn EventSink> = Arc::new(TracingEventSink::new());
    let tape: Arc<dyn TapeStorePort> = Arc::new(InMemoryTapeStore::new());

    let policy = TurnPolicy {
        max_steps: cfg.runtime.max_steps.unwrap_or(DEFAULT_MAX_STEPS),
        turn_timeout_ms: cfg.runtime.turn_timeout_ms.unwrap_or(DEFAULT_TURN_TIMEOUT_MS),
        tool_timeout_ms: DEFAULT_TOOL_TIMEOUT_MS,
        ..TurnPolicy::default()
    };

    let runtime = RuntimeBuilder::new()
        .with_llm(llm)
        .with_tools(tools)
        .with_store(store)
        .with_events(events.clone())
        .with_default_model(cfg.runtime.default_model.clone())
        .with_policy(policy)
        .with_dispatch_mode(resolve_dispatch_mode(cfg.runtime.dispatch_mode))
        .build()?;

    let agent_loop = AgentLoop::new(runtime.clone(), tools_ref.clone())
        .with_tape(tape.clone())
        .with_events(events);

    let memory_store = SqliteMemoryStore::open(
        &cfg.memory.db_path,
        cfg.memory.vector_dimensions,
        cfg.memory.enable_vector,
    )?;
    let weights = HybridWeights::new(cfg.memory.bm25_weight, cfg.memory.vector_weight)?;
    let memory_service = Arc::new(MemoryService::new(
        Arc::new(memory_store),
        cfg.memory.recall_limit,
        weights,
        cfg.memory.vector_dimensions,
        cfg.memory.enable_vector,
    )?);

    let skill_composer = crate::skill_wiring::build_skill_composer(&cfg.skills)?;

    Ok(AliceRuntimeContext {
        agent_loop,
        runtime,
        tools: tools_ref,
        tape,
        memory_service,
        skill_composer,
        skill_token_budget: cfg.skills.token_budget,
        default_model: cfg.runtime.default_model.clone(),
    })
}

const fn resolve_dispatch_mode(mode: Option<ConfigDispatchMode>) -> DispatchMode {
    match mode {
        Some(ConfigDispatchMode::PromptGuided) => DispatchMode::PromptGuided,
        Some(ConfigDispatchMode::NativePreferred) | None => DispatchMode::NativePreferred,
    }
}

async fn build_tool_port(cfg: &AliceConfig) -> eyre::Result<Arc<dyn ToolPort>> {
    if cfg.mcp.servers.is_empty() {
        return Ok(Arc::new(NoOpToolPort));
    }

    if cfg.mcp.servers.len() == 1 {
        return build_single_tool_port(&cfg.mcp.servers[0]).await;
    }

    let mut ports = Vec::with_capacity(cfg.mcp.servers.len());
    for server in &cfg.mcp.servers {
        let port = build_single_tool_port(server).await?;
        ports.push((server.id.clone(), port));
    }

    Ok(Arc::new(CompositeToolPort::new(ports)))
}

async fn build_single_tool_port(server: &McpServerConfig) -> eyre::Result<Arc<dyn ToolPort>> {
    let env = server
        .env
        .as_ref()
        .map_or_else(Vec::new, |vars| vars.iter().map(|(k, v)| (k.clone(), v.clone())).collect());

    let adapter = McpToolAdapter::connect_stdio(&server.id, &server.command, &server.args, &env)
        .await
        .map_err(|error| eyre::eyre!("failed to connect MCP server '{}': {error}", server.id))?;

    let timeout = server.tool_timeout_ms.unwrap_or(DEFAULT_TOOL_TIMEOUT_MS);
    let layer = TimeoutToolLayer::new(timeout);

    Ok(layer.wrap(Arc::new(adapter)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AliceConfig, ChannelsConfig, McpConfig, MemoryConfig, RuntimeConfig, SkillsConfig,
    };

    fn base_config() -> AliceConfig {
        AliceConfig {
            runtime: RuntimeConfig {
                default_model: "openai:gpt-4o-mini".to_string(),
                max_steps: Some(3),
                turn_timeout_ms: Some(10_000),
                dispatch_mode: Some(ConfigDispatchMode::PromptGuided),
            },
            memory: MemoryConfig {
                db_path: format!(
                    "{}/alice-bootstrap-test-{}.db",
                    std::env::temp_dir().display(),
                    std::process::id()
                ),
                ..MemoryConfig::default()
            },
            skills: SkillsConfig::default(),
            channels: ChannelsConfig::default(),
            mcp: McpConfig::default(),
        }
    }

    #[tokio::test]
    async fn build_runtime_without_mcp() {
        let cfg = base_config();
        let built = build_runtime(&cfg).await;
        assert!(built.is_ok(), "runtime should build without mcp");
        let Ok(built) = built else { return };
        assert_eq!(built.default_model, "openai:gpt-4o-mini");
    }

    #[test]
    fn dispatch_mode_mapping() {
        assert_eq!(
            resolve_dispatch_mode(Some(ConfigDispatchMode::PromptGuided)),
            DispatchMode::PromptGuided
        );
        assert_eq!(
            resolve_dispatch_mode(Some(ConfigDispatchMode::NativePreferred)),
            DispatchMode::NativePreferred
        );
        assert_eq!(resolve_dispatch_mode(None), DispatchMode::NativePreferred);
    }
}
