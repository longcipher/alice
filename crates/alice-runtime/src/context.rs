//! Alice runtime context.

use std::sync::Arc;

use alice_core::memory::service::MemoryService;
use bob_adapters::skills_agent::SkillPromptComposer;
use bob_core::ports::{TapeStorePort, ToolPort};
use bob_runtime::{AgentRuntime, agent_loop::AgentLoop};

/// Fully wired Alice runtime context.
pub struct AliceRuntimeContext {
    /// Agent loop with slash-command support and tape recording.
    pub agent_loop: AgentLoop,
    /// Bob runtime executor (also held by agent_loop, exposed for direct access).
    pub runtime: Arc<dyn AgentRuntime>,
    /// Composed tool port.
    pub tools: Arc<dyn ToolPort>,
    /// Tape store for turn recording.
    pub tape: Arc<dyn TapeStorePort>,
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
