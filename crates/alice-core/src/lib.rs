//! Alice core domain layer.
//!
//! Pure domain types, port traits, and service logic with zero adapter
//! dependencies. This is the innermost hexagonal layer.

/// Local memory subsystem.
pub mod memory;

/// Runtime state subsystem for identity bindings, active sessions, and schedules.
pub mod runtime_state;
