//! Bob agent backend implementation.
//!
//! Wraps [`bob_runtime::Agent`] and [`bob_runtime::Session`] behind the
//! [`AgentBackend`](super::AgentBackend) / [`AgentSession`](super::AgentSession) traits.

use std::sync::Arc;

use bob_core::types::RequestContext;
use bob_runtime::{Agent, AgentResponse, Session};

use super::{AgentBackend, AgentSession};

/// Bob-runtime backed session.
#[derive(Debug)]
pub struct BobAgentSession {
    session: Session,
}

#[async_trait::async_trait]
impl AgentSession for BobAgentSession {
    async fn chat(&self, input: &str, context: RequestContext) -> eyre::Result<AgentResponse> {
        let response = self.session.chat_with_context(input, context).await?;
        Ok(response)
    }
}

/// Bob-runtime backed agent backend.
///
/// Holds a pre-built [`Agent`] and produces [`Session`] instances on demand.
#[derive(Debug, Clone)]
pub struct BobAgentBackend {
    agent: Agent,
}

impl BobAgentBackend {
    /// Create a new backend from a pre-built agent.
    #[must_use]
    pub const fn new(agent: Agent) -> Self {
        Self { agent }
    }
}

impl AgentBackend for BobAgentBackend {
    fn create_session(&self) -> Arc<dyn AgentSession> {
        let session = self.agent.start_session();
        Arc::new(BobAgentSession { session })
    }

    fn create_session_with_id(&self, session_id: &str) -> Arc<dyn AgentSession> {
        let session = self.agent.start_session_with_id(session_id);
        Arc::new(BobAgentSession { session })
    }
}
