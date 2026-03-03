//! Command implementations for Alice CLI.

use std::{io::Write, sync::Arc};

use crate::{context::AliceRuntimeContext, memory_context::run_turn_with_memory};

/// Execute a single prompt and print the response.
///
/// # Errors
///
/// Returns an error if the turn execution fails or stdout write fails.
pub async fn cmd_run(
    context: &AliceRuntimeContext,
    session_id: &str,
    prompt: &str,
) -> eyre::Result<()> {
    let response = run_turn_with_memory(context, session_id, prompt).await?;
    writeln!(std::io::stdout(), "{}", response.content)?;
    Ok(())
}

/// Run interactive REPL session via the channel runner.
///
/// Creates a [`CliReplChannel`](alice_adapters::channel::cli_repl::CliReplChannel)
/// and drives it through [`run_channels`](crate::channel_runner::run_channels).
///
/// # Errors
///
/// Returns an error if the channel runner fails.
pub async fn cmd_chat(context: Arc<AliceRuntimeContext>, session_id: &str) -> eyre::Result<()> {
    writeln!(std::io::stderr(), "Alice ready (model: {})", context.default_model)?;
    writeln!(std::io::stderr(), "Type /quit to exit.\n")?;

    let channel = alice_adapters::channel::cli_repl::CliReplChannel::new(session_id.to_string());
    let channels: Vec<Box<dyn bob_core::channel::Channel>> = vec![Box::new(channel)];
    crate::channel_runner::run_channels(context, channels).await
}

/// Run enabled message channels concurrently.
///
/// Starts a CLI REPL channel and optionally adds Discord and/or Telegram
/// channels based on configuration and environment variables.
///
/// # Errors
///
/// Returns an error if the channel runner fails.
pub async fn cmd_channel(
    context: Arc<AliceRuntimeContext>,
    config: &crate::config::ChannelsConfig,
) -> eyre::Result<()> {
    let mut channels: Vec<Box<dyn bob_core::channel::Channel>> = Vec::new();

    // Always include CLI REPL
    let cli = alice_adapters::channel::cli_repl::CliReplChannel::new("alice-channel".to_string());
    channels.push(Box::new(cli));

    // Optionally add Discord channel
    #[cfg(feature = "discord")]
    if config.discord.enabled {
        if let Ok(token) = std::env::var("ALICE_DISCORD_TOKEN") {
            match alice_adapters::channel::discord::DiscordChannel::new(&token).await {
                Ok(ch) => {
                    tracing::info!("discord channel enabled");
                    channels.push(Box::new(ch));
                }
                Err(e) => {
                    tracing::warn!("discord channel failed to start: {e}");
                }
            }
        } else {
            tracing::warn!("discord enabled but ALICE_DISCORD_TOKEN not set");
        }
    }

    // Optionally add Telegram channel
    #[cfg(feature = "telegram")]
    if config.telegram.enabled {
        if let Ok(token) = std::env::var("ALICE_TELEGRAM_TOKEN") {
            match alice_adapters::channel::telegram::TelegramChannel::new(&token).await {
                Ok(ch) => {
                    tracing::info!("telegram channel enabled");
                    channels.push(Box::new(ch));
                }
                Err(e) => {
                    tracing::warn!("telegram channel failed to start: {e}");
                }
            }
        } else {
            tracing::warn!("telegram enabled but ALICE_TELEGRAM_TOKEN not set");
        }
    }

    // Suppress unused variable warning when features are disabled
    let _ = config;

    crate::channel_runner::run_channels(context, channels).await
}
