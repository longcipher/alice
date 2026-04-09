//! SQLite schema creation for runtime-state storage.

use alice_core::runtime_state::error::RuntimeStateStoreError;
use rusqlite::Connection;

fn db_err(err: rusqlite::Error) -> RuntimeStateStoreError {
    RuntimeStateStoreError::Database(err.to_string())
}

/// Initialize all required SQLite objects.
pub fn init_schema(conn: &Connection) -> Result<(), RuntimeStateStoreError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS identity_bindings (
            provider TEXT NOT NULL,
            external_user_id TEXT NOT NULL,
            global_user_id TEXT NOT NULL,
            bound_at_epoch_ms INTEGER NOT NULL,
            PRIMARY KEY (provider, external_user_id)
        );

        CREATE INDEX IF NOT EXISTS idx_identity_bindings_global_user
            ON identity_bindings(global_user_id);

        CREATE TABLE IF NOT EXISTS bind_tokens (
            token TEXT PRIMARY KEY,
            global_user_id TEXT NOT NULL,
            provider TEXT,
            expires_at_epoch_ms INTEGER NOT NULL,
            consumed_at_epoch_ms INTEGER,
            created_at_epoch_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_bind_tokens_global_user
            ON bind_tokens(global_user_id);

        CREATE TABLE IF NOT EXISTS active_sessions (
            global_user_id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            channel TEXT,
            thread_id TEXT,
            updated_at_epoch_ms INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS scheduled_tasks (
            task_id TEXT PRIMARY KEY,
            global_user_id TEXT NOT NULL,
            channel TEXT,
            prompt TEXT NOT NULL,
            schedule_kind TEXT NOT NULL,
            schedule_payload TEXT NOT NULL,
            next_run_epoch_ms INTEGER NOT NULL,
            enabled INTEGER NOT NULL,
            last_run_epoch_ms INTEGER
        );

        CREATE INDEX IF NOT EXISTS idx_scheduled_tasks_due
            ON scheduled_tasks(enabled, next_run_epoch_ms);
        ",
    )
    .map_err(db_err)?;

    ensure_column_exists(conn, "active_sessions", "thread_id")?;

    Ok(())
}

fn ensure_column_exists(
    conn: &Connection,
    table: &str,
    column: &str,
) -> Result<(), RuntimeStateStoreError> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})")).map_err(db_err)?;
    let mut rows = stmt.query([]).map_err(db_err)?;
    while let Some(row) = rows.next().map_err(db_err)? {
        let existing_column: String = row.get(1).map_err(db_err)?;
        if existing_column == column {
            return Ok(());
        }
    }

    conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {column} TEXT"), []).map_err(db_err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::init_schema;

    #[test]
    fn schema_init_is_idempotent() {
        let conn = Connection::open_in_memory();
        assert!(conn.is_ok(), "open in-memory db should succeed");
        let Ok(conn) = conn else {
            return;
        };

        assert!(init_schema(&conn).is_ok());
        assert!(init_schema(&conn).is_ok());
    }

    #[test]
    fn schema_adds_thread_id_column_for_existing_tables() {
        let conn = Connection::open_in_memory();
        assert!(conn.is_ok(), "open in-memory db should succeed");
        let Ok(conn) = conn else {
            return;
        };

        assert!(
            conn.execute_batch(
                "
                CREATE TABLE active_sessions (
                    global_user_id TEXT PRIMARY KEY,
                    session_id TEXT NOT NULL,
                    channel TEXT,
                    updated_at_epoch_ms INTEGER NOT NULL
                );
                "
            )
            .is_ok()
        );
        assert!(init_schema(&conn).is_ok());

        let stmt = conn.prepare("PRAGMA table_info(active_sessions)");
        assert!(stmt.is_ok());
        let Ok(mut stmt) = stmt else {
            return;
        };
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .expect("table_info query should succeed");
        let columns = rows.collect::<Result<Vec<_>, _>>();
        assert!(columns.is_ok());
        let Ok(columns) = columns else {
            return;
        };

        assert!(columns.iter().any(|column| column == "thread_id"));
    }
}
