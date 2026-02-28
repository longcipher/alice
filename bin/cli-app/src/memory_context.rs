//! Turn execution helpers with memory recall/writeback integration.

use bob_core::types::{AgentRequest, AgentResponse, AgentRunResult, RequestContext};

use crate::bootstrap::AliceRuntimeContext;

/// Execute one turn with memory-aware prompt context.
pub async fn run_turn_with_memory(
    context: &AliceRuntimeContext,
    session_id: &str,
    input: &str,
) -> eyre::Result<AgentResponse> {
    let recalled = match context.memory_service.recall_for_turn(session_id, input) {
        Ok(hits) => hits,
        Err(error) => {
            tracing::warn!("memory recall failed: {error}");
            Vec::new()
        }
    };
    let system_prompt = common::memory::service::MemoryService::render_recall_context(&recalled);

    let request = AgentRequest {
        input: input.to_string(),
        session_id: session_id.to_string(),
        model: Some(context.default_model.clone()),
        context: RequestContext { system_prompt, ..RequestContext::default() },
        cancel_token: None,
    };

    let result = context.runtime.run(request).await?;
    match result {
        AgentRunResult::Finished(response) => {
            if let Err(error) =
                context.memory_service.persist_turn(session_id, input, &response.content)
            {
                tracing::warn!("memory persistence failed: {error}");
            }
            Ok(response)
        }
    }
}
