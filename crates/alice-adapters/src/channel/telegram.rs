//! Telegram channel adapter.
//!
//! Uses the `teloxide` crate for Telegram Bot API.
//! Maps Telegram updates to [`bob_core::channel::ChannelMessage`] and
//! agent responses back via `bot.send_message()`.

use std::sync::Arc;

use async_trait::async_trait;
use bob_core::channel::{Channel, ChannelError, ChannelMessage, ChannelOutput};
use teloxide::prelude::*;
use tokio::sync::mpsc;
use tracing::{info, warn};

/// Telegram channel adapter implementing Bob's [`Channel`] trait.
///
/// Incoming Telegram messages are forwarded through an mpsc channel.
/// Outgoing responses are sent back to the last known chat.
pub struct TelegramChannel {
    rx: mpsc::Receiver<ChannelMessage>,
    bot: Bot,
    last_chat_id: Arc<parking_lot::Mutex<Option<ChatId>>>,
}

impl std::fmt::Debug for TelegramChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TelegramChannel").finish_non_exhaustive()
    }
}

impl TelegramChannel {
    /// Create a new Telegram channel adapter and start polling.
    ///
    /// # Errors
    ///
    /// Returns an error if the bot cannot be created from the token.
    pub async fn new(token: &str) -> eyre::Result<Self> {
        let (tx, rx) = mpsc::channel(128);
        let bot = Bot::new(token);
        let last_chat_id: Arc<parking_lot::Mutex<Option<ChatId>>> =
            Arc::new(parking_lot::Mutex::new(None));

        info!("telegram: starting bot");

        // Clone for the dispatcher closure
        let tx_clone = tx;
        let last_chat_clone = Arc::clone(&last_chat_id);
        let bot_clone = bot.clone();

        tokio::spawn(async move {
            let handler = Update::filter_message().endpoint(move |msg: Message, _bot: Bot| {
                let tx = tx_clone.clone();
                let last_chat = Arc::clone(&last_chat_clone);
                async move {
                    let text = msg.text().unwrap_or_default().to_string();
                    if text.is_empty() {
                        return respond(());
                    }

                    let chat_id = msg.chat.id;
                    let session_id = format!("telegram-{chat_id}");

                    // Update last chat id for replies
                    {
                        let mut guard = last_chat.lock();
                        *guard = Some(chat_id);
                    }

                    let sender = msg.from.as_ref().and_then(|u| u.username.clone());

                    let channel_msg = ChannelMessage { text, session_id, sender };

                    if tx.send(channel_msg).await.is_err() {
                        warn!("telegram: failed to forward message, receiver dropped");
                    }

                    respond(())
                }
            });

            Dispatcher::builder(bot_clone, handler).enable_ctrlc_handler().build().dispatch().await;
        });

        Ok(Self { rx, bot, last_chat_id })
    }
}

#[async_trait]
impl Channel for TelegramChannel {
    async fn recv(&mut self) -> Option<ChannelMessage> {
        self.rx.recv().await
    }

    async fn send(&self, output: ChannelOutput) -> Result<(), ChannelError> {
        let chat_id = {
            let guard = self.last_chat_id.lock();
            match *guard {
                Some(id) => id,
                None => return Err(ChannelError::SendFailed("no chat id".to_string())),
            }
        };

        let text = if output.is_error { format!("⚠️ {}", output.text) } else { output.text };

        self.bot
            .send_message(chat_id, text)
            .await
            .map_err(|e| ChannelError::SendFailed(e.to_string()))?;

        Ok(())
    }
}
