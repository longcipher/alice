//! Chat adapter event loop.
//!
//! Manually polls [`ChatAdapter`] instances for incoming events and routes
//! agent responses back via [`ChatAdapter::post_message`].
//!
//! This bypasses [`ChatBot`]'s handler-based dispatch because bob-chat 0.2.1
//! wires a `NullAdapter` into `ThreadHandle`, making `thread.post()` a no-op.
//! Instead we directly hold adapter references and call `post_message` ourselves.

use std::sync::Arc;

use bob_chat::{adapter::ChatAdapter, event::ChatEvent, message::AdapterPostableMessage};
use bob_runtime::agent_loop::AgentLoopOutput;
use futures_util::stream::{self, StreamExt as _, select_all};

use crate::{
    channel_dispatch::SharedChatAdapter,
    context::AliceRuntimeContext,
    handle_input::{handle_input_with_skills, output_to_text},
    identity::IdentityResolver,
};

/// Run the event loop for one or more chat adapters.
///
/// Each adapter is polled concurrently via `select_all`. When an event
/// arrives, the agent pipeline processes the message and the response is
/// posted back through the *same* adapter that produced the event.
///
/// The loop terminates when every adapter's `recv_event` returns `None`.
///
/// # Errors
///
/// Returns an error if no adapters are provided.
pub async fn run_chatbot(
    ctx: Arc<AliceRuntimeContext>,
    adapters: Vec<Box<dyn ChatAdapter>>,
) -> eyre::Result<()> {
    if adapters.is_empty() {
        return Err(eyre::eyre!("no chat adapters provided"));
    }

    // Wrap each adapter in a tokio Mutex so we can share it between the
    // polling stream (needs &mut for recv_event) and the reply path
    // (needs &self for post_message).
    let shared: Vec<SharedChatAdapter> =
        adapters.into_iter().map(|a| Arc::new(tokio::sync::Mutex::new(a))).collect();

    for adapter in &shared {
        let adapter_name = {
            let guard = adapter.lock().await;
            guard.name().to_owned()
        };
        let _ = ctx.channel_dispatcher().register_adapter(adapter_name, Arc::clone(adapter));
    }

    // Build one stream per adapter, tagged with its index.
    let streams: Vec<_> = shared
        .iter()
        .cloned()
        .enumerate()
        .map(|(idx, adapter)| {
            Box::pin(stream::unfold((adapter, idx), |(a, idx)| async move {
                let mut guard = a.lock().await;
                let event = guard.recv_event().await;
                drop(guard);
                event.map(|e| ((idx, e), (a, idx)))
            }))
        })
        .collect();

    let mut merged = select_all(streams);

    tracing::info!("chatbot event loop started ({} adapter(s))", shared.len());

    while let Some((adapter_idx, event)) = merged.next().await {
        let adapter = Arc::clone(&shared[adapter_idx]);
        let ctx = Arc::clone(&ctx);
        handle_event(ctx, adapter, event).await;
    }

    tracing::info!("chatbot event loop finished — all adapters exhausted");
    Ok(())
}

/// Process a single chat event.
async fn handle_event(
    ctx: Arc<AliceRuntimeContext>,
    adapter: Arc<tokio::sync::Mutex<Box<dyn ChatAdapter>>>,
    event: ChatEvent,
) {
    let (thread_id, text, author_user_id) = match event {
        ChatEvent::Message { thread_id, message } | ChatEvent::Mention { thread_id, message } => {
            (thread_id, message.text, message.author.user_id)
        }
        _ => {
            tracing::debug!("ignoring non-message event");
            return;
        }
    };

    let adapter_name = {
        let guard = adapter.lock().await;
        guard.name().to_owned()
    };
    let resolver = IdentityResolver::new(&ctx);

    match resolver.consume_bind_command(&adapter_name, &author_user_id, &text) {
        Ok(Some(outcome)) => {
            let guard = adapter.lock().await;
            let msg = AdapterPostableMessage::Text(outcome.message);
            if let Err(error) = guard.post_message(&thread_id, &msg).await {
                tracing::warn!("failed to post bind response: {error}");
            }
            return;
        }
        Ok(None) => {}
        Err(error) => {
            let guard = adapter.lock().await;
            let msg = AdapterPostableMessage::Text(format!("Error: {error}"));
            if let Err(post_error) = guard.post_message(&thread_id, &msg).await {
                tracing::warn!("failed to post identity error reply: {post_error}");
            }
            return;
        }
    }

    let identity = match resolver.resolve_message_turn(&adapter_name, &author_user_id, &thread_id) {
        Ok(identity) => identity,
        Err(error) => {
            let guard = adapter.lock().await;
            let msg = AdapterPostableMessage::Text(format!("Error: {error}"));
            if let Err(post_error) = guard.post_message(&thread_id, &msg).await {
                tracing::warn!("failed to post identity resolution error reply: {post_error}");
            }
            return;
        }
    };

    match handle_input_with_skills(&ctx, &identity.session_id, Some(&identity.profile_id), &text)
        .await
    {
        Ok(AgentLoopOutput::Quit) => {
            tracing::info!("quit signal received from session {}", identity.session_id);
        }
        Ok(ref output) => {
            if let Err(error) = resolver.remember_active_session_with_thread_id(
                &identity,
                Some(&adapter_name),
                Some(&thread_id),
            ) {
                tracing::warn!("failed to persist active session: {error}");
            }
            if let Some(response_text) = output_to_text(output) &&
                !response_text.is_empty()
            {
                let guard = adapter.lock().await;
                let msg = AdapterPostableMessage::Text(response_text.to_string());
                if let Err(e) = guard.post_message(&thread_id, &msg).await {
                    tracing::warn!("failed to post reply: {e}");
                }
            }
        }
        Err(error) => {
            let guard = adapter.lock().await;
            let msg = AdapterPostableMessage::Text(format!("Error: {error}"));
            if let Err(e) = guard.post_message(&thread_id, &msg).await {
                tracing::warn!("failed to post error reply: {e}");
            }
        }
    }
}
