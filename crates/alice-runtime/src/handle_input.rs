//! Unified input handler with slash command routing, skill injection, and memory.

use bob_runtime::agent_loop::AgentLoopOutput;

use crate::context::AliceRuntimeContext;

/// Handle a single user input with full pipeline: slash commands, skills, memory.
///
/// For slash commands: delegates to `AgentLoop` for deterministic handling.
/// For natural language: uses `AgentLoop::handle_input_with_context` with injected memory and
/// skills.
///
/// # Errors
///
/// Returns an error if the agent runtime or agent loop fails.
pub async fn handle_input_with_skills(
    context: &AliceRuntimeContext,
    session_id: &str,
    input: &str,
) -> eyre::Result<AgentLoopOutput> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(AgentLoopOutput::CommandOutput(String::new()));
    }

    // Route: slash command or natural language
    match bob_runtime::router::route(trimmed) {
        bob_runtime::router::RouteResult::SlashCommand(_) => {
            // Delegate slash commands to AgentLoop for deterministic handling
            let output = context.agent_loop().handle_input(trimmed, session_id).await?;
            Ok(output)
        }
        bob_runtime::router::RouteResult::NaturalLanguage(_) => {
            // NL input: inject memory + skills into RequestContext for AgentLoop
            let recalled = match context.memory_service().recall_for_turn(session_id, trimmed) {
                Ok(hits) => hits,
                Err(error) => {
                    tracing::warn!("memory recall failed: {error}");
                    Vec::new()
                }
            };
            let memory_prompt =
                alice_core::memory::service::MemoryService::render_recall_context(&recalled);

            let skills_bundle = context.skill_composer().map(|composer| {
                crate::skill_wiring::inject_skills_context(
                    composer,
                    trimmed,
                    context.skill_token_budget(),
                )
            });

            // Compose system prompt: memory + skills
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

            // Build request context with skills metadata
            let (selected_skills, tool_policy) = if let Some(ref bundle) = skills_bundle {
                let policy = if bundle.selected_allowed_tools.is_empty() {
                    bob_core::types::RequestToolPolicy::default()
                } else {
                    bob_core::types::RequestToolPolicy {
                        allow_tools: Some(bundle.selected_allowed_tools.clone()),
                        ..bob_core::types::RequestToolPolicy::default()
                    }
                };
                (bundle.selected_skill_names.clone(), policy)
            } else {
                (Vec::new(), bob_core::types::RequestToolPolicy::default())
            };

            let request_context =
                bob_core::types::RequestContext { system_prompt, selected_skills, tool_policy };

            // Use AgentLoop.handle_input_with_context for per-request context injection
            let output = context
                .agent_loop()
                .handle_input_with_context(trimmed, session_id, request_context)
                .await?;

            // Handle memory persistence (AgentLoop doesn't do this automatically)
            if let AgentLoopOutput::Response(response) = &output {
                let bob_core::types::AgentRunResult::Finished(finished) = response;
                if let Err(error) =
                    context.memory_service().persist_turn(session_id, trimmed, &finished.content)
                {
                    tracing::warn!("memory persistence failed: {error}");
                }
            }

            Ok(output)
        }
    }
}

/// Extract displayable text from an `AgentLoopOutput`.
pub fn output_to_text(output: &AgentLoopOutput) -> Option<&str> {
    match output {
        AgentLoopOutput::Response(bob_core::types::AgentRunResult::Finished(resp)) => {
            Some(&resp.content)
        }
        AgentLoopOutput::CommandOutput(text) => Some(text),
        AgentLoopOutput::Quit => None,
    }
}
