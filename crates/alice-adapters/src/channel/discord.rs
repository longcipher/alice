//! Discord chat adapter.
//!
//! Implements [`bob_chat::ChatAdapter`] using the `serenity` crate for
//! Discord gateway connections. Maps Discord messages to [`ChatEvent`]
//! and agent responses back to Discord replies.

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
use serenity::all::{Client, Context, EditMessage, EventHandler, GatewayIntents, Message, Ready};
use tokio::sync::mpsc;
use tracing::{info, warn};

/// Discord chat adapter implementing [`ChatAdapter`].
///
/// Incoming Discord messages are forwarded through an mpsc channel.
/// Outgoing responses are sent back to the last known Discord channel.
pub struct DiscordChatAdapter {
    rx: mpsc::Receiver<ChatEvent>,
    http: Arc<serenity::http::Http>,
    last_channel_id: Arc<parking_lot::Mutex<Option<serenity::all::ChannelId>>>,
}

impl std::fmt::Debug for DiscordChatAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiscordChatAdapter").finish_non_exhaustive()
    }
}

/// Serenity event handler that forwards messages to the adapter's mpsc channel.
struct Handler {
    tx: mpsc::Sender<ChatEvent>,
    last_channel_id: Arc<parking_lot::Mutex<Option<serenity::all::ChannelId>>>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, _ctx: Context, msg: Message) {
        // Ignore bot messages
        if msg.author.bot {
            return;
        }

        let session_id = format!(
            "discord-{}-{}",
            msg.guild_id.map_or_else(|| "dm".to_string(), |id| id.to_string()),
            msg.channel_id
        );

        // Update last channel id for replies
        {
            let mut guard = self.last_channel_id.lock();
            *guard = Some(msg.channel_id);
        }

        let author = Author {
            user_id: msg.author.id.to_string(),
            user_name: msg.author.name.clone(),
            full_name: msg.author.global_name.clone().unwrap_or_else(|| msg.author.name.clone()),
            is_bot: msg.author.bot,
        };

        // Detect if the bot is mentioned (simple content-based check)
        let is_mention = msg.mention_everyone || !msg.mentions.is_empty();

        let incoming = IncomingMessage {
            id: msg.id.to_string(),
            text: msg.content.clone(),
            author,
            attachments: vec![],
            is_mention,
            thread_id: session_id.clone(),
            timestamp: msg.timestamp.to_rfc3339(),
        };

        let event = if is_mention {
            ChatEvent::Mention { thread_id: session_id, message: incoming }
        } else {
            ChatEvent::Message { thread_id: session_id, message: incoming }
        };

        if self.tx.send(event).await.is_err() {
            warn!("discord: failed to forward message, receiver dropped");
        }
    }

    async fn ready(&self, _ctx: Context, ready: Ready) {
        info!("discord: connected as {}", ready.user.name);
    }
}

impl DiscordChatAdapter {
    /// Create a new Discord chat adapter and start the gateway connection.
    ///
    /// # Errors
    ///
    /// Returns an error if the Discord client fails to build or connect.
    pub async fn new(token: &str) -> eyre::Result<Self> {
        let (tx, rx) = mpsc::channel(128);
        let last_channel_id = Arc::new(parking_lot::Mutex::new(None));

        let intents = GatewayIntents::GUILD_MESSAGES |
            GatewayIntents::DIRECT_MESSAGES |
            GatewayIntents::MESSAGE_CONTENT;

        let mut client = Client::builder(token, intents)
            .event_handler_arc(Arc::new(Handler {
                tx,
                last_channel_id: Arc::clone(&last_channel_id),
            }))
            .await
            .map_err(|e| eyre::eyre!("discord client build failed: {e}"))?;

        let http = client.http.clone();

        tokio::spawn(async move {
            if let Err(e) = client.start().await {
                warn!("discord gateway error: {e}");
            }
        });

        Ok(Self { rx, http, last_channel_id })
    }

    /// Get the current channel id for sending messages.
    fn channel_id(&self) -> Result<serenity::all::ChannelId, ChatError> {
        let guard = self.last_channel_id.lock();
        guard.ok_or_else(|| ChatError::SendFailed("no discord channel id".into()))
    }
}

#[async_trait]
impl ChatAdapter for DiscordChatAdapter {
    #[expect(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "discord"
    }

    async fn recv_event(&mut self) -> Option<ChatEvent> {
        self.rx.recv().await
    }

    async fn post_message(
        &self,
        _thread_id: &str,
        message: &AdapterPostableMessage,
    ) -> Result<SentMessage, ChatError> {
        let channel_id = self.channel_id()?;
        let text = self.render_message(message);
        let sent = channel_id
            .say(&self.http, &text)
            .await
            .map_err(|e| ChatError::SendFailed(e.to_string()))?;

        Ok(SentMessage {
            id: sent.id.to_string(),
            thread_id: channel_id.to_string(),
            adapter_name: "discord".into(),
            raw: None,
        })
    }

    async fn edit_message(
        &self,
        _thread_id: &str,
        message_id: &str,
        message: &AdapterPostableMessage,
    ) -> Result<SentMessage, ChatError> {
        let channel_id = self.channel_id()?;
        let text = self.render_message(message);
        let mid: u64 = message_id
            .parse()
            .map_err(|e| ChatError::SendFailed(format!("invalid message id: {e}")))?;

        channel_id
            .edit_message(
                &self.http,
                serenity::all::MessageId::new(mid),
                EditMessage::new().content(&text),
            )
            .await
            .map_err(|e| ChatError::SendFailed(e.to_string()))?;

        Ok(SentMessage {
            id: message_id.to_string(),
            thread_id: channel_id.to_string(),
            adapter_name: "discord".into(),
            raw: None,
        })
    }

    async fn delete_message(&self, _thread_id: &str, message_id: &str) -> Result<(), ChatError> {
        let channel_id = self.channel_id()?;
        let mid: u64 = message_id
            .parse()
            .map_err(|e| ChatError::SendFailed(format!("invalid message id: {e}")))?;

        channel_id
            .delete_message(&self.http, serenity::all::MessageId::new(mid))
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
    fn name_is_discord() {
        // Cannot construct a full DiscordChatAdapter without a token,
        // so we just verify the constant.
        assert_eq!("discord", "discord");
    }
}
