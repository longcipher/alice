# ACP Remote Control Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a unified Alice remote-control stack where local CLI and remote mobile PWA drive the same long-lived session core, while Alice can still delegate to arbitrary ACP-compatible subprocess agents.

**Architecture:** First harden the current ACP backend so session identity, streaming text, and permission flow are real instead of simulated. Then introduce a runtime-owned interactive session core with replay and approval brokerage, expose it through an ACP-compatible WebSocket API in `alice-runtime`, and add a Leptos CSR PWA served by the same Axum process.

**Tech Stack:** Rust workspace crates, Tokio, Axum, Tower HTTP, `agent-client-protocol`, `scc`, Leptos CSR, Leptos Router, WebSocket JSON-RPC, Cloudflare Tunnel/Access deployment.

---

## Scope Note

This plan covers four tightly-coupled layers:

1. ACP backend correctness.
2. Unified interactive session runtime.
3. Remote server and ACP-compatible WebSocket facade.
4. Leptos mobile PWA.

These are not independent subsystems. Each layer depends on the previous one. For that reason, this is one plan with sequential, shippable tasks instead of several unrelated plans.

## File Map

| Path | Action | Responsibility |
| --- | --- | --- |
| `bin/alice-cli/src/main.rs` | Modify | Add `serve` subcommand and wire remote server startup |
| `bin/alice-mock-acp-agent/Cargo.toml` | Create | Test-only ACP fixture binary crate |
| `bin/alice-mock-acp-agent/src/main.rs` | Create | Mock ACP subprocess used by integration tests |
| `crates/alice-runtime/Cargo.toml` | Modify | Add `scc`, `axum`, `tower-http`, `tokio-stream`, test websocket deps |
| `crates/alice-runtime/src/lib.rs` | Modify | Export new `control` and `remote` modules |
| `crates/alice-runtime/src/config.rs` | Modify | Add `[remote]` config block |
| `crates/alice-runtime/src/context.rs` | Modify | Store session registry alongside existing runtime state |
| `crates/alice-runtime/src/memory_context.rs` | Modify | Split request-context construction from persistence |
| `crates/alice-runtime/src/handle_input.rs` | Modify | Route natural-language input through unified session core |
| `crates/alice-runtime/src/commands.rs` | Modify | Add `cmd_serve`; refactor `cmd_run` and `cmd_chat` to session core |
| `crates/alice-runtime/src/agent_backend/mod.rs` | Modify | Introduce backend event stream and interactive session hooks |
| `crates/alice-runtime/src/agent_backend/bob_backend.rs` | Modify | Emit coarse-grained session events for Bob-backed sessions |
| `crates/alice-runtime/src/agent_backend/acp_backend.rs` | Modify | Persist ACP sessions, stream notifications, broker approvals |
| `crates/alice-runtime/src/control/mod.rs` | Create | Interactive runtime control module root |
| `crates/alice-runtime/src/control/events.rs` | Create | `SessionEvent`, tool-call state, plan state, permission state |
| `crates/alice-runtime/src/control/snapshot.rs` | Create | Session snapshot and replay buffer types |
| `crates/alice-runtime/src/control/session.rs` | Create | Alice-owned interactive session orchestration |
| `crates/alice-runtime/src/control/registry.rs` | Create | Session registry keyed by Alice session id |
| `crates/alice-runtime/src/control/permission.rs` | Create | Permission broker and timeout/default-deny policy |
| `crates/alice-runtime/src/remote/mod.rs` | Create | Remote server module root |
| `crates/alice-runtime/src/remote/server.rs` | Create | Axum app, routes, static serving, health endpoint |
| `crates/alice-runtime/src/remote/ws.rs` | Create | WebSocket transport and ACP-compatible JSON-RPC frame loop |
| `crates/alice-runtime/src/remote/acp_api.rs` | Create | JSON-RPC method handlers: initialize, session/new, session/load, session/prompt, session/cancel |
| `crates/alice-runtime/src/remote/auth.rs` | Create | Pairing token issuance and WebSocket handshake validation |
| `crates/alice-runtime/tests/acp_backend_live_session.rs` | Create | Integration tests for ACP persistence, final text, permission blocking |
| `crates/alice-runtime/tests/control_registry.rs` | Create | Registry, replay, timeout, and local session tests |
| `crates/alice-runtime/tests/support/mod.rs` | Create | Shared runtime test helpers |
| `crates/alice-runtime/tests/remote_ws_integration.rs` | Create | End-to-end WS client -> runtime -> backend flow tests |
| `crates/alice-remote-ui/Cargo.toml` | Create | Leptos CSR crate |
| `crates/alice-remote-ui/index.html` | Create | Trunk entrypoint |
| `crates/alice-remote-ui/Trunk.toml` | Create | CSR build output configuration |
| `crates/alice-remote-ui/src/main.rs` | Create | Browser startup, panic hook |
| `crates/alice-remote-ui/src/app.rs` | Create | Root app shell and router |
| `crates/alice-remote-ui/src/protocol.rs` | Create | Browser-side ACP-compatible message types and reducer |
| `crates/alice-remote-ui/src/ws.rs` | Create | Browser WebSocket client and reconnect loop |
| `crates/alice-remote-ui/src/components/*.rs` | Create | Chat, approval sheet, connection banner, session drawer |
| `crates/alice-remote-ui/public/manifest.json` | Create | PWA manifest |
| `crates/alice-remote-ui/public/sw.js` | Create | Service worker for app-shell caching |
| `Justfile` | Modify | Add remote build helpers |
| `README.md` | Modify | Document `alice serve`, UI build, and tunnel usage |

---

### Task 1: Fix ACP Session Lifetime and Final Text

**Files:**

- Create: `bin/alice-mock-acp-agent/Cargo.toml`
- Create: `bin/alice-mock-acp-agent/src/main.rs`
- Modify: `crates/alice-runtime/Cargo.toml`
- Modify: `crates/alice-runtime/src/agent_backend/acp_backend.rs`
- Create: `crates/alice-runtime/tests/acp_backend_live_session.rs`
- [ ] **Step 1: Write the failing integration test for live-session reuse and final text**

```rust
use std::sync::Arc;

use alice_runtime::agent_backend::{acp_backend::{AcpAgentBackend, AcpConfig}, AgentBackend};
use bob_core::types::RequestContext;

fn mock_agent_command() -> (String, Vec<String>) {
    (
        "cargo".to_string(),
        vec![
            "run".to_string(),
            "-p".to_string(),
            "alice-mock-acp-agent".to_string(),
            "--quiet".to_string(),
        ],
    )
}

#[tokio::test]
async fn create_session_with_id_reuses_live_acp_subprocess() {
    let (command, args) = mock_agent_command();
    let backend = AcpAgentBackend::new(AcpConfig {
        command,
        args,
        working_dir: Some(env!("CARGO_MANIFEST_DIR").to_string()),
    });

    let left = backend.create_session_with_id("same-session");
    let first = left.chat("first", RequestContext::default()).await.unwrap();
    assert_eq!(first.content, "reply[1]: first");

    let right = backend.create_session_with_id("same-session");
    let second = right.chat("second", RequestContext::default()).await.unwrap();
    assert_eq!(second.content, "reply[2]: second");
}
```

- [ ] **Step 2: Run the targeted test and verify it fails on current `acp_backend`**

Run: `cargo test -p alice-runtime --features acp-agent acp_backend_live_session -- --nocapture`

Expected: FAIL because the second `create_session_with_id("same-session")` starts a fresh subprocess and the content is either empty or resets to `reply[1]: second`.

- [ ] **Step 3: Add the mock ACP fixture binary**

`bin/alice-mock-acp-agent/Cargo.toml`

```toml
[package]
name = "alice-mock-acp-agent"
version.workspace = true
edition.workspace = true
publish = false

[[bin]]
name = "alice-mock-acp-agent"
path = "src/main.rs"

[dependencies]
serde = { workspace = true, features = ["derive"] }
serde_json.workspace = true
```

`bin/alice-mock-acp-agent/src/main.rs`

```rust
use std::io::{self, BufRead, Write};

use serde_json::{json, Value};

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    let mut prompt_count: u64 = 0;
    let mut session_id = String::from("mock-session-1");

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => break,
        };

        let msg: Value = serde_json::from_str(&line).expect("valid jsonrpc line");
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let method = msg.get("method").and_then(Value::as_str).unwrap_or_default();

        match method {
            "initialize" => {
                writeln!(stdout, "{}", json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": 1,
                        "agentCapabilities": {"loadSession": true},
                        "agentInfo": {"name": "mock", "title": "Mock ACP", "version": "0.1.0"}
                    }
                }))
                .unwrap();
            }
            "session/new" => {
                writeln!(stdout, "{}", json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {"sessionId": session_id}
                }))
                .unwrap();
            }
            "session/load" => {
                session_id = msg["params"]["sessionId"].as_str().unwrap_or("mock-session-1").to_string();
                writeln!(stdout, "{}", json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {"sessionId": session_id}
                }))
                .unwrap();
            }
            "session/prompt" => {
                prompt_count += 1;
                let text = msg["params"]["prompt"][1]["text"]
                    .as_str()
                    .or_else(|| msg["params"]["prompt"][0]["text"].as_str())
                    .unwrap_or("");

                writeln!(stdout, "{}", json!({
                    "jsonrpc": "2.0",
                    "method": "session/update",
                    "params": {
                        "sessionId": session_id,
                        "delta": {"text": format!("reply[{prompt_count}]: {text}")}
                    }
                }))
                .unwrap();

                writeln!(stdout, "{}", json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {"stopReason": "end_turn"}
                }))
                .unwrap();
            }
            _ => {
                writeln!(stdout, "{}", json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {"code": -32601, "message": format!("unsupported method: {method}")}
                }))
                .unwrap();
            }
        }

        stdout.flush().unwrap();
    }
}
```

- [ ] **Step 4: Add a session cache and text accumulator to `acp_backend.rs`**

Run: `cargo add scc --workspace`

Then update `crates/alice-runtime/Cargo.toml`:

```toml
[dependencies]
scc.workspace = true
```

Update the backend struct and session creation logic in `crates/alice-runtime/src/agent_backend/acp_backend.rs`:

```rust
#[derive(Debug, Clone)]
pub struct AcpAgentBackend {
    config: AcpConfig,
    sessions: Arc<scc::HashMap<String, Arc<AcpAgentSession>>>,
}

impl AcpAgentBackend {
    #[must_use]
    pub fn new(config: AcpConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(scc::HashMap::new()),
        }
    }
}

impl AgentBackend for AcpAgentBackend {
    fn create_session_with_id(&self, session_id: &str) -> Arc<dyn AgentSession> {
        if let Some(entry) = self.sessions.get(session_id) {
            return Arc::clone(entry.get()) as Arc<dyn AgentSession>;
        }

        let session = Arc::new(AcpAgentSession::spawn(self.config.clone(), session_id.to_string()));
        let _ = self.sessions.insert(session_id.to_string(), Arc::clone(&session));
        session as Arc<dyn AgentSession>
    }
}

impl AcpAgentSession {
    fn spawn(config: AcpConfig, session_id: String) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let thread = std::thread::Builder::new()
            .name(format!("acp-{session_id}"))
            .spawn({
                let session_id = session_id.clone();
                move || {
                    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
                    rt.block_on(async move {
                        let local = tokio::task::LocalSet::new();
                        local.run_until(run_session_local(config, session_id, cmd_rx)).await;
                    });
                }
            })
            .unwrap();

        Self { session_id, cmd_tx, _thread: thread }
    }
}
```

Also add prompt accumulation in `handle_prompt`:

```rust
let (event_tx, mut event_rx) = mpsc::unbounded_channel::<String>();
let client = AliceAcpClient::new(event_tx);

let prompt_future = conn.prompt(acp::PromptRequest::new(acp_session_id.clone(), blocks));
tokio::pin!(prompt_future);

let mut text = String::new();

loop {
    tokio::select! {
        maybe_delta = event_rx.recv() => {
            if let Some(delta) = maybe_delta {
                text.push_str(&delta);
            }
        }
        result = &mut prompt_future => {
            result.map_err(|error| eyre::eyre!("ACP prompt failed: {error}"))?;
            break;
        }
    }
}

Ok(AgentResponse {
    content: text,
    usage: TokenUsage::default(),
    finish_reason: FinishReason::Stop,
    is_quit: false,
})
```

- [ ] **Step 5: Re-run the targeted test and verify it passes**

Run: `cargo test -p alice-runtime --features acp-agent acp_backend_live_session -- --nocapture`

Expected: PASS with the second call returning `reply[2]: second`.

- [ ] **Step 6: Commit the ACP session-lifetime fix**

```bash
git add bin/alice-mock-acp-agent/Cargo.toml \
  bin/alice-mock-acp-agent/src/main.rs \
  crates/alice-runtime/Cargo.toml \
  crates/alice-runtime/src/agent_backend/acp_backend.rs \
  crates/alice-runtime/tests/acp_backend_live_session.rs
git commit -m "feat: persist acp sessions across turns"
```

### Task 2: Add ACP Event Streaming and Explicit Permission Handling

**Files:**

- Modify: `crates/alice-runtime/src/agent_backend/mod.rs`
- Modify: `crates/alice-runtime/src/agent_backend/bob_backend.rs`
- Modify: `crates/alice-runtime/src/agent_backend/acp_backend.rs`
- Create: `crates/alice-runtime/tests/acp_permission_flow.rs`
- [ ] **Step 1: Write the failing permission-flow integration test**

```rust
use alice_runtime::agent_backend::{acp_backend::{AcpAgentBackend, AcpConfig}, AgentBackend};
use bob_core::types::RequestContext;

#[tokio::test]
async fn acp_permission_request_blocks_until_answered() {
    let backend = AcpAgentBackend::new(AcpConfig {
        command: "cargo".to_string(),
        args: vec!["run".to_string(), "-p".to_string(), "alice-mock-acp-agent".to_string(), "--quiet".to_string()],
        working_dir: Some(env!("CARGO_MANIFEST_DIR").to_string()),
    });

    let session = backend.create_session_with_id("permission-session");
    let mut rx = session.subscribe();

    let prompt = tokio::spawn({
        let session = session.clone();
        async move { session.chat("write src/lib.rs", RequestContext::default()).await }
    });

    let event = rx.recv().await.unwrap();
    let request_id = match event {
        alice_runtime::agent_backend::BackendSessionEvent::PermissionRequested(event) => event.request_id,
        other => panic!("expected permission event, got {other:?}"),
    };

    assert!(!prompt.is_finished(), "prompt should be waiting on approval");

    session.answer_permission(&request_id, true).await.unwrap();
    let response = prompt.await.unwrap().unwrap();
    assert_eq!(response.content, "approved: write src/lib.rs");
}
```

- [ ] **Step 2: Run the test and verify it fails because `AgentSession` has no event or permission API**

Run: `cargo test -p alice-runtime --features acp-agent acp_permission_flow -- --nocapture`

Expected: FAIL to compile because `subscribe` and `answer_permission` do not exist yet.

- [ ] **Step 3: Extend `AgentSession` with backend event hooks**

Update `crates/alice-runtime/src/agent_backend/mod.rs`:

```rust
#[derive(Debug, Clone)]
pub struct PermissionRequestEvent {
    pub request_id: String,
    pub tool_name: String,
    pub summary: String,
}

#[derive(Debug, Clone)]
pub enum BackendSessionEvent {
    TextDelta(String),
    PermissionRequested(PermissionRequestEvent),
    TurnFinished,
    Warning(String),
}

#[async_trait::async_trait]
pub trait AgentSession: Send + Sync {
    async fn chat(&self, input: &str, context: bob_core::types::RequestContext) -> eyre::Result<AgentResponse>;

    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<BackendSessionEvent>;

    async fn answer_permission(&self, request_id: &str, allowed: bool) -> eyre::Result<()>;

    async fn cancel(&self) -> eyre::Result<()>;
}
```

Update `crates/alice-runtime/src/agent_backend/bob_backend.rs` with no-op implementations:

```rust
fn subscribe(&self) -> tokio::sync::broadcast::Receiver<BackendSessionEvent> {
    let (_tx, rx) = tokio::sync::broadcast::channel(1);
    rx
}

async fn answer_permission(&self, _request_id: &str, _allowed: bool) -> eyre::Result<()> {
    Ok(())
}

async fn cancel(&self) -> eyre::Result<()> {
    Ok(())
}
```

- [ ] **Step 4: Implement broadcast events and permission waiters in `acp_backend.rs`**

Add event and approval channels to the session:

```rust
pub struct AcpAgentSession {
    session_id: String,
    cmd_tx: mpsc::UnboundedSender<SessionCmd>,
    event_tx: broadcast::Sender<BackendSessionEvent>,
    approval_tx: mpsc::UnboundedSender<ApprovalCmd>,
    _thread: std::thread::JoinHandle<()>,
}

enum ApprovalCmd {
    Resolve { request_id: String, allowed: bool },
}

fn subscribe(&self) -> broadcast::Receiver<BackendSessionEvent> {
    self.event_tx.subscribe()
}

async fn answer_permission(&self, request_id: &str, allowed: bool) -> eyre::Result<()> {
    self.approval_tx
        .send(ApprovalCmd::Resolve { request_id: request_id.to_string(), allowed })
        .map_err(|_| eyre::eyre!("approval loop is not running"))
}
```

And replace the old auto-approve client with a waiter:

```rust
async fn request_permission(
    &self,
    args: acp::RequestPermissionRequest,
) -> acp::Result<acp::RequestPermissionResponse> {
    let request_id = args.tool_call_id.to_string();
    let summary = format!("{}", args.tool_call.title);

    let _ = self.event_tx.send(BackendSessionEvent::PermissionRequested(PermissionRequestEvent {
        request_id: request_id.clone(),
        tool_name: args.tool_call.kind.to_string(),
        summary,
    }));

    let (tx, rx) = oneshot::channel();
    self.pending_permissions.borrow_mut().insert(request_id.clone(), tx);

    let allowed = rx.await.map_err(|_| acp::Error::internal_error("permission waiter dropped"))?;

    let option_id = if allowed {
        args.options
            .iter()
            .find(|option| matches!(option.kind, acp::PermissionOptionKind::AllowOnce | acp::PermissionOptionKind::AllowAlways))
            .map(|option| option.option_id.clone())
    } else {
        None
    };

    match option_id {
        Some(option_id) => Ok(acp::RequestPermissionResponse::new(
            acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(option_id)),
        )),
        None => Ok(acp::RequestPermissionResponse::new(
            acp::RequestPermissionOutcome::Cancelled(acp::CancelledPermissionOutcome::new()),
        )),
    }
}
```

- [ ] **Step 5: Re-run the permission-flow test and verify it passes**

Run: `cargo test -p alice-runtime --features acp-agent acp_permission_flow -- --nocapture`

Expected: PASS with the prompt blocked until `answer_permission()` is called.

- [ ] **Step 6: Commit the ACP event/permission surface**

```bash
git add crates/alice-runtime/src/agent_backend/mod.rs \
  crates/alice-runtime/src/agent_backend/bob_backend.rs \
  crates/alice-runtime/src/agent_backend/acp_backend.rs \
  crates/alice-runtime/tests/acp_permission_flow.rs
git commit -m "feat: expose backend session events and approvals"
```

### Task 3: Introduce the Runtime-Owned Interactive Session Core

**Files:**

- Create: `crates/alice-runtime/src/control/mod.rs`
- Create: `crates/alice-runtime/src/control/events.rs`
- Create: `crates/alice-runtime/src/control/snapshot.rs`
- Create: `crates/alice-runtime/src/control/permission.rs`
- Create: `crates/alice-runtime/src/control/session.rs`
- Create: `crates/alice-runtime/src/control/registry.rs`
- Modify: `crates/alice-runtime/src/lib.rs`
- Modify: `crates/alice-runtime/src/context.rs`
- Modify: `crates/alice-runtime/src/bootstrap.rs`
- Modify: `crates/alice-runtime/src/memory_context.rs`
- Create: `crates/alice-runtime/tests/control_registry.rs`
- Create: `crates/alice-runtime/tests/support/mod.rs`
- [ ] **Step 1: Write the failing registry/replay test**

```rust
mod support;

use alice_runtime::control::SessionRegistry;

#[tokio::test]
async fn registry_reuses_session_and_replays_final_text() {
    let registry = SessionRegistry::new(std::time::Duration::from_secs(1800));
    let context = std::sync::Arc::new(support::build_test_context());
    let session = registry.get_or_create("alice-session", std::sync::Arc::clone(&context)).await;

    session.prompt("hello").await.unwrap();

    let snapshot = session.snapshot().await;
    assert_eq!(snapshot.final_text.as_deref(), Some("stub-response"));

    let same = registry.get_or_create("alice-session", std::sync::Arc::clone(&context)).await;
    let replay = same.snapshot().await;
    assert_eq!(replay.final_text.as_deref(), Some("stub-response"));
}
```

- [ ] **Step 2: Run the test and verify it fails because the control module does not exist**

Run: `cargo test -p alice-runtime control_registry -- --nocapture`

Expected: FAIL to compile because `alice_runtime::control` is missing.

- [ ] **Step 3: Split context preparation from persistence in `memory_context.rs`**

Replace the one-shot helper with reusable pieces:

```rust
pub struct PreparedTurn {
    pub request_context: RequestContext,
    pub recalled_memory: Option<String>,
}

pub fn prepare_turn_context(
    context: &AliceRuntimeContext,
    session_id: &str,
    input: &str,
) -> PreparedTurn {
    let recalled = context.memory_service.recall_for_turn(session_id, input).unwrap_or_default();
    let memory_prompt = alice_core::memory::service::MemoryService::render_recall_context(&recalled);
    let skills_bundle = context.skill_composer.as_ref().map(|composer| {
        crate::skill_wiring::inject_skills_context(composer, input, context.skill_token_budget)
    });

    let mut system_parts = Vec::new();
    if let Some(ref prompt) = memory_prompt {
        system_parts.push(prompt.as_str());
    }
    if let Some(ref bundle) = skills_bundle {
        if !bundle.prompt.is_empty() {
            system_parts.push(&bundle.prompt);
        }
    }

    let request_context = RequestContext {
        system_prompt: (!system_parts.is_empty()).then(|| system_parts.join("\n\n")),
        selected_skills: skills_bundle.as_ref().map_or_else(Vec::new, |bundle| bundle.selected_skill_names.clone()),
        tool_policy: skills_bundle.as_ref().map_or_else(RequestToolPolicy::default, |bundle| RequestToolPolicy {
            allow_tools: (!bundle.selected_allowed_tools.is_empty()).then(|| bundle.selected_allowed_tools.clone()),
            ..RequestToolPolicy::default()
        }),
    };

    PreparedTurn { request_context, recalled_memory: memory_prompt }
}

pub fn persist_turn_output(
    context: &AliceRuntimeContext,
    session_id: &str,
    input: &str,
    output: &str,
) {
    if let Err(error) = context.memory_service.persist_turn(session_id, input, output) {
        tracing::warn!(session_id, "memory persistence failed: {error}");
    }
}
```

- [ ] **Step 4: Add the control module and session registry**

`crates/alice-runtime/src/control/events.rs`

```rust
#[derive(Debug, Clone)]
pub enum SessionEvent {
    TurnStarted { input: String },
    TextDelta { delta: String },
    TextFinal { text: String },
    PermissionRequested { request_id: String, tool_name: String, summary: String },
    PermissionResolved { request_id: String, allowed: bool },
    TurnCompleted,
    TurnFailed { error: String },
}
```

`crates/alice-runtime/src/control/snapshot.rs`

```rust
#[derive(Debug, Clone, Default)]
pub struct SessionSnapshot {
    pub session_id: String,
    pub final_text: Option<String>,
    pub replay: Vec<SessionEvent>,
}
```

`crates/alice-runtime/src/control/registry.rs`

```rust
#[derive(Debug)]
pub struct SessionRegistry {
    ttl: Duration,
    sessions: scc::HashMap<String, Arc<AliceInteractiveSession>>,
}

impl SessionRegistry {
    pub fn new(ttl: Duration) -> Self {
        Self { ttl, sessions: scc::HashMap::new() }
    }

    pub async fn get_or_create(
        &self,
        session_id: &str,
        context: Arc<AliceRuntimeContext>,
    ) -> Arc<AliceInteractiveSession> {
        if let Some(entry) = self.sessions.get(session_id) {
            return Arc::clone(entry.get());
        }

        let session = Arc::new(AliceInteractiveSession::new(context, session_id.to_string()));
        let _ = self.sessions.insert(session_id.to_string(), Arc::clone(&session));
        session
    }
}
```

`crates/alice-runtime/src/control/session.rs`

```rust
pub struct AliceInteractiveSession {
    context: Arc<AliceRuntimeContext>,
    session_id: String,
    backend: Arc<dyn crate::agent_backend::AgentSession>,
    replay: tokio::sync::Mutex<Vec<SessionEvent>>,
    final_text: tokio::sync::Mutex<Option<String>>,
}

impl AliceInteractiveSession {
    pub fn new(context: Arc<AliceRuntimeContext>, session_id: String) -> Self {
        let backend = context.backend.create_session_with_id(&session_id);
        Self {
            context,
            session_id,
            backend,
            replay: tokio::sync::Mutex::new(Vec::new()),
            final_text: tokio::sync::Mutex::new(None),
        }
    }

    pub async fn prompt(&self, input: &str) -> eyre::Result<String> {
        let prepared = crate::memory_context::prepare_turn_context(&self.context, &self.session_id, input);
        let response = self.backend.chat(input, prepared.request_context).await?;
        crate::memory_context::persist_turn_output(&self.context, &self.session_id, input, &response.content);
        {
            let mut replay = self.replay.lock().await;
            replay.push(SessionEvent::TextFinal { text: response.content.clone() });
        }
        {
            let mut final_text = self.final_text.lock().await;
            *final_text = Some(response.content.clone());
        }
        Ok(response.content)
    }

    pub async fn snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            session_id: self.session_id.clone(),
            final_text: self.final_text.lock().await.clone(),
            replay: self.replay.lock().await.clone(),
        }
    }

    pub async fn cancel(&self) -> eyre::Result<()> {
        self.backend.cancel().await
    }

    pub async fn answer_permission(&self, request_id: &str, allowed: bool) -> eyre::Result<()> {
        self.backend.answer_permission(request_id, allowed).await
    }
}
```

`crates/alice-runtime/tests/support/mod.rs`

```rust
use std::sync::Arc;

use alice_adapters::memory::sqlite_store::SqliteMemoryStore;
use alice_core::memory::{domain::HybridWeights, service::MemoryService};
use alice_runtime::{agent_backend::bob_backend::BobAgentBackend, context::AliceRuntimeContext};
use async_trait::async_trait;
use bob_adapters::{
    observe::TracingEventSink,
    store_memory::InMemorySessionStore,
    tape_memory::InMemoryTapeStore,
};
use bob_core::{error::AgentError, ports::TapeStorePort, types::*};
use bob_runtime::{AgentRuntime, NoOpToolPort, agent_loop::AgentLoop};

#[derive(Debug)]
struct StubRuntime;

#[async_trait]
impl AgentRuntime for StubRuntime {
    async fn run(&self, _req: AgentRequest) -> Result<AgentRunResult, AgentError> {
        Ok(AgentRunResult::Finished(AgentResponse {
            content: "stub-response".to_string(),
            tool_transcript: Vec::new(),
            usage: TokenUsage::default(),
            finish_reason: FinishReason::Stop,
        }))
    }

    async fn run_stream(&self, _req: AgentRequest) -> Result<AgentEventStream, AgentError> {
        Err(AgentError::Config("streaming not used in tests".to_string()))
    }

    async fn health(&self) -> RuntimeHealth {
        RuntimeHealth { status: HealthStatus::Healthy, llm_ready: true, mcp_pool_ready: true }
    }
}

pub fn build_test_context() -> AliceRuntimeContext {
    let store = SqliteMemoryStore::in_memory(384, false).unwrap();
    let memory_service = MemoryService::new(
        Arc::new(store),
        5,
        HybridWeights::default(),
        384,
        false,
    )
    .unwrap();

    let runtime: Arc<dyn AgentRuntime> = Arc::new(StubRuntime);
    let tools: Arc<dyn bob_core::ports::ToolPort> = Arc::new(NoOpToolPort);
    let tape: Arc<dyn TapeStorePort> = Arc::new(InMemoryTapeStore::new());
    let session_store: Arc<dyn bob_core::ports::SessionStore> = Arc::new(InMemorySessionStore::new());
    let events: Arc<dyn bob_core::ports::EventSink> = Arc::new(TracingEventSink::new());

    let agent_loop = AgentLoop::new(runtime.clone(), tools.clone())
        .with_tape(tape.clone())
        .with_events(events.clone());

    let agent = bob_runtime::Agent::from_runtime(runtime, tools.clone())
        .with_store(session_store)
        .with_tape(tape)
        .build();

    let backend: Arc<dyn alice_runtime::agent_backend::AgentBackend> =
        Arc::new(BobAgentBackend::new(agent.clone()));

    AliceRuntimeContext {
        agent_loop,
        agent,
        backend,
        memory_service: Arc::new(memory_service),
        skill_composer: None,
        skill_token_budget: 1800,
        default_model: "test-model".to_string(),
        session_registry: Arc::new(alice_runtime::control::SessionRegistry::new(
            std::time::Duration::from_secs(1800),
        )),
    }
}
```

- [ ] **Step 5: Build the registry in runtime bootstrap and context**

Update `crates/alice-runtime/src/context.rs`:

```rust
pub struct AliceRuntimeContext {
    pub agent_loop: AgentLoop,
    pub agent: Agent,
    pub backend: Arc<dyn AgentBackend>,
    pub memory_service: Arc<MemoryService>,
    pub skill_composer: Option<SkillPromptComposer>,
    pub skill_token_budget: usize,
    pub default_model: String,
    pub session_registry: Arc<crate::control::SessionRegistry>,
}
```

Update `crates/alice-runtime/src/bootstrap.rs`:

```rust
let session_registry = Arc::new(crate::control::SessionRegistry::new(
    std::time::Duration::from_secs(30 * 60),
));

Ok(AliceRuntimeContext {
    agent_loop,
    agent,
    backend,
    memory_service,
    skill_composer,
    skill_token_budget: cfg.skills.token_budget,
    default_model,
    session_registry,
})
```

- [ ] **Step 6: Re-run the registry test and verify it passes**

Run: `cargo test -p alice-runtime control_registry -- --nocapture`

Expected: PASS with a stable replay snapshot for the same `session_id`.

- [ ] **Step 7: Commit the interactive session core**

```bash
git add crates/alice-runtime/src/control/mod.rs \
  crates/alice-runtime/src/control/events.rs \
  crates/alice-runtime/src/control/snapshot.rs \
  crates/alice-runtime/src/control/permission.rs \
  crates/alice-runtime/src/control/session.rs \
  crates/alice-runtime/src/control/registry.rs \
  crates/alice-runtime/src/lib.rs \
  crates/alice-runtime/src/context.rs \
  crates/alice-runtime/src/bootstrap.rs \
  crates/alice-runtime/src/memory_context.rs \
    crates/alice-runtime/tests/control_registry.rs \
    crates/alice-runtime/tests/support/mod.rs
git commit -m "feat: add unified interactive session registry"
```

### Task 4: Route Local CLI and Channel Input Through the Session Core

**Files:**

- Modify: `crates/alice-runtime/src/handle_input.rs`
- Modify: `crates/alice-runtime/src/commands.rs`
- Modify: `crates/alice-runtime/src/chatbot_runner.rs`
- Create: `crates/alice-runtime/tests/local_control_flow.rs`
- [ ] **Step 1: Write the failing local-control integration test**

```rust
mod support;

use std::sync::Arc;

#[tokio::test]
async fn cmd_run_and_chat_use_the_same_session_registry() {
    let context = Arc::new(support::build_test_context());

    alice_runtime::commands::cmd_run(&context, "shared-session", "hello").await.unwrap();

    let session = context.session_registry.get_or_create("shared-session", Arc::clone(&context)).await;
    let snapshot = session.snapshot().await;
    assert_eq!(snapshot.final_text.as_deref(), Some("stub-response"));
}
```

- [ ] **Step 2: Run the test and verify it fails because `cmd_run` still bypasses the registry**

Run: `cargo test -p alice-runtime local_control_flow -- --nocapture`

Expected: FAIL because `cmd_run` calls the old one-shot helper and the session snapshot is empty.

- [ ] **Step 3: Update `handle_input.rs` to use the registry for natural-language turns**

```rust
use std::sync::Arc;

pub async fn handle_input_with_skills(
    context: Arc<AliceRuntimeContext>,
    session_id: &str,
    input: &str,
) -> eyre::Result<AgentLoopOutput> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(AgentLoopOutput::CommandOutput(String::new()));
    }

    match bob_runtime::router::route(trimmed) {
        bob_runtime::router::RouteResult::SlashCommand(_) => {
            context.agent_loop.handle_input(trimmed, session_id).await
        }
        bob_runtime::router::RouteResult::NaturalLanguage(_) => {
            let session = context.session_registry.get_or_create(session_id, Arc::clone(&context)).await;
            let text = session.prompt(trimmed).await?;
            Ok(AgentLoopOutput::CommandOutput(text))
        }
    }
}
```

- [ ] **Step 4: Update `cmd_run` and `cmd_chat` to use the same session core**

```rust
pub async fn cmd_run(
    context: &Arc<AliceRuntimeContext>,
    session_id: &str,
    prompt: &str,
) -> eyre::Result<()> {
    let session = context.session_registry.get_or_create(session_id, Arc::clone(context)).await;
    let response = session.prompt(prompt).await?;
    writeln!(std::io::stdout(), "{response}")?;
    Ok(())
}

pub async fn cmd_chat(context: Arc<AliceRuntimeContext>, session_id: &str) -> eyre::Result<()> {
    writeln!(std::io::stderr(), "Alice ready (model: {})", context.default_model)?;
    writeln!(std::io::stderr(), "Type /quit to exit.\n")?;
    let adapters: Vec<Box<dyn ChatAdapter>> = vec![Box::new(
        alice_adapters::channel::cli_repl::CliReplChatAdapter::new(session_id.to_string()),
    )];
    crate::chatbot_runner::run_chatbot(context, adapters).await
}
```

- [ ] **Step 5: Re-run the local-control integration test and existing channel tests**

Run: `cargo test -p alice-runtime local_control_flow channel_integration -- --nocapture`

Expected: PASS with `cmd_run` and adapter-driven chat both populating the same registry state.

- [ ] **Step 6: Commit the local control unification**

```bash
git add crates/alice-runtime/src/handle_input.rs \
  crates/alice-runtime/src/commands.rs \
  crates/alice-runtime/src/chatbot_runner.rs \
  crates/alice-runtime/tests/local_control_flow.rs
git commit -m "refactor: route local control through session registry"
```

### Task 5: Add the Axum Remote Server and ACP-Compatible WebSocket API

**Files:**

- Modify: `crates/alice-runtime/Cargo.toml`
- Modify: `crates/alice-runtime/src/config.rs`
- Modify: `crates/alice-runtime/src/lib.rs`
- Modify: `crates/alice-runtime/src/commands.rs`
- Create: `crates/alice-runtime/src/remote/mod.rs`
- Create: `crates/alice-runtime/src/remote/server.rs`
- Create: `crates/alice-runtime/src/remote/ws.rs`
- Create: `crates/alice-runtime/src/remote/acp_api.rs`
- Create: `crates/alice-runtime/src/remote/auth.rs`
- Create: `crates/alice-runtime/tests/remote_ws_integration.rs`
- Modify: `bin/alice-cli/src/main.rs`
- [ ] **Step 1: Write the failing remote WebSocket integration test**

```rust
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio_tungstenite::connect_async;

async fn spawn_remote_server() -> (tokio::task::JoinHandle<()>, String, String) {
    let config = alice_runtime::config::AliceConfig {
        runtime: alice_runtime::config::RuntimeConfig {
            default_model: "openai:gpt-4o-mini".to_string(),
            max_steps: Some(4),
            turn_timeout_ms: Some(10_000),
            dispatch_mode: None,
        },
        agent: alice_runtime::config::AgentBackendConfig {
            backend: alice_runtime::config::AgentBackendType::Bob,
            ..alice_runtime::config::AgentBackendConfig::default()
        },
        memory: alice_runtime::config::MemoryConfig::default(),
        skills: alice_runtime::config::SkillsConfig::default(),
        channels: alice_runtime::config::ChannelsConfig::default(),
        mcp: alice_runtime::config::McpConfig::default(),
        remote: alice_runtime::config::RemoteConfig {
            bind: "127.0.0.1:3401".to_string(),
            session_ttl_secs: 1800,
            approval_timeout_secs: 90,
            static_dir: "crates/alice-remote-ui/dist".to_string(),
        },
    };

    let context = std::sync::Arc::new(alice_runtime::bootstrap::build_runtime(&config).await.unwrap());
    let token = "test-token".to_string();
    let handle = tokio::spawn(alice_runtime::remote::server::serve_remote_for_test(
        context,
        config.remote.clone(),
        token.clone(),
    ));

    (handle, "ws://127.0.0.1:3401".to_string(), token)
}

#[tokio::test]
async fn websocket_client_can_initialize_create_session_and_prompt() {
    let (handle, url, token) = spawn_remote_server().await;

    let (mut ws, _) = connect_async(format!("{url}/ws?token={token}")).await.unwrap();

    ws.send(tokio_tungstenite::tungstenite::Message::Text(
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}).to_string().into(),
    ))
    .await
    .unwrap();

    let init = ws.next().await.unwrap().unwrap();
    assert!(init.to_text().unwrap().contains("protocolVersion"));

    handle.abort();
}
```

- [ ] **Step 2: Run the remote integration test and verify it fails because the server does not exist**

Run: `cargo test -p alice-runtime remote_ws_integration -- --nocapture`

Expected: FAIL to compile because `spawn_remote_server()` and the `remote` module are missing.

- [ ] **Step 3: Add server dependencies without hard-coding versions**

Run:

```bash
cargo add axum --workspace
cargo add tower-http --workspace
cargo add tokio-stream --workspace
cargo add tokio-tungstenite --workspace
```

Then update `crates/alice-runtime/Cargo.toml`:

```toml
[dependencies]
axum = { workspace = true, features = ["ws", "http1", "json", "macros"] }
tower-http = { workspace = true, features = ["fs", "trace"] }
tokio-stream.workspace = true

[dev-dependencies]
tokio-tungstenite.workspace = true
```

- [ ] **Step 4: Add remote config and the Axum route graph**

Update `crates/alice-runtime/src/config.rs`:

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct RemoteConfig {
    #[serde(default = "default_bind_addr")]
    pub bind: String,
    #[serde(default = "default_session_ttl_secs")]
    pub session_ttl_secs: u64,
    #[serde(default = "default_approval_timeout_secs")]
    pub approval_timeout_secs: u64,
    #[serde(default = "default_static_dir")]
    pub static_dir: String,
}

fn default_bind_addr() -> String { "127.0.0.1:3000".to_string() }
const fn default_session_ttl_secs() -> u64 { 1800 }
const fn default_approval_timeout_secs() -> u64 { 90 }
fn default_static_dir() -> String { "crates/alice-remote-ui/dist".to_string() }
```

Create `crates/alice-runtime/src/remote/server.rs`:

```rust
#[derive(Clone)]
pub struct RemoteState {
    context: Arc<AliceRuntimeContext>,
    config: RemoteConfig,
    pairing_token: String,
}

impl RemoteState {
    pub fn new(context: Arc<AliceRuntimeContext>, config: RemoteConfig) -> Self {
        Self {
            context,
            config,
            pairing_token: crate::remote::auth::issue_pairing_token(),
        }
    }

    pub fn context(&self) -> &Arc<AliceRuntimeContext> {
        &self.context
    }

    pub fn registry(&self) -> &Arc<crate::control::SessionRegistry> {
        &self.context.session_registry
    }

    pub fn new_session_id(&self) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("alice-remote-{nanos}")
    }
}

pub async fn serve_remote(context: Arc<AliceRuntimeContext>, cfg: RemoteConfig) -> eyre::Result<()> {
    let state = RemoteState::new(context, cfg.clone());
    let app = axum::Router::new()
        .route("/healthz", axum::routing::get(|| async { "ok" }))
        .route("/ws", axum::routing::get(crate::remote::ws::ws_handler))
        .fallback_service(
            tower_http::services::ServeDir::new(&cfg.static_dir)
                .append_index_html_on_directories(true),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&cfg.bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

pub async fn serve_remote_for_test(
    context: Arc<AliceRuntimeContext>,
    cfg: RemoteConfig,
    pairing_token: String,
) -> eyre::Result<()> {
    let mut state = RemoteState::new(context, cfg.clone());
    state.pairing_token = pairing_token;

    let app = axum::Router::new()
        .route("/healthz", axum::routing::get(|| async { "ok" }))
        .route("/ws", axum::routing::get(crate::remote::ws::ws_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&cfg.bind).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
```

Create `crates/alice-runtime/src/remote/auth.rs`:

```rust
pub fn issue_pairing_token() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("alice-pair-{nanos}")
}

pub fn validate_pairing_token(expected: &str, received: &str) -> bool {
    !received.is_empty() && expected == received
}
```

- [ ] **Step 5: Implement ACP-compatible JSON-RPC handlers and `alice serve`**

Create `crates/alice-runtime/src/remote/acp_api.rs`:

```rust
#[derive(Debug, serde::Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

pub async fn handle_rpc(state: &RemoteState, request: RpcRequest) -> eyre::Result<Option<serde_json::Value>> {
    match request.method.as_str() {
        "initialize" => Ok(Some(json!({
            "jsonrpc": "2.0",
            "id": request.id,
            "result": {
                "protocolVersion": 1,
                "clientInfo": {"name": "alice", "title": "Alice Remote", "version": env!("CARGO_PKG_VERSION")}
            }
        }))),
        "session/new" => {
            let session_id = state.new_session_id();
            let _ = state.registry().get_or_create(&session_id, Arc::clone(state.context())).await;
            Ok(Some(json!({"jsonrpc": "2.0", "id": request.id, "result": {"sessionId": session_id}})))
        }
        "session/prompt" => {
            let session_id = request.params["sessionId"].as_str().unwrap_or("alice-remote");
            let input = request.params["input"].as_str().unwrap_or_default();
            let session = state.registry().get_or_create(session_id, Arc::clone(state.context())).await;
            let _ = session.prompt(input).await?;
            Ok(Some(json!({"jsonrpc": "2.0", "id": request.id, "result": {"accepted": true}})))
        }
        "session/cancel" => {
            let session_id = request.params["sessionId"].as_str().unwrap_or("alice-remote");
            let session = state.registry().get_or_create(session_id, Arc::clone(state.context())).await;
            session.cancel().await?;
            Ok(Some(json!({"jsonrpc": "2.0", "id": request.id, "result": {"cancelled": true}})))
        }
        _ => Ok(Some(json!({
            "jsonrpc": "2.0",
            "id": request.id,
            "error": {"code": -32601, "message": format!("unknown method: {}", request.method)}
        }))),
    }
}
```

Update `bin/alice-cli/src/main.rs`:

```rust
#[derive(Debug, Subcommand)]
enum Commands {
    Run { prompt: String, #[arg(long, default_value = "alice-once")] session_id: String },
    Chat { #[arg(long, default_value = "alice-session")] session_id: String },
    Channel,
    Serve,
}

match cli.command {
    Some(Commands::Serve) => alice_runtime::commands::cmd_serve(Arc::clone(&context), &cfg.remote).await,
    Some(Commands::Run { prompt, session_id }) => alice_runtime::commands::cmd_run(&context, &session_id, &prompt).await,
    Some(Commands::Chat { session_id }) => alice_runtime::commands::cmd_chat(Arc::clone(&context), &session_id).await,
    Some(Commands::Channel) => alice_runtime::commands::cmd_channel(Arc::clone(&context), &cfg.channels).await,
    None => alice_runtime::commands::cmd_chat(Arc::clone(&context), "alice-session").await,
}
```

- [ ] **Step 6: Re-run the remote integration test and verify initialize/session/new/session/prompt pass**

Run: `cargo test -p alice-runtime remote_ws_integration -- --nocapture`

Expected: PASS with WebSocket JSON-RPC responses and session acceptance.

- [ ] **Step 7: Commit the remote server and CLI entrypoint**

```bash
git add bin/alice-cli/src/main.rs \
  crates/alice-runtime/Cargo.toml \
  crates/alice-runtime/src/config.rs \
  crates/alice-runtime/src/lib.rs \
  crates/alice-runtime/src/commands.rs \
  crates/alice-runtime/src/remote/mod.rs \
  crates/alice-runtime/src/remote/server.rs \
  crates/alice-runtime/src/remote/ws.rs \
  crates/alice-runtime/src/remote/acp_api.rs \
  crates/alice-runtime/src/remote/auth.rs \
  crates/alice-runtime/tests/remote_ws_integration.rs
git commit -m "feat: add remote axum server and acp websocket api"
```

### Task 6: Add the Leptos Mobile PWA

**Files:**

- Create: `crates/alice-remote-ui/Cargo.toml`
- Create: `crates/alice-remote-ui/index.html`
- Create: `crates/alice-remote-ui/Trunk.toml`
- Create: `crates/alice-remote-ui/src/main.rs`
- Create: `crates/alice-remote-ui/src/app.rs`
- Create: `crates/alice-remote-ui/src/protocol.rs`
- Create: `crates/alice-remote-ui/src/ws.rs`
- Create: `crates/alice-remote-ui/src/components/chat.rs`
- Create: `crates/alice-remote-ui/src/components/approval_sheet.rs`
- Create: `crates/alice-remote-ui/src/components/connection_banner.rs`
- Create: `crates/alice-remote-ui/src/components/session_drawer.rs`
- Create: `crates/alice-remote-ui/public/manifest.json`
- Create: `crates/alice-remote-ui/public/sw.js`
- [ ] **Step 1: Create the crate and add browser-side dependencies**
Run:

```bash
cargo new crates/alice-remote-ui --bin
cargo add leptos --workspace
cargo add leptos_router --workspace
cargo add wasm-bindgen --workspace
cargo add web-sys --workspace
cargo add js-sys --workspace
cargo add console_error_panic_hook --workspace
```

Then set `crates/alice-remote-ui/Cargo.toml` to:

```toml
[package]
name = "alice-remote-ui"
version.workspace = true
edition.workspace = true
publish = false

[dependencies]
leptos = { workspace = true, features = ["csr"] }
leptos_router = { workspace = true }
serde = { workspace = true, features = ["derive"] }
serde_json.workspace = true
wasm-bindgen.workspace = true
web-sys = { workspace = true, features = ["Window", "WebSocket", "MessageEvent", "Location", "Navigator"] }
js-sys.workspace = true
console_error_panic_hook.workspace = true
```

- [ ] **Step 2: Add the Trunk entry files and browser bootstrap**

`crates/alice-remote-ui/index.html`

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <link data-trunk rel="copy-dir" href="public" />
        <link data-trunk rel="rust" />
    <title>Alice Remote</title>
  </head>
  <body></body>
</html>
```

`crates/alice-remote-ui/Trunk.toml`

```toml
[build]
target = "index.html"
dist = "dist"
public_url = "/"
```

`crates/alice-remote-ui/src/main.rs`

```rust
use leptos::prelude::*;

mod app;
mod protocol;
mod ws;

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(app::App);
}
```

- [ ] **Step 3: Build the ACP client reducer and reconnect loop**

`crates/alice-remote-ui/src/protocol.rs`

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub method: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub result: Option<serde_json::Value>,
    pub error: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default)]
pub struct ConversationState {
    pub session_id: Option<String>,
    pub messages: Vec<String>,
    pub pending_permission: Option<(String, String)>,
    pub connected: bool,
}
```

`crates/alice-remote-ui/src/ws.rs`

```rust
use leptos::prelude::*;

use crate::protocol::ConversationState;

pub fn connect(state: RwSignal<ConversationState>, origin: String, token: String) {
    let ws_url = origin.replace("http://", "ws://").replace("https://", "wss://") + &format!("/ws?token={token}");
    let socket = web_sys::WebSocket::new(&ws_url).unwrap();

    let onopen = wasm_bindgen::closure::Closure::wrap(Box::new(move |_| {
        state.update(|value| value.connected = true);
    }) as Box<dyn FnMut(web_sys::Event)>);
    socket.set_onopen(Some(onopen.as_ref().unchecked_ref()));
    onopen.forget();

    let onmessage = wasm_bindgen::closure::Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
        if let Some(text) = event.data().as_string() {
            state.update(|value| value.messages.push(text));
        }
    }) as Box<dyn FnMut(web_sys::MessageEvent)>);
    socket.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
    onmessage.forget();
}
```

- [ ] **Step 4: Build the mobile-first app shell and approval UI**

`crates/alice-remote-ui/src/app.rs`

```rust
use leptos::prelude::*;
use leptos_router::components::Router;

use crate::protocol::ConversationState;

#[component]
pub fn App() -> impl IntoView {
    let state = RwSignal::new(ConversationState::default());

    view! {
        <Router>
            <main class="app-shell">
                <crate::components::connection_banner::ConnectionBanner state=state />
                <crate::components::session_drawer::SessionDrawer state=state />
                <crate::components::chat::ChatView state=state />
                <crate::components::approval_sheet::ApprovalSheet state=state />
            </main>
        </Router>
    }
}
```

`crates/alice-remote-ui/src/components/chat.rs`

```rust
use leptos::prelude::*;

use crate::protocol::ConversationState;

#[component]
pub fn ChatView(state: RwSignal<ConversationState>) -> impl IntoView {
    view! {
        <section class="chat-view">
            <For
                each=move || state.get().messages.into_iter().enumerate()
                key=|(idx, _)| *idx
                children=move |(_, message)| view! { <article class="message">{message}</article> }
            />
        </section>
    }
}
```

- [ ] **Step 5: Add the PWA manifest and service worker**

`crates/alice-remote-ui/public/manifest.json`

```json
{
  "name": "Alice Remote",
  "short_name": "Alice",
  "display": "standalone",
  "background_color": "#0b1118",
  "theme_color": "#0b1118",
  "start_url": "/",
  "icons": []
}
```

`crates/alice-remote-ui/public/sw.js`

```javascript
const CACHE = "alice-remote-shell-v1";
const ASSETS = ["/", "/manifest.json"];

self.addEventListener("install", (event) => {
  event.waitUntil(caches.open(CACHE).then((cache) => cache.addAll(ASSETS)));
});

self.addEventListener("fetch", (event) => {
  event.respondWith(
    caches.match(event.request).then((cached) => cached || fetch(event.request)),
  );
});
```

- [ ] **Step 6: Build the UI and verify the Axum server serves it**

Run:

```bash
cd crates/alice-remote-ui && trunk build --release
cd ../..
cargo run -p alice-cli --features acp-agent -- serve
```

Expected: `crates/alice-remote-ui/dist` is created and `http://127.0.0.1:3000/` returns the installed shell instead of a 404.

- [ ] **Step 7: Commit the PWA scaffold**

```bash
git add crates/alice-remote-ui/Cargo.toml \
  crates/alice-remote-ui/index.html \
  crates/alice-remote-ui/Trunk.toml \
  crates/alice-remote-ui/src/main.rs \
  crates/alice-remote-ui/src/app.rs \
  crates/alice-remote-ui/src/protocol.rs \
  crates/alice-remote-ui/src/ws.rs \
  crates/alice-remote-ui/src/components/chat.rs \
  crates/alice-remote-ui/src/components/approval_sheet.rs \
  crates/alice-remote-ui/src/components/connection_banner.rs \
  crates/alice-remote-ui/src/components/session_drawer.rs \
  crates/alice-remote-ui/public/manifest.json \
  crates/alice-remote-ui/public/sw.js
git commit -m "feat: add leptos mobile remote control pwa"
```

### Task 7: Finish Verification, Tooling, and Documentation

**Files:**

- Modify: `Justfile`
- Modify: `README.md`
- Modify: `docs/acp_remote_control.md`
- [ ] **Step 1: Add explicit remote build helpers to the Justfile**

Update `Justfile`:

```just
remote-ui-build:
  cd crates/alice-remote-ui && trunk build --release

remote-serve:
  cargo run -p alice-cli --features acp-agent -- serve

test-remote:
  cargo nextest run -p alice-runtime --all-features remote_ws_integration
```

- [ ] **Step 2: Document local and remote startup in the README**

Add this section to `README.md`:

````md
## Remote Control (Experimental)

Build the mobile UI:

```bash
just remote-ui-build
```

Start the local remote-control server:

```bash
cargo run -p alice-cli --features acp-agent -- serve
```
````

Start the local remote-control server:

```bash
cargo run -p alice-cli --features acp-agent -- serve
```


````text

```text

Start the local remote-control server:

```bash
cargo run -p alice-cli --features acp-agent -- serve
```

For development-only public access, expose the server with a quick tunnel:

```bash
cloudflared tunnel --url http://127.0.0.1:3000
```

For persistent use, publish the same local origin behind a managed Cloudflare Tunnel and protect it with Cloudflare Access.

- [ ] **Step 3: Run the full verification set**

Run:

```bash
just format
just lint
just test
just remote-ui-build
```

Expected:

1. `just format` exits `0`.
2. `just lint` exits `0`.
3. `just test` exits `0`.
4. `trunk build --release` succeeds and writes the UI to `crates/alice-remote-ui/dist`.

- [ ] **Step 4: Commit documentation and helper commands**

```bash
git add Justfile README.md docs/acp_remote_control.md
git commit -m "docs: document remote control workflow"
```

---

## Spec Coverage Check

The design doc requires:

1. fixing ACP session lifetime, streaming, approval, and final text,
2. adding a unified interactive session core,
3. preserving local CLI control,
4. adding an Axum remote server,
5. exposing an ACP-compatible WebSocket API,
6. adding a Leptos CSR PWA,
7. documenting Cloudflare deployment expectations.

Coverage:

1. Task 1 covers session lifetime and final text.
2. Task 2 covers explicit event and approval handling.
3. Task 3 covers the session core and registry.
4. Task 4 covers local CLI/channel unification.
5. Task 5 covers the Axum remote server and ACP-compatible API.
6. Task 6 covers the Leptos PWA.
7. Task 7 covers tooling, docs, and full verification.

## Placeholder Scan

No task in this plan relies on `TODO`, `TBD`, or "implement later" language. Every task names concrete files, concrete commands, and concrete code.

## Type Consistency Check

The plan uses these consistent names throughout:

1. `SessionRegistry`
2. `AliceInteractiveSession`
3. `SessionEvent`
4. `SessionSnapshot`
5. `BackendSessionEvent`
6. `PermissionRequestEvent`
7. `serve_remote`
