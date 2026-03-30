# Design Document: Alice Hexagonal Agent

| Metadata | Details |
| :--- | :--- |
| **Author** | pb-plan agent |
| **Status** | Implemented |
| **Created** | 2026-02-28 |
| **Updated** | 2026-03-03 |
| **Reviewers** | longcipher maintainers |
| **Related Issues** | N/A |

## 1. Executive Summary

Alice is a minimal, extensible AI agent CLI built on the Bob framework (v0.2.0) with hexagonal architecture and local hybrid memory backed by SQLite + FTS5 + sqlite-vec.

**Key design decisions (v0.2 update):**

- Lean heavily on Bob 0.2.0's `AgentLoop` for slash command routing, tape recording, and LLM pipeline execution.
- Use `BuiltinToolPort` for workspace-sandboxed file/shell operations out of the box.
- Use `InMemoryTapeStore` for append-only conversation history.
- Alice focuses on **composition** (wiring) and **local memory** (the unique value-add) -- everything else delegates to the framework.

---

## 2. Requirements & Goals

### 2.1 Problem Statement

Build a lean CLI agent named `alice` on top of Bob 0.2.0 that supports:

- Interactive REPL with slash commands (`/help`, `/tools`, `/tape.search`, `/quit`)
- One-shot prompt execution (`alice run "prompt"`)
- Local persistent memory with hybrid retrieval (BM25 + vector)

### 2.2 Functional Goals

1. **CLI subcommands:** `run` (one-shot), `chat` (interactive REPL), `channel` (future).
2. **Slash command routing:** Leverage `AgentLoop`'s built-in router -- `/help`, `/tools`, `/tool.describe`, `/tape.search`, `/tape.info`, `/anchors`, `/handoff`, `/quit`.
3. **Built-in tools:** Workspace-sandboxed `local/file_read`, `local/file_write`, `local/file_list`, `local/shell_exec` via `BuiltinToolPort`.
4. **Tape recording:** Append-only conversation history via `InMemoryTapeStore`.
5. **Local memory:** SQLite + FTS5 + sqlite-vec hybrid recall, injected as system prompt context.
6. **MCP tool composition:** Optional MCP server integration via `CompositeToolPort`.

### 2.3 Non-Functional Goals

- **Performance:** Memory recall bounded by top-K; FTS indexed; tape in-memory with scc.
- **Reliability:** Schema init idempotent; vector path degrades to FTS-only.
- **Security:** Memory local-only; built-in tools sandboxed to workspace directory.
- **Maintainability:** Minimal composition root; framework does the heavy lifting.

### 2.4 Out of Scope (v1)

- Full OpenClaw parity (web gateway, routine engines, sandbox stack).
- Distributed memory synchronization.
- Non-CLI channels (deferred to `channel` subcommand).
- Streaming responses (framework supports it; Alice v1 uses batch).
- Per-turn memory injection into system prompt (architectural limitation; see section 4.5).

---

## 3. Architecture Overview

### 3.1 System Context

```text
CLI subcommands (run / chat / channel)
  -> AgentLoop (bob-runtime 0.2.0)
      |-- Router: slash commands bypass LLM deterministically
      |-- TapeStore: append-only conversation recording
      |-- EventSink: tracing-based observability
      +-- AgentRuntime -> LLM + Tools
              |-- BuiltinToolPort (file/shell, workspace-sandboxed)
              |-- McpToolAdapter (optional, per config)
              +-- GenAiLlmAdapter (OpenAI/Anthropic/etc.)

  -> MemoryService (crates/common)
      |-- recall_for_turn: hybrid FTS5+vector retrieval
      +-- persist_turn: SQLite write-back
```

### 3.2 Key Design Principles

- **Framework-first:** Use Bob's `AgentLoop`, router, tools, tape instead of reimplementing.
- **Composition over construction:** Alice's bootstrap is a wiring layer, not an orchestration layer.
- **Trait boundaries:** Memory behind `MemoryStorePort`; SQLite in adapters.
- **Graceful degradation:** Vector retrieval optional; FTS path always valid.

### 3.3 Bob 0.2.0 Components Used

| Component | Bob Module | Purpose |
| :--- | :--- | :--- |
| `AgentLoop` | `bob-runtime::agent_loop` | Slash routing + tape recording + system prompt override |
| `router` | `bob-runtime::router` | `/help`, `/tools`, `/tape.*`, `/quit`, etc. |
| `BuiltinToolPort` | `bob-adapters::builtin_tools` | `local/file_read`, `file_write`, `file_list`, `shell_exec` |
| `InMemoryTapeStore` | `bob-adapters::tape_memory` | `scc::HashMap`-backed tape store |
| `CompositeToolPort` | `bob-runtime::composite` | Multi-namespace tool composition |
| `RuntimeBuilder` | `bob-runtime` | LLM/tools/store/events -> `AgentRuntime` |
| `LiterLlmAdapter` | `bob-adapters::llm_liter` | LLM provider adapter |
| `McpToolAdapter` | `bob-adapters::mcp_rmcp` | MCP stdio tool servers |
| `TracingEventSink` | `bob-adapters::observe` | Event observability |
| `InMemorySessionStore` | `bob-adapters::store_memory` | Session state |

---

## 4. Detailed Design

### 4.1 Module Structure

```text
bin/cli-app/
  src/main.rs             # CLI with clap subcommands (run/chat/channel)
  src/lib.rs              # cmd_run, cmd_chat public API
  src/config.rs           # AliceConfig, TOML loading
  src/bootstrap.rs        # Composition root: AgentLoop + memory wiring
  src/memory_context.rs   # Turn-level recall injection + writeback

crates/common/
  src/lib.rs              # Exports memory modules
  src/memory/mod.rs       # Module aggregator
  src/memory/domain.rs    # MemoryEntry, RecallHit, HybridWeights, enums
  src/memory/ports.rs     # MemoryStorePort trait
  src/memory/service.rs   # MemoryService (recall, persist, render)
  src/memory/error.rs     # thiserror types
  src/memory/sqlite_schema.rs  # DDL, FTS5, vec0 tables
  src/memory/sqlite_store.rs   # rusqlite adapter
  src/memory/hybrid.rs    # BM25 + vector score fusion
  tests/memory_sqlite_integration.rs
```

### 4.2 Data Structures

```rust
/// Composition root -- owns all shared resources.
pub struct AliceRuntimeContext {
    pub agent_loop: AgentLoop,
    pub runtime: Arc<dyn AgentRuntime>,
    pub tools: Arc<dyn ToolPort>,
    pub tape: Arc<dyn TapeStorePort>,
    pub memory_service: Arc<MemoryService>,
    pub default_model: String,
}
```

Memory types include: `MemoryEntry`, `RecallQuery`, `RecallHit`, `HybridWeights`, `MemoryStorePort`.

### 4.3 Interface Design

Public integration points:

```rust
// One-shot execution
pub async fn cmd_run(ctx: &AliceRuntimeContext, session_id: &str, prompt: &str) -> eyre::Result<()>;

// Interactive REPL
pub async fn cmd_chat(ctx: &AliceRuntimeContext, session_id: &str) -> eyre::Result<()>;

// Memory helpers (called by cmd_run/cmd_chat)
pub fn inject_memory_prompt(ctx: &AliceRuntimeContext, session_id: &str, input: &str);
pub async fn persist_to_memory(ctx: &AliceRuntimeContext, session_id: &str, input: &str);
```

### 4.4 Logic Flow

1. CLI parses subcommand (`run "prompt" | chat | channel`).
2. Bootstrap composes:
   - `RuntimeBuilder` -> `AgentRuntime` (LLM + tools + store + events + policy).
   - `BuiltinToolPort` (always) + optional MCP servers -> `CompositeToolPort`.
   - `AgentLoop::new(runtime, tools).with_tape(tape).with_events(events)`.
   - Optional `.agent/system-prompt.md` loaded as system prompt override.
   - `MemoryService` with SQLite adapter.
3. For `cmd_run`: single `handle_input -> print -> persist`.
4. For `cmd_chat`: REPL loop:
   - `inject_memory_prompt` (recall + log).
   - `agent_loop.handle_input(input, session_id)`.
   - Match `AgentLoopOutput::{Response, CommandOutput, Quit}`.
   - `persist_to_memory` on successful Response.

### 4.5 Known Limitations

**Per-turn memory injection:** The `AgentLoop` accepts a system prompt at construction time via `with_system_prompt()`. There is no per-request system prompt override API. Current implementation computes recall context per turn but cannot inject it into the LLM request dynamically. Future options:

1. Extend Bob's `AgentLoop` to accept per-request context.
2. Rebuild `AgentLoop` per turn with updated system prompt.
3. Use a memory-as-tool approach (expose recall as a tool the LLM can invoke).

### 4.6 Configuration

```toml
[runtime]
default_model = "openai:gpt-4o-mini"
max_steps = 12
turn_timeout_ms = 90000
# dispatch_mode = "native_preferred"  # or "prompt_guided"

[memory]
db_path = "./.alice/memory.db"
recall_limit = 6
bm25_weight = 0.3
vector_weight = 0.7
vector_dimensions = 384
enable_vector = true

# Built-in tools (local/file_read, file_write, file_list, shell_exec)
# are always available. MCP servers are optional:
# [[mcp.servers]]
# id = "my-server"
# command = "npx"
# args = ["-y", "@my/mcp-server"]
```

### 4.7 Error Handling

- **Library layer (`crates/common`):** `thiserror` enums (`MemoryStoreError`, `MemoryServiceError`).
- **Application layer (`bin/cli-app`):** `eyre::Report` for boot and wiring failures.
- **Memory failures:** Logged via `tracing::warn`, never block agent loop.
- **SQLite boot failure:** Hard error -- cannot open DB means cannot start.
- **Vector failure:** Degrades to FTS-only recall.

---

## 5. Verification & Testing Strategy

### 5.1 Test Coverage

| Test Suite | Count | Scope |
| :--- | :--- | :--- |
| `common` unit tests | 7 | Hybrid scoring, prompt rendering, schema init, sanitization |
| Memory integration tests | 3 | FTS recall, schema idempotency, hybrid vector signal |
| CLI smoke tests | 2 | Agent loop + memory persistence, slash command bypass |
| Bootstrap unit tests | 2 | Runtime build, dispatch mode mapping |
| **Total** | **14+** | |

### 5.2 Verification Harness

| Step | Command | Criteria |
| :--- | :--- | :--- |
| VP-01 | `cargo test -p common` | All memory + utility tests pass |
| VP-02 | `cargo test -p cli-app` | Bootstrap + smoke tests pass |
| VP-03 | `just format` | No formatting changes needed |
| VP-04 | `just lint` | Zero clippy/lint errors |
| VP-05 | `just test` | Full workspace green |

---

## 6. Cross-Functional Concerns

- **Security:** Memory local-only; built-in tools sandboxed to `$PWD`.
- **Observability:** `tracing` events for memory recall latency, hit counts, fallback.
- **Migration:** Schema init idempotent; future versions should version schema.
- **Scope control:** CLI-first; `channel` subcommand reserved for future phases.
