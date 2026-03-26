//! Unified input handler with slash command routing, skill injection, and memory.

use bob_runtime::agent_loop::AgentLoopOutput;

use crate::{context::AliceRuntimeContext, memory_context::run_turn_with_memory};

/// Handle a single user input with full pipeline: slash commands, skills, memory.
///
/// For slash commands: delegates to `AgentLoop` for deterministic handling.
/// For natural language: runs through `run_turn_with_memory` with skill injection.
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
            let output = context.agent_loop.handle_input(trimmed, session_id).await?;
            Ok(output)
        }
        bob_runtime::router::RouteResult::NaturalLanguage(_) => {
            // NL input: memory + skills + runtime via agent backend
            let response = run_turn_with_memory(context, session_id, trimmed).await?;
            if response.is_quit {
                return Ok(AgentLoopOutput::Quit);
            }
            // Convert bob_runtime::AgentResponse → AgentLoopOutput
            Ok(AgentLoopOutput::CommandOutput(response.content))
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
