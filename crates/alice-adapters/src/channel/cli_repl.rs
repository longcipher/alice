//! CLI REPL chat adapter.
//!
//! Implements [`bob_chat::ChatAdapter`] for an interactive terminal session
//! using tokio async stdin with a [`BufReader`].

use std::{
    io::Write,
    sync::atomic::{AtomicU64, Ordering},
};

use bob_chat::{
    adapter::ChatAdapter,
    card::CardElement,
    error::ChatError,
    event::ChatEvent,
    message::{AdapterPostableMessage, Author, IncomingMessage, SentMessage},
    render_card_as_text,
};
use tokio::io::{AsyncBufReadExt, BufReader};

/// Interactive terminal chat adapter that reads from stdin and writes to stdout/stderr.
///
/// Normal agent output is written to stdout; the prompt goes to stderr.
/// A `"> "` prompt is printed to stderr before each input line.
pub struct CliReplChatAdapter {
    /// Async buffered reader wrapping tokio stdin.
    stdin: BufReader<tokio::io::Stdin>,
    /// Fixed session identifier for this CLI session.
    session_id: String,
    /// Monotonically increasing message counter.
    msg_counter: AtomicU64,
}

impl std::fmt::Debug for CliReplChatAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CliReplChatAdapter")
            .field("session_id", &self.session_id)
            .finish_non_exhaustive()
    }
}

impl CliReplChatAdapter {
    /// Create a new CLI REPL chat adapter with the given session identifier.
    #[must_use]
    pub fn new(session_id: String) -> Self {
        Self {
            stdin: BufReader::new(tokio::io::stdin()),
            session_id,
            msg_counter: AtomicU64::new(0),
        }
    }

    /// Generate the next monotonic message id.
    fn next_id(&self) -> String {
        let n = self.msg_counter.fetch_add(1, Ordering::Relaxed);
        format!("cli-{n}")
    }
}

#[async_trait::async_trait]
impl ChatAdapter for CliReplChatAdapter {
    #[expect(clippy::unnecessary_literal_bound)]
    fn name(&self) -> &str {
        "cli"
    }

    async fn recv_event(&mut self) -> Option<ChatEvent> {
        // Print prompt to stderr (not stdout, to avoid mixing with agent output).
        if write!(std::io::stderr(), "> ").is_err() {
            return None;
        }
        if std::io::stderr().flush().is_err() {
            return None;
        }

        let mut line = String::new();
        match self.stdin.read_line(&mut line).await {
            Ok(0) => None, // EOF
            Ok(_) => {
                let text = line.trim_end().to_string();
                let id = self.next_id();
                Some(ChatEvent::Message {
                    thread_id: self.session_id.clone(),
                    message: IncomingMessage {
                        id,
                        text,
                        author: Author {
                            user_id: "local".into(),
                            user_name: "user".into(),
                            full_name: "Local User".into(),
                            is_bot: false,
                        },
                        attachments: vec![],
                        is_mention: false,
                        thread_id: self.session_id.clone(),
                        timestamp: None,
                    },
                })
            }
            Err(e) => {
                tracing::warn!("stdin read error: {e}");
                None
            }
        }
    }

    async fn post_message(
        &self,
        _thread_id: &str,
        message: &AdapterPostableMessage,
    ) -> Result<SentMessage, ChatError> {
        let text = self.render_message(message);
        writeln!(std::io::stdout(), "{text}").map_err(|e| ChatError::SendFailed(e.to_string()))?;
        Ok(SentMessage {
            id: self.next_id(),
            thread_id: self.session_id.clone(),
            adapter_name: "cli".into(),
            raw: None,
        })
    }

    async fn edit_message(
        &self,
        thread_id: &str,
        _message_id: &str,
        message: &AdapterPostableMessage,
    ) -> Result<SentMessage, ChatError> {
        // CLI has no editing — just re-print.
        self.post_message(thread_id, message).await
    }

    async fn delete_message(&self, _thread_id: &str, _message_id: &str) -> Result<(), ChatError> {
        Err(ChatError::NotSupported("delete in CLI".into()))
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
    use super::*;

    #[test]
    fn name_is_cli() {
        let adapter = CliReplChatAdapter::new("test".into());
        assert_eq!(adapter.name(), "cli");
    }

    #[test]
    fn render_message_text() {
        let adapter = CliReplChatAdapter::new("test".into());
        let msg = AdapterPostableMessage::Text("hello world".into());
        assert_eq!(adapter.render_message(&msg), "hello world");
    }

    #[test]
    fn render_message_markdown() {
        let adapter = CliReplChatAdapter::new("test".into());
        let msg = AdapterPostableMessage::Markdown("**bold**".into());
        assert_eq!(adapter.render_message(&msg), "**bold**");
    }

    #[test]
    fn render_card_produces_text() {
        use bob_chat::card::{CardChild, CardElement, SectionElement};
        let card = CardElement {
            title: Some("Test Card".into()),
            children: vec![CardChild::Section(SectionElement {
                text: Some("section text".into()),
                accessory: None,
            })],
            fallback_text: None,
        };
        let adapter = CliReplChatAdapter::new("test".into());
        let rendered = adapter.render_card(&card);
        assert!(!rendered.is_empty(), "render_card should produce non-empty text");
    }

    #[test]
    fn next_id_increments() {
        let adapter = CliReplChatAdapter::new("test".into());
        assert_eq!(adapter.next_id(), "cli-0");
        assert_eq!(adapter.next_id(), "cli-1");
        assert_eq!(adapter.next_id(), "cli-2");
    }
}
