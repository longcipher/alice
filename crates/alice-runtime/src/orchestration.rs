//! Runtime-owned orchestration primitives for multi-profile ACP flows.
//!
//! This module keeps orchestration separate from bootstrap/config wiring so we
//! can build and test multi-profile execution in isolation.

use std::{collections::BTreeMap, error::Error, fmt, sync::Arc};

use bob_core::types::RequestContext;
use bob_runtime::AgentResponse;
use futures_util::future::try_join_all;

use crate::agent_backend::{AgentBackend, AgentSession};

/// Error returned by orchestration operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrchestrationError {
    /// A requested profile name was not present in the registry.
    MissingProfile {
        /// The profile that could not be resolved.
        profile_name: String,
    },
    /// A backend session returned an error while processing a turn.
    BackendFailure {
        /// The profile that returned the error.
        profile_name: String,
        /// The backend error message.
        message: String,
    },
}

impl fmt::Display for OrchestrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingProfile { profile_name } => {
                write!(f, "missing orchestration profile '{profile_name}'")
            }
            Self::BackendFailure { profile_name, message } => {
                write!(f, "orchestration profile '{profile_name}' failed: {message}")
            }
        }
    }
}

impl Error for OrchestrationError {}

/// Result alias used by orchestration APIs.
pub type OrchestrationResult<T> = std::result::Result<T, OrchestrationError>;

/// Backend descriptor for a named orchestration profile.
#[derive(Clone)]
pub struct OrchestrationProfileDescriptor {
    name: String,
    backend: Arc<dyn AgentBackend>,
}

impl fmt::Debug for OrchestrationProfileDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OrchestrationProfileDescriptor")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl OrchestrationProfileDescriptor {
    /// Create a new profile descriptor.
    #[must_use]
    pub fn new(name: impl Into<String>, backend: Arc<dyn AgentBackend>) -> Self {
        Self { name: name.into(), backend }
    }

    /// Return the registered profile name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Return the backend bound to the profile.
    #[must_use]
    pub fn backend(&self) -> Arc<dyn AgentBackend> {
        Arc::clone(&self.backend)
    }
}

/// Registry for orchestration profiles.
#[derive(Debug, Clone, Default)]
pub struct OrchestrationProfileRegistry {
    profiles: BTreeMap<String, OrchestrationProfileDescriptor>,
}

impl OrchestrationProfileRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a backend under a profile name.
    pub fn register(
        &mut self,
        profile_name: impl Into<String>,
        backend: Arc<dyn AgentBackend>,
    ) -> Option<OrchestrationProfileDescriptor> {
        let descriptor = OrchestrationProfileDescriptor::new(profile_name.into(), backend);
        self.profiles.insert(descriptor.name.clone(), descriptor)
    }

    /// Register a pre-built profile descriptor.
    pub fn register_descriptor(
        &mut self,
        descriptor: OrchestrationProfileDescriptor,
    ) -> Option<OrchestrationProfileDescriptor> {
        self.profiles.insert(descriptor.name.clone(), descriptor)
    }

    /// Return a backend for the requested profile name.
    #[must_use]
    pub fn backend(&self, profile_name: &str) -> Option<Arc<dyn AgentBackend>> {
        self.profiles.get(profile_name).map(OrchestrationProfileDescriptor::backend)
    }

    /// Return the registered profile names in deterministic order.
    #[must_use]
    pub fn profile_names(&self) -> Vec<String> {
        self.profiles.keys().cloned().collect()
    }
}

/// A single worker task to execute during fan-out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerTask {
    profile_name: String,
    prompt: String,
}

impl WorkerTask {
    /// Create a new worker task.
    #[must_use]
    pub fn new(profile_name: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self { profile_name: profile_name.into(), prompt: prompt.into() }
    }

    fn profile_name(&self) -> &str {
        &self.profile_name
    }

    fn prompt(&self) -> &str {
        &self.prompt
    }
}

/// A single turn executed against a profile backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrchestrationTurn {
    /// Profile used to execute the turn.
    pub profile_name: String,
    /// Session identifier passed to the backend.
    pub session_id: String,
    /// Prompt sent to the backend.
    pub prompt: String,
    /// Assistant response returned by the backend.
    pub response: String,
}

/// Aggregated result for a complete orchestration run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrchestrationRun {
    /// Manager turn result.
    pub manager: OrchestrationTurn,
    /// Worker turn results in input order.
    pub workers: Vec<OrchestrationTurn>,
    /// Human-readable aggregation of the run.
    pub summary: String,
}

/// Runtime-owned orchestrator for a manager profile plus worker fan-out.
#[derive(Debug, Clone)]
pub struct Orchestrator {
    manager_profile_name: String,
    registry: OrchestrationProfileRegistry,
}

impl Orchestrator {
    /// Create a new orchestrator with the given manager profile name.
    #[must_use]
    pub fn new(
        manager_profile_name: impl Into<String>,
        registry: OrchestrationProfileRegistry,
    ) -> Self {
        Self { manager_profile_name: manager_profile_name.into(), registry }
    }

    /// Return deterministic worker profile names, excluding the manager profile.
    #[must_use]
    pub fn worker_profile_names(&self) -> Vec<String> {
        self.registry
            .profile_names()
            .into_iter()
            .filter(|profile_name| profile_name != &self.manager_profile_name)
            .collect()
    }

    /// Run the manager prompt and fan out worker prompts.
    pub async fn run(
        &self,
        root_session_id: &str,
        manager_prompt: &str,
        worker_tasks: Vec<WorkerTask>,
    ) -> OrchestrationResult<OrchestrationRun> {
        self.run_with_context(
            root_session_id,
            manager_prompt,
            RequestContext::default(),
            worker_tasks,
        )
        .await
    }

    /// Run the manager prompt and fan out worker prompts with an explicit request context.
    pub async fn run_with_context(
        &self,
        root_session_id: &str,
        manager_prompt: &str,
        request_context: RequestContext,
        worker_tasks: Vec<WorkerTask>,
    ) -> OrchestrationResult<OrchestrationRun> {
        let manager_backend = self.backend_for(&self.manager_profile_name)?;
        let manager_session_id = Self::manager_session_id(root_session_id);
        let manager = self
            .run_turn(
                manager_backend,
                &self.manager_profile_name,
                &manager_session_id,
                manager_prompt,
                &request_context,
            )
            .await?;

        let workers = try_join_all(worker_tasks.into_iter().map(|task| {
            let registry = self.registry.clone();
            let root_session_id = root_session_id.to_string();
            let request_context = request_context.clone();
            async move {
                let profile_name = task.profile_name().to_string();
                let prompt = task.prompt().to_string();
                let backend = registry.backend(&profile_name).ok_or_else(|| {
                    OrchestrationError::MissingProfile { profile_name: profile_name.clone() }
                })?;
                let session_id = Self::worker_session_id(&root_session_id, &profile_name);
                Self::run_turn_static(
                    backend,
                    &profile_name,
                    &session_id,
                    &prompt,
                    &request_context,
                )
                .await
            }
        }))
        .await?;

        let summary = Self::render_summary(&manager, &workers);
        Ok(OrchestrationRun { manager, workers, summary })
    }

    fn backend_for(&self, profile_name: &str) -> OrchestrationResult<Arc<dyn AgentBackend>> {
        self.registry.backend(profile_name).ok_or_else(|| OrchestrationError::MissingProfile {
            profile_name: profile_name.to_string(),
        })
    }

    async fn run_turn(
        &self,
        backend: Arc<dyn AgentBackend>,
        profile_name: &str,
        session_id: &str,
        prompt: &str,
        request_context: &RequestContext,
    ) -> OrchestrationResult<OrchestrationTurn> {
        Self::run_turn_static(backend, profile_name, session_id, prompt, request_context).await
    }

    async fn run_turn_static(
        backend: Arc<dyn AgentBackend>,
        profile_name: &str,
        session_id: &str,
        prompt: &str,
        request_context: &RequestContext,
    ) -> OrchestrationResult<OrchestrationTurn> {
        let session: Arc<dyn AgentSession> = backend.create_session_with_id(session_id);
        let response: AgentResponse =
            session.chat(prompt, request_context.clone()).await.map_err(|error| {
                OrchestrationError::BackendFailure {
                    profile_name: profile_name.to_string(),
                    message: error.to_string(),
                }
            })?;

        Ok(OrchestrationTurn {
            profile_name: profile_name.to_string(),
            session_id: session_id.to_string(),
            prompt: prompt.to_string(),
            response: response.content,
        })
    }

    fn manager_session_id(root_session_id: &str) -> String {
        format!("{root_session_id}::manager")
    }

    fn worker_session_id(root_session_id: &str, profile_name: &str) -> String {
        format!("{root_session_id}::worker::{profile_name}")
    }

    fn render_summary(manager: &OrchestrationTurn, workers: &[OrchestrationTurn]) -> String {
        let mut lines = vec![format!("manager: {}", manager.response)];
        lines.extend(
            workers.iter().map(|worker| format!("{}: {}", worker.profile_name, worker.response)),
        );
        lines.join("\n")
    }
}
