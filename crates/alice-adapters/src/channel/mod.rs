//! Channel adapter implementations.
//!
//! Concrete implementations of Bob's [`bob_core::channel::Channel`] trait
//! for different transport backends.

/// CLI REPL channel — interactive terminal I/O.
pub mod cli_repl;

/// Discord channel adapter (requires `discord` feature).
#[cfg(feature = "discord")]
pub mod discord;

/// Telegram channel adapter (requires `telegram` feature).
#[cfg(feature = "telegram")]
pub mod telegram;
