//! Agent backend abstraction.
//!
//! Defines the [`AgentBackend`] trait that decouples the runtime wiring
//! from the concrete agent implementation (bob-agent, acp-agent, etc.).

use std::sync::Arc;

use bob_runtime::AgentResponse;

/// Per-session agent handle returned by an [`AgentBackend`].
///
/// Each session holds its own conversation state and can process messages
/// independently. Implementations must be `Send + Sync` so sessions can
/// be held across async task boundaries.
#[async_trait::async_trait]
pub trait AgentSession: Send + Sync {
    /// Process a user message and return the agent's response.
    ///
    /// The `context` carries per-request overrides such as system prompt
    /// augmentation, skill selections, and tool policy adjustments.
    async fn chat(
        &self,
        input: &str,
        context: bob_core::types::RequestContext,
    ) -> eyre::Result<AgentResponse>;
}

/// Factory for creating agent sessions.
///
/// An `AgentBackend` is stateless — it holds configuration and shared
/// resources (LLM client, tool port, etc.) and stamps out independent
/// [`AgentSession`] instances on demand.
pub trait AgentBackend: Send + Sync {
    /// Create a new session with a generated identifier.
    fn create_session(&self) -> Arc<dyn AgentSession>;

    /// Create a new session with a specific identifier.
    fn create_session_with_id(&self, session_id: &str) -> Arc<dyn AgentSession>;
}

pub mod bob_backend;

#[cfg(feature = "acp-agent")]
pub mod acp_backend;
