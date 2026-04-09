//! Integration tests for cross-channel identity resolution.

use std::sync::Arc;

use alice_adapters::{
    memory::sqlite_store::SqliteMemoryStore, runtime_state::sqlite_store::SqliteRuntimeStateStore,
};
use alice_core::{
    memory::{domain::HybridWeights, service::MemoryService},
    runtime_state::service::RuntimeStateService,
};
use alice_runtime::{
    agent_backend::bob_backend::BobAgentBackend,
    config::SkillsConfig,
    context::{AliceRuntimeContext, AliceRuntimeServices},
    identity::IdentityResolver,
};
use async_trait::async_trait;
use bob_adapters::tape_memory::InMemoryTapeStore;
use bob_core::{error::AgentError, ports::TapeStorePort, types::*};
use bob_runtime::{AgentRuntime, NoOpToolPort, agent_loop::AgentLoop};

#[derive(Debug)]
struct StubRuntime;

#[async_trait]
impl AgentRuntime for StubRuntime {
    async fn run(&self, _req: AgentRequest) -> Result<AgentRunResult, AgentError> {
        Ok(AgentRunResult::Finished(AgentResponse {
            content: "stub-response".to_string(),
            tool_transcript: Vec::new(),
            usage: TokenUsage::default(),
            finish_reason: FinishReason::Stop,
        }))
    }

    async fn run_stream(&self, _req: AgentRequest) -> Result<AgentEventStream, AgentError> {
        Err(AgentError::Config("streaming not used in identity tests".to_string()))
    }

    async fn health(&self) -> RuntimeHealth {
        RuntimeHealth { status: HealthStatus::Healthy, llm_ready: true, mcp_pool_ready: true }
    }
}

fn build_test_context() -> Option<AliceRuntimeContext> {
    let store = SqliteMemoryStore::in_memory(384, false).ok()?;
    let memory_service =
        MemoryService::new(Arc::new(store), 5, HybridWeights::default(), 384, false).ok()?;

    let runtime: Arc<dyn AgentRuntime> = Arc::new(StubRuntime);
    let tools: Arc<dyn bob_core::ports::ToolPort> = Arc::new(NoOpToolPort);
    let tape: Arc<dyn TapeStorePort> = Arc::new(InMemoryTapeStore::new());
    let session_store: Arc<dyn bob_core::ports::SessionStore> =
        Arc::new(bob_adapters::store_memory::InMemorySessionStore::new());
    let events: Arc<dyn bob_core::ports::EventSink> =
        Arc::new(bob_adapters::observe::TracingEventSink::new());

    let agent_loop = AgentLoop::new(runtime.clone(), tools.clone())
        .with_tape(tape.clone())
        .with_events(events.clone());

    let agent = bob_runtime::Agent::from_runtime(runtime, tools.clone())
        .with_store(session_store)
        .with_tape(tape)
        .build();

    let backend: Arc<dyn alice_runtime::agent_backend::AgentBackend> =
        Arc::new(BobAgentBackend::new(agent.clone()));
    let runtime_state_store = SqliteRuntimeStateStore::in_memory().ok()?;
    let runtime_state_service = RuntimeStateService::new(Arc::new(runtime_state_store)).ok()?;

    Some(AliceRuntimeContext::new(
        agent_loop,
        agent,
        AliceRuntimeServices {
            backend,
            memory_service: Arc::new(memory_service),
            runtime_state_service: Arc::new(runtime_state_service),
            channel_dispatcher: alice_runtime::channel_dispatch::ChannelDispatcher::new(),
            orchestrator: None,
            auto_orchestrate: false,
            skills_config: SkillsConfig::default(),
            reflector: None,
            default_model: "test-model".to_string(),
        },
    ))
}

#[test]
fn explicit_cli_global_user_reuses_active_session() {
    let Some(context) = build_test_context() else {
        return;
    };
    let resolver = IdentityResolver::new(&context);

    let lease = context.runtime_state_service().upsert_active_session(
        "global-user",
        "shared-session",
        Some("cli"),
    );
    assert!(lease.is_ok(), "active session lease should be stored");

    let resolved = resolver.resolve_cli_turn("fallback-session", Some("global-user"));
    assert!(resolved.is_ok(), "cli identity should resolve");
    let Ok(resolved) = resolved else {
        return;
    };

    assert_eq!(resolved.session_id, "shared-session");
    assert_eq!(resolved.profile_id, "global-user");
    assert_eq!(resolved.global_user_id.as_deref(), Some("global-user"));
}

#[test]
fn bind_command_consumes_token_and_resolves_future_messages() {
    let Some(context) = build_test_context() else {
        return;
    };
    let resolver = IdentityResolver::new(&context);

    let token =
        context.runtime_state_service().issue_bind_token("global-user", Some("telegram"), 60_000);
    assert!(token.is_ok(), "bind token should be issued");
    let Ok(token) = token else {
        return;
    };

    let outcome =
        resolver.consume_bind_command("telegram", "tg-user-1", &format!("/bind {}", token.token));
    assert!(outcome.is_ok(), "bind command should succeed");
    let Ok(Some(outcome)) = outcome else {
        panic!("bind command should produce an outcome");
    };

    assert_eq!(outcome.global_user_id.as_deref(), Some("global-user"));

    let resolved = resolver.resolve_message_turn("telegram", "tg-user-1", "telegram-chat-1");
    assert!(resolved.is_ok(), "bound user should resolve");
    let Ok(resolved) = resolved else {
        return;
    };

    assert_eq!(resolved.global_user_id.as_deref(), Some("global-user"));
    assert_eq!(resolved.profile_id, "global-user");
}

#[test]
fn bound_identity_prefers_active_session_over_channel_thread() {
    let Some(context) = build_test_context() else {
        return;
    };
    let resolver = IdentityResolver::new(&context);

    let binding =
        context.runtime_state_service().bind_identity("discord", "user-42", "global-user");
    assert!(binding.is_ok(), "binding should be stored");
    let lease = context.runtime_state_service().upsert_active_session(
        "global-user",
        "shared-session",
        Some("cli"),
    );
    assert!(lease.is_ok(), "active session lease should be stored");

    let resolved = resolver.resolve_message_turn("discord", "user-42", "discord-thread-9");
    assert!(resolved.is_ok(), "bound user should resolve");
    let Ok(resolved) = resolved else {
        return;
    };

    assert_eq!(resolved.session_id, "shared-session");
    assert_eq!(resolved.profile_id, "global-user");
    assert_eq!(resolved.global_user_id.as_deref(), Some("global-user"));
}

#[test]
fn remember_active_session_persists_global_session_lease() {
    let Some(context) = build_test_context() else {
        return;
    };
    let resolver = IdentityResolver::new(&context);

    let resolved = resolver.resolve_cli_turn("cli-session", Some("global-user"));
    assert!(resolved.is_ok(), "cli identity should resolve");
    let Ok(resolved) = resolved else {
        return;
    };

    let persisted = resolver.remember_active_session(&resolved, Some("cli"));
    assert!(persisted.is_ok(), "session lease should be persisted");

    let lease = context.runtime_state_service().get_active_session("global-user");
    assert!(lease.is_ok(), "stored lease should be readable");
    let Ok(Some(lease)) = lease else {
        panic!("stored lease should exist");
    };

    assert_eq!(lease.session_id, "cli-session");
    assert_eq!(lease.channel.as_deref(), Some("cli"));
    assert_eq!(lease.thread_id.as_deref(), Some("cli-session"));
}
