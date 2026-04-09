//! Port traits for runtime-state persistence.

use crate::runtime_state::{
    domain::{ActiveSessionLease, BindToken, GlobalIdentityBinding, ScheduledTask},
    error::RuntimeStateStoreError,
};

/// Storage boundary for runtime-state implementations.
pub trait RuntimeStateStorePort: Send + Sync {
    /// Ensure required schema exists.
    fn init_schema(&self) -> Result<(), RuntimeStateStoreError>;

    /// Insert or update a global identity binding.
    fn upsert_identity_binding(
        &self,
        binding: &GlobalIdentityBinding,
    ) -> Result<(), RuntimeStateStoreError>;

    /// Resolve a global identity binding by provider and external user id.
    fn get_identity_binding(
        &self,
        provider: &str,
        external_user_id: &str,
    ) -> Result<Option<GlobalIdentityBinding>, RuntimeStateStoreError>;

    /// Insert a bind token.
    fn insert_bind_token(&self, token: &BindToken) -> Result<(), RuntimeStateStoreError>;

    /// Load a bind token by token string.
    fn get_bind_token(&self, token: &str) -> Result<Option<BindToken>, RuntimeStateStoreError>;

    /// Mark a bind token as consumed.
    fn mark_bind_token_consumed(
        &self,
        token: &str,
        consumed_at_epoch_ms: i64,
    ) -> Result<(), RuntimeStateStoreError>;

    /// Insert or update an active session lease.
    fn upsert_active_session(
        &self,
        lease: &ActiveSessionLease,
    ) -> Result<(), RuntimeStateStoreError>;

    /// Resolve an active session lease by global user id.
    fn get_active_session(
        &self,
        global_user_id: &str,
    ) -> Result<Option<ActiveSessionLease>, RuntimeStateStoreError>;

    /// Insert or update a scheduled task.
    fn upsert_scheduled_task(&self, task: &ScheduledTask) -> Result<(), RuntimeStateStoreError>;

    /// Load a scheduled task by id.
    fn get_scheduled_task(
        &self,
        task_id: &str,
    ) -> Result<Option<ScheduledTask>, RuntimeStateStoreError>;

    /// List all scheduled tasks ordered by next run timestamp and task id.
    fn list_scheduled_tasks(&self) -> Result<Vec<ScheduledTask>, RuntimeStateStoreError>;

    /// List tasks that are due at or before the given timestamp.
    fn list_due_tasks(
        &self,
        now_epoch_ms: i64,
    ) -> Result<Vec<ScheduledTask>, RuntimeStateStoreError>;
}
