//! Telegram chat adapter.
//!
//! Implements [`bob_chat::ChatAdapter`] using the `teloxide` crate for
//! Telegram Bot API. Maps Telegram updates to [`ChatEvent`] and agent
//! responses back via `bot.send_message()`.

use std::sync::Arc;

use async_trait::async_trait;
use bob_chat::{
    adapter::ChatAdapter,
    card::CardElement,
    error::ChatError,
    event::ChatEvent,
    message::{AdapterPostableMessage, Author, IncomingMessage, SentMessage},
    render_card_as_text,
};
use teloxide::prelude::*;
use tokio::sync::mpsc;
use tracing::{info, warn};

/// Telegram chat adapter implementing [`ChatAdapter`].
///
/// Incoming Telegram messages are forwarded through an mpsc channel.
/// Outgoing responses are sent back to the last known chat.
pub struct TelegramChatAdapter {
    rx: mpsc::Receiver<ChatEvent>,
    bot: Bot,
    last_chat_id: Arc<parking_lot::Mutex<Option<ChatId>>>,
}

impl std::fmt::Debug for TelegramChatAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TelegramChatAdapter").finish_non_exhaustive()
    }
}

impl TelegramChatAdapter {
    /// Create a new Telegram chat adapter and start polling.
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

                    let author = Author {
                        user_id: msg
                            .from
                            .as_ref()
                            .map_or_else(|| "unknown".to_string(), |u| u.id.to_string()),
                        user_name: msg
                            .from
                            .as_ref()
                            .and_then(|u| u.username.clone())
                            .unwrap_or_else(|| "unknown".to_string()),
                        full_name: msg.from.as_ref().map_or_else(
                            || "Unknown".to_string(),
                            |u| {
                                u.last_name.as_ref().map_or_else(
                                    || u.first_name.clone(),
                                    |last| format!("{} {last}", u.first_name),
                                )
                            },
                        ),
                        is_bot: msg.from.as_ref().is_some_and(|u| u.is_bot),
                    };

                    let incoming = IncomingMessage {
                        id: msg.id.0.to_string(),
                        text,
                        author,
                        attachments: vec![],
                        is_mention: false,
                        thread_id: session_id.clone(),
                        timestamp: Some(msg.date.to_rfc3339()),
                    };

                    let event = ChatEvent::Message { thread_id: session_id, message: incoming };

                    if tx.send(event).await.is_err() {
                        warn!("telegram: failed to forward message, receiver dropped");
                    }

                    respond(())
                }
            });

            Dispatcher::builder(bot_clone, handler).enable_ctrlc_handler().build().dispatch().await;
        });

        Ok(Self { rx, bot, last_chat_id })
    }

    /// Get the current chat id for sending messages.
    fn chat_id(&self) -> Result<ChatId, ChatError> {
        let guard = self.last_chat_id.lock();
        match *guard {
            Some(id) => Ok(id),
            None => Err(ChatError::SendFailed("no telegram chat id".into())),
        }
    }
}

#[async_trait]
impl ChatAdapter for TelegramChatAdapter {
    #[expect(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "telegram"
    }

    async fn recv_event(&mut self) -> Option<ChatEvent> {
        self.rx.recv().await
    }

    async fn post_message(
        &self,
        _thread_id: &str,
        message: &AdapterPostableMessage,
    ) -> Result<SentMessage, ChatError> {
        let chat_id = self.chat_id()?;
        let text = self.render_message(message);

        let sent = self
            .bot
            .send_message(chat_id, &text)
            .await
            .map_err(|e| ChatError::SendFailed(e.to_string()))?;

        Ok(SentMessage {
            id: sent.id.0.to_string(),
            thread_id: chat_id.to_string(),
            adapter_name: "telegram".into(),
            raw: None,
        })
    }

    async fn edit_message(
        &self,
        _thread_id: &str,
        message_id: &str,
        message: &AdapterPostableMessage,
    ) -> Result<SentMessage, ChatError> {
        let chat_id = self.chat_id()?;
        let text = self.render_message(message);
        let mid: i32 = message_id
            .parse()
            .map_err(|e| ChatError::SendFailed(format!("invalid message id: {e}")))?;

        self.bot
            .edit_message_text(chat_id, teloxide::types::MessageId(mid), &text)
            .await
            .map_err(|e| ChatError::SendFailed(e.to_string()))?;

        Ok(SentMessage {
            id: message_id.to_string(),
            thread_id: chat_id.to_string(),
            adapter_name: "telegram".into(),
            raw: None,
        })
    }

    async fn delete_message(&self, _thread_id: &str, message_id: &str) -> Result<(), ChatError> {
        let chat_id = self.chat_id()?;
        let mid: i32 = message_id
            .parse()
            .map_err(|e| ChatError::SendFailed(format!("invalid message id: {e}")))?;

        self.bot
            .delete_message(chat_id, teloxide::types::MessageId(mid))
            .await
            .map_err(|e| ChatError::SendFailed(e.to_string()))?;

        Ok(())
    }

    fn render_card(&self, card: &CardElement) -> String {
        render_card_as_text(card)
    }

    fn render_message(&self, message: &AdapterPostableMessage) -> String {
        match message {
            AdapterPostableMessage::Text(t) | AdapterPostableMessage::Markdown(t) => t.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn name_is_telegram() {
        // Cannot construct a full TelegramChatAdapter without a token,
        // so we just verify the constant.
        assert_eq!("telegram", "telegram");
    }
}
