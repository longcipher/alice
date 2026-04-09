//! Application service wrapping runtime-state workflows.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use crate::runtime_state::{
    domain::{ActiveSessionLease, BindToken, GlobalIdentityBinding, ScheduledTask},
    error::{RuntimeStateServiceError, RuntimeStateValidationError},
    ports::RuntimeStateStorePort,
};

static TOKEN_COUNTER: AtomicU64 = AtomicU64::new(1);

/// High-level runtime-state use-cases for Alice runtime integration.
pub struct RuntimeStateService {
    store: Arc<dyn RuntimeStateStorePort>,
}

impl std::fmt::Debug for RuntimeStateService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeStateService").finish_non_exhaustive()
    }
}

impl RuntimeStateService {
    /// Construct a runtime-state service and initialize store schema.
    pub fn new(store: Arc<dyn RuntimeStateStorePort>) -> Result<Self, RuntimeStateServiceError> {
        store.init_schema()?;
        Ok(Self { store })
    }

    /// Issue a one-time bind token.
    pub fn issue_bind_token(
        &self,
        global_user_id: &str,
        provider: Option<&str>,
        ttl_ms: i64,
    ) -> Result<BindToken, RuntimeStateServiceError> {
        if ttl_ms <= 0 {
            return Err(RuntimeStateValidationError::InvalidBindTokenTtl.into());
        }

        let now_ms = current_time_millis();
        let counter = TOKEN_COUNTER.fetch_add(1, Ordering::Relaxed);
        let token = BindToken {
            token: format!("bind-{now_ms}-{counter:04}"),
            global_user_id: global_user_id.to_string(),
            provider: provider.map(ToString::to_string),
            expires_at_epoch_ms: now_ms + ttl_ms,
            consumed_at_epoch_ms: None,
            created_at_epoch_ms: now_ms,
        };
        self.store.insert_bind_token(&token)?;
        Ok(token)
    }

    /// Consume a bind token and materialize the provider binding when valid.
    pub fn consume_bind_token(
        &self,
        token: &str,
        provider: &str,
        external_user_id: &str,
    ) -> Result<Option<GlobalIdentityBinding>, RuntimeStateServiceError> {
        let now_ms = current_time_millis();
        let Some(existing) = self.store.get_bind_token(token)? else {
            return Ok(None);
        };
        if existing.consumed_at_epoch_ms.is_some() || existing.expires_at_epoch_ms < now_ms {
            return Ok(None);
        }
        if let Some(expected_provider) = &existing.provider &&
            expected_provider != provider
        {
            return Ok(None);
        }

        let binding = GlobalIdentityBinding {
            provider: provider.to_string(),
            external_user_id: external_user_id.to_string(),
            global_user_id: existing.global_user_id,
            bound_at_epoch_ms: now_ms,
        };

        self.store.upsert_identity_binding(&binding)?;
        self.store.mark_bind_token_consumed(token, now_ms)?;
        Ok(Some(binding))
    }

    /// Persist an explicit identity binding.
    pub fn bind_identity(
        &self,
        provider: &str,
        external_user_id: &str,
        global_user_id: &str,
    ) -> Result<GlobalIdentityBinding, RuntimeStateServiceError> {
        let binding = GlobalIdentityBinding {
            provider: provider.to_string(),
            external_user_id: external_user_id.to_string(),
            global_user_id: global_user_id.to_string(),
            bound_at_epoch_ms: current_time_millis(),
        };
        self.store.upsert_identity_binding(&binding)?;
        Ok(binding)
    }

    /// Resolve the global user id for a provider identity.
    pub fn resolve_global_user_id(
        &self,
        provider: &str,
        external_user_id: &str,
    ) -> Result<Option<String>, RuntimeStateServiceError> {
        Ok(self
            .store
            .get_identity_binding(provider, external_user_id)?
            .map(|binding| binding.global_user_id))
    }

    /// Upsert the active session lease for a user.
    pub fn upsert_active_session(
        &self,
        global_user_id: &str,
        session_id: &str,
        channel: Option<&str>,
    ) -> Result<ActiveSessionLease, RuntimeStateServiceError> {
        self.upsert_active_session_with_thread_id(global_user_id, session_id, channel, None)
    }

    /// Upsert the active session lease for a user with an explicit thread id.
    pub fn upsert_active_session_with_thread_id(
        &self,
        global_user_id: &str,
        session_id: &str,
        channel: Option<&str>,
        thread_id: Option<&str>,
    ) -> Result<ActiveSessionLease, RuntimeStateServiceError> {
        let lease = ActiveSessionLease {
            global_user_id: global_user_id.to_string(),
            session_id: session_id.to_string(),
            channel: channel.map(ToString::to_string),
            thread_id: thread_id.map(ToString::to_string),
            updated_at_epoch_ms: current_time_millis(),
        };
        self.store.upsert_active_session(&lease)?;
        Ok(lease)
    }

    /// Load the active session lease for a user.
    pub fn get_active_session(
        &self,
        global_user_id: &str,
    ) -> Result<Option<ActiveSessionLease>, RuntimeStateServiceError> {
        self.store.get_active_session(global_user_id).map_err(RuntimeStateServiceError::from)
    }

    /// Persist a scheduled task.
    pub fn insert_scheduled_task(
        &self,
        task: ScheduledTask,
    ) -> Result<ScheduledTask, RuntimeStateServiceError> {
        task.validate()?;
        self.store.upsert_scheduled_task(&task)?;
        Ok(task)
    }

    /// List tasks due at or before the given timestamp.
    pub fn list_due_tasks(
        &self,
        now_epoch_ms: i64,
    ) -> Result<Vec<ScheduledTask>, RuntimeStateServiceError> {
        self.store.list_due_tasks(now_epoch_ms).map_err(RuntimeStateServiceError::from)
    }

    /// List all scheduled tasks in storage.
    pub fn list_scheduled_tasks(&self) -> Result<Vec<ScheduledTask>, RuntimeStateServiceError> {
        self.store.list_scheduled_tasks().map_err(RuntimeStateServiceError::from)
    }

    /// Mark a scheduled task as executed and advance its next run timestamp.
    pub fn mark_task_executed(
        &self,
        task_id: &str,
        ran_at_epoch_ms: i64,
    ) -> Result<Option<ScheduledTask>, RuntimeStateServiceError> {
        let Some(mut task) = self.store.get_scheduled_task(task_id)? else {
            return Ok(None);
        };
        task.last_run_epoch_ms = Some(ran_at_epoch_ms);
        task.next_run_epoch_ms = task.schedule.next_run_after(ran_at_epoch_ms);
        self.store.upsert_scheduled_task(&task)?;
        Ok(Some(task))
    }
}

fn current_time_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use parking_lot::Mutex;

    use super::RuntimeStateService;
    use crate::runtime_state::{
        domain::{
            ActiveSessionLease, BindToken, GlobalIdentityBinding, ScheduleKind, ScheduledTask,
        },
        error::RuntimeStateStoreError,
        ports::RuntimeStateStorePort,
    };

    #[derive(Debug, Default)]
    struct MockStore {
        bindings: Mutex<Vec<GlobalIdentityBinding>>,
        tokens: Mutex<Vec<BindToken>>,
        sessions: Mutex<Vec<ActiveSessionLease>>,
        tasks: Mutex<Vec<ScheduledTask>>,
    }

    impl RuntimeStateStorePort for MockStore {
        fn init_schema(&self) -> Result<(), RuntimeStateStoreError> {
            Ok(())
        }

        fn upsert_identity_binding(
            &self,
            binding: &GlobalIdentityBinding,
        ) -> Result<(), RuntimeStateStoreError> {
            let mut bindings = self.bindings.lock();
            if let Some(existing) = bindings.iter_mut().find(|item| {
                item.provider == binding.provider &&
                    item.external_user_id == binding.external_user_id
            }) {
                *existing = binding.clone();
            } else {
                bindings.push(binding.clone());
            }
            Ok(())
        }

        fn get_identity_binding(
            &self,
            provider: &str,
            external_user_id: &str,
        ) -> Result<Option<GlobalIdentityBinding>, RuntimeStateStoreError> {
            Ok(self
                .bindings
                .lock()
                .iter()
                .find(|item| item.provider == provider && item.external_user_id == external_user_id)
                .cloned())
        }

        fn insert_bind_token(&self, token: &BindToken) -> Result<(), RuntimeStateStoreError> {
            self.tokens.lock().push(token.clone());
            Ok(())
        }

        fn get_bind_token(&self, token: &str) -> Result<Option<BindToken>, RuntimeStateStoreError> {
            Ok(self.tokens.lock().iter().find(|item| item.token == token).cloned())
        }

        fn mark_bind_token_consumed(
            &self,
            token: &str,
            consumed_at_epoch_ms: i64,
        ) -> Result<(), RuntimeStateStoreError> {
            if let Some(existing) = self.tokens.lock().iter_mut().find(|item| item.token == token) {
                existing.consumed_at_epoch_ms = Some(consumed_at_epoch_ms);
            }
            Ok(())
        }

        fn upsert_active_session(
            &self,
            lease: &ActiveSessionLease,
        ) -> Result<(), RuntimeStateStoreError> {
            let mut sessions = self.sessions.lock();
            if let Some(existing) =
                sessions.iter_mut().find(|item| item.global_user_id == lease.global_user_id)
            {
                *existing = lease.clone();
            } else {
                sessions.push(lease.clone());
            }
            Ok(())
        }

        fn get_active_session(
            &self,
            global_user_id: &str,
        ) -> Result<Option<ActiveSessionLease>, RuntimeStateStoreError> {
            Ok(self
                .sessions
                .lock()
                .iter()
                .find(|item| item.global_user_id == global_user_id)
                .cloned())
        }

        fn upsert_scheduled_task(
            &self,
            task: &ScheduledTask,
        ) -> Result<(), RuntimeStateStoreError> {
            let mut tasks = self.tasks.lock();
            if let Some(existing) = tasks.iter_mut().find(|item| item.task_id == task.task_id) {
                *existing = task.clone();
            } else {
                tasks.push(task.clone());
            }
            Ok(())
        }

        fn get_scheduled_task(
            &self,
            task_id: &str,
        ) -> Result<Option<ScheduledTask>, RuntimeStateStoreError> {
            Ok(self.tasks.lock().iter().find(|item| item.task_id == task_id).cloned())
        }

        fn list_due_tasks(
            &self,
            now_epoch_ms: i64,
        ) -> Result<Vec<ScheduledTask>, RuntimeStateStoreError> {
            Ok(self
                .tasks
                .lock()
                .iter()
                .filter(|item| item.enabled && item.next_run_epoch_ms <= now_epoch_ms)
                .cloned()
                .collect())
        }

        fn list_scheduled_tasks(&self) -> Result<Vec<ScheduledTask>, RuntimeStateStoreError> {
            let mut tasks = self.tasks.lock().clone();
            tasks.sort_by(|left, right| {
                left.next_run_epoch_ms
                    .cmp(&right.next_run_epoch_ms)
                    .then_with(|| left.task_id.cmp(&right.task_id))
            });
            Ok(tasks)
        }
    }

    #[test]
    fn issue_and_consume_bind_token_creates_binding() {
        let store: Arc<dyn RuntimeStateStorePort> = Arc::new(MockStore::default());
        let Ok(service) = RuntimeStateService::new(store) else {
            return;
        };

        let Ok(token) = service.issue_bind_token("global-1", Some("telegram"), 60_000) else {
            return;
        };
        let Ok(binding) = service.consume_bind_token(&token.token, "telegram", "123") else {
            return;
        };
        let Some(binding) = binding else {
            panic!("bind token should be consumed into a binding");
        };

        assert_eq!(binding.global_user_id, "global-1");
        assert_eq!(binding.provider, "telegram");
    }

    #[test]
    fn active_session_roundtrip() {
        let store: Arc<dyn RuntimeStateStorePort> = Arc::new(MockStore::default());
        let Ok(service) = RuntimeStateService::new(store) else {
            return;
        };

        assert!(
            service
                .upsert_active_session_with_thread_id(
                    "global-1",
                    "session-1",
                    Some("cli"),
                    Some("thread-1"),
                )
                .is_ok()
        );
        let Ok(lease) = service.get_active_session("global-1") else {
            return;
        };
        let Some(lease) = lease else {
            panic!("active session should be readable");
        };

        assert_eq!(lease.session_id, "session-1");
        assert_eq!(lease.channel.as_deref(), Some("cli"));
        assert_eq!(lease.thread_id.as_deref(), Some("thread-1"));
    }

    #[test]
    fn mark_task_executed_advances_next_run() {
        let store: Arc<dyn RuntimeStateStorePort> = Arc::new(MockStore::default());
        let Ok(service) = RuntimeStateService::new(store) else {
            return;
        };

        let task = ScheduledTask {
            task_id: "task-1".to_string(),
            global_user_id: "global-1".to_string(),
            channel: Some("telegram".to_string()),
            prompt: "Summarize alerts".to_string(),
            schedule: ScheduleKind::EveryMinutes(15),
            next_run_epoch_ms: 1_000,
            enabled: true,
            last_run_epoch_ms: None,
        };
        assert!(service.insert_scheduled_task(task).is_ok());

        let Ok(updated) = service.mark_task_executed("task-1", 1_000) else {
            return;
        };
        let Some(updated) = updated else {
            panic!("scheduled task should be updated");
        };

        assert_eq!(updated.last_run_epoch_ms, Some(1_000));
        assert_eq!(updated.next_run_epoch_ms, 901_000);
    }
}
