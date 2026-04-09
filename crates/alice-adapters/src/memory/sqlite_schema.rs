//! SQLite schema creation for memory storage.

use alice_core::memory::error::MemoryStoreError;
use rusqlite::Connection;

/// Convert `rusqlite::Error` into `MemoryStoreError`.
fn db_err(err: rusqlite::Error) -> MemoryStoreError {
    MemoryStoreError::Database(err.to_string())
}

/// Initialize all required SQLite objects.
pub fn init_schema(
    conn: &Connection,
    vector_dimensions: usize,
    enable_vector: bool,
) -> Result<(), MemoryStoreError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS memories (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            topic TEXT NOT NULL,
            summary TEXT NOT NULL,
            raw_excerpt TEXT NOT NULL,
            keywords TEXT NOT NULL,
            importance TEXT NOT NULL,
            created_at_epoch_ms INTEGER NOT NULL,
            embedding BLOB
        );

        CREATE INDEX IF NOT EXISTS idx_memories_session ON memories(session_id);
        CREATE INDEX IF NOT EXISTS idx_memories_topic ON memories(topic);
        CREATE INDEX IF NOT EXISTS idx_memories_created ON memories(created_at_epoch_ms);

        CREATE TABLE IF NOT EXISTS user_profiles (
            profile_id TEXT PRIMARY KEY,
            summary TEXT NOT NULL,
            traits TEXT NOT NULL,
            updated_at_epoch_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_user_profiles_updated
            ON user_profiles(updated_at_epoch_ms);

        CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
            id,
            session_id,
            topic,
            summary,
            raw_excerpt,
            keywords,
            content='memories',
            content_rowid='rowid'
        );

        CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
            INSERT INTO memories_fts(rowid, id, session_id, topic, summary, raw_excerpt, keywords)
            VALUES (
                new.rowid,
                new.id,
                new.session_id,
                new.topic,
                new.summary,
                new.raw_excerpt,
                new.keywords
            );
        END;

        CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
            INSERT INTO memories_fts(memories_fts, rowid, id, session_id, topic, summary, raw_excerpt, keywords)
            VALUES (
                'delete',
                old.rowid,
                old.id,
                old.session_id,
                old.topic,
                old.summary,
                old.raw_excerpt,
                old.keywords
            );
        END;

        CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
            INSERT INTO memories_fts(memories_fts, rowid, id, session_id, topic, summary, raw_excerpt, keywords)
            VALUES (
                'delete',
                old.rowid,
                old.id,
                old.session_id,
                old.topic,
                old.summary,
                old.raw_excerpt,
                old.keywords
            );

            INSERT INTO memories_fts(rowid, id, session_id, topic, summary, raw_excerpt, keywords)
            VALUES (
                new.rowid,
                new.id,
                new.session_id,
                new.topic,
                new.summary,
                new.raw_excerpt,
                new.keywords
            );
        END;
        ",
    )
    .map_err(db_err)?;

    if enable_vector {
        let dimensions = vector_dimensions.max(1);
        let query = format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS vec_memories USING vec0(\
                memory_id TEXT PRIMARY KEY,\
                embedding float[{dimensions}] distance_metric=cosine\
            )"
        );
        conn.execute_batch(&query).map_err(db_err)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_init_is_idempotent() {
        let conn = Connection::open_in_memory();
        assert!(conn.is_ok(), "open in-memory db should succeed");
        let Ok(conn) = conn else {
            return;
        };
        assert!(init_schema(&conn, 128, false).is_ok());
        assert!(init_schema(&conn, 128, false).is_ok());
    }
}
