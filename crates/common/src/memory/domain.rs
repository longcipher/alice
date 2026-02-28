//! Core domain types for memory storage and recall.

use serde::{Deserialize, Serialize};

use crate::memory::error::MemoryValidationError;

/// Importance level attached to a memory entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryImportance {
    /// High-priority facts that should not be discarded.
    Critical,
    /// Important but not immutable.
    High,
    /// Default importance.
    Medium,
    /// Low-priority memory.
    Low,
}

impl MemoryImportance {
    /// Return canonical database representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }

    /// Parse from a canonical database representation.
    #[must_use]
    pub fn from_db(value: &str) -> Self {
        match value {
            "critical" => Self::Critical,
            "high" => Self::High,
            "low" => Self::Low,
            _ => Self::Medium,
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
