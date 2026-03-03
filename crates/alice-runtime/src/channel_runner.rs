//! Channel runner — spawns and manages concurrent channel tasks.
//!
//! Each [`Channel`] is driven in its own tokio task with the loop:
//! `recv → handle_input_with_skills → send` until the channel closes
//! or the agent signals quit.

use std::sync::Arc;

use bob_core::channel::{Channel, ChannelOutput};
use bob_runtime::agent_loop::AgentLoopOutput;

use crate::{
    context::AliceRuntimeContext,
    handle_input::{handle_input_with_skills, output_to_text},
};

/// Run all channels concurrently until they close or the agent quits.
///
/// Spawns one tokio task per channel. Each task loops:
/// `recv → handle_input_with_skills → send` until `recv` returns `None`
/// or the handler produces [`AgentLoopOutput::Quit`].
///
/// Returns when all channel tasks have completed.
///
/// # Errors
///
/// Returns an error if any spawned channel task panics.
pub async fn run_channels(
    ctx: Arc<AliceRuntimeContext>,
    channels: Vec<Box<dyn Channel>>,
) -> eyre::Result<()> {
    let mut handles = Vec::with_capacity(channels.len());

    for channel in channels {
        let ctx = Arc::clone(&ctx);
        let handle = tokio::spawn(run_single_channel(ctx, channel));
        handles.push(handle);
    }

    for handle in handles {
        handle.await?;
    }

    Ok(())
}

/// Drive a single channel to completion.
async fn run_single_channel(ctx: Arc<AliceRuntimeContext>, mut channel: Box<dyn Channel>) {
    loop {
        let Some(msg) = channel.recv().await else {
            break;
        };

        if msg.text.trim().is_empty() {
            continue;
        }

        match handle_input_with_skills(&ctx, &msg.session_id, &msg.text).await {
            Ok(AgentLoopOutput::Quit) => break,
            Ok(ref output) => {
                if let Some(text) = output_to_text(output) &&
                    !text.is_empty()
                {
                    let co = ChannelOutput { text: text.to_string(), is_error: false };
                    if let Err(e) = channel.send(co).await {
                        tracing::warn!("channel send failed: {e}");
                    }
                }
            }
            Err(error) => {
                let co = ChannelOutput { text: format!("error: {error}"), is_error: true };
                if let Err(e) = channel.send(co).await {
                    tracing::warn!("channel error send failed: {e}");
                }
            }
        }
    }
}
