//! Integration tests for SQLite memory store behavior.

use common::memory::{
    domain::{HybridWeights, MemoryEntry, MemoryImportance, RecallQuery},
    ports::MemoryStorePort,
    sqlite_store::SqliteMemoryStore,
};

fn memory(id: &str, session_id: &str, text: &str, embedding: Option<Vec<f32>>) -> MemoryEntry {
    MemoryEntry {
        id: id.to_string(),
        session_id: session_id.to_string(),
        topic: session_id.to_string(),
        summary: text.to_string(),
        raw_excerpt: text.to_string(),
        keywords: vec!["memory".to_string(), "test".to_string()],
        importance: MemoryImportance::Medium,
        embedding,
        created_at_epoch_ms: 1,
    }
}

#[test]
fn init_schema_is_idempotent() {
    let store = SqliteMemoryStore::in_memory(384, true);
    assert!(store.is_ok(), "in-memory store should initialize");
    let Ok(store) = store else {
        return;
    };
    assert!(store.init_schema().is_ok(), "first init should pass");
    assert!(store.init_schema().is_ok(), "second init should pass");
}

#[test]
fn recall_fts_returns_match() {
    let store = SqliteMemoryStore::in_memory(384, false);
    assert!(store.is_ok(), "in-memory store should initialize");
    let Ok(store) = store else {
        return;
    };
    assert!(store.init_schema().is_ok(), "schema initialization should pass");

    let inserted = store.insert(&memory("m1", "s1", "sqlite fts memory retrieval works", None));
    assert!(inserted.is_ok(), "insert should pass");

    let results = store.recall_hybrid(
        &RecallQuery {
            session_id: Some("s1".to_string()),
            text: "fts retrieval".to_string(),
            query_embedding: None,
            limit: 5,
        },
        HybridWeights::new(0.3, 0.7).unwrap_or_default(),
    );
    assert!(results.is_ok(), "recall should pass");
    let Ok(results) = results else {
        return;
    };

    assert!(!results.is_empty());
    assert_eq!(results[0].entry.id, "m1");
}

#[test]
fn recall_hybrid_includes_vector_signal() {
    let store = SqliteMemoryStore::in_memory(384, true);
    assert!(store.is_ok(), "in-memory store should initialize");
    let Ok(store) = store else {
        return;
    };
    assert!(store.init_schema().is_ok(), "schema initialization should pass");

    let mut near = vec![0.0_f32; 384];
    near[0] = 1.0;

    let mut far = vec![0.0_f32; 384];
    far[1] = 1.0;

    let inserted_near =
        store.insert(&memory("near", "s2", "vector candidate near", Some(near.clone())));
    assert!(inserted_near.is_ok(), "near vector insert should pass");
    let inserted_far = store.insert(&memory("far", "s2", "vector candidate far", Some(far)));
    assert!(inserted_far.is_ok(), "far vector insert should pass");

    let results = store.recall_hybrid(
        &RecallQuery {
            session_id: Some("s2".to_string()),
            text: "vector candidate".to_string(),
            query_embedding: Some(near),
            limit: 5,
        },
        HybridWeights::new(0.3, 0.7).unwrap_or_default(),
    );
    assert!(results.is_ok(), "hybrid recall should pass");
    let Ok(results) = results else {
        return;
    };

    assert!(!results.is_empty());
    assert_eq!(results[0].entry.id, "near");
}
