//! Alice CLI binary.
//!
//! Supports three modes:
//! - `run`  — one-shot prompt execution
//! - `chat` — interactive REPL with slash command routing
//! - `channel` — message channel runtime (Telegram, Discord, etc.)

use std::sync::Arc;

use clap::{Parser, Subcommand};

/// Alice — a collaborative AI agent built on the Bob framework.
#[derive(Debug, Parser)]
#[command(name = "alice", version, about = "Alice CLI agent")]
struct Cli {
    /// Path to configuration file.
    #[arg(short, long, default_value = "alice.toml", global = true)]
    config: String,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Run a single prompt and exit.
    Run {
        /// The prompt to execute.
        prompt: String,

        /// Session identifier for memory continuity.
        #[arg(long, default_value = "alice-once")]
        session_id: String,
    },

    /// Start an interactive REPL session.
    Chat {
        /// Session identifier for memory continuity.
        #[arg(long, default_value = "alice-session")]
        session_id: String,
    },

    /// Run enabled message channels (Telegram, Discord, etc.).
    Channel,
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt().with_target(false).init();

    let cli = Cli::parse();
    let cfg = alice_runtime::config::load_config(&cli.config)?;
    let context = Arc::new(alice_runtime::bootstrap::build_runtime(&cfg).await?);

    match cli.command {
        Some(Commands::Run { prompt, session_id }) => {
            alice_runtime::commands::cmd_run(&context, &session_id, &prompt).await
        }
        Some(Commands::Chat { session_id }) => {
            alice_runtime::commands::cmd_chat(Arc::clone(&context), &session_id).await
        }
        Some(Commands::Channel) => {
            alice_runtime::commands::cmd_channel(Arc::clone(&context), &cfg.channels).await
        }
        // Default to chat when no subcommand is given.
        None => alice_runtime::commands::cmd_chat(Arc::clone(&context), "alice-session").await,
    }
}
