//! ACP (Agent Client Protocol) agent backend implementation.
//!
//! Connects to an external coding agent via the
//! [Agent Client Protocol](https://agentclientprotocol.com), spawning an
//! ACP-compatible subprocess (e.g. `claude-agent-acp`, `codex --acp`) per
//! session and communicating over stdin/stdout.
//!
//! ## Architecture
//!
//! ```text
//! AliceRuntime ──► AcpAgentBackend ──► AcpAgentSession
//!                                            │
//!                                     ┌──────┴──────┐
//!                                     LocalSet thread
//!                                     │  subprocess  │
//!                                     │  stdin/stdout│
//!                                     │  ACP conn    │
//!                                     └─────────────┘
//! ```
//!
//! Each session spawns its own agent subprocess because ACP
//! `ClientSideConnection` is `!Send`. Communication between the async
//! world and the LocalSet thread happens via unbounded channels.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use agent_client_protocol as acp;
use bob_core::types::{FinishReason, RequestContext, TokenUsage};
use bob_runtime::AgentResponse;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use super::{AgentBackend, AgentSession};

static ACP_COUNTER: AtomicU64 = AtomicU64::new(1);

// ── ACP Client implementation ────────────────────────────────────────

/// Alice's ACP client that handles agent notifications.
///
/// Implements `acp::Client` to receive streaming updates from the agent
/// subprocess. This must be `!Send` because ACP connections are pinned
/// to a single-threaded `LocalSet`.
struct AliceAcpClient {
    content: Mutex<String>,
}

impl AliceAcpClient {
    fn new() -> Self {
        Self { content: Mutex::new(String::new()) }
    }

    async fn append_content(&self, text: &str) {
        let mut content = self.content.lock().await;
        content.push_str(text);
    }

    async fn take_content(&self) -> String {
        let mut content = self.content.lock().await;
        std::mem::take(&mut *content)
    }
}

#[async_trait::async_trait(?Send)]
impl acp::Client for AliceAcpClient {
    async fn request_permission(
        &self,
        args: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        // Auto-approve: pick the first "allow" option, otherwise first option.
        let option_id = args
            .options
            .iter()
            .find(|o| {
                matches!(
                    o.kind,
                    acp::PermissionOptionKind::AllowAlways | acp::PermissionOptionKind::AllowOnce
                )
            })
            .or_else(|| args.options.first())
            .map_or_else(|| acp::PermissionOptionId::new("allow_always"), |o| o.option_id.clone());

        Ok(acp::RequestPermissionResponse::new(acp::RequestPermissionOutcome::Selected(
            acp::SelectedPermissionOutcome::new(option_id),
        )))
    }

    async fn session_notification(&self, args: acp::SessionNotification) -> acp::Result<()> {
        if let acp::SessionUpdate::AgentMessageChunk(chunk) = &args.update {
            if let acp::ContentBlock::Text(text_content) = &chunk.content {
                self.append_content(&text_content.text).await;
            }
        }
        Ok(())
    }
}

// ── Session ──────────────────────────────────────────────────────────

/// Per-session command sent to the LocalSet runtime task.
enum SessionCmd {
    /// Send a prompt and wait for the response.
    Prompt {
        text: String,
        context: RequestContext,
        result_tx: oneshot::Sender<eyre::Result<AgentResponse>>,
    },
}

/// ACP-backed agent session.
///
/// Each session owns an ACP subprocess running inside a dedicated
/// `tokio::task::LocalSet` thread. Communication uses unbounded channels.
pub struct AcpAgentSession {
    session_id: String,
    cmd_tx: mpsc::UnboundedSender<SessionCmd>,
    /// Thread handle kept alive to drop the session runtime on session drop.
    _thread: std::thread::JoinHandle<()>,
}

impl std::fmt::Debug for AcpAgentSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcpAgentSession")
            .field("session_id", &self.session_id)
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl AgentSession for AcpAgentSession {
    async fn chat(&self, input: &str, context: RequestContext) -> eyre::Result<AgentResponse> {
        let (result_tx, result_rx) = oneshot::channel();
        self.cmd_tx
            .send(SessionCmd::Prompt { text: input.to_string(), context, result_tx })
            .map_err(|_| eyre::eyre!("ACP session '{}' runtime task exited", self.session_id))?;

        result_rx.await.map_err(|_| {
            eyre::eyre!("ACP session '{}' response channel dropped", self.session_id)
        })?
    }
}

// ── Backend ──────────────────────────────────────────────────────────

/// Configuration for the ACP backend.
#[derive(Debug, Clone)]
pub struct AcpConfig {
    /// Shell command to invoke the ACP agent.
    pub command: String,
    /// Arguments passed to the command.
    pub args: Vec<String>,
    /// Working directory for the agent subprocess.
    pub working_dir: Option<String>,
}

/// ACP-backed agent backend.
///
/// Spawns one ACP subprocess per session. Each subprocess communicates
/// via the Agent Client Protocol over stdin/stdout.
#[derive(Debug, Clone)]
pub struct AcpAgentBackend {
    config: AcpConfig,
}

impl AcpAgentBackend {
    /// Create a new ACP backend from configuration.
    #[must_use]
    pub const fn new(config: AcpConfig) -> Self {
        Self { config }
    }
}

impl AgentBackend for AcpAgentBackend {
    fn create_session(&self) -> Arc<dyn AgentSession> {
        let id = format!("acp-{}", ACP_COUNTER.fetch_add(1, Ordering::Relaxed));
        self.create_session_with_id(&id)
    }

    fn create_session_with_id(&self, session_id: &str) -> Arc<dyn AgentSession> {
        let config = self.config.clone();
        let session_id_owned = session_id.to_string();
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

        let thread = match std::thread::Builder::new()
            .name(format!("acp-{session_id_owned}"))
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                    Ok(rt) => rt,
                    Err(e) => {
                        tracing::error!("failed to build ACP session runtime: {e}");
                        return;
                    }
                };
                rt.block_on(async move {
                    let local = tokio::task::LocalSet::new();
                    local.run_until(run_session_local(config, session_id_owned, cmd_rx)).await;
                });
            }) {
            Ok(handle) => handle,
            Err(e) => {
                tracing::error!("failed to spawn ACP session thread: {e}");
                return Arc::new(AcpAgentSession {
                    session_id: session_id.to_string(),
                    cmd_tx,
                    _thread: std::thread::spawn(|| {}),
                });
            }
        };

        Arc::new(AcpAgentSession { session_id: session_id.to_string(), cmd_tx, _thread: thread })
    }
}

// ── LocalSet session runtime ─────────────────────────────────────────

/// Run a single ACP session inside a `LocalSet`.
///
/// Spawns the ACP agent subprocess, creates the ACP connection,
/// initializes the session, and enters a command loop for `Prompt` requests.
async fn run_session_local(
    config: AcpConfig,
    session_id: String,
    mut cmd_rx: mpsc::UnboundedReceiver<SessionCmd>,
) {
    tracing::info!(session_id, command = %config.command, "ACP session starting");

    // Spawn subprocess directly without shell interpolation
    let mut child = match tokio::process::Command::new(&config.command)
        .args(&config.args)
        .current_dir(config.working_dir.as_deref().unwrap_or("."))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            tracing::error!(session_id, "failed to spawn ACP agent: {e}");
            while let Some(cmd) = cmd_rx.recv().await {
                let SessionCmd::Prompt { result_tx, .. } = cmd;
                let _ = result_tx.send(Err(eyre::eyre!("ACP agent spawn failed: {e}")));
            }
            return;
        }
    };

    let Some(stdin) = child.stdin.take() else {
        tracing::error!(session_id, "ACP agent stdin not available");
        return;
    };
    let Some(stdout) = child.stdout.take() else {
        tracing::error!(session_id, "ACP agent stdout not available");
        return;
    };
    let Some(stderr) = child.stderr.take() else {
        tracing::error!(session_id, "ACP agent stderr not available");
        return;
    };

    let stdin = stdin.compat_write();
    let stdout = stdout.compat();

    // Drain stderr in the background
    tokio::task::spawn_local(async move {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::debug!("ACP agent stderr: {line}");
        }
    });

    // Create ACP client wrapped in Arc for shared access
    let client = Arc::new(AliceAcpClient::new());
    let (conn, handle_io) =
        acp::ClientSideConnection::new(Arc::clone(&client), stdin, stdout, |fut| {
            tokio::task::spawn_local(fut);
        });

    // Spawn the IO handler
    let sid = session_id.clone();
    tokio::task::spawn_local(async move {
        if let Err(e) = handle_io.await {
            tracing::error!(session_id = %sid, "ACP IO error: {e}");
        }
    });

    // Initialize the ACP connection
    use acp::Agent;
    if let Err(e) = conn
        .initialize(acp::InitializeRequest::new(acp::ProtocolVersion::V1).client_info(
            acp::Implementation::new("alice", env!("CARGO_PKG_VERSION")).title("Alice Agent"),
        ))
        .await
    {
        tracing::error!(session_id = %session_id, "ACP initialize failed: {e}");
        let _ = child.kill().await;
        while let Some(cmd) = cmd_rx.recv().await {
            let SessionCmd::Prompt { result_tx, .. } = cmd;
            let _ = result_tx.send(Err(eyre::eyre!("ACP initialize failed: {e}")));
        }
        return;
    }

    // Create a new session
    let acp_session_id =
        match conn.new_session(acp::NewSessionRequest::new(std::path::Path::new("."))).await {
            Ok(resp) => resp.session_id,
            Err(e) => {
                tracing::error!(session_id, "ACP new_session failed: {e}");
                let _ = child.kill().await;
                while let Some(cmd) = cmd_rx.recv().await {
                    let SessionCmd::Prompt { result_tx, .. } = cmd;
                    let _ = result_tx.send(Err(eyre::eyre!("ACP new_session failed: {e}")));
                }
                return;
            }
        };

    tracing::info!(
        session_id,
        acp_session_id = %acp_session_id,
        "ACP session initialized"
    );

    // Command loop
    while let Some(cmd) = cmd_rx.recv().await {
        let SessionCmd::Prompt { text, context, result_tx } = cmd;
        let response = handle_prompt(&conn, &client, &acp_session_id, &text, &context).await;
        let _ = result_tx.send(response);
    }

    // Clean up
    let _ = child.kill().await;
    tracing::info!(session_id, "ACP session stopped");
}

/// Handle a single prompt: send to agent, return response.
async fn handle_prompt(
    conn: &acp::ClientSideConnection,
    client: &AliceAcpClient,
    acp_session_id: &acp::SessionId,
    text: &str,
    context: &RequestContext,
) -> eyre::Result<AgentResponse> {
    use acp::Agent;

    // Build prompt content blocks
    let mut blocks: Vec<acp::ContentBlock> = Vec::new();

    // Prepend system prompt (memory + skills) if present
    if let Some(ref system_prompt) = context.system_prompt {
        blocks.push(acp::ContentBlock::Text(acp::TextContent::new(format!(
            "[System Context]\n{system_prompt}"
        ))));
    }

    blocks.push(acp::ContentBlock::Text(acp::TextContent::new(text.to_string())));

    // Send the prompt
    let prompt_result = conn.prompt(acp::PromptRequest::new(acp_session_id.clone(), blocks)).await;

    match prompt_result {
        Ok(resp) => {
            let reason = format!("{:?}", resp.stop_reason);
            tracing::info!(stop_reason = %reason, "ACP prompt completed");

            let content = client.take_content().await;

            Ok(AgentResponse {
                content,
                usage: TokenUsage::default(),
                finish_reason: FinishReason::Stop,
                is_quit: false,
            })
        }
        Err(e) => {
            tracing::error!("ACP prompt error: {e}");
            Err(eyre::eyre!("ACP prompt failed: {e}"))
        }
    }
}
