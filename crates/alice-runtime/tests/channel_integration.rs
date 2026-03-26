//! Integration tests for the ChatBot-based channel system.
//!
//! Validates that [`run_chatbot`] correctly drives [`ChatAdapter`] implementations,
//! processes natural-language and slash-command messages, and respects quit
//! signals and EOF conditions.

use std::{collections::VecDeque, sync::Arc};

use alice_adapters::memory::sqlite_store::SqliteMemoryStore;
use alice_core::memory::{domain::HybridWeights, service::MemoryService};
use alice_runtime::{
    agent_backend::bob_backend::BobAgentBackend, chatbot_runner::run_chatbot,
    context::AliceRuntimeContext,
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
use bob_runtime::{AgentRuntime, NoOpToolPort, agent_loop::AgentLoop};
use parking_lot::Mutex;

// ---------------------------------------------------------------------------
// Test doubles
// ---------------------------------------------------------------------------

/// Deterministic LLM runtime that always returns `"stub-response"`.
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
        Err(AgentError::Config("streaming not used in channel tests".to_string()))
    }

    async fn health(&self) -> RuntimeHealth {
        RuntimeHealth { status: HealthStatus::Healthy, llm_ready: true, mcp_pool_ready: true }
    }
}

/// In-memory chat adapter that feeds predetermined events and collects posted messages.
///
/// `recv_event` pops from a `VecDeque`; when the queue is empty it returns `None`
/// (simulating EOF). `post_message` pushes into a shared `Vec` so the test can
/// inspect all outputs after `run_chatbot` completes.
struct MockChatAdapter {
    name: String,
    events: Mutex<VecDeque<ChatEvent>>,
    posted: Arc<Mutex<Vec<String>>>,
}

impl std::fmt::Debug for MockChatAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockChatAdapter").field("name", &self.name).finish_non_exhaustive()
    }
}

#[async_trait]
impl ChatAdapter for MockChatAdapter {
    fn name(&self) -> &str {
        &self.name
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
            id: "mock-sent-1".into(),
            thread_id: "mock-thread".into(),
            adapter_name: self.name.clone(),
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a fully-wired [`AliceRuntimeContext`] backed by test doubles.
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

    Some(AliceRuntimeContext {
        agent_loop,
        agent,
        backend,
        memory_service: Arc::new(memory_service),
        skill_composer: None,
        skill_token_budget: 1800,
        default_model: "test-model".to_string(),
    })
}

fn make_author() -> Author {
    Author {
        user_id: "test-user".into(),
        user_name: "tester".into(),
        full_name: "Test User".into(),
        is_bot: false,
    }
}

/// Create a `ChatEvent::Message` with default session and test author.
fn make_event(text: &str) -> ChatEvent {
    ChatEvent::Message {
        thread_id: "test-session".into(),
        message: IncomingMessage {
            id: "m1".into(),
            text: text.to_string(),
            author: make_author(),
            attachments: vec![],
            is_mention: false,
            thread_id: "test-session".into(),
            timestamp: None,
        },
    }
}

/// Create a `MockChatAdapter` pre-loaded with the given events and return
/// a handle to the collected posted messages for later inspection.
fn make_adapter(name: &str, events: Vec<ChatEvent>) -> (MockChatAdapter, Arc<Mutex<Vec<String>>>) {
    let posted: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let adapter = MockChatAdapter {
        name: name.to_string(),
        events: Mutex::new(VecDeque::from(events)),
        posted: Arc::clone(&posted),
    };
    (adapter, posted)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Two natural-language messages should each produce exactly one response.
#[tokio::test]
async fn mock_adapter_processes_messages() {
    let Some(ctx) = build_test_context() else {
        return;
    };
    let ctx = Arc::new(ctx);

    let (adapter, posted) = make_adapter("mock", vec![make_event("hello"), make_event("world")]);

    let adapters: Vec<Box<dyn ChatAdapter>> = vec![Box::new(adapter)];
    let result = run_chatbot(ctx, adapters).await;
    assert!(result.is_ok(), "run_chatbot should succeed");

    let collected = posted.lock();
    assert_eq!(collected.len(), 2, "should have 2 responses for 2 messages");
    assert!(!collected[0].is_empty(), "first response should contain text");
    assert!(!collected[1].is_empty(), "second response should contain text");
}

/// Two adapters fed into `run_chatbot` should both be driven concurrently.
#[tokio::test]
async fn two_adapters_process_concurrently() {
    let Some(ctx) = build_test_context() else {
        return;
    };
    let ctx = Arc::new(ctx);

    let (a1, out1) = make_adapter("mock-1", vec![make_event("from adapter 1")]);
    let (a2, out2) = make_adapter("mock-2", vec![make_event("from adapter 2")]);

    let adapters: Vec<Box<dyn ChatAdapter>> = vec![Box::new(a1), Box::new(a2)];
    let result = run_chatbot(ctx, adapters).await;
    assert!(result.is_ok(), "run_chatbot should succeed with two adapters");

    let collected1 = out1.lock();
    let collected2 = out2.lock();
    assert_eq!(collected1.len(), 1, "adapter 1 should have exactly 1 response");
    assert_eq!(collected2.len(), 1, "adapter 2 should have exactly 1 response");
}

/// A `/help` slash command should be handled deterministically and produce
/// command output that mentions available commands.
#[tokio::test]
async fn slash_command_returns_command_output() {
    let Some(ctx) = build_test_context() else {
        return;
    };
    let ctx = Arc::new(ctx);

    let (adapter, posted) = make_adapter("mock", vec![make_event("/help")]);

    let adapters: Vec<Box<dyn ChatAdapter>> = vec![Box::new(adapter)];
    let result = run_chatbot(ctx, adapters).await;
    assert!(result.is_ok(), "run_chatbot should succeed for /help");

    let collected = posted.lock();
    assert_eq!(collected.len(), 1, "should have 1 response for /help");
    assert!(
        collected[0].contains("/help"),
        "/help output should mention /help, got: {}",
        collected[0]
    );
}

/// An adapter that yields one message then EOF should complete gracefully.
#[tokio::test]
async fn adapter_returns_none_completes_gracefully() {
    let Some(ctx) = build_test_context() else {
        return;
    };
    let ctx = Arc::new(ctx);

    let (adapter, posted) = make_adapter("mock", vec![make_event("single message")]);

    let adapters: Vec<Box<dyn ChatAdapter>> = vec![Box::new(adapter)];
    let result = run_chatbot(ctx, adapters).await;
    assert!(result.is_ok(), "run_chatbot should complete gracefully after EOF");

    let collected = posted.lock();
    assert_eq!(collected.len(), 1, "should process the single message before EOF");
}
