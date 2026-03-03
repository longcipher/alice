# Alice Hexagonal Agent -- Implementation Tasks

| Metadata | Details |
| :--- | :--- |
| **Design Doc** | specs/2026-02-28-01-alice-hexagonal-agent/design.md |
| **Owner** | longcipher team |
| **Start Date** | 2026-02-28 |
| **Target Date** | 2026-03-07 |
| **Status** | Phase 1-3 Complete, Phase 4 In Progress |

## Summary

Implement Alice in dependency order: foundation + scaffolding, core memory logic, integration with Bob 0.2.0 AgentLoop, then polish/docs.

---

## Phase 1: Foundation & Scaffolding

### Task 1.1: Establish Dependency Baseline

- **Status:** DONE
- [x] Add workspace dependencies (Bob crates, Tokio, tracing, config, rusqlite/sqlite-vec, serde)
- [x] Add crate-level dependencies with `workspace = true`
- [x] `cargo check --workspace` succeeds

### Task 1.2: Create CLI Config and Bootstrap

- **Status:** DONE
- [x] `config.rs` with typed runtime/memory config and TOML loading
- [x] `bootstrap.rs` returns composed `AliceRuntimeContext`
- [x] `main.rs` with clap subcommands: `run`, `chat`, `channel`
- [x] Default (no subcommand) falls through to `chat`

### Task 1.3: Define Hexagonal Memory Contracts

- **Status:** DONE
- [x] Memory modules: `domain`, `ports`, `service`, `error`, `hybrid`, `sqlite_schema`, `sqlite_store`
- [x] `MemoryStorePort` trait with recall and persist methods
- [x] Unit tests for query validation, scoring, prompt rendering

---

## Phase 2: Core Logic

### Task 2.1: SQLite Schema and Initialization

- **Status:** DONE
- [x] DDL for `memories`, `memories_fts` (FTS5), `vec_memories`
- [x] sqlite-vec extension registration
- [x] Idempotent schema init tests

### Task 2.2: Memory Store with Hybrid Retrieval

- **Status:** DONE
- [x] Insert/load/recall via `rusqlite`
- [x] FTS query sanitization + weighted score fusion
- [x] Integration tests: BM25-only, hybrid ranking, empty results

### Task 2.3: Turn Memory Writeback and Context Rendering

- **Status:** DONE
- [x] `persist_turn` converts user/assistant text to memory entries
- [x] `render_recall_context` formats top-K hits for prompt injection
- [x] Unit tests: formatting, empty-hit returns None

---

## Phase 3: Integration & Features

### Task 3.1: Compose Bob 0.2.0 Runtime with AgentLoop

- **Status:** DONE
- [x] `RuntimeBuilder` -> `AgentRuntime` (LLM, tools, store, events, policy)
- [x] `BuiltinToolPort` always included (workspace-sandboxed file/shell)
- [x] `InMemoryTapeStore` for conversation tape
- [x] `AgentLoop::new(runtime, tools).with_tape(tape).with_events(events)`
- [x] Optional `.agent/system-prompt.md` override
- [x] Optional MCP servers via `CompositeToolPort`
- [x] Bootstrap unit tests pass

### Task 3.2: Memory Recall/Writeback in Turn Execution

- **Status:** DONE
- [x] `memory_context::inject_memory_prompt` called before each turn
- [x] `memory_context::persist_to_memory` called after successful turns
- [x] Graceful degradation on memory errors (tracing warn, continue)
- [x] Smoke test: agent loop + memory persistence

### Task 3.3: Alice CLI with Slash Commands

- **Status:** DONE
- [x] `cmd_run`: one-shot via `AgentLoop::handle_input`
- [x] `cmd_chat`: REPL loop matching `AgentLoopOutput::{Response, CommandOutput, Quit}`
- [x] Slash commands routed by framework: `/help`, `/tools`, `/quit`, `/tape.*`, etc.
- [x] Smoke test: `/help` and `/tools` return `CommandOutput` without LLM

---

## Phase 4: Polish, QA & Docs

### Task 4.1: Full Verification Harness

- **Status:** DONE
- [x] 17 tests passing across all crates
- [x] `cargo fmt` + `cargo clippy` clean
- [x] Smoke tests cover agent loop, memory, slash commands

### Task 4.2: Update Design Documents

- **Status:** DONE
- [x] design.md reflects AgentLoop, BuiltinToolPort, tape, slash commands
- [x] tasks.md reflects completion status

### Task 4.3: Run Final Quality Gates

- **Status:** DONE
- [x] `cargo fmt --all` -- no changes
- [x] `cargo clippy --workspace --all-targets` -- zero errors
- [x] `cargo test --workspace` -- 17 tests, 0 failures

---

## Known Future Work

1. **Per-turn memory injection:** AgentLoop needs per-request system prompt API (see design.md section 4.5).
2. **Channel subcommand:** Implement Telegram/Discord channels via Bob's `Channel` trait.
3. **Streaming responses:** AgentLoop supports streaming; wire it for REPL UX.
4. **Persistent tape store:** Replace `InMemoryTapeStore` with SQLite-backed tape.
5. **Memory-as-tool:** Expose memory recall as a tool the LLM can invoke proactively.

---

## Definition of Done

1. [x] **Linted:** No lint errors.
2. [x] **Tested:** Unit + integration tests covering all added logic.
3. [x] **Formatted:** Code formatter applied.
4. [x] **Verified:** All task-specific verification criteria met.
