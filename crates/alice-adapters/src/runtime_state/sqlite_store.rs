//! SQLite-backed runtime-state store.

use std::{path::Path, sync::Mutex};

use alice_core::runtime_state::{
    domain::{ActiveSessionLease, BindToken, GlobalIdentityBinding, ScheduledTask},
    error::RuntimeStateStoreError,
    ports::RuntimeStateStorePort,
};
use rusqlite::{Connection, OptionalExtension, params};

use super::sqlite_schema;

fn db_err(err: rusqlite::Error) -> RuntimeStateStoreError {
    RuntimeStateStoreError::Database(err.to_string())
}

/// SQLite-backed implementation of [`RuntimeStateStorePort`].
#[derive(Debug)]
pub struct SqliteRuntimeStateStore {
    conn: Mutex<Connection>,
}

impl SqliteRuntimeStateStore {
    /// Open a file-backed SQLite store.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RuntimeStateStoreError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() &&
            !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|error| RuntimeStateStoreError::Database(error.to_string()))?;
        }

        let conn = Connection::open(path).map_err(db_err)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;").map_err(db_err)?;
        let store = Self { conn: Mutex::new(conn) };
        store.init_schema()?;
        Ok(store)
    }

    /// Open an in-memory SQLite store for tests.
    pub fn in_memory() -> Result<Self, RuntimeStateStoreError> {
        let conn = Connection::open_in_memory().map_err(db_err)?;
        conn.execute_batch("PRAGMA foreign_keys=ON;").map_err(db_err)?;
        let store = Self { conn: Mutex::new(conn) };
        store.init_schema()?;
        Ok(store)
    }

    fn row_to_binding(row: &rusqlite::Row<'_>) -> Result<GlobalIdentityBinding, rusqlite::Error> {
        Ok(GlobalIdentityBinding {
            provider: row.get(0)?,
            external_user_id: row.get(1)?,
            global_user_id: row.get(2)?,
            bound_at_epoch_ms: row.get(3)?,
        })
    }

    fn row_to_bind_token(row: &rusqlite::Row<'_>) -> Result<BindToken, rusqlite::Error> {
        Ok(BindToken {
            token: row.get(0)?,
            global_user_id: row.get(1)?,
            provider: row.get(2)?,
            expires_at_epoch_ms: row.get(3)?,
            consumed_at_epoch_ms: row.get(4)?,
            created_at_epoch_ms: row.get(5)?,
        })
    }

    fn row_to_active_session(
        row: &rusqlite::Row<'_>,
    ) -> Result<ActiveSessionLease, rusqlite::Error> {
        Ok(ActiveSessionLease {
            global_user_id: row.get(0)?,
            session_id: row.get(1)?,
            channel: row.get(2)?,
            thread_id: row.get(3)?,
            updated_at_epoch_ms: row.get(4)?,
        })
    }

    fn row_to_scheduled_task(row: &rusqlite::Row<'_>) -> Result<ScheduledTask, rusqlite::Error> {
        let schedule_payload: String = row.get(5)?;
        let schedule = serde_json::from_str(&schedule_payload).map_err(|error| {
            tracing::warn!(error = %error, "failed to parse scheduled task payload");
            rusqlite::Error::InvalidColumnType(
                5,
                "schedule_payload".to_string(),
                rusqlite::types::Type::Text,
            )
        })?;

        Ok(ScheduledTask {
            task_id: row.get(0)?,
            global_user_id: row.get(1)?,
            channel: row.get(2)?,
            prompt: row.get(3)?,
            schedule,
            next_run_epoch_ms: row.get(6)?,
            enabled: row.get::<_, i64>(7)? != 0,
            last_run_epoch_ms: row.get(8)?,
        })
    }
}

impl RuntimeStateStorePort for SqliteRuntimeStateStore {
    fn init_schema(&self) -> Result<(), RuntimeStateStoreError> {
        let conn = self.conn.lock().map_err(|_| {
            RuntimeStateStoreError::Database("runtime-state sqlite mutex poisoned".to_string())
        })?;
        sqlite_schema::init_schema(&conn)
    }

    fn upsert_identity_binding(
        &self,
        binding: &GlobalIdentityBinding,
    ) -> Result<(), RuntimeStateStoreError> {
        let conn = self.conn.lock().map_err(|_| {
            RuntimeStateStoreError::Database("runtime-state sqlite mutex poisoned".to_string())
        })?;
        conn.execute(
            "INSERT INTO identity_bindings (provider, external_user_id, global_user_id, bound_at_epoch_ms)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(provider, external_user_id) DO UPDATE SET
                global_user_id = excluded.global_user_id,
                bound_at_epoch_ms = excluded.bound_at_epoch_ms",
            params![
                binding.provider,
                binding.external_user_id,
                binding.global_user_id,
                binding.bound_at_epoch_ms,
            ],
        )
        .map_err(db_err)?;
        Ok(())
    }

    fn get_identity_binding(
        &self,
        provider: &str,
        external_user_id: &str,
    ) -> Result<Option<GlobalIdentityBinding>, RuntimeStateStoreError> {
        let conn = self.conn.lock().map_err(|_| {
            RuntimeStateStoreError::Database("runtime-state sqlite mutex poisoned".to_string())
        })?;
        let mut stmt = conn
            .prepare(
                "SELECT provider, external_user_id, global_user_id, bound_at_epoch_ms
                 FROM identity_bindings
                 WHERE provider = ?1 AND external_user_id = ?2",
            )
            .map_err(db_err)?;
        stmt.query_row(params![provider, external_user_id], Self::row_to_binding)
            .optional()
            .map_err(db_err)
    }

    fn insert_bind_token(&self, token: &BindToken) -> Result<(), RuntimeStateStoreError> {
        let conn = self.conn.lock().map_err(|_| {
            RuntimeStateStoreError::Database("runtime-state sqlite mutex poisoned".to_string())
        })?;
        conn.execute(
            "INSERT INTO bind_tokens
             (token, global_user_id, provider, expires_at_epoch_ms, consumed_at_epoch_ms, created_at_epoch_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                token.token,
                token.global_user_id,
                token.provider,
                token.expires_at_epoch_ms,
                token.consumed_at_epoch_ms,
                token.created_at_epoch_ms,
            ],
        )
        .map_err(db_err)?;
        Ok(())
    }

    fn get_bind_token(&self, token: &str) -> Result<Option<BindToken>, RuntimeStateStoreError> {
        let conn = self.conn.lock().map_err(|_| {
            RuntimeStateStoreError::Database("runtime-state sqlite mutex poisoned".to_string())
        })?;
        let mut stmt = conn
            .prepare(
                "SELECT token, global_user_id, provider, expires_at_epoch_ms, consumed_at_epoch_ms, created_at_epoch_ms
                 FROM bind_tokens
                 WHERE token = ?1",
            )
            .map_err(db_err)?;
        stmt.query_row(params![token], Self::row_to_bind_token).optional().map_err(db_err)
    }

    fn mark_bind_token_consumed(
        &self,
        token: &str,
        consumed_at_epoch_ms: i64,
    ) -> Result<(), RuntimeStateStoreError> {
        let conn = self.conn.lock().map_err(|_| {
            RuntimeStateStoreError::Database("runtime-state sqlite mutex poisoned".to_string())
        })?;
        conn.execute(
            "UPDATE bind_tokens SET consumed_at_epoch_ms = ?2 WHERE token = ?1",
            params![token, consumed_at_epoch_ms],
        )
        .map_err(db_err)?;
        Ok(())
    }

    fn upsert_active_session(
        &self,
        lease: &ActiveSessionLease,
    ) -> Result<(), RuntimeStateStoreError> {
        let conn = self.conn.lock().map_err(|_| {
            RuntimeStateStoreError::Database("runtime-state sqlite mutex poisoned".to_string())
        })?;
        conn.execute(
            "INSERT INTO active_sessions (global_user_id, session_id, channel, thread_id, updated_at_epoch_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(global_user_id) DO UPDATE SET
                session_id = excluded.session_id,
                channel = excluded.channel,
                thread_id = excluded.thread_id,
                updated_at_epoch_ms = excluded.updated_at_epoch_ms",
            params![
                lease.global_user_id,
                lease.session_id,
                lease.channel,
                lease.thread_id,
                lease.updated_at_epoch_ms
            ],
        )
        .map_err(db_err)?;
        Ok(())
    }

    fn get_active_session(
        &self,
        global_user_id: &str,
    ) -> Result<Option<ActiveSessionLease>, RuntimeStateStoreError> {
        let conn = self.conn.lock().map_err(|_| {
            RuntimeStateStoreError::Database("runtime-state sqlite mutex poisoned".to_string())
        })?;
        let mut stmt = conn
            .prepare(
                "SELECT global_user_id, session_id, channel, thread_id, updated_at_epoch_ms
                 FROM active_sessions
                 WHERE global_user_id = ?1",
            )
            .map_err(db_err)?;
        stmt.query_row(params![global_user_id], Self::row_to_active_session)
            .optional()
            .map_err(db_err)
    }

    fn upsert_scheduled_task(&self, task: &ScheduledTask) -> Result<(), RuntimeStateStoreError> {
        let schedule_payload = serde_json::to_string(&task.schedule)?;
        let schedule_kind = match task.schedule {
            alice_core::runtime_state::domain::ScheduleKind::EveryMinutes(_) => "every_minutes",
            alice_core::runtime_state::domain::ScheduleKind::Hourly { .. } => "hourly",
            alice_core::runtime_state::domain::ScheduleKind::DailyAt { .. } => "daily_at",
        };

        let conn = self.conn.lock().map_err(|_| {
            RuntimeStateStoreError::Database("runtime-state sqlite mutex poisoned".to_string())
        })?;
        conn.execute(
            "INSERT INTO scheduled_tasks
             (task_id, global_user_id, channel, prompt, schedule_kind, schedule_payload, next_run_epoch_ms, enabled, last_run_epoch_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(task_id) DO UPDATE SET
                global_user_id = excluded.global_user_id,
                channel = excluded.channel,
                prompt = excluded.prompt,
                schedule_kind = excluded.schedule_kind,
                schedule_payload = excluded.schedule_payload,
                next_run_epoch_ms = excluded.next_run_epoch_ms,
                enabled = excluded.enabled,
                last_run_epoch_ms = excluded.last_run_epoch_ms",
            params![
                task.task_id,
                task.global_user_id,
                task.channel,
                task.prompt,
                schedule_kind,
                schedule_payload,
                task.next_run_epoch_ms,
                i64::from(task.enabled),
                task.last_run_epoch_ms,
            ],
        )
        .map_err(db_err)?;
        Ok(())
    }

    fn get_scheduled_task(
        &self,
        task_id: &str,
    ) -> Result<Option<ScheduledTask>, RuntimeStateStoreError> {
        let conn = self.conn.lock().map_err(|_| {
            RuntimeStateStoreError::Database("runtime-state sqlite mutex poisoned".to_string())
        })?;
        let mut stmt = conn
            .prepare(
                "SELECT task_id, global_user_id, channel, prompt, schedule_kind, schedule_payload, next_run_epoch_ms, enabled, last_run_epoch_ms
                 FROM scheduled_tasks
                 WHERE task_id = ?1",
            )
            .map_err(db_err)?;
        stmt.query_row(params![task_id], Self::row_to_scheduled_task).optional().map_err(db_err)
    }

    fn list_scheduled_tasks(&self) -> Result<Vec<ScheduledTask>, RuntimeStateStoreError> {
        let conn = self.conn.lock().map_err(|_| {
            RuntimeStateStoreError::Database("runtime-state sqlite mutex poisoned".to_string())
        })?;
        let mut stmt = conn
            .prepare(
                "SELECT task_id, global_user_id, channel, prompt, schedule_kind, schedule_payload, next_run_epoch_ms, enabled, last_run_epoch_ms
                 FROM scheduled_tasks
                 ORDER BY next_run_epoch_ms ASC, task_id ASC",
            )
            .map_err(db_err)?;

        let rows = stmt.query_map([], Self::row_to_scheduled_task).map_err(db_err)?;
        let mut tasks = Vec::new();
        for row in rows {
            tasks.push(row.map_err(db_err)?);
        }
        Ok(tasks)
    }

    fn list_due_tasks(
        &self,
        now_epoch_ms: i64,
    ) -> Result<Vec<ScheduledTask>, RuntimeStateStoreError> {
        let conn = self.conn.lock().map_err(|_| {
            RuntimeStateStoreError::Database("runtime-state sqlite mutex poisoned".to_string())
        })?;
        let mut stmt = conn
            .prepare(
                "SELECT task_id, global_user_id, channel, prompt, schedule_kind, schedule_payload, next_run_epoch_ms, enabled, last_run_epoch_ms
                 FROM scheduled_tasks
                 WHERE enabled = 1 AND next_run_epoch_ms <= ?1
                 ORDER BY next_run_epoch_ms ASC, task_id ASC",
            )
            .map_err(db_err)?;

        let rows =
            stmt.query_map(params![now_epoch_ms], Self::row_to_scheduled_task).map_err(db_err)?;
        let mut tasks = Vec::new();
        for row in rows {
            tasks.push(row.map_err(db_err)?);
        }
        Ok(tasks)
    }
}

#[cfg(test)]
mod tests {
    use alice_core::runtime_state::{
        domain::{ScheduleKind, ScheduledTask},
        ports::RuntimeStateStorePort,
    };

    use super::SqliteRuntimeStateStore;

    #[test]
    fn open_creates_schema() {
        let store = SqliteRuntimeStateStore::in_memory();
        assert!(store.is_ok(), "in-memory store should initialize");
    }

    #[test]
    fn scheduled_task_roundtrip() {
        let Ok(store) = SqliteRuntimeStateStore::in_memory() else {
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

        assert!(store.upsert_scheduled_task(&task).is_ok());
        let Ok(loaded) = store.get_scheduled_task("task-1") else {
            return;
        };
        let Some(loaded) = loaded else {
            panic!("scheduled task should exist");
        };

        assert_eq!(loaded.prompt, task.prompt);
        assert_eq!(loaded.schedule, task.schedule);
    }
}
