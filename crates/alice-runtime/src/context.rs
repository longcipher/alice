//! Alice runtime context.

use std::sync::Arc;

use alice_core::{memory::service::MemoryService, runtime_state::service::RuntimeStateService};
use bob_runtime::{Agent, agent_loop::AgentLoop};

use crate::{
    agent_backend::AgentBackend, channel_dispatch::ChannelDispatcher, config::SkillsConfig,
    orchestration::Orchestrator, reflection::AgentReflector,
};

/// Bundled services and configuration used to construct an [`AliceRuntimeContext`].
pub struct AliceRuntimeServices {
    /// Agent backend abstraction (bob-agent or acp-agent).
    pub backend: Arc<dyn AgentBackend>,
    /// Local memory service.
    pub memory_service: Arc<MemoryService>,
    /// Runtime-state service for identity bindings, sessions, and schedules.
    pub runtime_state_service: Arc<RuntimeStateService>,
    /// Channel dispatcher used by background workflows to post results.
    pub channel_dispatcher: ChannelDispatcher,
    /// Optional orchestrator for multi-profile ACP flows.
    pub orchestrator: Option<Orchestrator>,
    /// Whether ordinary natural-language turns should auto-use the orchestrator.
    pub auto_orchestrate: bool,
    /// Skill configuration used to render per-turn skill context.
    pub skills_config: SkillsConfig,
    /// Optional post-turn reflector.
    pub reflector: Option<AgentReflector>,
    /// Default model id.
    pub default_model: String,
}

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
    /// Runtime-state service for identity bindings, sessions, and schedules.
    pub runtime_state_service: Arc<RuntimeStateService>,
    /// Channel dispatcher used by background workflows to post results.
    pub channel_dispatcher: ChannelDispatcher,
    /// Optional orchestrator for multi-profile ACP flows.
    pub orchestrator: Option<Orchestrator>,
    /// Whether ordinary natural-language turns should auto-use the orchestrator.
    pub auto_orchestrate: bool,
    /// Skill configuration used to render per-turn skill context.
    pub skills_config: SkillsConfig,
    /// Optional post-turn reflector.
    pub reflector: Option<AgentReflector>,
    /// Default model id.
    pub default_model: String,
}

impl AliceRuntimeContext {
    /// Create a new runtime context.
    #[must_use]
    pub fn new(agent_loop: AgentLoop, agent: Agent, services: AliceRuntimeServices) -> Self {
        let AliceRuntimeServices {
            backend,
            memory_service,
            runtime_state_service,
            channel_dispatcher,
            orchestrator,
            auto_orchestrate,
            skills_config,
            reflector,
            default_model,
        } = services;
        Self {
            agent_loop,
            agent,
            backend,
            memory_service,
            runtime_state_service,
            channel_dispatcher,
            orchestrator,
            auto_orchestrate,
            skills_config,
            reflector,
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

    /// Runtime-state service for identity bindings, sessions, and schedules.
    #[must_use]
    pub const fn runtime_state_service(&self) -> &Arc<RuntimeStateService> {
        &self.runtime_state_service
    }

    /// Channel dispatcher used by background workflows to post results.
    #[must_use]
    pub const fn channel_dispatcher(&self) -> &ChannelDispatcher {
        &self.channel_dispatcher
    }

    /// Optional multi-profile orchestrator.
    #[must_use]
    pub const fn orchestrator(&self) -> Option<&Orchestrator> {
        self.orchestrator.as_ref()
    }

    /// Whether ordinary natural-language turns should auto-use the orchestrator.
    #[must_use]
    pub const fn auto_orchestrate(&self) -> bool {
        self.auto_orchestrate
    }

    /// Skill configuration used to render per-turn skill context.
    #[must_use]
    pub const fn skills_config(&self) -> &SkillsConfig {
        &self.skills_config
    }

    /// Token budget for skill prompt injection.
    #[must_use]
    pub const fn skill_token_budget(&self) -> usize {
        self.skills_config.token_budget
    }

    /// Optional post-turn reflector.
    #[must_use]
    pub const fn reflector(&self) -> Option<&AgentReflector> {
        self.reflector.as_ref()
    }

    /// Default model id.
    #[must_use]
    pub fn default_model(&self) -> &str {
        &self.default_model
    }
}

impl std::fmt::Debug for AliceRuntimeServices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AliceRuntimeServices")
            .field("orchestration_active", &self.orchestrator.is_some())
            .field("auto_orchestrate", &self.auto_orchestrate)
            .field("skills_active", &self.skills_config.enabled)
            .field("reflection_active", &self.reflector.is_some())
            .field("default_model", &self.default_model)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for AliceRuntimeContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AliceRuntimeContext")
            .field("default_model", &self.default_model)
            .field("orchestration_active", &self.orchestrator.is_some())
            .field("auto_orchestrate", &self.auto_orchestrate)
            .field("skills_active", &self.skills_config.enabled)
            .field("reflection_active", &self.reflector.is_some())
            .finish_non_exhaustive()
    }
}
