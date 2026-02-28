//! Application service wrapping memory recall and persistence workflows.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use crate::memory::{
    domain::{HybridWeights, MemoryEntry, MemoryImportance, RecallHit, RecallQuery},
    error::{MemoryServiceError, MemoryValidationError},
    hybrid::simple_text_embedding,
    ports::MemoryStorePort,
};

static MEMORY_COUNTER: AtomicU64 = AtomicU64::new(1);

/// High-level memory use-cases for Alice runtime integration.
pub struct MemoryService {
    store: Arc<dyn MemoryStorePort>,
    recall_limit: usize,
    weights: HybridWeights,
    vector_dimensions: usize,
    enable_vector: bool,
}

impl std::fmt::Debug for MemoryService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryService")
            .field("recall_limit", &self.recall_limit)
            .field("weights", &self.weights)
            .field("vector_dimensions", &self.vector_dimensions)
            .field("enable_vector", &self.enable_vector)
            .finish_non_exhaustive()
    }
}

impl MemoryService {
    /// Construct a memory service and initialize store schema.
    pub fn new(
        store: Arc<dyn MemoryStorePort>,
        recall_limit: usize,
        weights: HybridWeights,
        vector_dimensions: usize,
        enable_vector: bool,
    ) -> Result<Self, MemoryServiceError> {
        if recall_limit == 0 {
            return Err(MemoryValidationError::InvalidRecallLimit.into());
        }
        store.init_schema()?;
        Ok(Self {
            store,
            recall_limit,
            weights,
            vector_dimensions: vector_dimensions.max(1),
            enable_vector,
        })
    }

    /// Recall memory hits relevant to current input.
    pub fn recall_for_turn(
        &self,
        session_id: &str,
        input: &str,
    ) -> Result<Vec<RecallHit>, MemoryServiceError> {
        let query_embedding =
            self.enable_vector.then(|| simple_text_embedding(input, self.vector_dimensions));

        let query = RecallQuery {
            session_id: Some(session_id.to_string()),
            text: input.to_string(),
            query_embedding,
            limit: self.recall_limit,
        };

        self.store.recall_hybrid(&query, self.weights).map_err(MemoryServiceError::from)
    }

    /// Render recalled memory as prompt context.
    #[must_use]
    pub fn render_recall_context(hits: &[RecallHit]) -> Option<String> {
        if hits.is_empty() {
            return None;
        }

        let mut output = String::from("Relevant prior memory:\n");
        for (index, hit) in hits.iter().enumerate() {
            let number = index + 1;
            output.push_str(&format!(
                "{number}. [{}] {}\n",
                hit.entry.topic,
                hit.entry.summary.trim()
            ));
        }

        Some(output)
    }

    /// Persist one turn as a memory entry.
    pub fn persist_turn(
        &self,
        session_id: &str,
        user_input: &str,
        assistant_output: &str,
    ) -> Result<(), MemoryServiceError> {
        let now_ms = current_time_millis();
        let id = format!("mem-{now_ms}-{}", MEMORY_COUNTER.fetch_add(1, Ordering::Relaxed));

        let summary = truncate(assistant_output.trim(), 300);
        let raw_excerpt =
            format!("user: {}\nassistant: {}", user_input.trim(), assistant_output.trim());

        let embedding = self.enable_vector.then(|| {
            simple_text_embedding(
                &format!("{} {}", user_input.trim(), assistant_output.trim()),
                self.vector_dimensions,
            )
        });

        let entry = MemoryEntry {
            id,
            session_id: session_id.to_string(),
            topic: session_id.to_string(),
            summary,
            raw_excerpt,
            keywords: extract_keywords(user_input, assistant_output),
            importance: MemoryImportance::Medium,
            embedding,
            created_at_epoch_ms: now_ms,
        };

        self.store.insert(&entry)?;
        Ok(())
    }
}

fn current_time_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    let duration = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(value) => value,
        Err(_) => return 0,
    };
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

fn truncate(input: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for (idx, ch) in input.chars().enumerate() {
        if idx >= max_chars {
            break;
        }
        output.push(ch);
    }
    output
}

fn extract_keywords(user_input: &str, assistant_output: &str) -> Vec<String> {
    let mut keywords = Vec::new();
    for token in user_input
        .split_whitespace()
        .chain(assistant_output.split_whitespace())
        .map(|token| token.trim_matches(|ch: char| !ch.is_ascii_alphanumeric()).to_lowercase())
        .filter(|token| token.len() >= 4)
    {
        if keywords.iter().any(|existing| existing == &token) {
            continue;
        }
        keywords.push(token);
        if keywords.len() >= 12 {
            break;
        }
    }
    if keywords.is_empty() {
        keywords.push("conversation".to_string());
    }
    keywords
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use parking_lot::Mutex;

    use super::MemoryService;
    use crate::memory::{
        domain::{HybridWeights, MemoryEntry, MemoryImportance, RecallHit, RecallQuery},
        error::MemoryStoreError,
        ports::MemoryStorePort,
    };

    #[derive(Debug, Default)]
    struct MockStore {
        rows: Mutex<Vec<MemoryEntry>>,
    }

    impl MemoryStorePort for MockStore {
        fn init_schema(&self) -> Result<(), MemoryStoreError> {
            Ok(())
        }

        fn insert(&self, entry: &MemoryEntry) -> Result<(), MemoryStoreError> {
            self.rows.lock().push(entry.clone());
            Ok(())
        }

        fn recall_hybrid(
            &self,
            query: &RecallQuery,
            _weights: HybridWeights,
        ) -> Result<Vec<RecallHit>, MemoryStoreError> {
            let rows = self
                .rows
                .lock()
                .iter()
                .filter(|row| {
                    query.session_id.as_ref().is_none_or(|session_id| &row.session_id == session_id)
                })
                .cloned()
                .collect::<Vec<_>>();

            Ok(rows
                .into_iter()
                .map(|entry| RecallHit {
                    entry,
                    bm25_score: 0.5,
                    vector_score: Some(0.5),
                    final_score: 0.5,
                })
                .collect())
        }
    }

    #[test]
    fn render_empty_hits_returns_none() {
        assert!(MemoryService::render_recall_context(&[]).is_none());
    }

    #[test]
    fn persist_then_recall_roundtrip() {
        let store: Arc<dyn MemoryStorePort> = Arc::new(MockStore::default());
        let service = MemoryService::new(store, 5, HybridWeights::default(), 128, false);
        assert!(service.is_ok(), "service construction should succeed");
        let Ok(service) = service else {
            return;
        };

        assert!(service.persist_turn("s1", "user asks", "assistant answers").is_ok());
        let hits = service.recall_for_turn("s1", "asks");
        assert!(hits.is_ok(), "recall should succeed");
        let Ok(hits) = hits else {
            return;
        };

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entry.importance, MemoryImportance::Medium);
    }
}
