//! Channel-aware message dispatch for background runtime workflows.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
};

use async_trait::async_trait;
use bob_chat::{
    adapter::ChatAdapter,
    message::{AdapterPostableMessage, SentMessage},
};

/// Shared chat adapter handle used by the runtime event loop.
pub type SharedChatAdapter = Arc<tokio::sync::Mutex<Box<dyn ChatAdapter>>>;

/// Abstraction for posting messages back to a specific channel/thread.
#[async_trait]
pub trait ChannelPoster: Send + Sync {
    /// Post text back into a specific thread.
    async fn post_text(&self, thread_id: &str, text: &str) -> eyre::Result<SentMessage>;
}

/// [`ChannelPoster`] backed by a live [`ChatAdapter`].
pub struct ChatAdapterPoster {
    adapter: SharedChatAdapter,
}

impl ChatAdapterPoster {
    /// Wrap a shared chat adapter for dispatcher registration.
    #[must_use]
    pub const fn new(adapter: SharedChatAdapter) -> Self {
        Self { adapter }
    }
}

impl std::fmt::Debug for ChatAdapterPoster {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatAdapterPoster").finish_non_exhaustive()
    }
}

#[async_trait]
impl ChannelPoster for ChatAdapterPoster {
    async fn post_text(&self, thread_id: &str, text: &str) -> eyre::Result<SentMessage> {
        let guard = self.adapter.lock().await;
        guard
            .post_message(thread_id, &AdapterPostableMessage::Text(text.to_string()))
            .await
            .map_err(eyre::Error::from)
    }
}

/// Runtime registry of channel posters keyed by adapter name.
#[derive(Clone, Default)]
pub struct ChannelDispatcher {
    posters: Arc<Mutex<HashMap<String, Arc<dyn ChannelPoster>>>>,
}

impl ChannelDispatcher {
    /// Create an empty dispatcher.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a pre-built poster for a channel.
    pub fn register_poster(
        &self,
        channel: impl Into<String>,
        poster: Arc<dyn ChannelPoster>,
    ) -> Option<Arc<dyn ChannelPoster>> {
        self.posters_lock().insert(channel.into(), poster)
    }

    /// Register a live chat adapter for a channel.
    pub fn register_adapter(
        &self,
        channel: impl Into<String>,
        adapter: SharedChatAdapter,
    ) -> Option<Arc<dyn ChannelPoster>> {
        self.register_poster(channel, Arc::new(ChatAdapterPoster::new(adapter)))
    }

    /// Post text to a registered channel/thread pair.
    ///
    /// Returns `Ok(false)` when the channel is not currently registered.
    pub async fn post_text(
        &self,
        channel: &str,
        thread_id: &str,
        text: &str,
    ) -> eyre::Result<bool> {
        let poster = self.posters_lock().get(channel).cloned();
        let Some(poster) = poster else {
            return Ok(false);
        };
        let _ = poster.post_text(thread_id, text).await?;
        Ok(true)
    }

    fn posters_lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<dyn ChannelPoster>>> {
        self.posters.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl std::fmt::Debug for ChannelDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelDispatcher")
            .field("registered_channels", &self.posters_lock().keys().cloned().collect::<Vec<_>>())
            .finish()
    }
}
