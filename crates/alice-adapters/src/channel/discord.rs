//! Discord channel adapter.
//!
//! Uses the `serenity` crate for Discord gateway connection.
//! Maps Discord messages to [`bob_core::channel::ChannelMessage`] and
//! agent responses back to Discord replies.

use std::sync::Arc;

use async_trait::async_trait;
use bob_core::channel::{Channel, ChannelError, ChannelMessage, ChannelOutput};
use serenity::all::{Client, Context, EventHandler, GatewayIntents, Message, Ready};
use tokio::sync::mpsc;
use tracing::{info, warn};

/// Discord channel adapter implementing Bob's [`Channel`] trait.
///
/// Incoming Discord messages are forwarded through an mpsc channel.
/// Outgoing responses are sent back to the last known Discord channel.
pub struct DiscordChannel {
    rx: mpsc::Receiver<ChannelMessage>,
    tx_reply: Arc<DiscordReplySender>,
}

impl std::fmt::Debug for DiscordChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiscordChannel").finish_non_exhaustive()
    }
}

/// Holds the serenity HTTP client and last channel id for sending replies.
struct DiscordReplySender {
    http: Arc<serenity::http::Http>,
    last_channel_id: parking_lot::Mutex<Option<serenity::all::ChannelId>>,
}

impl DiscordReplySender {
    async fn send_reply(&self, output: ChannelOutput) -> Result<(), ChannelError> {
        let channel_id = {
            let guard = self.last_channel_id.lock();
            guard.ok_or_else(|| ChannelError::SendFailed("no channel id".to_string()))?
        };
        let text = if output.is_error { format!("⚠️ {}", output.text) } else { output.text };
        channel_id
            .say(&self.http, &text)
            .await
            .map_err(|e| ChannelError::SendFailed(e.to_string()))?;
        Ok(())
    }
}

/// Serenity event handler that forwards messages to the channel receiver.
struct Handler {
    tx: mpsc::Sender<ChannelMessage>,
    reply_sender: Arc<DiscordReplySender>,
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
            let mut guard = self.reply_sender.last_channel_id.lock();
            *guard = Some(msg.channel_id);
        }

        let channel_msg = ChannelMessage {
            text: msg.content.clone(),
            session_id,
            sender: Some(msg.author.name.clone()),
        };

        if self.tx.send(channel_msg).await.is_err() {
            warn!("discord: failed to forward message, receiver dropped");
        }
    }

    async fn ready(&self, _ctx: Context, ready: Ready) {
        info!("discord: connected as {}", ready.user.name);
    }
}

impl DiscordChannel {
    /// Create a new Discord channel adapter and start the gateway connection.
    ///
    /// # Errors
    ///
    /// Returns an error if the Discord client fails to build or connect.
    pub async fn new(token: &str) -> eyre::Result<Self> {
        let (tx, rx) = mpsc::channel(128);

        let intents = GatewayIntents::GUILD_MESSAGES |
            GatewayIntents::DIRECT_MESSAGES |
            GatewayIntents::MESSAGE_CONTENT;

        let mut client = Client::builder(token, intents)
            .event_handler_arc(Arc::new(Handler {
                tx,
                reply_sender: Arc::new(DiscordReplySender {
                    http: Arc::new(serenity::http::Http::new(token)),
                    last_channel_id: parking_lot::Mutex::new(None),
                }),
            }))
            .await
            .map_err(|e| eyre::eyre!("discord client build failed: {e}"))?;

        // Spawn the gateway connection in the background
        let reply_sender = Arc::new(DiscordReplySender {
            http: client.http.clone(),
            last_channel_id: parking_lot::Mutex::new(None),
        });

        tokio::spawn(async move {
            if let Err(e) = client.start().await {
                warn!("discord gateway error: {e}");
            }
        });

        Ok(Self { rx, tx_reply: reply_sender })
    }
}

#[async_trait]
impl Channel for DiscordChannel {
    async fn recv(&mut self) -> Option<ChannelMessage> {
        self.rx.recv().await
    }

    async fn send(&self, output: ChannelOutput) -> Result<(), ChannelError> {
        self.tx_reply.send_reply(output).await
    }
}
