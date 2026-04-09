//! Turn execution helpers with memory recall/writeback and skill injection.

use bob_core::types::{RequestContext, RequestToolPolicy};
use bob_runtime::AgentResponse;

use crate::context::AliceRuntimeContext;

/// Build a request context with memory recall, user profile context, and skills.
#[must_use]
pub fn build_request_context(
    context: &AliceRuntimeContext,
    session_id: &str,
    profile_id: Option<&str>,
    input: &str,
) -> RequestContext {
    let effective_profile_id = profile_id.unwrap_or(session_id);

    let recalled = match context.memory_service().recall_for_turn(session_id, input) {
        Ok(hits) => hits,
        Err(error) => {
            tracing::warn!("memory recall failed: {error}");
            Vec::new()
        }
    };
    let memory_prompt =
        alice_core::memory::service::MemoryService::render_recall_context(&recalled);

    let profile_prompt = match context.memory_service().load_user_profile(effective_profile_id) {
        Ok(profile) => profile
            .as_ref()
            .and_then(alice_core::memory::service::MemoryService::render_user_profile_context),
        Err(error) => {
            tracing::warn!("user profile load failed: {error}");
            None
        }
    };

    let skills_bundle =
        match crate::skill_wiring::render_skills_context(context.skills_config(), input) {
            Ok(bundle) => bundle,
            Err(error) => {
                tracing::warn!("skill rendering failed: {error}");
                None
            }
        };

    let mut system_parts = Vec::new();
    if let Some(ref profile) = profile_prompt {
        system_parts.push(profile.as_str());
    }
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

    RequestContext { system_prompt, selected_skills, tool_policy }
}

/// Persist turn memory, update long-term user profile, and spawn reflection.
pub fn persist_turn_side_effects(
    context: &AliceRuntimeContext,
    session_id: &str,
    profile_id: &str,
    user_input: &str,
    assistant_output: &str,
) {
    if let Err(error) =
        context.memory_service().persist_turn(session_id, user_input, assistant_output)
    {
        tracing::warn!("memory persistence failed: {error}");
    }
    if let Err(error) =
        context.memory_service().update_profile_from_turn(profile_id, user_input, assistant_output)
    {
        tracing::warn!("user profile update failed: {error}");
    }
    if let Some(reflector) = context.reflector().cloned() {
        let session_id = session_id.to_string();
        let profile_id = profile_id.to_string();
        let user_input = user_input.to_string();
        let assistant_output = assistant_output.to_string();
        tokio::spawn(async move {
            if let Err(error) = reflector
                .reflect_and_persist(&session_id, &profile_id, &user_input, &assistant_output)
                .await
            {
                tracing::warn!("reflection failed: {error}");
            }
        });
    }
}

/// Execute one turn with memory-aware + skill-augmented prompt context.
///
/// This function is primarily used for CLI commands and background scheduler
/// tasks that need direct agent backend access.
///
/// # Errors
///
/// Returns an error if the agent runtime fails to execute the turn.
pub async fn run_turn_with_memory(
    context: &AliceRuntimeContext,
    session_id: &str,
    profile_id: Option<&str>,
    input: &str,
) -> eyre::Result<AgentResponse> {
    let effective_profile_id = profile_id.unwrap_or(session_id);
    let request_context =
        build_request_context(context, session_id, Some(effective_profile_id), input);

    let session = context.backend().create_session_with_id(session_id);
    let response = session.chat(input, request_context).await?;

    persist_turn_side_effects(context, session_id, effective_profile_id, input, &response.content);

    Ok(response)
}
