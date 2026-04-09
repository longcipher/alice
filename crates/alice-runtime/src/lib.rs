//! Alice runtime wiring layer.
//!
//! Configuration, bootstrap, context, and command implementations
//! for the Alice agent.

pub mod agent_backend;
pub mod bootstrap;
pub mod channel_dispatch;
pub mod chatbot_runner;
pub mod commands;
pub mod config;
pub mod context;
pub mod handle_input;
pub mod identity;
pub mod memory_context;
pub mod orchestration;
pub mod reflection;
pub mod scheduler;
pub mod skill_wiring;
