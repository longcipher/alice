//! Integration tests for runtime-owned multi-profile orchestration.

use std::sync::{Arc, Mutex};

use alice_runtime::{
    agent_backend::{AgentBackend, AgentSession},
    orchestration::{OrchestrationError, OrchestrationProfileRegistry, Orchestrator, WorkerTask},
};
use async_trait::async_trait;
use bob_core::types::{FinishReason, RequestContext, TokenUsage};
use bob_runtime::AgentResponse;

#[derive(Debug, Default)]
struct BackendLog {
    created_sessions: Mutex<Vec<String>>,
    calls: Mutex<Vec<(String, String, Option<String>)>>,
}

impl BackendLog {
    fn created_sessions(&self) -> Vec<String> {
        self.created_sessions.lock().expect("log should be accessible").clone()
    }

    fn calls(&self) -> Vec<(String, String, Option<String>)> {
        self.calls.lock().expect("log should be accessible").clone()
    }
}

#[derive(Debug)]
struct MockBackend {
    profile_name: String,
    response_prefix: String,
    log: Arc<BackendLog>,
}

#[derive(Debug)]
struct MockSession {
    profile_name: String,
    session_id: String,
    response_prefix: String,
    log: Arc<BackendLog>,
}

#[async_trait]
impl AgentSession for MockSession {
    async fn chat(&self, input: &str, context: RequestContext) -> eyre::Result<AgentResponse> {
        self.log.calls.lock().expect("call log should be accessible").push((
            self.session_id.clone(),
            input.to_string(),
            context.system_prompt.clone(),
        ));

        Ok(AgentResponse::new(
            format!("{}:{}:{}", self.response_prefix, self.profile_name, input),
            TokenUsage::default(),
            FinishReason::Stop,
        ))
    }
}

impl AgentBackend for MockBackend {
    fn create_session(&self) -> Arc<dyn AgentSession> {
        let session_id = format!("{}-auto", self.profile_name);
        self.create_session_with_id(&session_id)
    }

    fn create_session_with_id(&self, session_id: &str) -> Arc<dyn AgentSession> {
        self.log
            .created_sessions
            .lock()
            .expect("creation log should be accessible")
            .push(session_id.to_string());

        Arc::new(MockSession {
            profile_name: self.profile_name.clone(),
            session_id: session_id.to_string(),
            response_prefix: self.response_prefix.clone(),
            log: Arc::clone(&self.log),
        })
    }
}

fn mock_backend(profile_name: &str, response_prefix: &str) -> (Arc<MockBackend>, Arc<BackendLog>) {
    let log = Arc::new(BackendLog::default());
    (
        Arc::new(MockBackend {
            profile_name: profile_name.to_string(),
            response_prefix: response_prefix.to_string(),
            log: Arc::clone(&log),
        }),
        log,
    )
}

#[tokio::test]
async fn registry_stores_multiple_named_profiles() {
    let (manager_backend, _) = mock_backend("manager", "mgr");
    let (worker_backend, _) = mock_backend("worker", "wrk");

    let mut registry = OrchestrationProfileRegistry::new();
    registry.register("manager", manager_backend);
    registry.register("worker", worker_backend);

    let names = registry.profile_names();
    assert_eq!(names, vec!["manager".to_string(), "worker".to_string()]);
}

#[tokio::test]
async fn worker_profile_names_exclude_manager() {
    let (manager_backend, _) = mock_backend("manager", "mgr");
    let (planner_backend, _) = mock_backend("planner", "plan");
    let (writer_backend, _) = mock_backend("writer", "write");

    let mut registry = OrchestrationProfileRegistry::new();
    registry.register("manager", manager_backend);
    registry.register("planner", planner_backend);
    registry.register("writer", writer_backend);

    let orchestrator = Orchestrator::new("manager", registry);
    assert_eq!(
        orchestrator.worker_profile_names(),
        vec!["planner".to_string(), "writer".to_string()]
    );
}

#[tokio::test]
async fn worker_session_ids_remain_isolated_from_manager_session_id() {
    let (manager_backend, manager_log) = mock_backend("manager", "mgr");
    let (worker_backend, worker_log) = mock_backend("worker", "wrk");

    let mut registry = OrchestrationProfileRegistry::new();
    registry.register("manager", manager_backend);
    registry.register("worker", worker_backend);

    let orchestrator = Orchestrator::new("manager", registry);
    let result = orchestrator
        .run("run-42", "manager prompt", vec![WorkerTask::new("worker", "worker prompt")])
        .await
        .expect("orchestration should succeed");

    assert_eq!(manager_log.created_sessions(), vec!["run-42::manager".to_string()]);
    assert_eq!(worker_log.created_sessions(), vec!["run-42::worker::worker".to_string()]);
    assert_ne!(result.manager.session_id, result.workers[0].session_id);
    assert_eq!(result.manager.session_id, "run-42::manager");
    assert_eq!(result.workers[0].session_id, "run-42::worker::worker");
}

#[tokio::test]
async fn fan_out_invokes_expected_backends_and_returns_aggregate_output() {
    let (manager_backend, manager_log) = mock_backend("manager", "mgr");
    let (planner_backend, planner_log) = mock_backend("planner", "plan");
    let (writer_backend, writer_log) = mock_backend("writer", "write");

    let mut registry = OrchestrationProfileRegistry::new();
    registry.register("manager", manager_backend);
    registry.register("planner", planner_backend);
    registry.register("writer", writer_backend);

    let orchestrator = Orchestrator::new("manager", registry);
    let result = orchestrator
        .run(
            "session-root",
            "manager prompt",
            vec![
                WorkerTask::new("planner", "planner prompt"),
                WorkerTask::new("writer", "writer prompt"),
            ],
        )
        .await
        .expect("orchestration should succeed");

    assert_eq!(result.manager.response, "mgr:manager:manager prompt");
    assert_eq!(
        result.workers.iter().map(|worker| worker.response.as_str()).collect::<Vec<_>>(),
        vec!["plan:planner:planner prompt", "write:writer:writer prompt"]
    );
    assert!(result.summary.contains("manager: mgr:manager:manager prompt"));
    assert!(result.summary.contains("planner: plan:planner:planner prompt"));
    assert!(result.summary.contains("writer: write:writer:writer prompt"));
    assert_eq!(
        manager_log.calls(),
        vec![("session-root::manager".to_string(), "manager prompt".to_string(), None,)]
    );
    assert_eq!(
        planner_log.calls(),
        vec![("session-root::worker::planner".to_string(), "planner prompt".to_string(), None,)]
    );
    assert_eq!(
        writer_log.calls(),
        vec![("session-root::worker::writer".to_string(), "writer prompt".to_string(), None,)]
    );
}

#[tokio::test]
async fn run_with_context_passes_system_prompt_to_manager_and_workers() {
    let (manager_backend, manager_log) = mock_backend("manager", "mgr");
    let (worker_backend, worker_log) = mock_backend("worker", "wrk");

    let mut registry = OrchestrationProfileRegistry::new();
    registry.register("manager", manager_backend);
    registry.register("worker", worker_backend);

    let orchestrator = Orchestrator::new("manager", registry);
    let result = orchestrator
        .run_with_context(
            "ctx-1",
            "manager prompt",
            RequestContext {
                system_prompt: Some("Known user profile".to_string()),
                selected_skills: vec!["planner".to_string()],
                tool_policy: bob_core::types::RequestToolPolicy::default(),
            },
            vec![WorkerTask::new("worker", "worker prompt")],
        )
        .await;
    assert!(result.is_ok(), "orchestration should succeed");

    assert_eq!(
        manager_log.calls(),
        vec![(
            "ctx-1::manager".to_string(),
            "manager prompt".to_string(),
            Some("Known user profile".to_string()),
        )]
    );
    assert_eq!(
        worker_log.calls(),
        vec![(
            "ctx-1::worker::worker".to_string(),
            "worker prompt".to_string(),
            Some("Known user profile".to_string()),
        )]
    );
}

#[tokio::test]
async fn missing_profiles_surface_as_errors() {
    let (manager_backend, _) = mock_backend("manager", "mgr");
    let mut registry = OrchestrationProfileRegistry::new();
    registry.register("manager", manager_backend);

    let orchestrator = Orchestrator::new("manager", registry);
    let error = orchestrator
        .run("run-7", "manager prompt", vec![WorkerTask::new("worker", "worker prompt")])
        .await
        .expect_err("missing worker profile should fail");

    match error {
        OrchestrationError::MissingProfile { profile_name } => {
            assert_eq!(profile_name, "worker");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}
