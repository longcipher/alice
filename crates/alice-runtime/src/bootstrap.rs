//! Runtime/bootstrap wiring for Alice.

use std::sync::Arc;

use alice_adapters::memory::sqlite_store::SqliteMemoryStore;
use alice_core::memory::{domain::HybridWeights, service::MemoryService};
use bob_adapters::{
    llm_liter::LiterLlmAdapter, mcp_rmcp::McpToolAdapter, observe::TracingEventSink,
    store_memory::InMemorySessionStore, tape_memory::InMemoryTapeStore,
};
use bob_core::ports::{EventSink, LlmPort, SessionStore, TapeStorePort, ToolPort};
use bob_runtime::{
    Agent, AgentBootstrap, DispatchMode, NoOpToolPort, RuntimeBuilder, TimeoutToolLayer, ToolLayer,
    agent_loop::AgentLoop, composite::CompositeToolPort,
};

use crate::{
    agent_backend::AgentBackend,
    config::{AgentBackendType, AliceConfig, DispatchMode as ConfigDispatchMode, McpServerConfig},
    context::AliceRuntimeContext,
};

const DEFAULT_MAX_STEPS: u32 = 12;
const DEFAULT_TURN_TIMEOUT_MS: u64 = 90_000;
const DEFAULT_TOOL_TIMEOUT_MS: u64 = 15_000;

/// Convert a model identifier from the `provider:model` format (used by the
/// bob runtime config) to the `provider/model` format expected by `liter_llm`.
fn normalize_model_name(model: &str) -> String {
    if let Some((provider, rest)) = model.split_once(':') {
        format!("{provider}/{rest}")
    } else {
        model.to_string()
    }
}

/// Build runtime context from configuration.
///
/// # Errors
///
/// Returns an error if any adapter fails to initialize.
pub async fn build_runtime(cfg: &AliceConfig) -> eyre::Result<AliceRuntimeContext> {
    let default_model = normalize_model_name(&cfg.runtime.default_model);

    let config = liter_llm::ClientConfig::new("");
    let client = liter_llm::DefaultClient::new(config, None)
        .map_err(|e| eyre::eyre!("failed to create liter-llm client: {e}"))?;
    let llm: Arc<dyn LlmPort> = Arc::new(LiterLlmAdapter::new(Arc::new(client)));
    let tools = build_tool_port(cfg).await?;
    let tools_ref = tools.clone();
    let store: Arc<dyn SessionStore> = Arc::new(InMemorySessionStore::new());
    let events: Arc<dyn EventSink> = Arc::new(TracingEventSink::new());
    let tape: Arc<dyn TapeStorePort> = Arc::new(InMemoryTapeStore::new());

    let policy = bob_core::types::TurnPolicy {
        max_steps: cfg.runtime.max_steps.unwrap_or(DEFAULT_MAX_STEPS),
        turn_timeout_ms: cfg.runtime.turn_timeout_ms.unwrap_or(DEFAULT_TURN_TIMEOUT_MS),
        tool_timeout_ms: DEFAULT_TOOL_TIMEOUT_MS,
        ..bob_core::types::TurnPolicy::default()
    };

    let runtime = RuntimeBuilder::new()
        .with_llm(llm)
        .with_tools(tools)
        .with_store(store.clone())
        .with_events(events.clone())
        .with_default_model(default_model.clone())
        .with_policy(policy)
        .with_dispatch_mode(resolve_dispatch_mode(cfg.runtime.dispatch_mode))
        .build()?;

    // Build Agent + Session API (bob 0.2.2)
    let agent = Agent::from_runtime(runtime.clone(), tools_ref.clone())
        .with_store(store)
        .with_tape(tape.clone())
        .build();

    let agent_loop = AgentLoop::new(runtime, tools_ref.clone()).with_tape(tape).with_events(events);

    // Build agent backend based on configuration
    let backend: Arc<dyn AgentBackend> = build_agent_backend(cfg, &agent)?;

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

    Ok(AliceRuntimeContext::new(
        agent_loop,
        agent,
        backend,
        memory_service,
        skill_composer,
        cfg.skills.token_budget,
        default_model,
    ))
}

/// Build the appropriate agent backend from configuration.
fn build_agent_backend(cfg: &AliceConfig, agent: &Agent) -> eyre::Result<Arc<dyn AgentBackend>> {
    match cfg.agent.backend {
        AgentBackendType::Bob => {
            let backend = crate::agent_backend::bob_backend::BobAgentBackend::new(agent.clone());
            Ok(Arc::new(backend))
        }
        AgentBackendType::Acp => {
            #[cfg(feature = "acp-agent")]
            {
                let command =
                    cfg.agent.acp_command.clone().ok_or_else(|| {
                        eyre::eyre!("agent.acp_command is required for acp backend")
                    })?;
                let config = crate::agent_backend::acp_backend::AcpConfig {
                    command,
                    args: cfg.agent.acp_args.clone(),
                    working_dir: cfg.agent.acp_working_dir.clone(),
                };
                let backend = crate::agent_backend::acp_backend::AcpAgentBackend::new(config);
                Ok(Arc::new(backend))
            }
            #[cfg(not(feature = "acp-agent"))]
            {
                let _ = agent;
                Err(eyre::eyre!(
                    "acp backend requires the 'acp-agent' feature; \
                     rebuild with --features acp-agent"
                ))
            }
        }
    }
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
        AgentBackendConfig, AliceConfig, ChannelsConfig, McpConfig, MemoryConfig, RuntimeConfig,
        SkillsConfig,
    };

    fn base_config() -> AliceConfig {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        AliceConfig {
            runtime: RuntimeConfig {
                default_model: "openai:gpt-4o-mini".to_string(),
                max_steps: Some(3),
                turn_timeout_ms: Some(10_000),
                dispatch_mode: Some(ConfigDispatchMode::PromptGuided),
            },
            agent: AgentBackendConfig::default(),
            memory: MemoryConfig {
                db_path: format!(
                    "{}/alice-bootstrap-test-{}-{}.db",
                    std::env::temp_dir().display(),
                    std::process::id(),
                    n
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
        assert_eq!(built.default_model(), "openai/gpt-4o-mini");
    }

    #[tokio::test]
    async fn build_runtime_with_bob_backend() {
        let cfg = base_config();
        let built = build_runtime(&cfg).await;
        assert!(built.is_ok(), "runtime should build with bob backend");
    }

    #[cfg(feature = "acp-agent")]
    #[tokio::test]
    async fn build_runtime_with_acp_backend() {
        let mut cfg = base_config();
        cfg.agent = AgentBackendConfig {
            backend: AgentBackendType::Acp,
            acp_command: Some("mock-agent".to_string()),
            acp_args: vec![],
            acp_working_dir: None,
        };
        let built = build_runtime(&cfg).await;
        assert!(built.is_ok(), "runtime should build with acp backend");
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

    #[test]
    fn normalize_model_name_converts_colon_to_slash() {
        assert_eq!(normalize_model_name("openai:gpt-4o-mini"), "openai/gpt-4o-mini");
        assert_eq!(
            normalize_model_name("anthropic:claude-sonnet-4-20250514"),
            "anthropic/claude-sonnet-4-20250514"
        );
        assert_eq!(normalize_model_name("groq:llama3-70b"), "groq/llama3-70b");
    }

    #[test]
    fn normalize_model_name_preserves_slash_format() {
        assert_eq!(normalize_model_name("openai/gpt-4o-mini"), "openai/gpt-4o-mini");
    }

    #[test]
    fn normalize_model_name_preserves_bare_model() {
        assert_eq!(normalize_model_name("gpt-4o-mini"), "gpt-4o-mini");
    }
}
