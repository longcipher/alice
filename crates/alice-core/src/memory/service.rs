//! Application service wrapping memory recall and persistence workflows.

use std::{
    fmt::Write as _,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::memory::{
    domain::{HybridWeights, MemoryEntry, MemoryImportance, RecallHit, RecallQuery, UserProfile},
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
            let _ = writeln!(
                output,
                "{}. [{}] {}",
                index + 1,
                hit.entry.topic,
                hit.entry.summary.trim()
            );
        }

        Some(output)
    }

    /// Load the durable profile associated with a user identity.
    pub fn load_user_profile(
        &self,
        profile_id: &str,
    ) -> Result<Option<UserProfile>, MemoryServiceError> {
        self.store.get_user_profile(profile_id).map_err(MemoryServiceError::from)
    }

    /// Render a persisted user profile as prompt context.
    #[must_use]
    pub fn render_user_profile_context(profile: &UserProfile) -> Option<String> {
        if profile.summary.trim().is_empty() && profile.traits.is_empty() {
            return None;
        }

        let mut output = String::from("Known user profile:\n");
        if !profile.summary.trim().is_empty() {
            let _ = writeln!(output, "Summary: {}", profile.summary.trim());
        }
        if !profile.traits.is_empty() {
            output.push_str("Traits:\n");
            for trait_line in &profile.traits {
                let _ = writeln!(output, "- {}", trait_line.trim());
            }
        }

        Some(output)
    }

    /// Update a user's durable profile using the latest turn.
    pub fn update_profile_from_turn(
        &self,
        profile_id: &str,
        user_input: &str,
        assistant_output: &str,
    ) -> Result<Option<UserProfile>, MemoryServiceError> {
        let existing = self.store.get_user_profile(profile_id)?;
        let mut traits = existing.as_ref().map_or_else(Vec::new, |profile| profile.traits.clone());

        for trait_line in extract_profile_traits(user_input, assistant_output) {
            if traits.iter().any(|existing| existing.eq_ignore_ascii_case(&trait_line)) {
                continue;
            }
            traits.push(trait_line);
        }

        if traits.is_empty() {
            return Ok(existing);
        }

        let profile = UserProfile {
            profile_id: profile_id.to_string(),
            summary: build_profile_summary(&traits),
            traits: cap_profile_traits(traits),
            updated_at_epoch_ms: current_time_millis(),
        };

        self.store.upsert_user_profile(&profile)?;
        Ok(Some(profile))
    }

    /// Persist one turn as a memory entry.
    pub fn persist_turn(
        &self,
        session_id: &str,
        user_input: &str,
        assistant_output: &str,
    ) -> Result<(), MemoryServiceError> {
        let now_ms = current_time_millis();
        let counter = MEMORY_COUNTER.fetch_add(1, Ordering::Relaxed);
        let id = format!("mem-{now_ms}-{counter:04}");

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

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

fn truncate(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
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

fn split_sentences(input: &str) -> impl Iterator<Item = &str> {
    input.split(['.', '!', '?', '\n', ';'])
}

fn looks_like_profile_signal(sentence: &str) -> bool {
    let trimmed = sentence.trim();
    if trimmed.is_empty() {
        return false;
    }

    let lower = trimmed.to_lowercase();
    ["i ", "i'm", "i am", "my ", "we ", "our ", "prefer", "project", "repo", "working on", "use "]
        .iter()
        .any(|marker| lower.contains(marker)) ||
        ["我", "我们", "项目", "偏好"].iter().any(|marker| trimmed.contains(marker))
}

fn extract_profile_traits(user_input: &str, _assistant_output: &str) -> Vec<String> {
    let mut traits = Vec::new();

    for sentence in split_sentences(user_input) {
        let trimmed = sentence.trim();
        if !looks_like_profile_signal(trimmed) {
            continue;
        }

        let trait_line = truncate(trimmed, 160);
        if trait_line.is_empty() || traits.iter().any(|existing| existing == &trait_line) {
            continue;
        }
        traits.push(trait_line);
    }

    cap_profile_traits(traits)
}

fn cap_profile_traits(mut traits: Vec<String>) -> Vec<String> {
    traits.truncate(8);
    traits
}

fn build_profile_summary(traits: &[String]) -> String {
    let mut summary_parts = traits.iter().take(3).map(|item| item.trim()).collect::<Vec<_>>();
    if summary_parts.is_empty() {
        return String::new();
    }

    let mut summary = summary_parts.remove(0).to_string();
    if !summary_parts.is_empty() {
        summary.push(' ');
        summary.push_str(&summary_parts.join(" "));
    }
    truncate(summary.trim(), 240)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use parking_lot::Mutex;

    use super::MemoryService;
    use crate::memory::{
        domain::{
            HybridWeights, MemoryEntry, MemoryImportance, RecallHit, RecallQuery, UserProfile,
        },
        error::MemoryStoreError,
        ports::MemoryStorePort,
    };

    #[derive(Debug, Default)]
    struct MockStore {
        rows: Mutex<Vec<MemoryEntry>>,
        profiles: Mutex<Vec<UserProfile>>,
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

        fn upsert_user_profile(&self, profile: &UserProfile) -> Result<(), MemoryStoreError> {
            let mut profiles = self.profiles.lock();
            if let Some(existing) =
                profiles.iter_mut().find(|item| item.profile_id == profile.profile_id)
            {
                *existing = profile.clone();
            } else {
                profiles.push(profile.clone());
            }
            Ok(())
        }

        fn get_user_profile(
            &self,
            profile_id: &str,
        ) -> Result<Option<UserProfile>, MemoryStoreError> {
            Ok(self
                .profiles
                .lock()
                .iter()
                .find(|profile| profile.profile_id == profile_id)
                .cloned())
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

    /// `recall_for_turn` populates the correct session filter in the query.
    #[test]
    fn recall_for_turn_uses_mock_store() {
        let mock = Arc::new(MockStore::default());
        let store: Arc<dyn MemoryStorePort> = Arc::clone(&mock) as _;
        let Ok(service) = MemoryService::new(store, 3, HybridWeights::default(), 32, false) else {
            return;
        };

        // Insert two entries for different sessions.
        assert!(service.persist_turn("s-a", "hi", "hello").is_ok());
        assert!(service.persist_turn("s-b", "bye", "farewell").is_ok());

        let Ok(hits) = service.recall_for_turn("s-a", "hi") else {
            return;
        };
        // Only the s-a entry should match.
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entry.session_id, "s-a");
    }

    /// `render_recall_context` formats hits with numbered topic/summary lines.
    #[test]
    fn render_recall_context_with_hits() {
        let entry = MemoryEntry {
            id: "m1".to_string(),
            session_id: "s1".to_string(),
            topic: "rust".to_string(),
            summary: "ownership rules".to_string(),
            raw_excerpt: String::new(),
            keywords: vec![],
            importance: MemoryImportance::Medium,
            embedding: None,
            created_at_epoch_ms: 0,
        };
        let hit = RecallHit { entry, bm25_score: 0.5, vector_score: Some(0.5), final_score: 0.5 };
        let rendered = MemoryService::render_recall_context(&[hit]);
        assert!(rendered.is_some());
        let Ok(text) = rendered.ok_or("none") else {
            return;
        };
        assert!(text.contains("1."));
        assert!(text.contains("[rust]"));
        assert!(text.contains("ownership rules"));
    }

    /// Service respects `recall_limit` — cannot be zero.
    #[test]
    fn recall_limit_must_be_positive() {
        let store: Arc<dyn MemoryStorePort> = Arc::new(MockStore::default());
        let result = MemoryService::new(store, 0, HybridWeights::default(), 128, false);
        assert!(result.is_err());
    }

    #[test]
    fn update_profile_from_turn_extracts_traits() {
        let store: Arc<dyn MemoryStorePort> = Arc::new(MockStore::default());
        let Ok(service) = MemoryService::new(store, 5, HybridWeights::default(), 128, false) else {
            return;
        };

        let profile = service.update_profile_from_turn(
            "user-1",
            "I prefer Rust for agent runtimes. Our Alice project uses ACP for execution.",
            "Acknowledged.",
        );
        assert!(profile.is_ok(), "profile update should succeed");
        let Ok(profile) = profile else {
            return;
        };
        let Some(profile) = profile else {
            panic!("profile should be created from durable user signals");
        };

        assert_eq!(profile.profile_id, "user-1");
        assert!(profile.summary.contains("Rust"));
        assert_eq!(profile.traits.len(), 2);

        let rendered = MemoryService::render_user_profile_context(&profile);
        assert!(rendered.is_some(), "profile context should render when traits exist");
        let Some(rendered) = rendered else {
            return;
        };
        assert!(rendered.contains("Known user profile"));
        assert!(rendered.contains("Traits:"));
    }
}
