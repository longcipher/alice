//! Channel adapter implementations.
//!
//! Concrete implementations of [`bob_chat::ChatAdapter`] for different
//! chat platforms (CLI, Discord, Telegram).

/// CLI REPL chat adapter — interactive terminal I/O.
pub mod cli_repl;

/// Discord chat adapter (requires `discord` feature).
#[cfg(feature = "discord")]
pub mod discord;

/// Telegram chat adapter (requires `telegram` feature).
#[cfg(feature = "telegram")]
pub mod telegram;
