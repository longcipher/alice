# Design Document: Alice Hexagonal Agent

| Metadata | Details |
| :--- | :--- |
| **Author** | pb-plan agent |
| **Status** | Draft |
| **Created** | 2026-02-28 |
| **Reviewers** | longcipher maintainers |
| **Related Issues** | N/A |

## 1. Executive Summary

The current repository is still a Rust workspace template (`bin/cli-app` + `crates/common`) and does not yet provide an OpenClaw-like assistant runtime or persistent memory. The requirement is to build a minimal, reusable app named `alice` using the Bob framework and a hexagonal architecture, with local memory backed by SQLite + FTS5 + sqlite-vec.

**Problem:** No agent runtime composition, no long-term memory layer, and no Alice-specific CLI/application behavior exist in the current codebase.
**Solution:** Evolve the template into a lean `alice` CLI composition root over `bob-core`/`bob-runtime`/`bob-adapters`, and add a reusable memory module in `crates/common` that implements hybrid retrieval (BM25 + vector) using SQLite FTS5 and sqlite-vec.

---

## 2. Requirements & Goals

### 2.1 Problem Statement

The workspace currently contains only:

- an example CLI that prints a greeting (`bin/cli-app/src/main.rs`), and
- a tiny utility crate (`crates/common/src/lib.rs`).

It lacks:

- a production-ready Bob runtime bootstrap,
- an Alice-facing CLI interaction model,
- any hexagonal domain/application/adapter boundaries for memory,
- local persistent memory and hybrid search.

### 2.2 Functional Goals

1. **Alice Application:** Deliver a runnable assistant app named `alice` (CLI identity and behavior) on top of Bob runtime crates.
2. **Hexagonal Architecture:** Keep clear boundaries between core use-cases and infrastructure adapters (especially memory persistence/retrieval).
3. **Bob Framework Reuse:** Build orchestration using Bob contracts and builders rather than implementing a custom agent runtime.
4. **Local Memory System:** Implement persistent memory with SQLite, FTS5, and sqlite-vec, inspired by ICM’s hybrid retrieval model.
5. **Memory-Driven Contexting:** Recall relevant memories before each turn and inject them into request context; persist turn outcomes back into memory.
6. **Lean + Reusable Structure:** Keep project small and composable by extending existing crates and shared workspace conventions.

### 2.3 Non-Functional Goals

- **Performance:** Memory recall remains bounded and fast (top-K query, indexed FTS, bounded vector search); no unbounded scans in turn path.
- **Reliability:** Schema initialization is idempotent; failures in vector path degrade to FTS-only recall instead of total runtime failure.
- **Security:** Memory remains local (single SQLite file), no implicit external storage; configurable DB path.
- **Maintainability:** Reuse existing workspace lint/test/format pipeline (`just format`, `just lint`, `just test`) and keep modules focused.

### 2.4 Out of Scope

- Full feature parity with OpenClaw/IronClaw/ZeroClaw (web gateway, background routine engines, full sandbox stack, etc.).
- Distributed/multi-node memory synchronization.
- Rich memoir knowledge-graph subsystem (ICM has this; Alice v1 memory focuses on episodic retrieval for agent turns).
- Non-CLI channels (HTTP server, Telegram, Slack) in this phase.

### 2.5 Assumptions

- Bob crates `bob-core`, `bob-runtime`, and `bob-adapters` (v0.1.2) are the runtime foundation for Alice.
- `sqlite-vec` can be registered via `sqlite3_auto_extension` (as demonstrated in ICM and sqlite-vec crate usage).
- If vector embeddings are unavailable at runtime, Alice still provides FTS-based memory recall.
- Existing crate names (`bin/cli-app`, `crates/common`) can be retained to minimize churn; Alice identity is surfaced through CLI command/config/docs.

### 2.6 Requirements Coverage Checklist

| Requirement from Input | Coverage in Design | Planned Task Coverage |
| :--- | :--- | :--- |
| Build an OpenClaw-like app named Alice | Sections 1, 2.2, 4.4 | Tasks 1.2, 3.1, 3.3 |
| Use Bob AI agent framework | Sections 2.2, 3.1, 3.3, 4.3 | Tasks 1.1, 3.1, 3.2 |
| Use hexagonal architecture | Sections 2.2, 3.2, 4.1, 4.3 | Tasks 1.3, 2.1, 2.2 |
| Keep project minimal/reusable | Sections 2.2, 3.2, 3.3, 4.1 | Tasks 1.1, 1.3, 4.2 |
| Memory inspired by ICM with SQLite + FTS5 + sqlite-vec | Sections 2.2, 4.1, 4.2, 4.4 | Tasks 2.1, 2.2, 2.3, 4.1 |
| Explicitly out-of-scope scope control | Sections 2.4, 7 | Tasks 4.2 (docs clarify boundaries) |

---

## 3. Architecture Overview

### 3.1 System Context

Alice will be a CLI composition root that wires Bob runtime ports/adapters and a local memory subsystem.

```text
User CLI Input
   -> alice CLI (bin/cli-app)
      -> Memory Application Service (crates/common)
         -> Recall from SQLite (FTS5 + vec0)
      -> bob_runtime::AgentRuntime::run(...)
         -> bob_adapters LLM + MCP tool adapters
      -> Memory Application Service
         -> Persist user/assistant turn artifacts
```

The memory subsystem is local and process-embedded, while agent reasoning and tool orchestration stay in Bob runtime.

### 3.2 Key Design Principles

- **Trait boundaries first:** keep memory use-cases behind ports; keep SQLite/sqlite-vec in adapters.
- **Reuse before rebuild:** use Bob runtime/adapters directly instead of creating parallel orchestration layers.
- **Minimal surface area:** keep v1 CLI-first and avoid introducing additional channels/services.
- **Graceful degradation:** vector retrieval optional at runtime; FTS path remains valid.
- **Workspace consistency:** follow existing workspace linting/testing and dependency management conventions.

### 3.3 Existing Components to Reuse

| Component | Location | How to Reuse |
| :--- | :--- | :--- |
| Workspace layout conventions | `Cargo.toml` (`members = ["bin/*", "crates/*"]`) | Keep Alice implementation in existing `bin/` and `crates/` structure. |
| Existing CLI entrypoint pattern | `bin/cli-app/src/main.rs` | Evolve into Alice CLI composition root instead of creating an unrelated bootstrap style. |
| Shared library crate | `crates/common/src/lib.rs` | Extend into reusable memory domain/application/adapter modules. |
| Workspace quality gates | `Justfile`, `.github/workflows/ci.yml` | Keep verification anchored to `just format`, `just lint`, `just test` and CI pipeline. |
| Bob runtime builder and contracts | `bob-runtime` (`RuntimeBuilder`, `AgentRuntime`) | Use as the core runtime orchestration entrypoint for Alice. |
| Bob adapter set | `bob-adapters` (`llm_genai`, `mcp_rmcp`, `observe`, `skills_agent`, `store_memory`) | Reuse for LLM/tools/observability/session plumbing; avoid custom adapter rewrites. |
| ICM schema and sqlite-vec registration pattern | `rtk-ai/icm` (`crates/icm-store/src/schema.rs`, `store.rs`) | Reuse proven schema/index/trigger and `sqlite3_auto_extension` integration concepts for hybrid memory. |

---

## 4. Detailed Design

### 4.1 Module Structure

Planned file/module layout (new or modified):

```text
bin/cli-app/
  src/main.rs                # update: Alice CLI identity + REPL/once mode
  src/config.rs              # new: Alice runtime + memory config loading
  src/bootstrap.rs           # new: Bob runtime wiring + memory service wiring
  src/memory_context.rs      # new: turn-level recall injection + writeback flow

crates/common/
  src/lib.rs                 # update: export memory modules
  src/memory/domain.rs       # new: MemoryEntry, RecallQuery, RecallHit, enums
  src/memory/ports.rs        # new: MemoryStorePort, optional EmbedderPort
  src/memory/service.rs      # new: use-cases (recall_for_turn, persist_turn)
  src/memory/sqlite_schema.rs# new: SQLite DDL, FTS5 tables/triggers, vec table
  src/memory/sqlite_store.rs # new: rusqlite adapter implementing MemoryStorePort
  src/memory/hybrid.rs       # new: score fusion (BM25 + vector)
  src/memory/error.rs        # new: thiserror-based library errors
  tests/memory_sqlite_integration.rs  # new: DB/schema/hybrid search tests
```

### 4.2 Data Structures & Types

Code sketches (design-level):

```rust
pub enum MemoryImportance {
    Critical,
    High,
    Medium,
    Low,
}

pub struct MemoryEntry {
    pub id: String,
    pub session_id: String,
    pub topic: String,
    pub summary: String,
    pub raw_excerpt: String,
    pub keywords: Vec<String>,
    pub importance: MemoryImportance,
    pub embedding: Option<Vec<f32>>,
    pub created_at_epoch_ms: i64,
}

pub struct RecallQuery {
    pub session_id: Option<String>,
    pub text: String,
    pub limit: usize,
}

pub struct RecallHit {
    pub entry: MemoryEntry,
    pub bm25_score: f32,
    pub vector_score: Option<f32>,
    pub final_score: f32,
}

pub struct HybridWeights {
    pub bm25: f32,
    pub vector: f32,
}
```

```rust
pub trait MemoryStorePort: Send + Sync {
    fn init_schema(&self) -> Result<(), MemoryStoreError>;
    fn insert(&self, entry: &MemoryEntry) -> Result<(), MemoryStoreError>;
    fn recall_hybrid(
        &self,
        query: &RecallQuery,
        weights: HybridWeights,
    ) -> Result<Vec<RecallHit>, MemoryStoreError>;
}
```

### 4.3 Interface Design

Public integration points to wire Alice with Bob runtime:

```rust
pub struct AliceRuntimeContext {
    pub runtime: std::sync::Arc<dyn bob_runtime::AgentRuntime>,
    pub memory_service: std::sync::Arc<MemoryService>,
    pub default_model: String,
}

pub async fn build_alice_runtime(cfg: &AliceConfig) -> eyre::Result<AliceRuntimeContext>;

pub async fn run_turn_with_memory(
    ctx: &AliceRuntimeContext,
    session_id: &str,
    input: &str,
) -> eyre::Result<bob_runtime::core::types::AgentResponse>;
```

Request enrichment strategy:

- Build `RequestContext.system_prompt` with top-K recalled memory snippets.
- Keep Bob runtime invocation unchanged (`AgentRuntime::run`).
- Persist turn artifacts after successful/failed runs through `MemoryService`.

### 4.4 Logic Flow

1. CLI loads `alice.toml` config.
2. Bootstrap composes:
   - Bob LLM/tool/session/event adapters via `RuntimeBuilder`.
   - Memory service with SQLite adapter.
3. For each user turn:
   - Query memory with input text (`recall_hybrid`).
   - Inject compact memory context into request system prompt.
   - Run Bob runtime turn.
   - Persist user input + assistant output as new memory entries.
4. On memory adapter errors:
   - Log warning via `tracing`.
   - Continue turn execution with no recalled memory (degraded mode).

### 4.5 Configuration

New config surface in `alice.toml` (design target):

```toml
[runtime]
default_model = "openai:gpt-4o-mini"
max_steps = 12
turn_timeout_ms = 90000

[memory]
db_path = "./.alice/memory.db"
recall_limit = 6
bm25_weight = 0.3
vector_weight = 0.7
vector_dimensions = 384
enable_vector = true
```

Optional extension points:

- memory topic strategy (single topic vs session/topic split),
- recall token budget cap for injected context.

### 4.6 Error Handling

- `crates/common` (library layer): concrete `thiserror` error enums (`MemoryStoreError`, `MemoryServiceError`).
- `bin/cli-app` (application layer): `eyre::Report` for user-facing and wiring failures.
- SQLite startup failure: hard error at boot if DB cannot open.
- Vector-specific failure (e.g., vec table/query issue): downgrade recall to FTS path and emit structured warning.
- Invalid config values (weights, limits, db path): fail fast during config parse/validation.

---

## 5. Verification & Testing Strategy

### 5.1 Unit Testing

- `hybrid.rs`: score fusion correctness and deterministic ordering.
- query sanitization for FTS5 MATCH safety.
- config validation (weight bounds, recall limits).
- memory service logic (recall prompt formatting and persist payload mapping).

### 5.2 Integration Testing

- `crates/common/tests/memory_sqlite_integration.rs`:
  - schema initialization idempotency,
  - FTS-only recall,
  - hybrid recall ordering with vector rows,
  - fallback path when vector data missing.
- `bin/cli-app/tests/alice_once_smoke.rs`:
  - bootstrap with local config,
  - single non-interactive turn path (`--once`) using test doubles where needed.

### 5.3 Critical Path Verification (The "Harness")

| Verification Step | Command | Success Criteria |
| :--- | :--- | :--- |
| **VP-01** | `cargo test -p common --test memory_sqlite_integration` | SQLite schema + recall integration tests pass. |
| **VP-02** | `cargo test -p cli-app --test alice_once_smoke` | End-to-end turn path with memory context injection passes. |
| **VP-03** | `just format` | Formatting checks complete without changes needed afterward. |
| **VP-04** | `just lint` | Clippy/typos/TOML/markdown checks pass. |
| **VP-05** | `just test` | Full workspace tests pass. |

### 5.4 Validation Rules

| Test Case ID | Action | Expected Outcome | Verification Method |
| :--- | :--- | :--- | :--- |
| **TC-01** | Store a memory entry then query with matching keywords | Entry is returned with non-zero BM25 contribution | Integration test with deterministic fixture rows |
| **TC-02** | Query with vector-enabled config and matching embedding | Ranking includes vector contribution and fused score ordering | Integration test asserting final_score ordering |
| **TC-03** | Run a turn with prior memories for same session/topic | Recalled memory text is injected into `RequestContext.system_prompt` | CLI integration test with captured request context |
| **TC-04** | Simulate vector query failure | Alice still runs turn using FTS-only or empty recall path | Integration test + warning assertion |
| **TC-05** | Start with new DB path | Schema/tables/triggers are created once and startup succeeds repeatedly | Repeated init test (idempotency) |

---

## 6. Implementation Plan

- [ ] **Phase 1: Foundation** — Dependency wiring, config/bootstrap scaffolding, hexagonal memory contracts
- [ ] **Phase 2: Core Logic** — SQLite schema, storage adapter, hybrid retrieval, persistence logic
- [ ] **Phase 3: Integration** — Bob runtime + memory recall/writeback + Alice CLI behavior
- [ ] **Phase 4: Polish** — Tests, docs, verification commands, CI alignment

---

## 7. Cross-Functional Concerns

- **Security:** Keep memory local-only by default; do not introduce telemetry/export in v1.
- **Backward compatibility:** Existing template behavior changes from greeting sample to Alice runtime; document this as intentional replacement.
- **Migration:** If future schema versions evolve, keep `sqlite_schema` migration steps idempotent and versioned.
- **Observability:** Use `tracing` events for memory recall latency, hit counts, and fallback events.
- **Scope control:** Preserve minimal CLI-first boundary; defer web/multi-channel/runtime daemon concerns until a later spec.
