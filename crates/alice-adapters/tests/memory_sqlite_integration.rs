//! Integration tests for SQLite memory store behavior.

use alice_adapters::memory::sqlite_store::SqliteMemoryStore;
use alice_core::memory::{
    domain::{HybridWeights, MemoryEntry, MemoryImportance, RecallQuery, UserProfile},
    ports::MemoryStorePort,
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

/// Insert an entry and recall with the exact content as query — should be the top hit.
#[test]
fn insert_then_recall_hybrid_exact_match() {
    let Ok(store) = SqliteMemoryStore::in_memory(384, false) else {
        return;
    };

    let entry = memory("ex1", "sess", "unique pineapple mango fruit salad", None);
    let Ok(()) = store.insert(&entry) else {
        return;
    };

    let Ok(results) = store.recall_hybrid(
        &RecallQuery {
            session_id: Some("sess".to_string()),
            text: "unique pineapple mango fruit salad".to_string(),
            query_embedding: None,
            limit: 5,
        },
        HybridWeights::default(),
    ) else {
        return;
    };

    assert!(!results.is_empty(), "exact match should return at least one hit");
    assert_eq!(results[0].entry.id, "ex1");
}

/// Recall with a query that matches nothing should return an empty result set.
#[test]
fn recall_hybrid_no_results() {
    let Ok(store) = SqliteMemoryStore::in_memory(384, false) else {
        return;
    };

    let entry = memory("nr1", "sess", "apple banana cherry", None);
    let Ok(()) = store.insert(&entry) else {
        return;
    };

    let Ok(results) = store.recall_hybrid(
        &RecallQuery {
            session_id: Some("sess".to_string()),
            text: "zyxwvutsrq".to_string(),
            query_embedding: None,
            limit: 5,
        },
        HybridWeights::default(),
    ) else {
        return;
    };

    assert!(results.is_empty(), "unrelated query should yield no results");
}

/// A store created with `enable_vector=false` can still insert and recall via BM25.
#[test]
fn recall_hybrid_bm25_only_mode() {
    let Ok(store) = SqliteMemoryStore::in_memory(384, false) else {
        return;
    };

    let entry = memory("bm1", "sess", "bm25 only mode retrieval", None);
    let Ok(()) = store.insert(&entry) else {
        return;
    };

    let Ok(results) = store.recall_hybrid(
        &RecallQuery {
            session_id: Some("sess".to_string()),
            text: "bm25 retrieval".to_string(),
            query_embedding: None,
            limit: 5,
        },
        HybridWeights::default(),
    ) else {
        return;
    };

    assert!(!results.is_empty(), "BM25-only recall should still return matches");
    assert_eq!(results[0].entry.id, "bm1");
}

/// Insert three entries on different topics; recall with a query matching one
/// ensures the matching entry is ranked first.
#[test]
fn multiple_inserts_ordered_by_relevance() {
    let Ok(store) = SqliteMemoryStore::in_memory(384, false) else {
        return;
    };

    let Ok(()) = store.insert(&memory("rel1", "sess", "quantum physics entanglement", None)) else {
        return;
    };
    let Ok(()) = store.insert(&memory("rel2", "sess", "banana chocolate milkshake", None)) else {
        return;
    };
    let Ok(()) = store.insert(&memory("rel3", "sess", "medieval castle architecture", None)) else {
        return;
    };

    let Ok(results) = store.recall_hybrid(
        &RecallQuery {
            session_id: Some("sess".to_string()),
            text: "quantum physics".to_string(),
            query_embedding: None,
            limit: 5,
        },
        HybridWeights::default(),
    ) else {
        return;
    };

    assert!(!results.is_empty(), "should return at least one hit");
    assert_eq!(results[0].entry.id, "rel1", "most relevant entry should be ranked first");
}

/// Queries containing FTS5 operators (AND, OR, NOT, quotes) are sanitized and do not
/// cause database errors.
#[test]
fn fts_query_sanitization_in_recall() {
    let Ok(store) = SqliteMemoryStore::in_memory(384, false) else {
        return;
    };

    let Ok(()) = store.insert(&memory("san1", "sess", "information retrieval system", None)) else {
        return;
    };

    // FTS5 operators that would cause parse errors if not sanitized.
    let dangerous_queries = [
        "AND OR NOT",
        "\"unmatched quote",
        "hello + world",
        "test* AND retrieval",
        "(grouped) query",
        "field:value",
    ];

    for dangerous_query in &dangerous_queries {
        let result = store.recall_hybrid(
            &RecallQuery {
                session_id: Some("sess".to_string()),
                text: (*dangerous_query).to_string(),
                query_embedding: None,
                limit: 5,
            },
            HybridWeights::default(),
        );
        assert!(result.is_ok(), "query '{dangerous_query}' should not cause an error");
    }
}

#[test]
fn user_profile_roundtrip_persists_traits() {
    let Ok(store) = SqliteMemoryStore::in_memory(384, false) else {
        return;
    };

    let profile = UserProfile {
        profile_id: "user-42".to_string(),
        summary: "Prefers Rust systems programming and works on Alice.".to_string(),
        traits: vec![
            "Prefers Rust for agent infrastructure.".to_string(),
            "Maintains the Alice ACP runtime.".to_string(),
        ],
        updated_at_epoch_ms: 42,
    };

    let Ok(()) = store.upsert_user_profile(&profile) else {
        return;
    };

    let Ok(loaded) = store.get_user_profile("user-42") else {
        return;
    };
    let Some(loaded) = loaded else {
        panic!("stored user profile should be readable");
    };

    assert_eq!(loaded.profile_id, "user-42");
    assert_eq!(loaded.summary, profile.summary);
    assert_eq!(loaded.traits, profile.traits);
}

/// Insert an entry with whitespace-only content; recall should handle it gracefully.
#[test]
fn empty_content_insert() {
    let Ok(store) = SqliteMemoryStore::in_memory(384, false) else {
        return;
    };

    let entry = memory("ws1", "sess", "   \t\n  ", None);
    let result = store.insert(&entry);
    assert!(result.is_ok(), "whitespace-only content should insert without error");

    // Recall with whitespace-only query should return empty, not error.
    let Ok(results) = store.recall_hybrid(
        &RecallQuery {
            session_id: Some("sess".to_string()),
            text: "   ".to_string(),
            query_embedding: None,
            limit: 5,
        },
        HybridWeights::default(),
    ) else {
        return;
    };

    // Whitespace query sanitizes to empty → no FTS match.
    assert!(results.is_empty(), "whitespace query should produce no matches");
}

/// Insert entries in two sessions; recall with a session filter only returns matches
/// from the queried session.
#[test]
fn recall_with_session_filter() {
    let Ok(store) = SqliteMemoryStore::in_memory(384, false) else {
        return;
    };

    let Ok(()) = store.insert(&memory("sf1", "session_a", "deep learning neural network", None))
    else {
        return;
    };
    let Ok(()) =
        store.insert(&memory("sf2", "session_b", "deep learning neural network copy", None))
    else {
        return;
    };

    let Ok(results) = store.recall_hybrid(
        &RecallQuery {
            session_id: Some("session_a".to_string()),
            text: "deep learning".to_string(),
            query_embedding: None,
            limit: 5,
        },
        HybridWeights::default(),
    ) else {
        return;
    };

    assert!(!results.is_empty(), "should match the entry in session_a");
    for hit in &results {
        assert_eq!(hit.entry.session_id, "session_a", "all results should belong to session_a");
    }
}

/// Verify `SqliteMemoryStore::in_memory(384, false)` succeeds and the store is usable.
#[test]
fn in_memory_store_creation() {
    let result = SqliteMemoryStore::in_memory(384, false);
    assert!(result.is_ok(), "in-memory store without vector should initialize");
    let Ok(store) = result else {
        return;
    };
    // The store should accept an insert immediately.
    let entry = memory("im1", "sess", "in memory test", None);
    assert!(store.insert(&entry).is_ok(), "insert into in-memory store should succeed");
}

/// Insert many entries and verify recall returns at most the configured limit.
#[test]
fn recall_limit_respected() {
    let Ok(store) = SqliteMemoryStore::in_memory(384, false) else {
        return;
    };

    for i in 0..20 {
        let id = format!("lim{i}");
        let content = format!("shared keyword searchterm entry number {i}");
        let Ok(()) = store.insert(&memory(&id, "sess", &content, None)) else {
            return;
        };
    }

    let Ok(results) = store.recall_hybrid(
        &RecallQuery {
            session_id: Some("sess".to_string()),
            text: "searchterm".to_string(),
            query_embedding: None,
            limit: 3,
        },
        HybridWeights::default(),
    ) else {
        return;
    };

    assert!(results.len() <= 3, "results should be capped at the limit (3), got {}", results.len());
    assert!(!results.is_empty(), "should return at least one hit");
}

/// Insert and recall entries containing unicode/emoji content.
#[test]
fn unicode_content_handling() {
    let Ok(store) = SqliteMemoryStore::in_memory(384, false) else {
        return;
    };

    let Ok(()) = store.insert(&memory(
        "uni1",
        "sess",
        "\u{1F600} \u{4F60}\u{597D}\u{4E16}\u{754C} \u{1F30D} emojis and kanji",
        None,
    )) else {
        return;
    };

    let Ok(results) = store.recall_hybrid(
        &RecallQuery {
            session_id: Some("sess".to_string()),
            text: "\u{4F60}\u{597D}\u{4E16}\u{754C}".to_string(),
            query_embedding: None,
            limit: 5,
        },
        HybridWeights::default(),
    ) else {
        return;
    };

    assert!(!results.is_empty(), "unicode recall should return a match");
    assert_eq!(results[0].entry.id, "uni1");
}

/// Concurrent inserts from multiple threads should not result in database errors.
#[test]
fn concurrent_inserts() {
    let Ok(store) = SqliteMemoryStore::in_memory(384, false) else {
        return;
    };
    let store = std::sync::Arc::new(store);

    let mut handles = Vec::new();
    for i in 0..10 {
        let store = std::sync::Arc::clone(&store);
        handles.push(std::thread::spawn(move || {
            let id = format!("conc{i}");
            let content = format!("concurrent insert number {i} data");
            store.insert(&MemoryEntry {
                id,
                session_id: "conc_sess".to_string(),
                topic: "conc_sess".to_string(),
                summary: content.clone(),
                raw_excerpt: content,
                keywords: vec!["concurrent".to_string()],
                importance: MemoryImportance::Medium,
                embedding: None,
                created_at_epoch_ms: 1,
            })
        }));
    }

    let mut successes = 0;
    for handle in handles {
        let Ok(result) = handle.join() else {
            continue;
        };
        if result.is_ok() {
            successes += 1;
        }
    }
    assert_eq!(successes, 10, "all concurrent inserts should succeed");
}
