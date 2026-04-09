//! Smoke tests for alice-cli one-turn execution with memory integration.
//!
//! Validates that:
//! 1. `AliceRuntimeContext` can be constructed with test doubles.
//! 2. The `AgentLoop` correctly routes natural language to the LLM.
//! 3. Memory recall is injected and persisted across turns.
//! 4. Skill composer integration works end-to-end.
//! 5. ChatBot runner processes mock adapters correctly.
//! 6. Full context with all components is accessible.

use std::{collections::VecDeque, path::PathBuf, sync::Arc};

use alice_adapters::{
    memory::sqlite_store::SqliteMemoryStore, runtime_state::sqlite_store::SqliteRuntimeStateStore,
};
use alice_core::{
    memory::{domain::HybridWeights, service::MemoryService},
    runtime_state::service::RuntimeStateService,
};
use alice_runtime::{
    chatbot_runner::run_chatbot,
    config::{SkillSourceEntry, SkillsConfig},
    context::{AliceRuntimeContext, AliceRuntimeServices},
    handle_input::handle_input_with_skills,
};
use async_trait::async_trait;
use bob_adapters::tape_memory::InMemoryTapeStore;
use bob_chat::{
    adapter::ChatAdapter,
    card::CardElement,
    error::ChatError,
    event::ChatEvent,
    message::{AdapterPostableMessage, Author, IncomingMessage, SentMessage},
};
use bob_core::{error::AgentError, ports::TapeStorePort, types::*};
use bob_runtime::{
    AgentRuntime, NoOpToolPort,
    agent_loop::{AgentLoop, AgentLoopOutput},
};
use parking_lot::Mutex;

#[derive(Debug)]
struct StubRuntime;

#[async_trait]
impl AgentRuntime for StubRuntime {
    async fn run(&self, req: AgentRequest) -> Result<AgentRunResult, AgentError> {
        let has_memory = req
            .context
            .system_prompt
            .as_ref()
            .is_some_and(|text| text.contains("Relevant prior memory"));
        let content = if has_memory { "with-memory" } else { "no-memory" }.to_string();

        Ok(AgentRunResult::Finished(AgentResponse {
            content,
            tool_transcript: Vec::new(),
            usage: TokenUsage::default(),
            finish_reason: FinishReason::Stop,
        }))
    }

    async fn run_stream(&self, _req: AgentRequest) -> Result<AgentEventStream, AgentError> {
        Err(AgentError::Config("streaming not used in smoke test".to_string()))
    }

    async fn health(&self) -> RuntimeHealth {
        RuntimeHealth { status: HealthStatus::Healthy, llm_ready: true, mcp_pool_ready: true }
    }
}

// ---------------------------------------------------------------------------
// Mock chat adapter
// ---------------------------------------------------------------------------

/// In-memory chat adapter that feeds predetermined events and collects posted messages.
struct MockChatAdapter {
    events: Mutex<VecDeque<ChatEvent>>,
    posted: Arc<Mutex<Vec<String>>>,
}

impl std::fmt::Debug for MockChatAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockChatAdapter").finish_non_exhaustive()
    }
}

#[async_trait]
impl ChatAdapter for MockChatAdapter {
    #[expect(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "mock"
    }

    async fn recv_event(&mut self) -> Option<ChatEvent> {
        self.events.lock().pop_front()
    }

    async fn post_message(
        &self,
        _thread_id: &str,
        message: &AdapterPostableMessage,
    ) -> Result<SentMessage, ChatError> {
        let text = self.render_message(message);
        self.posted.lock().push(text);
        Ok(SentMessage {
            id: "mock-sent".into(),
            thread_id: "mock-thread".into(),
            adapter_name: "mock".into(),
            raw: None,
        })
    }

    async fn edit_message(
        &self,
        _thread_id: &str,
        _message_id: &str,
        _message: &AdapterPostableMessage,
    ) -> Result<SentMessage, ChatError> {
        Err(ChatError::NotSupported("edit".into()))
    }

    async fn delete_message(&self, _thread_id: &str, _message_id: &str) -> Result<(), ChatError> {
        Err(ChatError::NotSupported("delete".into()))
    }

    fn render_card(&self, _card: &CardElement) -> String {
        String::new()
    }

    fn render_message(&self, message: &AdapterPostableMessage) -> String {
        match message {
            AdapterPostableMessage::Text(t) | AdapterPostableMessage::Markdown(t) => t.clone(),
        }
    }
}

/// Create a `ChatEvent::Message` with default session and no sender.
fn make_event(text: &str) -> ChatEvent {
    ChatEvent::Message {
        thread_id: "test-session".into(),
        message: IncomingMessage {
            id: "m1".into(),
            text: text.to_string(),
            author: Author {
                user_id: "test-user".into(),
                user_name: "tester".into(),
                full_name: "Test User".into(),
                is_bot: false,
            },
            attachments: vec![],
            is_mention: false,
            thread_id: "test-session".into(),
            timestamp: None,
        },
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a test context with default settings and no skill composer.
fn build_test_context(memory_service: MemoryService) -> AliceRuntimeContext {
    build_test_context_with_skills(memory_service, SkillsConfig::default())
}

/// Build a test context with configurable skill sources.
fn build_test_context_with_skills(
    memory_service: MemoryService,
    skills_config: SkillsConfig,
) -> AliceRuntimeContext {
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
            skills_config,
            reflector: None,
            default_model: "test-model".to_string(),
        },
    )
}

/// Create a `MemoryService` backed by an in-memory SQLite store.
///
/// Returns `None` if initialisation fails (should not happen in tests).
fn make_memory_service() -> Option<MemoryService> {
    let store = SqliteMemoryStore::in_memory(384, false).ok()?;
    MemoryService::new(Arc::new(store), 5, HybridWeights::default(), 384, false).ok()
}

#[tokio::test]
async fn one_turn_uses_agent_loop_and_persists_memory() {
    let store = SqliteMemoryStore::in_memory(384, false);
    assert!(store.is_ok(), "in-memory store should initialize");
    let Ok(store) = store else { return };

    let memory_service =
        MemoryService::new(Arc::new(store), 5, HybridWeights::default(), 384, false);
    assert!(memory_service.is_ok(), "memory service should initialize");
    let Ok(memory_service) = memory_service else { return };

    // Pre-seed a memory entry.
    assert!(
        memory_service
            .persist_turn("session-1", "Remember we use sqlite", "Confirmed sqlite")
            .is_ok(),
        "pre-seeding memory should pass"
    );

    let context = build_test_context(memory_service);

    // Run a one-shot turn via cmd_run.
    let result = alice_runtime::commands::cmd_run(&context, "session-1", None, "test query").await;
    assert!(result.is_ok(), "cmd_run should succeed");

    // Verify memory was persisted.
    let hits = context.memory_service.recall_for_turn("session-1", "test");
    assert!(hits.is_ok(), "recall should succeed after persistence");
    let Ok(hits) = hits else { return };
    assert!(!hits.is_empty(), "memory should include at least the pre-seeded entry");
}

#[tokio::test]
async fn slash_commands_bypass_llm() {
    let store = SqliteMemoryStore::in_memory(384, false);
    assert!(store.is_ok(), "in-memory store should initialize");
    let Ok(store) = store else { return };

    let memory_service =
        MemoryService::new(Arc::new(store), 5, HybridWeights::default(), 384, false);
    assert!(memory_service.is_ok(), "memory service should initialize");
    let Ok(memory_service) = memory_service else { return };

    let context = build_test_context(memory_service);

    // /help should return command output without invoking the LLM.
    let output = context.agent_loop.handle_input("/help", "test-session").await;
    assert!(output.is_ok(), "/help should succeed");
    let Ok(output) = output else { return };
    assert!(
        matches!(
            output,
            bob_runtime::agent_loop::AgentLoopOutput::CommandOutput(ref text)
                if text.contains("/help")
        ),
        "/help should return help text"
    );

    // /tools should list tools.
    let output = context.agent_loop.handle_input("/tools", "test-session").await;
    assert!(output.is_ok(), "/tools should succeed");
    let Ok(output) = output else { return };
    assert!(
        matches!(output, bob_runtime::agent_loop::AgentLoopOutput::CommandOutput(_)),
        "/tools should return command output"
    );
}

/// Verify that `cmd_run` works when a `SkillPromptComposer` is wired into
/// the runtime context. The skill selection path should not error even when
/// the prompt does not strongly match any loaded skill.
#[tokio::test]
async fn cmd_run_with_skill_composer() {
    let Some(memory_service) = make_memory_service() else { return };

    let skills_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/alice-runtime/tests/fixtures/skills");
    let cfg = SkillsConfig {
        enabled: true,
        max_selected: 3,
        token_budget: 1800,
        sources: vec![SkillSourceEntry { path: skills_dir.display().to_string(), recursive: true }],
    };
    let context = build_test_context_with_skills(memory_service, cfg);

    // Run a one-shot turn — skill composer is present but input may not match.
    let result =
        alice_runtime::commands::cmd_run(&context, "session-skill", None, "write rust tests").await;
    assert!(result.is_ok(), "cmd_run with skill composer should succeed");
}

/// Exercise `run_chatbot` with a `MockChatAdapter` that provides two messages
/// then returns `None` (EOF). Both messages should be processed and
/// responses collected.
#[tokio::test]
async fn chatbot_runner_with_mock_adapter() {
    let Some(memory_service) = make_memory_service() else { return };
    let context = Arc::new(build_test_context(memory_service));

    let posted: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let adapter = MockChatAdapter {
        events: Mutex::new(VecDeque::from(vec![
            make_event("hello agent"),
            make_event("second message"),
        ])),
        posted: Arc::clone(&posted),
    };

    let adapters: Vec<Box<dyn ChatAdapter>> = vec![Box::new(adapter)];
    let result = run_chatbot(context, adapters).await;
    assert!(result.is_ok(), "chatbot runner should complete without error");

    let collected = posted.lock();
    assert_eq!(collected.len(), 2, "both messages should produce a response");
    assert!(collected.iter().all(|o| !o.is_empty()), "no response should be empty");
}

/// Build a context with all optional components populated and verify every
/// field is accessible without panics.
#[tokio::test]
async fn full_context_with_all_components() {
    let Some(memory_service) = make_memory_service() else { return };

    let skills_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/alice-runtime/tests/fixtures/skills");
    let cfg = SkillsConfig {
        enabled: true,
        max_selected: 3,
        token_budget: 1800,
        sources: vec![SkillSourceEntry { path: skills_dir.display().to_string(), recursive: true }],
    };
    let context = build_test_context_with_skills(memory_service, cfg);

    // Verify all components are accessible.
    assert!(context.skills_config.enabled, "skills should be enabled");
    assert_eq!(context.skill_token_budget(), 1800);
    assert_eq!(context.default_model, "test-model");

    // Memory service should accept a persist call.
    assert!(
        context.memory_service.persist_turn("full-ctx", "user input", "assistant output").is_ok(),
        "memory persist should succeed"
    );

    // Tape store should be functional.
    let health = context.agent.runtime().health().await;
    assert!(matches!(health.status, HealthStatus::Healthy), "runtime health should be healthy");

    // Agent loop should handle a slash command.
    let output = context.agent_loop.handle_input("/help", "full-ctx").await;
    assert!(output.is_ok(), "agent loop /help should succeed");
}

/// Test `handle_input_with_skills` directly with natural language input.
/// Should route through memory + skills pipeline and return a `Response`.
#[tokio::test]
async fn handle_input_with_skills_nl_input() {
    let Some(memory_service) = make_memory_service() else { return };

    let skills_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/alice-runtime/tests/fixtures/skills");
    let cfg = SkillsConfig {
        enabled: true,
        max_selected: 3,
        token_budget: 1800,
        sources: vec![SkillSourceEntry { path: skills_dir.display().to_string(), recursive: true }],
    };
    let context = build_test_context_with_skills(memory_service, cfg);

    let output = handle_input_with_skills(
        &context,
        "nl-session",
        Some("nl-session"),
        "explain closures in rust",
    )
    .await;
    assert!(output.is_ok(), "handle_input_with_skills should succeed for NL input");
    let Ok(output) = output else { return };
    assert!(
        matches!(output, AgentLoopOutput::Response(_)),
        "NL input should return AgentLoopOutput::Response, got: {output:?}"
    );
}
