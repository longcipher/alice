//! SQLite-backed memory store.

use std::{collections::HashMap, path::Path, sync::Once};

use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};

use crate::memory::{
    domain::{HybridWeights, MemoryEntry, MemoryImportance, RecallHit, RecallQuery},
    error::MemoryStoreError,
    hybrid::{fuse_scores, normalize_bm25_rank, sanitize_fts_query},
    ports::MemoryStorePort,
    sqlite_schema,
};

static SQLITE_VEC_INIT: Once = Once::new();

fn ensure_sqlite_vec() {
    SQLITE_VEC_INIT.call_once(|| {
        // SAFETY: sqlite3_auto_extension expects a function pointer compatible with sqlite
        // extension init.
        unsafe {
            type SqliteEntryPoint = unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut std::os::raw::c_char,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> std::os::raw::c_int;

            let init_fn = std::mem::transmute::<*const (), SqliteEntryPoint>(
                sqlite_vec::sqlite3_vec_init as *const (),
            );
            rusqlite::ffi::sqlite3_auto_extension(Some(init_fn));
        }
    });
}

/// SQLite-backed implementation of [`MemoryStorePort`].
#[derive(Debug)]
pub struct SqliteMemoryStore {
    conn: Mutex<Connection>,
    vector_dimensions: usize,
    enable_vector: bool,
}

impl SqliteMemoryStore {
    /// Open a file-backed SQLite store.
    pub fn open(
        path: impl AsRef<Path>,
        vector_dimensions: usize,
        enable_vector: bool,
    ) -> Result<Self, MemoryStoreError> {
        if enable_vector {
            ensure_sqlite_vec();
        }

        let path = path.as_ref();
        if let Some(parent) = path.parent() &&
            !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|error| MemoryStoreError::Database(error.to_string()))?;
        }

        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

        let store = Self {
            conn: Mutex::new(conn),
            vector_dimensions: vector_dimensions.max(1),
            enable_vector,
        };
        store.init_schema()?;
        Ok(store)
    }

    /// Open an in-memory SQLite store for tests.
    pub fn in_memory(
        vector_dimensions: usize,
        enable_vector: bool,
    ) -> Result<Self, MemoryStoreError> {
        if enable_vector {
            ensure_sqlite_vec();
        }

        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;

        let store = Self {
            conn: Mutex::new(conn),
            vector_dimensions: vector_dimensions.max(1),
            enable_vector,
        };
        store.init_schema()?;
        Ok(store)
    }

    fn normalize_embedding(&self, embedding: Option<&[f32]>) -> Option<Vec<f32>> {
        let values = embedding?;
        let mut normalized = values.to_vec();
        normalized.resize(self.vector_dimensions, 0.0);
        normalized.truncate(self.vector_dimensions);
        Some(normalized)
    }

    fn embedding_to_blob(values: &[f32]) -> Vec<u8> {
        let mut output = Vec::with_capacity(values.len() * 4);
        for value in values {
            output.extend_from_slice(&value.to_le_bytes());
        }
        output
    }

    fn blob_to_embedding(blob: &[u8]) -> Vec<f32> {
        blob.chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect()
    }

    fn row_to_entry(row: &rusqlite::Row<'_>) -> Result<MemoryEntry, rusqlite::Error> {
        let keywords_json: String = row.get(5)?;
        let keywords: Vec<String> = serde_json::from_str(&keywords_json).unwrap_or_default();

        let importance: String = row.get(6)?;
        let embedding_blob: Option<Vec<u8>> = row.get(8)?;

        Ok(MemoryEntry {
            id: row.get(0)?,
            session_id: row.get(1)?,
            topic: row.get(2)?,
            summary: row.get(3)?,
            raw_excerpt: row.get(4)?,
            keywords,
            importance: MemoryImportance::from_db(&importance),
            embedding: embedding_blob.as_deref().map(Self::blob_to_embedding),
            created_at_epoch_ms: row.get(7)?,
        })
    }

    fn get_memory_by_id(&self, id: &str) -> Result<Option<MemoryEntry>, MemoryStoreError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, topic, summary, raw_excerpt, keywords, importance, created_at_epoch_ms, embedding
             FROM memories WHERE id = ?1",
        )?;
        let item = stmt.query_row(params![id], Self::row_to_entry).optional()?;
        Ok(item)
    }

    fn fts_candidates(
        &self,
        query: &RecallQuery,
        limit: usize,
    ) -> Result<HashMap<String, (MemoryEntry, f32)>, MemoryStoreError> {
        let mut output = HashMap::new();
        let sanitized = sanitize_fts_query(&query.text);
        if sanitized.is_empty() {
            return Ok(output);
        }

        let conn = self.conn.lock();
        if let Some(session_id) = &query.session_id {
            let mut stmt = conn.prepare(
                "SELECT m.id, m.session_id, m.topic, m.summary, m.raw_excerpt, m.keywords,
                        m.importance, m.created_at_epoch_ms, m.embedding,
                        bm25(memories_fts) AS rank
                 FROM memories_fts
                 JOIN memories m ON m.rowid = memories_fts.rowid
                 WHERE memories_fts MATCH ?1 AND m.session_id = ?2
                 ORDER BY rank
                 LIMIT ?3",
            )?;

            let rows = stmt.query_map(params![sanitized, session_id, limit as i64], |row| {
                let entry = Self::row_to_entry(row)?;
                let rank: f32 = row.get(9)?;
                Ok((entry, rank))
            })?;

            for row in rows {
                let (entry, rank) = row?;
                output.insert(entry.id.clone(), (entry, normalize_bm25_rank(rank)));
            }
            return Ok(output);
        }

        let mut stmt = conn.prepare(
            "SELECT m.id, m.session_id, m.topic, m.summary, m.raw_excerpt, m.keywords,
                    m.importance, m.created_at_epoch_ms, m.embedding,
                    bm25(memories_fts) AS rank
             FROM memories_fts
             JOIN memories m ON m.rowid = memories_fts.rowid
             WHERE memories_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![sanitized, limit as i64], |row| {
            let entry = Self::row_to_entry(row)?;
            let rank: f32 = row.get(9)?;
            Ok((entry, rank))
        })?;

        for row in rows {
            let (entry, rank) = row?;
            output.insert(entry.id.clone(), (entry, normalize_bm25_rank(rank)));
        }

        Ok(output)
    }

    fn vector_candidates(
        &self,
        query: &RecallQuery,
        limit: usize,
    ) -> Result<HashMap<String, f32>, MemoryStoreError> {
        let mut output = HashMap::new();
        if !self.enable_vector {
            return Ok(output);
        }

        let Some(query_embedding) = query.query_embedding.as_deref() else {
            return Ok(output);
        };

        let Some(normalized) = self.normalize_embedding(Some(query_embedding)) else {
            return Ok(output);
        };
        let blob = Self::embedding_to_blob(&normalized);

        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT memory_id, distance
             FROM vec_memories
             WHERE embedding MATCH ?1
             ORDER BY distance
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![blob, limit as i64], |row| {
            let memory_id: String = row.get(0)?;
            let distance: f32 = row.get(1)?;
            Ok((memory_id, distance))
        })?;

        for row in rows {
            let (memory_id, distance) = row?;
            let similarity = (1.0 - distance).clamp(0.0, 1.0);
            output.insert(memory_id, similarity);
        }

        Ok(output)
    }
}

impl MemoryStorePort for SqliteMemoryStore {
    fn init_schema(&self) -> Result<(), MemoryStoreError> {
        let conn = self.conn.lock();
        sqlite_schema::init_schema(&conn, self.vector_dimensions, self.enable_vector)
    }

    fn insert(&self, entry: &MemoryEntry) -> Result<(), MemoryStoreError> {
        let keywords = serde_json::to_string(&entry.keywords)?;
        let embedding = self.normalize_embedding(entry.embedding.as_deref());
        let embedding_blob = embedding.as_deref().map(Self::embedding_to_blob).unwrap_or_default();

        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO memories (id, session_id, topic, summary, raw_excerpt, keywords, importance, created_at_epoch_ms, embedding)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                entry.id,
                entry.session_id,
                entry.topic,
                entry.summary,
                entry.raw_excerpt,
                keywords,
                entry.importance.as_str(),
                entry.created_at_epoch_ms,
                if embedding_blob.is_empty() {
                    None
                } else {
                    Some(embedding_blob.as_slice())
                },
            ],
        )?;

        if self.enable_vector &&
            let Some(embedding_values) = embedding
        {
            let blob = Self::embedding_to_blob(&embedding_values);
            conn.execute(
                "INSERT INTO vec_memories (memory_id, embedding) VALUES (?1, ?2)",
                params![entry.id, blob],
            )?;
        }

        Ok(())
    }

    fn recall_hybrid(
        &self,
        query: &RecallQuery,
        weights: HybridWeights,
    ) -> Result<Vec<RecallHit>, MemoryStoreError> {
        let limit = query.limit.max(1);
        let pool = limit.saturating_mul(4).max(limit);

        let mut by_id = self.fts_candidates(query, pool)?;
        let vector_scores = self.vector_candidates(query, pool)?;

        for id in vector_scores.keys() {
            if by_id.contains_key(id) {
                continue;
            }
            if let Some(entry) = self.get_memory_by_id(id)? {
                if let Some(session_id) = &query.session_id &&
                    &entry.session_id != session_id
                {
                    continue;
                }
                by_id.insert(id.clone(), (entry, 0.0));
            }
        }

        let mut hits = by_id
            .into_iter()
            .map(|(id, (entry, bm25_score))| {
                let vector_score = vector_scores.get(&id).copied();
                let final_score = fuse_scores(bm25_score, vector_score, weights);
                RecallHit { entry, bm25_score, vector_score, final_score }
            })
            .collect::<Vec<_>>();

        hits.sort_by(|left, right| {
            right
                .final_score
                .total_cmp(&left.final_score)
                .then_with(|| left.entry.id.cmp(&right.entry.id))
        });
        hits.truncate(limit);

        Ok(hits)
    }
}
