//! Turn execution helpers with memory recall/writeback and skill injection.

use bob_core::types::{RequestContext, RequestToolPolicy};
use bob_runtime::AgentResponse;

use crate::context::AliceRuntimeContext;

/// Execute one turn with memory-aware + skill-augmented prompt context.
///
/// # Errors
///
/// Returns an error if the agent runtime fails to execute the turn.
pub async fn run_turn_with_memory(
    context: &AliceRuntimeContext,
    session_id: &str,
    input: &str,
) -> eyre::Result<AgentResponse> {
    // 1. Memory recall
    let recalled = match context.memory_service.recall_for_turn(session_id, input) {
        Ok(hits) => hits,
        Err(error) => {
            tracing::warn!("memory recall failed: {error}");
            Vec::new()
        }
    };
    let memory_prompt =
        alice_core::memory::service::MemoryService::render_recall_context(&recalled);

    // 2. Skill injection
    let skills_bundle = context.skill_composer.as_ref().map(|composer| {
        crate::skill_wiring::inject_skills_context(composer, input, context.skill_token_budget)
    });

    // 3. Compose system prompt: memory + skills
    let mut system_parts = Vec::new();
    if let Some(ref mem) = memory_prompt {
        system_parts.push(mem.as_str());
    }
    if let Some(ref bundle) = skills_bundle &&
        !bundle.prompt.is_empty()
    {
        system_parts.push(&bundle.prompt);
    }
    let system_prompt =
        if system_parts.is_empty() { None } else { Some(system_parts.join("\n\n")) };

    // 4. Build request context with skills metadata
    let (selected_skills, tool_policy) = if let Some(ref bundle) = skills_bundle {
        let policy = if bundle.selected_allowed_tools.is_empty() {
            RequestToolPolicy::default()
        } else {
            RequestToolPolicy {
                allow_tools: Some(bundle.selected_allowed_tools.clone()),
                ..RequestToolPolicy::default()
            }
        };
        (bundle.selected_skill_names.clone(), policy)
    } else {
        (Vec::new(), RequestToolPolicy::default())
    };

    let request_context = RequestContext { system_prompt, selected_skills, tool_policy };

    // 5. Execute turn via agent backend
    let session = context.backend.create_session_with_id(session_id);
    let response = session.chat(input, request_context).await?;

    // 6. Persist memory
    if let Err(error) = context.memory_service.persist_turn(session_id, input, &response.content) {
        tracing::warn!("memory persistence failed: {error}");
    }

    Ok(response)
}
