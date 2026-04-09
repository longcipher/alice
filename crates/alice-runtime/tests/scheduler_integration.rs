//! Integration tests for the reusable scheduler tick executor.

use std::sync::Arc;

use alice_adapters::{
    memory::sqlite_store::SqliteMemoryStore, runtime_state::sqlite_store::SqliteRuntimeStateStore,
};
use alice_core::{
    memory::{domain::HybridWeights, service::MemoryService},
    runtime_state::{
        domain::{ScheduleKind, ScheduledTask},
        service::RuntimeStateService,
    },
};
use alice_runtime::{
    agent_backend::{AgentBackend, AgentSession},
    channel_dispatch::{ChannelDispatcher, ChannelPoster},
    config::SkillsConfig,
    context::{AliceRuntimeContext, AliceRuntimeServices},
    scheduler::{SchedulerSessionSource, SchedulerTickExecutor, SchedulerTickOutcome},
};
use async_trait::async_trait;
use bob_adapters::tape_memory::InMemoryTapeStore;
use bob_core::{
    error::AgentError,
    ports::TapeStorePort,
    types::{AgentRequest, AgentRunResult, FinishReason, HealthStatus, RuntimeHealth, TokenUsage},
};
use bob_runtime::{AgentResponse, AgentRuntime, NoOpToolPort, agent_loop::AgentLoop};
use parking_lot::Mutex;

#[derive(Debug)]
struct StubRuntime;

#[async_trait]
impl AgentRuntime for StubRuntime {
    async fn run(&self, _req: AgentRequest) -> Result<AgentRunResult, AgentError> {
        Err(AgentError::Config("scheduler tests do not use direct runtime calls".to_string()))
    }

    async fn run_stream(
        &self,
        _req: AgentRequest,
    ) -> Result<bob_core::types::AgentEventStream, AgentError> {
        Err(AgentError::Config("scheduler tests do not use streaming".to_string()))
    }

    async fn health(&self) -> RuntimeHealth {
        RuntimeHealth { status: HealthStatus::Healthy, llm_ready: true, mcp_pool_ready: true }
    }
}

#[derive(Debug, Clone)]
struct BackendCall {
    session_id: String,
    input: String,
}

#[derive(Debug, Default)]
struct RecordingBackend {
    calls: Arc<Mutex<Vec<BackendCall>>>,
}

#[derive(Debug)]
struct RecordingSession {
    session_id: String,
    calls: Arc<Mutex<Vec<BackendCall>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PostedMessage {
    thread_id: String,
    text: String,
}

#[derive(Debug, Default)]
struct RecordingPoster {
    posted: Arc<Mutex<Vec<PostedMessage>>>,
}

#[async_trait]
impl AgentSession for RecordingSession {
    async fn chat(
        &self,
        input: &str,
        _context: bob_core::types::RequestContext,
    ) -> eyre::Result<AgentResponse> {
        self.calls
            .lock()
            .push(BackendCall { session_id: self.session_id.clone(), input: input.to_string() });
        Ok(AgentResponse::new(
            format!("processed: {input}"),
            TokenUsage::default(),
            FinishReason::Stop,
        ))
    }
}

impl AgentBackend for RecordingBackend {
    fn create_session(&self) -> Arc<dyn AgentSession> {
        self.create_session_with_id("generated-session")
    }

    fn create_session_with_id(&self, session_id: &str) -> Arc<dyn AgentSession> {
        Arc::new(RecordingSession {
            session_id: session_id.to_string(),
            calls: Arc::clone(&self.calls),
        })
    }
}

#[async_trait]
impl ChannelPoster for RecordingPoster {
    async fn post_text(
        &self,
        thread_id: &str,
        text: &str,
    ) -> eyre::Result<bob_chat::message::SentMessage> {
        self.posted
            .lock()
            .push(PostedMessage { thread_id: thread_id.to_string(), text: text.to_string() });
        Ok(bob_chat::message::SentMessage {
            id: "sent-1".to_string(),
            thread_id: thread_id.to_string(),
            adapter_name: "telegram".to_string(),
            raw: None,
        })
    }
}

fn build_test_context() -> Option<(AliceRuntimeContext, Arc<RecordingBackend>, Arc<RecordingPoster>)>
{
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

    let agent = bob_runtime::Agent::from_runtime(runtime, tools)
        .with_store(session_store)
        .with_tape(tape)
        .build();

    let backend = Arc::new(RecordingBackend::default());
    let poster = Arc::new(RecordingPoster::default());
    let channel_dispatcher = ChannelDispatcher::new();
    let _ = channel_dispatcher.register_poster("telegram", poster.clone());
    let runtime_state_store = SqliteRuntimeStateStore::in_memory().ok()?;
    let runtime_state_service = RuntimeStateService::new(Arc::new(runtime_state_store)).ok()?;

    Some((
        AliceRuntimeContext::new(
            agent_loop,
            agent,
            AliceRuntimeServices {
                backend: backend.clone(),
                memory_service: Arc::new(memory_service),
                runtime_state_service: Arc::new(runtime_state_service),
                channel_dispatcher,
                orchestrator: None,
                auto_orchestrate: false,
                skills_config: SkillsConfig::default(),
                reflector: None,
                default_model: "test-model".to_string(),
            },
        ),
        backend,
        poster,
    ))
}

fn due_task(task_id: &str, global_user_id: &str, next_run_epoch_ms: i64) -> ScheduledTask {
    ScheduledTask {
        task_id: task_id.to_string(),
        global_user_id: global_user_id.to_string(),
        channel: Some("telegram".to_string()),
        prompt: format!("run {task_id}"),
        schedule: ScheduleKind::EveryMinutes(30),
        next_run_epoch_ms,
        enabled: true,
        last_run_epoch_ms: None,
    }
}

#[tokio::test]
async fn due_enabled_tasks_execute() {
    let Some((context, backend, _poster)) = build_test_context() else {
        return;
    };
    let task = due_task("task-due", "global-1", 1_000);
    let inserted = context.runtime_state_service().insert_scheduled_task(task.clone());
    assert!(inserted.is_ok(), "task should persist");

    let report = SchedulerTickExecutor::default().run(&context, 1_000).await;
    assert!(report.is_ok(), "scheduler tick should succeed");
    let Ok(report) = report else {
        return;
    };

    assert_eq!(report.due_task_count, 1);
    assert_eq!(report.executions.len(), 1);
    assert_eq!(report.executions[0].task_id, "task-due");
    assert_eq!(report.executions[0].outcome, SchedulerTickOutcome::Executed);

    let calls = backend.calls.lock();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].input, "run task-due");
}

#[tokio::test]
async fn disabled_and_not_due_tasks_are_skipped() {
    let Some((context, backend, _poster)) = build_test_context() else {
        return;
    };
    let due = due_task("task-due", "global-1", 1_000);
    let disabled = ScheduledTask { enabled: false, ..due_task("task-disabled", "global-2", 500) };
    let not_due = due_task("task-not-due", "global-3", 5_000);

    for task in [due.clone(), disabled.clone(), not_due.clone()] {
        let inserted = context.runtime_state_service().insert_scheduled_task(task);
        assert!(inserted.is_ok(), "task should persist");
    }

    let report = SchedulerTickExecutor::default().run(&context, 1_000).await;
    assert!(report.is_ok(), "scheduler tick should succeed");
    let Ok(report) = report else {
        return;
    };

    assert_eq!(report.due_task_count, 1);
    assert_eq!(report.executions.len(), 1);
    assert_eq!(report.executions[0].task_id, "task-due");

    let calls = backend.calls.lock();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].session_id, "scheduled-global-1");
}

#[tokio::test]
async fn active_session_lease_is_preferred_over_fallback() {
    let Some((context, backend, _poster)) = build_test_context() else {
        return;
    };
    let task = due_task("task-session", "global-42", 1_000);
    assert!(context.runtime_state_service().insert_scheduled_task(task).is_ok());
    assert!(
        context
            .runtime_state_service()
            .upsert_active_session("global-42", "lease-session-42", Some("cli"))
            .is_ok()
    );

    let report = SchedulerTickExecutor::default().run(&context, 1_000).await;
    assert!(report.is_ok(), "scheduler tick should succeed");
    let Ok(report) = report else {
        return;
    };

    assert_eq!(report.executions.len(), 1);
    assert_eq!(report.executions[0].session_id, "lease-session-42");
    assert_eq!(report.executions[0].session_source, SchedulerSessionSource::ActiveLease);

    let calls = backend.calls.lock();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].session_id, "lease-session-42");
}

#[tokio::test]
async fn next_run_epoch_advances_after_success() {
    let Some((context, _backend, _poster)) = build_test_context() else {
        return;
    };
    let task = due_task("task-next-run", "global-77", 1_000);
    assert!(context.runtime_state_service().insert_scheduled_task(task).is_ok());

    let report = SchedulerTickExecutor::default().run(&context, 1_000).await;
    assert!(report.is_ok(), "scheduler tick should succeed");
    let Ok(report) = report else {
        return;
    };
    assert_eq!(report.executions[0].outcome, SchedulerTickOutcome::Executed);

    let still_not_due = context
        .runtime_state_service()
        .list_due_tasks(1_800_999)
        .expect("listing due tasks should succeed");
    assert!(still_not_due.is_empty(), "task should advance beyond the old window");

    let due_again = context
        .runtime_state_service()
        .list_due_tasks(1_801_000)
        .expect("listing due tasks should succeed");
    assert_eq!(due_again.len(), 1, "task should reappear at the next scheduled time");
    assert_eq!(due_again[0].task_id, "task-next-run");
}

#[tokio::test]
async fn scheduled_task_result_posts_back_to_active_thread() {
    let Some((context, _backend, poster)) = build_test_context() else {
        return;
    };
    let task = due_task("task-push", "global-88", 1_000);
    assert!(context.runtime_state_service().insert_scheduled_task(task).is_ok());
    assert!(
        context
            .runtime_state_service()
            .upsert_active_session_with_thread_id(
                "global-88",
                "lease-session-88",
                Some("telegram"),
                Some("telegram-thread-88"),
            )
            .is_ok()
    );

    let report = SchedulerTickExecutor::default().run(&context, 1_000).await;
    assert!(report.is_ok(), "scheduler tick should succeed");

    let posted = poster.posted.lock();
    assert_eq!(posted.len(), 1, "scheduled result should be delivered once");
    assert_eq!(posted[0].thread_id, "telegram-thread-88");
    assert_eq!(posted[0].text, "processed: run task-push");
}
