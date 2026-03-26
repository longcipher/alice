//! Alice runtime context.

use std::sync::Arc;

use alice_core::memory::service::MemoryService;
use bob_adapters::skills_agent::SkillPromptComposer;
use bob_runtime::{Agent, agent_loop::AgentLoop};

use crate::agent_backend::AgentBackend;

/// Fully wired Alice runtime context.
pub struct AliceRuntimeContext {
    /// Agent loop with slash-command support and tape recording.
    pub agent_loop: AgentLoop,
    /// Bob Agent for high-level Session-based interaction.
    pub agent: Agent,
    /// Agent backend abstraction (bob-agent or acp-agent).
    pub backend: Arc<dyn AgentBackend>,
    /// Local memory service.
    pub memory_service: Arc<MemoryService>,
    /// Skill prompt composer (None when skills disabled).
    pub skill_composer: Option<SkillPromptComposer>,
    /// Token budget for skill prompt injection.
    pub skill_token_budget: usize,
    /// Default model id.
    pub default_model: String,
}

impl std::fmt::Debug for AliceRuntimeContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AliceRuntimeContext")
            .field("default_model", &self.default_model)
            .field("skills_active", &self.skill_composer.is_some())
            .finish_non_exhaustive()
    }
}
