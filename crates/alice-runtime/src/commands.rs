//! Command implementations for Alice CLI.

use std::{io::Write, sync::Arc};

use bob_chat::adapter::ChatAdapter;

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

/// Run interactive REPL session via the chatbot runner.
///
/// Creates a [`CliReplChatAdapter`](alice_adapters::channel::cli_repl::CliReplChatAdapter)
/// and drives it through [`run_chatbot`](crate::chatbot_runner::run_chatbot).
///
/// # Errors
///
/// Returns an error if the chatbot runner fails.
pub async fn cmd_chat(context: Arc<AliceRuntimeContext>, session_id: &str) -> eyre::Result<()> {
    writeln!(std::io::stderr(), "Alice ready (model: {})", context.default_model)?;
    writeln!(std::io::stderr(), "Type /quit to exit.\n")?;

    let adapters: Vec<Box<dyn ChatAdapter>> = vec![Box::new(
        alice_adapters::channel::cli_repl::CliReplChatAdapter::new(session_id.to_string()),
    )];
    crate::chatbot_runner::run_chatbot(context, adapters).await
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
