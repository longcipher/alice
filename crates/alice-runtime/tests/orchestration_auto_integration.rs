//! Integration tests for auto-orchestration in normal natural-language turns.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use alice_adapters::{
    memory::sqlite_store::SqliteMemoryStore, runtime_state::sqlite_store::SqliteRuntimeStateStore,
};
use alice_core::{
    memory::{domain::HybridWeights, service::MemoryService},
    runtime_state::service::RuntimeStateService,
};
use alice_runtime::{
    agent_backend::{AgentBackend, AgentSession},
    channel_dispatch::ChannelDispatcher,
    config::SkillsConfig,
    context::{AliceRuntimeContext, AliceRuntimeServices},
    handle_input::handle_input_with_skills,
    orchestration::{OrchestrationProfileRegistry, Orchestrator},
};
use async_trait::async_trait;
use bob_adapters::tape_memory::InMemoryTapeStore;
use bob_core::{
    error::AgentError,
    ports::TapeStorePort,
    types::{
        AgentRequest, AgentRunResult, FinishReason, HealthStatus, RequestContext, RuntimeHealth,
        TokenUsage,
    },
};
use bob_runtime::{
    AgentResponse, AgentRuntime, NoOpToolPort,
    agent_loop::{AgentLoop, AgentLoopOutput},
};

#[derive(Debug)]
struct CountingRuntime {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl AgentRuntime for CountingRuntime {
    async fn run(&self, _req: AgentRequest) -> Result<AgentRunResult, AgentError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(AgentRunResult::Finished(bob_core::types::AgentResponse {
            content: "agent-loop".to_string(),
            tool_transcript: Vec::new(),
            usage: TokenUsage::default(),
            finish_reason: FinishReason::Stop,
        }))
    }

    async fn run_stream(
        &self,
        _req: AgentRequest,
    ) -> Result<bob_core::types::AgentEventStream, AgentError> {
        Err(AgentError::Config("streaming not used in auto orchestration tests".to_string()))
    }

    async fn health(&self) -> RuntimeHealth {
        RuntimeHealth { status: HealthStatus::Healthy, llm_ready: true, mcp_pool_ready: true }
    }
}

#[derive(Debug, Default)]
struct BackendLog {
    sessions: Mutex<Vec<String>>,
}

#[derive(Debug)]
struct MockBackend {
    response_prefix: String,
    log: Arc<BackendLog>,
}

#[derive(Debug)]
struct MockSession {
    response_prefix: String,
    session_id: String,
    log: Arc<BackendLog>,
}

#[async_trait]
impl AgentSession for MockSession {
    async fn chat(&self, input: &str, _context: RequestContext) -> eyre::Result<AgentResponse> {
        self.log
            .sessions
            .lock()
            .expect("session log should be available")
            .push(self.session_id.clone());
        Ok(AgentResponse::new(
            format!("{}:{input}", self.response_prefix),
            TokenUsage::default(),
            FinishReason::Stop,
        ))
    }
}

impl AgentBackend for MockBackend {
    fn create_session(&self) -> Arc<dyn AgentSession> {
        self.create_session_with_id("generated")
    }

    fn create_session_with_id(&self, session_id: &str) -> Arc<dyn AgentSession> {
        Arc::new(MockSession {
            response_prefix: self.response_prefix.clone(),
            session_id: session_id.to_string(),
            log: Arc::clone(&self.log),
        })
    }
}

fn build_context(
    orchestrator: Option<Orchestrator>,
    auto_orchestrate: bool,
) -> Option<(AliceRuntimeContext, Arc<AtomicUsize>)> {
    let store = SqliteMemoryStore::in_memory(384, false).ok()?;
    let memory_service =
        MemoryService::new(Arc::new(store), 5, HybridWeights::default(), 384, false).ok()?;
    let runtime_calls = Arc::new(AtomicUsize::new(0));

    let runtime: Arc<dyn AgentRuntime> =
        Arc::new(CountingRuntime { calls: Arc::clone(&runtime_calls) });
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

    let backend: Arc<dyn AgentBackend> =
        Arc::new(alice_runtime::agent_backend::bob_backend::BobAgentBackend::new(agent.clone()));
    let runtime_state_store = SqliteRuntimeStateStore::in_memory().ok()?;
    let runtime_state_service = RuntimeStateService::new(Arc::new(runtime_state_store)).ok()?;

    let context = AliceRuntimeContext::new(
        agent_loop,
        agent,
        AliceRuntimeServices {
            backend,
            memory_service: Arc::new(memory_service),
            runtime_state_service: Arc::new(runtime_state_service),
            channel_dispatcher: ChannelDispatcher::new(),
            orchestrator,
            auto_orchestrate,
            skills_config: SkillsConfig::default(),
            reflector: None,
            default_model: "test-model".to_string(),
        },
    );

    Some((context, runtime_calls))
}

fn make_orchestrator(manager_log: Arc<BackendLog>, worker_log: Arc<BackendLog>) -> Orchestrator {
    let mut registry = OrchestrationProfileRegistry::new();
    registry.register(
        "manager",
        Arc::new(MockBackend { response_prefix: "manager".to_string(), log: manager_log }),
    );
    registry.register(
        "worker",
        Arc::new(MockBackend { response_prefix: "worker".to_string(), log: worker_log }),
    );
    Orchestrator::new("manager", registry)
}

#[tokio::test]
async fn natural_language_uses_orchestrator_when_enabled() {
    let manager_log = Arc::new(BackendLog::default());
    let worker_log = Arc::new(BackendLog::default());
    let orchestrator = make_orchestrator(Arc::clone(&manager_log), Arc::clone(&worker_log));
    let Some((context, runtime_calls)) = build_context(Some(orchestrator), true) else {
        return;
    };

    let output =
        handle_input_with_skills(&context, "session-root", Some("user-1"), "build plan").await;
    assert!(output.is_ok(), "input handling should succeed");
    let Ok(output) = output else {
        return;
    };

    match output {
        AgentLoopOutput::Response(AgentRunResult::Finished(response)) => {
            assert!(response.content.contains("manager: manager:build plan"));
            assert!(response.content.contains("worker: worker:build plan"));
        }
        other => panic!("unexpected output: {other:?}"),
    }
    assert_eq!(runtime_calls.load(Ordering::Relaxed), 0, "agent loop runtime should be bypassed");
    assert_eq!(
        manager_log.sessions.lock().expect("manager log").clone(),
        vec!["session-root::manager".to_string()]
    );
    assert_eq!(
        worker_log.sessions.lock().expect("worker log").clone(),
        vec!["session-root::worker::worker".to_string()]
    );
}

#[tokio::test]
async fn natural_language_falls_back_to_agent_loop_when_auto_orchestration_disabled() {
    let manager_log = Arc::new(BackendLog::default());
    let worker_log = Arc::new(BackendLog::default());
    let orchestrator = make_orchestrator(Arc::clone(&manager_log), Arc::clone(&worker_log));
    let Some((context, runtime_calls)) = build_context(Some(orchestrator), false) else {
        return;
    };

    let output =
        handle_input_with_skills(&context, "session-root", Some("user-1"), "plain turn").await;
    assert!(output.is_ok(), "input handling should succeed");
    let Ok(output) = output else {
        return;
    };

    match output {
        AgentLoopOutput::Response(AgentRunResult::Finished(response)) => {
            assert_eq!(response.content, "agent-loop");
        }
        other => panic!("unexpected output: {other:?}"),
    }
    assert_eq!(runtime_calls.load(Ordering::Relaxed), 1, "agent loop runtime should be used");
    assert!(
        manager_log.sessions.lock().expect("manager log").is_empty(),
        "manager backend should stay idle"
    );
    assert!(
        worker_log.sessions.lock().expect("worker log").is_empty(),
        "worker backend should stay idle"
    );
}
