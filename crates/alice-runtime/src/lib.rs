//! Alice runtime wiring layer.
//!
//! Configuration, bootstrap, context, and command implementations
//! for the Alice agent.

pub mod bootstrap;
pub mod channel_runner;
pub mod commands;
pub mod config;
pub mod context;
pub mod handle_input;
pub mod memory_context;
pub mod skill_wiring;
