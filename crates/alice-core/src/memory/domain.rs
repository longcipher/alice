//! Domain types for the memory subsystem.

use serde::{Deserialize, Serialize};

use crate::memory::error::MemoryValidationError;

/// Importance level for memory entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryImportance {
    /// Low signal.
    Low,
    /// Default level.
    Medium,
    /// High signal.
    High,
}

impl MemoryImportance {
    /// Serialize to persistence-friendly string.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    /// Deserialize from storage string.
    ///
    /// # Errors
    ///
    /// Returns an error if the string does not match a known importance level.
    pub fn from_db(value: &str) -> Result<Self, crate::memory::error::MemoryValidationError> {
        match value {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            _ => Err(crate::memory::error::MemoryValidationError::InvalidImportance(
                value.to_string(),
            )),
        }
    }
}

/// Persisted memory record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Stable entry identifier.
    pub id: String,
    /// Conversation session identifier.
    pub session_id: String,
    /// Topic key.
    pub topic: String,
    /// Compact summary used for retrieval.
    pub summary: String,
    /// Full excerpt or concatenated turn content.
    pub raw_excerpt: String,
    /// Searchable keywords.
    pub keywords: Vec<String>,
    /// Importance signal.
    pub importance: MemoryImportance,
    /// Optional vector embedding.
    pub embedding: Option<Vec<f32>>,
    /// Unix epoch milliseconds.
    pub created_at_epoch_ms: i64,
}

/// Query used for turn recall.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecallQuery {
    /// Optional session filter.
    pub session_id: Option<String>,
    /// Free-form query text.
    pub text: String,
    /// Optional embedding used for vector retrieval.
    pub query_embedding: Option<Vec<f32>>,
    /// Max number of results.
    pub limit: usize,
}

/// Weighted recall hit returned by hybrid retrieval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecallHit {
    /// Matched entry.
    pub entry: MemoryEntry,
    /// Normalized BM25 score in `[0, 1]`.
    pub bm25_score: f32,
    /// Normalized vector similarity in `[0, 1]`.
    pub vector_score: Option<f32>,
    /// Final fused score in `[0, 1]`.
    pub final_score: f32,
}

/// Hybrid rank fusion weights.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HybridWeights {
    /// BM25 contribution.
    pub bm25: f32,
    /// Vector contribution.
    pub vector: f32,
}

impl HybridWeights {
    /// Build validated weights where each component is in `[0, 1]` and sum is positive.
    pub fn new(bm25: f32, vector: f32) -> Result<Self, MemoryValidationError> {
        if !(0.0..=1.0).contains(&bm25) || !(0.0..=1.0).contains(&vector) {
            return Err(MemoryValidationError::InvalidHybridWeights { bm25, vector });
        }
        let total = bm25 + vector;
        if total <= f32::EPSILON {
            return Err(MemoryValidationError::InvalidHybridWeights { bm25, vector });
        }
        Ok(Self { bm25: bm25 / total, vector: vector / total })
    }
}

impl Default for HybridWeights {
    fn default() -> Self {
        Self { bm25: 0.3, vector: 0.7 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All `MemoryImportance` variants return the expected persistence string.
    #[test]
    fn memory_importance_as_str() {
        assert_eq!(MemoryImportance::Low.as_str(), "low");
        assert_eq!(MemoryImportance::Medium.as_str(), "medium");
        assert_eq!(MemoryImportance::High.as_str(), "high");
    }

    /// Round-tripping through `as_str` then `from_db` preserves every variant.
    #[test]
    fn memory_importance_from_db_roundtrip() {
        for variant in [MemoryImportance::Low, MemoryImportance::Medium, MemoryImportance::High] {
            assert_eq!(MemoryImportance::from_db(variant.as_str()), Ok(variant));
        }
        // Unknown strings return an error.
        assert!(MemoryImportance::from_db("unknown").is_err());
    }

    /// `MemoryEntry` fields are stored exactly as provided.
    #[test]
    fn memory_entry_construction() {
        let entry = MemoryEntry {
            id: "id-1".to_string(),
            session_id: "sess-1".to_string(),
            topic: "greetings".to_string(),
            summary: "hello world".to_string(),
            raw_excerpt: "raw".to_string(),
            keywords: vec!["hello".to_string()],
            importance: MemoryImportance::High,
            embedding: None,
            created_at_epoch_ms: 1_000,
        };
        assert_eq!(entry.id, "id-1");
        assert_eq!(entry.session_id, "sess-1");
        assert_eq!(entry.topic, "greetings");
        assert_eq!(entry.importance, MemoryImportance::High);
        assert!(entry.embedding.is_none());
        assert_eq!(entry.created_at_epoch_ms, 1_000);
    }

    /// `RecallQuery` handles unicode and special characters without panicking.
    #[test]
    fn recall_query_with_special_chars() {
        let query = RecallQuery {
            session_id: Some("s-\u{1F600}".to_string()),
            text: "\u{4F60}\u{597D} hello <>&\"'".to_string(),
            query_embedding: None,
            limit: 10,
        };
        assert!(query.text.contains('\u{4F60}'));
        assert_eq!(query.limit, 10);
    }

    /// Default `HybridWeights` components sum to 1.0.
    #[test]
    fn hybrid_weights_default_sums_to_one() {
        let w = HybridWeights::default();
        let sum = w.bm25 + w.vector;
        assert!((sum - 1.0).abs() < f32::EPSILON);
    }
}
