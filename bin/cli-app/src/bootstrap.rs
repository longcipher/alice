//! Runtime/bootstrap wiring for Alice.

use std::sync::Arc;

use bob_adapters::{
    llm_genai::GenAiLlmAdapter, mcp_rmcp::McpToolAdapter, observe::TracingEventSink,
    store_memory::InMemorySessionStore,
};
use bob_core::{
    ports::{EventSink, LlmPort, SessionStore, ToolPort},
    types::TurnPolicy,
};
use bob_runtime::{
    AgentBootstrap, AgentRuntime, DispatchMode, NoOpToolPort, RuntimeBuilder, TimeoutToolLayer,
    ToolLayer, composite::CompositeToolPort,
};
use common::memory::{
    domain::HybridWeights, service::MemoryService, sqlite_store::SqliteMemoryStore,
};

use crate::config::{AliceConfig, DispatchMode as ConfigDispatchMode, McpServerConfig};

const DEFAULT_MAX_STEPS: u32 = 12;
const DEFAULT_TURN_TIMEOUT_MS: u64 = 90_000;
const DEFAULT_TOOL_TIMEOUT_MS: u64 = 15_000;

/// Fully wired Alice runtime context.
pub struct AliceRuntimeContext {
    /// Bob runtime executor.
    pub runtime: Arc<dyn AgentRuntime>,
    /// Local memory service.
    pub memory_service: Arc<MemoryService>,
    /// Default model id.
    pub default_model: String,
}

impl std::fmt::Debug for AliceRuntimeContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AliceRuntimeContext")
            .field("default_model", &self.default_model)
            .finish_non_exhaustive()
    }
}

/// Build runtime context from configuration.
pub async fn build_runtime(cfg: &AliceConfig) -> eyre::Result<AliceRuntimeContext> {
    let llm: Arc<dyn LlmPort> = Arc::new(GenAiLlmAdapter::new(Default::default()));
    let tools = build_tool_port(cfg).await?;
    let store: Arc<dyn SessionStore> = Arc::new(InMemorySessionStore::new());
    let events: Arc<dyn EventSink> = Arc::new(TracingEventSink::new());

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
        .with_events(events)
        .with_default_model(cfg.runtime.default_model.clone())
        .with_policy(policy)
        .with_dispatch_mode(resolve_dispatch_mode(cfg.runtime.dispatch_mode))
        .build()?;

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

    Ok(AliceRuntimeContext {
        runtime,
        memory_service,
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
    use crate::config::{AliceConfig, McpConfig, MemoryConfig, RuntimeConfig};

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
            mcp: McpConfig::default(),
        }
    }

    #[tokio::test]
    async fn build_runtime_without_mcp() {
        let cfg = base_config();
        let built = build_runtime(&cfg).await;
        assert!(built.is_ok(), "runtime should build without mcp");
        let Ok(built) = built else {
            return;
        };
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
