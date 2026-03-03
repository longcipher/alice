//! Alice adapter layer.
//!
//! Concrete implementations of core ports — SQLite memory store,
//! channel adapters, and skill adapter wrappers.

/// Channel adapters for multi-endpoint agent interaction.
pub mod channel;

/// Memory persistence adapters.
pub mod memory;
