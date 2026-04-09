//! Command implementations for Alice CLI.

use std::{
    io::Write,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use alice_core::runtime_state::domain::{ScheduleKind, ScheduledTask};
use bob_chat::adapter::ChatAdapter;

use crate::{
    context::AliceRuntimeContext, identity::IdentityResolver, memory_context::run_turn_with_memory,
    orchestration::WorkerTask,
};

static SCHEDULE_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Execute a single prompt and print the response.
///
/// # Errors
///
/// Returns an error if the turn execution fails or stdout write fails.
pub async fn cmd_run(
    context: &AliceRuntimeContext,
    session_id: &str,
    global_user_id: Option<&str>,
    prompt: &str,
) -> eyre::Result<()> {
    let resolver = IdentityResolver::new(context);
    let identity = resolver.resolve_cli_turn(session_id, global_user_id)?;
    let response =
        run_turn_with_memory(context, &identity.session_id, Some(&identity.profile_id), prompt)
            .await?;
    if let Err(error) = resolver.remember_active_session(&identity, Some("cli")) {
        tracing::warn!("failed to persist CLI active session: {error}");
    }
    writeln!(std::io::stdout(), "{}", response.content)?;
    Ok(())
}

/// Run interactive REPL session via the chatbot runner.
///
/// Creates a [`CliReplChatAdapter`](alice_adapters::channel::cli_repl::CliReplChatAdapter)
/// and drives it through [`run_chatbot`](crate::chatbot_runner::run_chatbot).
///
/// # Errors
///
/// Returns an error if the chatbot runner fails.
pub async fn cmd_chat(
    context: Arc<AliceRuntimeContext>,
    session_id: &str,
    global_user_id: Option<&str>,
) -> eyre::Result<()> {
    writeln!(std::io::stderr(), "Alice ready (model: {})", context.default_model())?;
    writeln!(std::io::stderr(), "Type /quit to exit.\n")?;

    let cli_user_id = global_user_id.unwrap_or("local");
    let adapters: Vec<Box<dyn ChatAdapter>> =
        vec![Box::new(alice_adapters::channel::cli_repl::CliReplChatAdapter::with_user_id(
            session_id.to_string(),
            cli_user_id.to_string(),
        ))];
    crate::chatbot_runner::run_chatbot(context, adapters).await
}

/// Issue a one-time bind token for linking a channel identity to a global user.
///
/// # Errors
///
/// Returns an error if token issuance or stdout write fails.
pub fn cmd_issue_bind_token(
    context: &AliceRuntimeContext,
    global_user_id: &str,
    provider: Option<&str>,
    ttl_minutes: u64,
) -> eyre::Result<()> {
    let ttl_ms = i64::try_from(ttl_minutes)
        .map_err(|_| eyre::eyre!("ttl_minutes is too large"))?
        .saturating_mul(60_000);
    let token =
        IdentityResolver::new(context).issue_bind_token(global_user_id, provider, ttl_ms)?;
    writeln!(std::io::stdout(), "{}", token.token)?;
    Ok(())
}

/// Create a scheduled task and print its identifier.
///
/// # Errors
///
/// Returns an error if task persistence or stdout write fails.
pub fn cmd_schedule_add(
    context: &AliceRuntimeContext,
    global_user_id: &str,
    channel: Option<&str>,
    prompt: &str,
    schedule: ScheduleKind,
) -> eyre::Result<()> {
    let now_epoch_ms = current_time_millis();
    let task_id =
        format!("task-{now_epoch_ms}-{:04}", SCHEDULE_COUNTER.fetch_add(1, Ordering::Relaxed));
    let task = ScheduledTask {
        task_id: task_id.clone(),
        global_user_id: global_user_id.to_string(),
        channel: channel.map(ToString::to_string),
        prompt: prompt.to_string(),
        schedule: schedule.clone(),
        next_run_epoch_ms: schedule.next_run_after(now_epoch_ms),
        enabled: true,
        last_run_epoch_ms: None,
    };
    context.runtime_state_service().insert_scheduled_task(task)?;
    writeln!(std::io::stdout(), "{task_id}")?;
    Ok(())
}

/// List all scheduled tasks in storage.
///
/// # Errors
///
/// Returns an error if listing tasks or stdout write fails.
pub fn cmd_schedule_list(context: &AliceRuntimeContext) -> eyre::Result<()> {
    let tasks = context.runtime_state_service().list_scheduled_tasks()?;
    for task in tasks {
        writeln!(
            std::io::stdout(),
            "{}\t{}\t{}\t{}\t{}",
            task.task_id,
            task.global_user_id,
            render_schedule(&task.schedule),
            task.next_run_epoch_ms,
            if task.enabled { "enabled" } else { "disabled" }
        )?;
    }
    Ok(())
}

/// Run a manager/worker orchestration flow from configured ACP profiles.
///
/// # Errors
///
/// Returns an error if orchestration is unavailable, execution fails, or stdout write fails.
pub async fn cmd_orchestrate(
    cfg: &crate::config::AliceConfig,
    session_id: &str,
    manager_prompt: &str,
    workers: Vec<(String, String)>,
) -> eyre::Result<()> {
    let Some(orchestrator) = crate::bootstrap::build_orchestrator_from_config(cfg)? else {
        return Err(eyre::eyre!("orchestration is only available when agent.backend = \"acp\""));
    };

    let worker_tasks =
        workers.into_iter().map(|(profile, prompt)| WorkerTask::new(profile, prompt)).collect();
    let run = orchestrator.run(session_id, manager_prompt, worker_tasks).await?;
    writeln!(std::io::stdout(), "{}", run.summary)?;
    Ok(())
}

/// Build a schedule kind from mutually exclusive CLI flags.
///
/// # Errors
///
/// Returns an error when the schedule flags are invalid or ambiguous.
pub fn build_schedule_kind(
    every_minutes: Option<u32>,
    hourly_minute: Option<u8>,
    daily_hour: Option<u8>,
    daily_minute: Option<u8>,
) -> eyre::Result<ScheduleKind> {
    let mut selected_modes = 0_u8;
    if every_minutes.is_some() {
        selected_modes += 1;
    }
    if hourly_minute.is_some() {
        selected_modes += 1;
    }
    if daily_hour.is_some() || daily_minute.is_some() {
        selected_modes += 1;
    }

    if selected_modes != 1 {
        return Err(eyre::eyre!(
            "choose exactly one schedule mode: --every-minutes, --hourly-minute, or --daily-hour/--daily-minute"
        ));
    }

    if let Some(minutes) = every_minutes {
        let schedule = ScheduleKind::EveryMinutes(minutes);
        schedule.validate()?;
        return Ok(schedule);
    }

    if let Some(minute) = hourly_minute {
        let schedule = ScheduleKind::Hourly { minute };
        schedule.validate()?;
        return Ok(schedule);
    }

    let (Some(hour), Some(minute)) = (daily_hour, daily_minute) else {
        return Err(eyre::eyre!("daily schedules require both --daily-hour and --daily-minute"));
    };
    let schedule = ScheduleKind::DailyAt { hour, minute };
    schedule.validate()?;
    Ok(schedule)
}

/// Run enabled message channels concurrently.
///
/// Starts a CLI REPL adapter and optionally adds Discord and/or Telegram
/// adapters based on configuration and environment variables.
///
/// # Errors
///
/// Returns an error if the chatbot runner fails.
pub async fn cmd_channel(
    context: Arc<AliceRuntimeContext>,
    config: &crate::config::ChannelsConfig,
) -> eyre::Result<()> {
    // `mut` needed when discord/telegram features add adapters at runtime.
    #[cfg_attr(not(any(feature = "discord", feature = "telegram")), expect(unused_mut))]
    let mut adapters: Vec<Box<dyn ChatAdapter>> = vec![
        // Always include CLI REPL
        Box::new(alice_adapters::channel::cli_repl::CliReplChatAdapter::new(
            "alice-channel".to_string(),
        )),
    ];

    // Optionally add Discord adapter
    #[cfg(feature = "discord")]
    if config.discord.enabled {
        if let Ok(token) = std::env::var("ALICE_DISCORD_TOKEN") {
            match alice_adapters::channel::discord::DiscordChatAdapter::new(&token).await {
                Ok(adapter) => {
                    tracing::info!("discord adapter enabled");
                    adapters.push(Box::new(adapter));
                }
                Err(e) => {
                    tracing::warn!("discord adapter failed to start: {e}");
                }
            }
        } else {
            tracing::warn!("discord enabled but ALICE_DISCORD_TOKEN not set");
        }
    }

    // Optionally add Telegram adapter
    #[cfg(feature = "telegram")]
    if config.telegram.enabled {
        if let Ok(token) = std::env::var("ALICE_TELEGRAM_TOKEN") {
            match alice_adapters::channel::telegram::TelegramChatAdapter::new(&token).await {
                Ok(adapter) => {
                    tracing::info!("telegram adapter enabled");
                    adapters.push(Box::new(adapter));
                }
                Err(e) => {
                    tracing::warn!("telegram adapter failed to start: {e}");
                }
            }
        } else {
            tracing::warn!("telegram enabled but ALICE_TELEGRAM_TOKEN not set");
        }
    }

    // Suppress unused variable warning when features are disabled
    let _ = config;

    crate::chatbot_runner::run_chatbot(context, adapters).await
}

fn render_schedule(schedule: &ScheduleKind) -> String {
    match schedule {
        ScheduleKind::EveryMinutes(minutes) => format!("every_{minutes}_minutes"),
        ScheduleKind::Hourly { minute } => format!("hourly_at_{minute:02}"),
        ScheduleKind::DailyAt { hour, minute } => format!("daily_at_{hour:02}:{minute:02}"),
    }
}

fn current_time_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
}
