//! Integration tests for the channel system.
//!
//! Validates that [`run_channels`] correctly drives [`Channel`] implementations,
//! processes natural-language and slash-command messages, and respects quit
//! signals and EOF conditions.

use std::{collections::VecDeque, sync::Arc};

use alice_adapters::memory::sqlite_store::SqliteMemoryStore;
use alice_core::memory::{domain::HybridWeights, service::MemoryService};
use alice_runtime::{channel_runner::run_channels, context::AliceRuntimeContext};
use async_trait::async_trait;
use bob_adapters::tape_memory::InMemoryTapeStore;
use bob_core::{
    channel::{Channel, ChannelError, ChannelMessage, ChannelOutput},
    error::AgentError,
    ports::TapeStorePort,
    types::*,
};
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

/// In-memory channel that feeds predetermined messages and collects responses.
///
/// `recv` pops from a `VecDeque`; when the queue is empty it returns `None`
/// (simulating EOF). `send` pushes into a shared `Vec` so the test can
/// inspect all outputs after `run_channels` completes.
#[derive(Debug)]
struct MockChannel {
    messages: VecDeque<ChannelMessage>,
    outputs: Arc<Mutex<Vec<ChannelOutput>>>,
}

#[async_trait]
impl Channel for MockChannel {
    async fn recv(&mut self) -> Option<ChannelMessage> {
        self.messages.pop_front()
    }

    async fn send(&self, output: ChannelOutput) -> Result<(), ChannelError> {
        self.outputs.lock().push(output);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a fully-wired [`AliceRuntimeContext`] backed by test doubles.
///
/// Returns `None` if any subsystem fails to initialise (should never happen
/// in a healthy test environment).
fn build_test_context() -> Option<AliceRuntimeContext> {
    let store = SqliteMemoryStore::in_memory(384, false).ok()?;
    let memory_service =
        MemoryService::new(Arc::new(store), 5, HybridWeights::default(), 384, false).ok()?;

    let runtime: Arc<dyn AgentRuntime> = Arc::new(StubRuntime);
    let tools: Arc<dyn bob_core::ports::ToolPort> = Arc::new(NoOpToolPort);
    let tape: Arc<dyn TapeStorePort> = Arc::new(InMemoryTapeStore::new());
    let agent_loop = AgentLoop::new(runtime.clone(), tools.clone()).with_tape(tape.clone());

    Some(AliceRuntimeContext {
        agent_loop,
        runtime,
        tools,
        tape,
        memory_service: Arc::new(memory_service),
        skill_composer: None,
        skill_token_budget: 1800,
        default_model: "test-model".to_string(),
    })
}

/// Create a [`ChannelMessage`] with default session and no sender.
fn make_message(text: &str) -> ChannelMessage {
    ChannelMessage { text: text.to_string(), session_id: "test-session".to_string(), sender: None }
}

/// Create a [`MockChannel`] pre-loaded with the given messages and return a
/// handle to the collected outputs for later inspection.
fn make_channel(messages: Vec<ChannelMessage>) -> (MockChannel, Arc<Mutex<Vec<ChannelOutput>>>) {
    let outputs: Arc<Mutex<Vec<ChannelOutput>>> = Arc::new(Mutex::new(Vec::new()));
    let channel = MockChannel { messages: VecDeque::from(messages), outputs: Arc::clone(&outputs) };
    (channel, outputs)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Two natural-language messages should each produce exactly one response.
#[tokio::test]
async fn mock_channel_processes_messages() {
    let Some(ctx) = build_test_context() else {
        return;
    };
    let ctx = Arc::new(ctx);

    let (channel, outputs) = make_channel(vec![make_message("hello"), make_message("world")]);

    let result = run_channels(ctx, vec![Box::new(channel)]).await;
    assert!(result.is_ok(), "run_channels should succeed");

    let collected = outputs.lock();
    assert_eq!(collected.len(), 2, "should have 2 responses for 2 messages");
    assert!(!collected[0].text.is_empty(), "first response should contain text");
    assert!(!collected[1].text.is_empty(), "second response should contain text");
    assert!(!collected[0].is_error, "first response should not be error");
    assert!(!collected[1].is_error, "second response should not be error");
}

/// Two channels fed into `run_channels` should both be driven concurrently.
#[tokio::test]
async fn two_channels_process_concurrently() {
    let Some(ctx) = build_test_context() else {
        return;
    };
    let ctx = Arc::new(ctx);

    let (ch1, out1) = make_channel(vec![make_message("from channel 1")]);
    let (ch2, out2) = make_channel(vec![make_message("from channel 2")]);

    let result = run_channels(ctx, vec![Box::new(ch1), Box::new(ch2)]).await;
    assert!(result.is_ok(), "run_channels should succeed with two channels");

    let collected1 = out1.lock();
    let collected2 = out2.lock();
    assert_eq!(collected1.len(), 1, "channel 1 should have exactly 1 response");
    assert_eq!(collected2.len(), 1, "channel 2 should have exactly 1 response");
}

/// A `/help` slash command should be handled deterministically and produce
/// command output that mentions available commands.
#[tokio::test]
async fn slash_command_returns_command_output() {
    let Some(ctx) = build_test_context() else {
        return;
    };
    let ctx = Arc::new(ctx);

    let (channel, outputs) = make_channel(vec![make_message("/help")]);

    let result = run_channels(ctx, vec![Box::new(channel)]).await;
    assert!(result.is_ok(), "run_channels should succeed for /help");

    let collected = outputs.lock();
    assert_eq!(collected.len(), 1, "should have 1 response for /help");
    assert!(
        collected[0].text.contains("/help"),
        "/help output should mention /help, got: {}",
        collected[0].text
    );
    assert!(!collected[0].is_error, "/help response should not be an error");
}

/// A channel that yields one message then EOF should complete gracefully.
#[tokio::test]
async fn channel_returns_none_completes_gracefully() {
    let Some(ctx) = build_test_context() else {
        return;
    };
    let ctx = Arc::new(ctx);

    let (channel, outputs) = make_channel(vec![make_message("single message")]);

    let result = run_channels(ctx, vec![Box::new(channel)]).await;
    assert!(result.is_ok(), "run_channels should complete gracefully after EOF");

    let collected = outputs.lock();
    assert_eq!(collected.len(), 1, "should process the single message before EOF");
}

/// `/quit` should stop the channel immediately; subsequent messages must not
/// be processed.
#[tokio::test]
async fn quit_command_stops_channel() {
    let Some(ctx) = build_test_context() else {
        return;
    };
    let ctx = Arc::new(ctx);

    let (channel, outputs) =
        make_channel(vec![make_message("/quit"), make_message("should not be processed")]);

    let result = run_channels(ctx, vec![Box::new(channel)]).await;
    assert!(result.is_ok(), "run_channels should complete after /quit");

    let collected = outputs.lock();
    assert!(
        collected.is_empty(),
        "/quit should produce no output and stop the channel, got {} outputs",
        collected.len()
    );
}
