//! Alice CLI binary.
//!
//! Supports three modes:
//! - `run`  — one-shot prompt execution
//! - `chat` — interactive REPL with slash command routing
//! - `channel` — message channel runtime (Telegram, Discord, etc.)

use std::sync::Arc;

use clap::{ArgAction, Parser, Subcommand};

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

        /// Stable global user id for cross-session continuity.
        #[arg(long)]
        global_user_id: Option<String>,
    },

    /// Start an interactive REPL session.
    Chat {
        /// Session identifier for memory continuity.
        #[arg(long, default_value = "alice-session")]
        session_id: String,

        /// Stable global user id for cross-session continuity.
        #[arg(long)]
        global_user_id: Option<String>,
    },

    /// Run enabled message channels (Telegram, Discord, etc.).
    Channel,

    /// Issue a one-time bind token for linking another channel identity.
    BindToken {
        /// Stable global user id the token should bind to.
        global_user_id: String,

        /// Optional provider restriction such as `telegram` or `discord`.
        #[arg(long)]
        provider: Option<String>,

        /// Token lifetime in minutes.
        #[arg(long, default_value_t = 10)]
        ttl_minutes: u64,
    },

    /// Manage background scheduled tasks.
    Schedule {
        #[command(subcommand)]
        command: ScheduleCommands,
    },

    /// Run a manager/worker orchestration flow across ACP profiles.
    Orchestrate {
        /// Root session identifier used to derive manager and worker sessions.
        #[arg(long, default_value = "orchestration-root")]
        session_id: String,

        /// Prompt sent to the manager profile.
        #[arg(long)]
        manager_prompt: String,

        /// Repeated worker specification as `<profile> <prompt>`.
        #[arg(long = "worker", value_names = ["PROFILE", "PROMPT"], num_args = 2, action = ArgAction::Append)]
        workers: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
enum ScheduleCommands {
    /// Create a new scheduled task.
    Add {
        /// Stable global user id that owns this task.
        #[arg(long)]
        global_user_id: String,

        /// Prompt executed by the task.
        #[arg(long)]
        prompt: String,

        /// Preferred output channel label.
        #[arg(long)]
        channel: Option<String>,

        /// Run every N minutes.
        #[arg(long)]
        every_minutes: Option<u32>,

        /// Run once per hour at the given minute.
        #[arg(long)]
        hourly_minute: Option<u8>,

        /// Run once per day at the given hour.
        #[arg(long)]
        daily_hour: Option<u8>,

        /// Run once per day at the given minute.
        #[arg(long)]
        daily_minute: Option<u8>,
    },

    /// List all scheduled tasks.
    List,
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt().with_target(false).init();

    let cli = Cli::parse();
    let cfg = alice_runtime::config::load_config(&cli.config)?;
    let context = Arc::new(alice_runtime::bootstrap::build_runtime(&cfg).await?);

    match cli.command {
        Some(Commands::Run { prompt, session_id, global_user_id }) => {
            alice_runtime::commands::cmd_run(
                &context,
                &session_id,
                global_user_id.as_deref(),
                &prompt,
            )
            .await
        }
        Some(Commands::Chat { session_id, global_user_id }) => {
            let _scheduler = cfg.scheduler.enabled.then(|| {
                alice_runtime::scheduler::spawn_scheduler_worker(
                    Arc::clone(&context),
                    cfg.scheduler.poll_interval_ms,
                )
            });
            alice_runtime::commands::cmd_chat(
                Arc::clone(&context),
                &session_id,
                global_user_id.as_deref(),
            )
            .await
        }
        Some(Commands::Channel) => {
            let _scheduler = cfg.scheduler.enabled.then(|| {
                alice_runtime::scheduler::spawn_scheduler_worker(
                    Arc::clone(&context),
                    cfg.scheduler.poll_interval_ms,
                )
            });
            alice_runtime::commands::cmd_channel(Arc::clone(&context), &cfg.channels).await
        }
        Some(Commands::BindToken { global_user_id, provider, ttl_minutes }) => {
            alice_runtime::commands::cmd_issue_bind_token(
                &context,
                &global_user_id,
                provider.as_deref(),
                ttl_minutes,
            )
        }
        Some(Commands::Schedule { command }) => match command {
            ScheduleCommands::Add {
                global_user_id,
                prompt,
                channel,
                every_minutes,
                hourly_minute,
                daily_hour,
                daily_minute,
            } => {
                let schedule = alice_runtime::commands::build_schedule_kind(
                    every_minutes,
                    hourly_minute,
                    daily_hour,
                    daily_minute,
                )?;
                alice_runtime::commands::cmd_schedule_add(
                    &context,
                    &global_user_id,
                    channel.as_deref(),
                    &prompt,
                    schedule,
                )
            }
            ScheduleCommands::List => alice_runtime::commands::cmd_schedule_list(&context),
        },
        Some(Commands::Orchestrate { session_id, manager_prompt, workers }) => {
            alice_runtime::commands::cmd_orchestrate(
                &cfg,
                &session_id,
                &manager_prompt,
                pair_worker_specs(workers)?,
            )
            .await
        }
        // Default to chat when no subcommand is given.
        None => {
            let _scheduler = cfg.scheduler.enabled.then(|| {
                alice_runtime::scheduler::spawn_scheduler_worker(
                    Arc::clone(&context),
                    cfg.scheduler.poll_interval_ms,
                )
            });
            alice_runtime::commands::cmd_chat(Arc::clone(&context), "alice-session", None).await
        }
    }
}

fn pair_worker_specs(workers: Vec<String>) -> eyre::Result<Vec<(String, String)>> {
    if !workers.len().is_multiple_of(2) {
        return Err(eyre::eyre!(
            "worker arguments must be provided as repeated <profile> <prompt> pairs"
        ));
    }

    let mut pairs = Vec::with_capacity(workers.len() / 2);
    for chunk in workers.chunks_exact(2) {
        pairs.push((chunk[0].clone(), chunk[1].clone()));
    }
    Ok(pairs)
}
