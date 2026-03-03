//! CLI REPL channel adapter.
//!
//! Implements Bob's [`Channel`] trait for an interactive terminal session
//! using tokio async stdin with a [`BufReader`].

use std::io::Write;

use bob_core::channel::{Channel, ChannelError, ChannelMessage, ChannelOutput};
use tokio::io::{AsyncBufReadExt, BufReader};

/// Interactive terminal channel that reads from stdin and writes to stdout/stderr.
///
/// Normal agent output is written to stdout; error output goes to stderr.
/// A `"> "` prompt is printed to stderr before each input line.
#[derive(Debug)]
pub struct CliReplChannel {
    /// Async buffered reader wrapping tokio stdin.
    stdin: BufReader<tokio::io::Stdin>,
    /// Fixed session identifier for this CLI session.
    session_id: String,
}

impl CliReplChannel {
    /// Create a new CLI REPL channel with the given session identifier.
    #[must_use]
    pub fn new(session_id: String) -> Self {
        Self { stdin: BufReader::new(tokio::io::stdin()), session_id }
    }
}

#[async_trait::async_trait]
impl Channel for CliReplChannel {
    async fn recv(&mut self) -> Option<ChannelMessage> {
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
            Ok(_) => Some(ChannelMessage {
                text: line.trim_end().to_string(),
                session_id: self.session_id.clone(),
                sender: None,
            }),
            Err(e) => {
                tracing::warn!("stdin read error: {e}");
                None
            }
        }
    }

    async fn send(&self, output: ChannelOutput) -> Result<(), ChannelError> {
        let result = if output.is_error {
            writeln!(std::io::stderr(), "{}", output.text)
        } else {
            writeln!(std::io::stdout(), "{}", output.text)
        };
        result.map_err(|e| ChannelError::SendFailed(e.to_string()))
    }
}
