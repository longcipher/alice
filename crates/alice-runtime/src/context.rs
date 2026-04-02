//! Alice runtime context.

use std::sync::Arc;

use alice_core::memory::service::MemoryService;
use bob_adapters::skills_agent::SkillPromptComposer;
use bob_runtime::{Agent, agent_loop::AgentLoop};

use crate::agent_backend::AgentBackend;

/// Fully wired Alice runtime context.
///
/// Fields are public for test construction. Prefer getter methods in production code.
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

impl AliceRuntimeContext {
    /// Create a new runtime context.
    #[must_use]
    pub fn new(
        agent_loop: AgentLoop,
        agent: Agent,
        backend: Arc<dyn AgentBackend>,
        memory_service: Arc<MemoryService>,
        skill_composer: Option<SkillPromptComposer>,
        skill_token_budget: usize,
        default_model: String,
    ) -> Self {
        Self {
            agent_loop,
            agent,
            backend,
            memory_service,
            skill_composer,
            skill_token_budget,
            default_model,
        }
    }

    /// Agent loop with slash-command support and tape recording.
    #[must_use]
    pub const fn agent_loop(&self) -> &AgentLoop {
        &self.agent_loop
    }

    /// Bob Agent for high-level Session-based interaction.
    #[must_use]
    pub const fn agent(&self) -> &Agent {
        &self.agent
    }

    /// Agent backend abstraction (bob-agent or acp-agent).
    #[must_use]
    pub fn backend(&self) -> &Arc<dyn AgentBackend> {
        &self.backend
    }

    /// Local memory service.
    #[must_use]
    pub const fn memory_service(&self) -> &Arc<MemoryService> {
        &self.memory_service
    }

    /// Skill prompt composer (None when skills disabled).
    #[must_use]
    pub const fn skill_composer(&self) -> Option<&SkillPromptComposer> {
        self.skill_composer.as_ref()
    }

    /// Token budget for skill prompt injection.
    #[must_use]
    pub const fn skill_token_budget(&self) -> usize {
        self.skill_token_budget
    }

    /// Default model id.
    #[must_use]
    pub fn default_model(&self) -> &str {
        &self.default_model
    }
}

impl std::fmt::Debug for AliceRuntimeContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AliceRuntimeContext")
            .field("default_model", &self.default_model)
            .field("skills_active", &self.skill_composer.is_some())
            .finish_non_exhaustive()
    }
}
