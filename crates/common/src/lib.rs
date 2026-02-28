//! Common utilities and shared code
//!
//! This crate contains shared utilities, types, and functions
//! used across multiple crates in the workspace.

/// Local memory subsystem.
pub mod memory;

/// Builds a greeting string for the CLI.
#[must_use]
pub fn greeting(name: &str) -> String {
    format!("Hello, {name}!")
}

/// Re-exports commonly used items.
pub mod prelude {}

#[cfg(test)]
mod tests {
    use crate::greeting;

    #[test]
    fn greeting_builds_message() {
        assert_eq!(greeting("Rust"), "Hello, Rust!");
    }
}
