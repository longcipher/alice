//! Integration tests for user profile prompt injection and learned-skill reflection.

use std::{path::PathBuf, sync::Arc};

use alice_adapters::{
    memory::sqlite_store::SqliteMemoryStore, runtime_state::sqlite_store::SqliteRuntimeStateStore,
};
use alice_core::{
    memory::{domain::HybridWeights, service::MemoryService},
    runtime_state::service::RuntimeStateService,
};
use alice_runtime::{
    agent_backend::{AgentBackend, AgentSession},
    config::{ReflectionConfig, SkillsConfig},
    context::{AliceRuntimeContext, AliceRuntimeServices},
    handle_input::handle_input_with_skills,
    reflection::AgentReflector,
};
use async_trait::async_trait;
use bob_adapters::tape_memory::InMemoryTapeStore;
use bob_core::{
    error::AgentError,
    ports::TapeStorePort,
    types::{
        AgentRequest, AgentResponse as CoreAgentResponse, AgentRunResult, FinishReason,
        HealthStatus, RequestContext, RuntimeHealth, TokenUsage,
    },
};
use bob_runtime::{
    AgentResponse as RuntimeAgentResponse, AgentRuntime, NoOpToolPort,
    agent_loop::{AgentLoop, AgentLoopOutput},
};

#[derive(Debug)]
struct StubRuntime;

#[async_trait]
impl AgentRuntime for StubRuntime {
    async fn run(&self, req: AgentRequest) -> Result<AgentRunResult, AgentError> {
        let has_profile = req
            .context
            .system_prompt
            .as_ref()
            .is_some_and(|text| text.contains("Known user profile"));

        let content = if has_profile { "with-profile" } else { "no-profile" }.to_string();
        Ok(AgentRunResult::Finished(CoreAgentResponse {
            content,
            tool_transcript: Vec::new(),
            usage: TokenUsage::default(),
            finish_reason: FinishReason::Stop,
        }))
    }

    async fn run_stream(
        &self,
        _req: AgentRequest,
    ) -> Result<bob_core::types::AgentEventStream, AgentError> {
        Err(AgentError::Config("streaming not used in tests".to_string()))
    }

    async fn health(&self) -> RuntimeHealth {
        RuntimeHealth { status: HealthStatus::Healthy, llm_ready: true, mcp_pool_ready: true }
    }
}

#[derive(Debug, Clone)]
struct MockReflectionBackend {
    response: String,
}

#[derive(Debug)]
struct MockReflectionSession {
    response: String,
}

#[async_trait]
impl AgentSession for MockReflectionSession {
    async fn chat(
        &self,
        _input: &str,
        _context: RequestContext,
    ) -> eyre::Result<RuntimeAgentResponse> {
        Ok(RuntimeAgentResponse::new(
            self.response.clone(),
            TokenUsage::default(),
            FinishReason::Stop,
        ))
    }
}

impl AgentBackend for MockReflectionBackend {
    fn create_session(&self) -> Arc<dyn AgentSession> {
        Arc::new(MockReflectionSession { response: self.response.clone() })
    }

    fn create_session_with_id(&self, _session_id: &str) -> Arc<dyn AgentSession> {
        self.create_session()
    }
}

fn build_test_context(memory_service: MemoryService) -> AliceRuntimeContext {
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

    let backend: Arc<dyn AgentBackend> =
        Arc::new(alice_runtime::agent_backend::bob_backend::BobAgentBackend::new(agent.clone()));
    let runtime_state_store = SqliteRuntimeStateStore::in_memory();
    assert!(runtime_state_store.is_ok(), "runtime-state store should initialize");
    let Ok(runtime_state_store) = runtime_state_store else {
        panic!("runtime-state store should initialize");
    };
    let runtime_state_service = RuntimeStateService::new(Arc::new(runtime_state_store));
    assert!(runtime_state_service.is_ok(), "runtime-state service should initialize");
    let Ok(runtime_state_service) = runtime_state_service else {
        panic!("runtime-state service should initialize");
    };

    AliceRuntimeContext::new(
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
    )
}

fn make_memory_service() -> Option<MemoryService> {
    let store = SqliteMemoryStore::in_memory(384, false).ok()?;
    MemoryService::new(Arc::new(store), 5, HybridWeights::default(), 384, false).ok()
}

#[tokio::test]
async fn handle_input_injects_known_user_profile_context() {
    let Some(memory_service) = make_memory_service() else {
        return;
    };
    let Ok(Some(_)) = memory_service.update_profile_from_turn(
        "user-1",
        "I prefer Rust for agent runtimes and our Alice project relies on ACP.",
        "Acknowledged.",
    ) else {
        return;
    };

    let context = build_test_context(memory_service);
    let output =
        handle_input_with_skills(&context, "session-1", Some("user-1"), "What should we do next?")
            .await;
    assert!(output.is_ok(), "turn execution should succeed");
    let Ok(output) = output else {
        return;
    };

    match output {
        AgentLoopOutput::Response(AgentRunResult::Finished(response)) => {
            assert_eq!(response.content, "with-profile");
        }
        other => panic!("unexpected output: {other:?}"),
    }
}

#[tokio::test]
async fn reflector_materializes_generated_skill_file() {
    let temp_dir = std::env::temp_dir().join(format!(
        "alice-reflector-test-{}-{}",
        std::process::id(),
        "skill"
    ));
    let created = std::fs::create_dir_all(&temp_dir);
    assert!(created.is_ok(), "temp dir should exist: {created:?}");
    let backend: Arc<dyn AgentBackend> = Arc::new(MockReflectionBackend {
        response: r#"---
name: alice-session-summaries
description: Capture concise session summary workflows for Alice.
---

# Alice Session Summaries

Summarize completed Alice sessions into compact reusable notes.
"#
        .to_string(),
    });

    let reflector = AgentReflector::new(
        backend,
        &ReflectionConfig { enabled: true, learned_skills_dir: temp_dir.display().to_string() },
    );
    let Some(reflector) = reflector else {
        panic!("enabled reflection should construct a reflector");
    };

    let written = reflector
        .reflect_and_persist(
            "session-9",
            "user-9",
            "Please summarize our Alice rollout.",
            "I summarized the rollout and key follow-up actions.",
        )
        .await;
    assert!(written.is_ok(), "reflection should succeed");
    let Ok(written) = written else {
        return;
    };
    let Some(written) = written else {
        panic!("reflection should persist a learned skill");
    };

    let skill_path = PathBuf::from(&written);
    assert!(skill_path.ends_with("SKILL.md"));
    assert!(skill_path.is_file(), "materialized skill should exist on disk");
}
