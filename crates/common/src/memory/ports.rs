//! Port traits for memory persistence and retrieval.

use crate::memory::{
    domain::{HybridWeights, MemoryEntry, RecallHit, RecallQuery},
    error::MemoryStoreError,
};

/// Storage boundary for memory implementations.
pub trait MemoryStorePort: Send + Sync {
    /// Ensure required schema exists.
    fn init_schema(&self) -> Result<(), MemoryStoreError>;

    /// Insert one memory entry.
    fn insert(&self, entry: &MemoryEntry) -> Result<(), MemoryStoreError>;

    /// Recall memories using hybrid search.
    fn recall_hybrid(
        &self,
        query: &RecallQuery,
        weights: HybridWeights,
    ) -> Result<Vec<RecallHit>, MemoryStoreError>;
}
