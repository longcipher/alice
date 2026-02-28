# Alice Hexagonal Agent — Implementation Tasks

| Metadata | Details |
| :--- | :--- |
| **Design Doc** | specs/2026-02-28-01-alice-hexagonal-agent/design.md |
| **Owner** | longcipher team |
| **Start Date** | 2026-02-28 |
| **Target Date** | 2026-03-07 |
| **Status** | Planning |

## Summary & Phasing

Implement Alice in dependency order: establish reusable contracts and wiring first, then build the SQLite hybrid memory core, then integrate memory into Bob runtime turn flow, and finally complete validation/documentation.

- **Phase 1: Foundation & Scaffolding** — Dependency wiring, config/bootstrap, memory contracts
- **Phase 2: Core Logic** — SQLite schema + storage + hybrid retrieval + writeback mapping
- **Phase 3: Integration & Features** — Bob runtime composition + memory-aware turn execution + Alice CLI UX
- **Phase 4: Polish, QA & Docs** — Full test harness, docs, final verification gates

---

## Phase 1: Foundation & Scaffolding

### Task 1.1: Establish Alice Dependency Baseline

> **Context:** The current workspace is a template. This task creates the minimal dependency baseline for Bob runtime integration and SQLite hybrid memory, while reusing workspace-level dependency rules from `Cargo.toml` and `AGENTS.md`.
> **Verification:** Workspace compiles after dependency wiring with no manual version drift.

- **Priority:** P0
- **Scope:** Build/dependency foundation
- **Status:** 🔴 TODO
- [ ] **Step 1:** Add required workspace dependencies via `cargo add --workspace` (Bob crates, Tokio, tracing, config, rusqlite/sqlite-vec, serde-related crates).
- [ ] **Step 2:** Add crate-level dependencies using `workspace = true` in `bin/cli-app/Cargo.toml` and `crates/common/Cargo.toml`.
- [ ] **Step 3:** Keep root `Cargo.toml` compliant with existing workspace dependency/version conventions.
- [ ] **Verification:** `cargo check --workspace` succeeds.

### Task 1.2: Create Alice CLI Config and Bootstrap Skeleton

> **Context:** Reuse the existing `bin/cli-app/src/main.rs` entrypoint pattern and Bob CLI composition style (`bootstrap.rs`, `config.rs`) to avoid one-off startup code.
> **Verification:** CLI crate parses config and compiles with bootstrap stubs.

- **Priority:** P0
- **Scope:** CLI composition root
- **Status:** 🔴 TODO
- [ ] **Step 1:** Add `bin/cli-app/src/config.rs` with typed runtime/memory config and TOML loading.
- [ ] **Step 2:** Add `bin/cli-app/src/bootstrap.rs` that returns a composed runtime context type.
- [ ] **Step 3:** Update `bin/cli-app/src/main.rs` to expose Alice identity (`name = "alice"`) and support both REPL and `--once` execution mode.
- [ ] **Verification:** `cargo test -p cli-app config` passes.

### Task 1.3: Define Hexagonal Memory Contracts in `crates/common`

> **Context:** This task introduces reusable domain and port boundaries before any SQLite details. It enables adapter swaps and keeps memory logic modular.
> **Verification:** Memory domain/port/service modules compile and unit tests pass.

- **Priority:** P0
- **Scope:** Domain/application contracts
- **Status:** 🔴 TODO
- [ ] **Step 1:** Add memory modules (`domain`, `ports`, `service`, `error`) and export them from `crates/common/src/lib.rs`.
- [ ] **Step 2:** Define `MemoryStorePort` and service methods for turn recall and turn persistence.
- [ ] **Step 3:** Add unit tests for query validation, scoring weight validation, and prompt assembly boundaries.
- [ ] **Verification:** `cargo test -p common --lib` passes.

---

## Phase 2: Core Logic

### Task 2.1: Implement SQLite Schema and Initialization Adapter

> **Context:** Build the storage foundation inspired by ICM’s proven SQLite layout (`memories` + FTS5 + vec table + triggers), adapted to Alice’s narrower v1 scope.
> **Verification:** Schema initialization is idempotent and includes required tables/indices/triggers.

- **Priority:** P0
- **Scope:** Persistence infrastructure
- **Status:** 🔴 TODO
- [ ] **Step 1:** Add `sqlite_schema.rs` with DDL for `memories`, `memories_fts`, and `vec_memories`.
- [ ] **Step 2:** Register sqlite-vec extension during adapter startup and initialize DB pragmas.
- [ ] **Step 3:** Add schema tests for repeated initialization and table existence checks.
- [ ] **Verification:** `cargo test -p common schema` passes.

### Task 2.2: Implement Memory Store Adapter with Hybrid Retrieval

> **Context:** Implement adapter logic behind `MemoryStorePort` using FTS5 BM25 plus vector similarity, with weighted score fusion. This is the core requirement for SQLite + FTS5 + sqlite-vec memory.
> **Verification:** Hybrid recall returns deterministic ranked results and supports FTS fallback.

- **Priority:** P0
- **Scope:** Storage adapter logic
- **Status:** 🔴 TODO
- [ ] **Step 1:** Implement insert/load/recall methods in `sqlite_store.rs` using `rusqlite` queries.
- [ ] **Step 2:** Implement safe FTS query sanitization and weighted score fusion (`bm25_weight`, `vector_weight`).
- [ ] **Step 3:** Add integration tests for BM25-only, hybrid ranking, and empty-result behavior.
- [ ] **Verification:** `cargo test -p common --test memory_sqlite_integration` passes.

### Task 2.3: Implement Turn Memory Writeback and Context Rendering

> **Context:** Convert agent turns into memory entries and render compact recall context for prompt injection. This bridges memory infrastructure to runtime behavior.
> **Verification:** Service outputs deterministic prompt context and persists expected memory artifacts.

- **Priority:** P1
- **Scope:** Memory application use-cases
- **Status:** 🔴 TODO
- [ ] **Step 1:** Implement service logic to persist user input and assistant output as structured memory entries.
- [ ] **Step 2:** Implement recall-to-prompt formatter with bounded snippet count and stable ordering.
- [ ] **Step 3:** Add unit tests for formatting, truncation, and no-hit behavior.
- [ ] **Verification:** `cargo test -p common memory::service` passes.

---

## Phase 3: Integration & Features

### Task 3.1: Compose Bob Runtime for Alice

> **Context:** Reuse Bob’s `RuntimeBuilder`, `GenAiLlmAdapter`, `McpToolAdapter`, `InMemorySessionStore`, and `TracingEventSink` in Alice bootstrap. This satisfies the Bob-framework requirement with minimal custom orchestration.
> **Verification:** Bootstrap returns a working `Arc<dyn AgentRuntime>` with configured defaults.

- **Priority:** P0
- **Scope:** Runtime wiring
- **Status:** 🔴 TODO
- [ ] **Step 1:** Implement runtime construction in `bin/cli-app/src/bootstrap.rs` using Bob components.
- [ ] **Step 2:** Wire policy/timeout/default-model values from `alice.toml` config.
- [ ] **Step 3:** Add bootstrap tests for minimal config and optional MCP server configuration.
- [ ] **Verification:** `cargo test -p cli-app bootstrap` passes.

### Task 3.2: Integrate Memory Recall/Writeback into Turn Execution

> **Context:** This task connects memory service and Bob runtime at the turn boundary: recall before `run`, writeback after completion, fallback on memory errors.
> **Verification:** Alice executes turns even if memory vector path fails, and uses recalled context when available.

- **Priority:** P0
- **Scope:** End-to-end runtime behavior
- **Status:** 🔴 TODO
- [ ] **Step 1:** Add `memory_context.rs` to build request context from memory recall hits.
- [ ] **Step 2:** Wrap runtime turn execution with post-turn memory persistence.
- [ ] **Step 3:** Add integration tests for normal path and degraded (memory-failure) path.
- [ ] **Verification:** `cargo test -p cli-app --test alice_once_smoke` passes.

### Task 3.3: Finalize Alice CLI Behavior and Identity

> **Context:** Ensure the app is clearly `alice` (not template CLI), while staying minimal: REPL + single-turn mode for automation/tests.
> **Verification:** CLI help and one-shot mode reflect Alice naming and behavior.

- **Priority:** P1
- **Scope:** User-facing CLI
- **Status:** 🔴 TODO
- [ ] **Step 1:** Update clap metadata and help text to Alice branding.
- [ ] **Step 2:** Implement `--once` path for deterministic non-interactive execution.
- [ ] **Step 3:** Add a smoke test covering argument parsing and one-shot output path.
- [ ] **Verification:** `cargo test -p cli-app cli` passes.

---

## Phase 4: Polish, QA & Docs

### Task 4.1: Build Full Verification Harness

> **Context:** Consolidate required tests and commands so `/pb-build` can validate completion deterministically.
> **Verification:** Design harness commands all pass in sequence.

- **Priority:** P1
- **Scope:** QA/harness
- **Status:** 🔴 TODO
- [ ] **Step 1:** Ensure integration tests named in `design.md` exist and are stable.
- [ ] **Step 2:** Validate command order and runtime assumptions for CI/local runs.
- [ ] **Step 3:** Fix flaky ordering/timing issues in hybrid ranking assertions.
- [ ] **Verification:** `cargo test -p common --test memory_sqlite_integration && cargo test -p cli-app --test alice_once_smoke` passes.

### Task 4.2: Update Documentation and Example Config

> **Context:** Keep scope and architecture clear for future contributors. Reuse existing `README.md` and workspace conventions instead of adding parallel docs structures.
> **Verification:** Documentation reflects actual runtime/config/memory behavior and out-of-scope boundaries.

- **Priority:** P2
- **Scope:** Docs and onboarding
- **Status:** 🔴 TODO
- [ ] **Step 1:** Update root README with Alice purpose, Bob architecture usage, and memory stack.
- [ ] **Step 2:** Add `alice.toml` example with runtime and memory settings.
- [ ] **Step 3:** Document fallback behavior when vector search is unavailable.
- [ ] **Verification:** `rg -n "alice|memory|sqlite|bob" README.md alice.toml` shows expected sections.

### Task 4.3: Run Final Workspace Quality Gates

> **Context:** Enforce repository workflow requirements after feature completion.
> **Verification:** Formatting, linting, and tests pass using existing workspace commands.

- **Priority:** P0
- **Scope:** Completion gate
- **Status:** 🔴 TODO
- [ ] **Step 1:** Run `just format`.
- [ ] **Step 2:** Run `just lint`.
- [ ] **Step 3:** Run `just test`.
- [ ] **Verification:** All three commands succeed with zero failures.

---

## Summary & Timeline

| Phase | Tasks | Target Date |
| :--- | :---: | :--- |
| **1. Foundation** | 3 | 03-01 |
| **2. Core Logic** | 3 | 03-03 |
| **3. Integration** | 3 | 03-05 |
| **4. Polish** | 3 | 03-07 |
| **Total** | **12** | |

## Definition of Done

1. [ ] **Linted:** No lint errors.
2. [ ] **Tested:** Unit tests covering added logic.
3. [ ] **Formatted:** Code formatter applied.
4. [ ] **Verified:** Task-specific verification criteria met.
