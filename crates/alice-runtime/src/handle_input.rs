//! Unified input handler with slash command routing, skill injection, and memory.

use bob_core::types::{AgentResponse, AgentRunResult, FinishReason, TokenUsage};
use bob_runtime::agent_loop::AgentLoopOutput;

use crate::{
    context::AliceRuntimeContext,
    memory_context::{build_request_context, persist_turn_side_effects},
    orchestration::WorkerTask,
};

/// Handle a single user input with full pipeline: slash commands, skills, memory,
/// and optional auto-orchestration.
///
/// For slash commands: delegates to `AgentLoop` for deterministic handling.
/// For natural language: injects memory and skills into the request context, then
/// either routes through the orchestrator or falls back to `AgentLoop`.
///
/// # Errors
///
/// Returns an error if the agent runtime or agent loop fails.
pub async fn handle_input_with_skills(
    context: &AliceRuntimeContext,
    session_id: &str,
    profile_id: Option<&str>,
    input: &str,
) -> eyre::Result<AgentLoopOutput> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(AgentLoopOutput::CommandOutput(String::new()));
    }

    match bob_runtime::router::route(trimmed) {
        bob_runtime::router::RouteResult::SlashCommand(_) => {
            let output = context.agent_loop().handle_input(trimmed, session_id).await?;
            Ok(output)
        }
        bob_runtime::router::RouteResult::NaturalLanguage(_) => {
            let effective_profile_id = profile_id.unwrap_or(session_id);
            let request_context =
                build_request_context(context, session_id, Some(effective_profile_id), trimmed);

            if context.auto_orchestrate() &&
                let Some(orchestrator) = context.orchestrator()
            {
                let worker_profile_names = orchestrator.worker_profile_names();
                if !worker_profile_names.is_empty() {
                    let worker_tasks = worker_profile_names
                        .into_iter()
                        .map(|profile_name| WorkerTask::new(profile_name, trimmed))
                        .collect();
                    let run = orchestrator
                        .run_with_context(session_id, trimmed, request_context, worker_tasks)
                        .await?;

                    persist_turn_side_effects(
                        context,
                        session_id,
                        effective_profile_id,
                        trimmed,
                        &run.summary,
                    );

                    return Ok(AgentLoopOutput::Response(AgentRunResult::Finished(AgentResponse {
                        content: run.summary,
                        tool_transcript: Vec::new(),
                        usage: TokenUsage::default(),
                        finish_reason: FinishReason::Stop,
                    })));
                }
            }

            let output = context
                .agent_loop()
                .handle_input_with_context(trimmed, session_id, request_context)
                .await?;

            if let AgentLoopOutput::Response(AgentRunResult::Finished(finished)) = &output {
                persist_turn_side_effects(
                    context,
                    session_id,
                    effective_profile_id,
                    trimmed,
                    &finished.content,
                );
            }

            Ok(output)
        }
    }
}

/// Extract displayable text from an `AgentLoopOutput`.
pub fn output_to_text(output: &AgentLoopOutput) -> Option<&str> {
    match output {
        AgentLoopOutput::Response(AgentRunResult::Finished(resp)) => Some(&resp.content),
        AgentLoopOutput::CommandOutput(text) => Some(text),
        AgentLoopOutput::Quit => None,
    }
}
