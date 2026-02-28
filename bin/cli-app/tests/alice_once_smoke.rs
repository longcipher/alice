//! Smoke test for one-turn execution with memory integration.

use std::sync::Arc;

use async_trait::async_trait;
use bob_core::{error::AgentError, types::*};
use bob_runtime::AgentRuntime;
use cli_app::{bootstrap::AliceRuntimeContext, memory_context::run_turn_with_memory};
use common::memory::{
    domain::HybridWeights, service::MemoryService, sqlite_store::SqliteMemoryStore,
};

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

#[tokio::test]
async fn one_turn_uses_memory_context_and_persists_output() {
    let store = SqliteMemoryStore::in_memory(384, false);
    assert!(store.is_ok(), "in-memory store should initialize");
    let Ok(store) = store else {
        return;
    };

    let memory_service =
        MemoryService::new(Arc::new(store), 5, HybridWeights::default(), 384, false);
    assert!(memory_service.is_ok(), "memory service should initialize");
    let Ok(memory_service) = memory_service else {
        return;
    };

    assert!(
        memory_service
            .persist_turn("session-1", "Remember we use sqlite", "Confirmed sqlite")
            .is_ok(),
        "pre-seeding memory should pass"
    );

    let context = AliceRuntimeContext {
        runtime: Arc::new(StubRuntime),
        memory_service: Arc::new(memory_service),
        default_model: "test-model".to_string(),
    };

    let response = run_turn_with_memory(&context, "session-1", "sqlite").await;
    assert!(response.is_ok(), "turn execution should succeed");
    let Ok(response) = response else {
        return;
    };
    assert_eq!(response.content, "with-memory");

    let hits = context.memory_service.recall_for_turn("session-1", "sqlite");
    assert!(hits.is_ok(), "recall should succeed after persistence");
    let Ok(hits) = hits else {
        return;
    };
    assert!(hits.len() >= 2, "memory should include pre-seed and persisted turn");
}
