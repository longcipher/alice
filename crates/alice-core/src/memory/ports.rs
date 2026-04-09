//! Port traits for memory persistence and retrieval.

use crate::memory::{
    domain::{HybridWeights, MemoryEntry, RecallHit, RecallQuery, UserProfile},
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

    /// Insert or update a long-lived user profile.
    fn upsert_user_profile(&self, profile: &UserProfile) -> Result<(), MemoryStoreError>;

    /// Load a user profile when one exists.
    fn get_user_profile(&self, profile_id: &str) -> Result<Option<UserProfile>, MemoryStoreError>;
}
