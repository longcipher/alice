//! Error types for the memory subsystem.

/// Validation failures for memory inputs.
#[derive(Debug, thiserror::Error, Clone, PartialEq)]
pub enum MemoryValidationError {
    /// Hybrid weights are outside expected range.
    #[error("invalid hybrid weights: bm25={bm25}, vector={vector}")]
    InvalidHybridWeights {
        /// Provided BM25 weight.
        bm25: f32,
        /// Provided vector weight.
        vector: f32,
    },
    /// Recall limit must be positive.
    #[error("recall limit must be greater than zero")]
    InvalidRecallLimit,
}

/// Storage adapter failures.
#[derive(Debug, thiserror::Error)]
pub enum MemoryStoreError {
    /// Database operation failed.
    #[error("database error: {0}")]
    Database(String),
    /// Serialization or deserialization failed.
    #[error("serialization error: {0}")]
    Serialization(String),
    /// Input validation failed.
    #[error(transparent)]
    Validation(#[from] MemoryValidationError),
}

/// Memory service failures.
#[derive(Debug, thiserror::Error)]
pub enum MemoryServiceError {
    /// Service-level validation issue.
    #[error(transparent)]
    Validation(#[from] MemoryValidationError),
    /// Store layer failure.
    #[error(transparent)]
    Store(#[from] MemoryStoreError),
}

impl From<serde_json::Error> for MemoryStoreError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value.to_string())
    }
}
