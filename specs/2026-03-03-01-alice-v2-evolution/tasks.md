# Alice v2 Evolution — Implementation Tasks

| Metadata | Details |
| :--- | :--- |
| **Design Doc** | specs/2026-03-03-01-alice-v2-evolution/design.md |
| **Owner** | longcipher team |
| **Start Date** | 2026-03-03 |
| **Target Date** | 2026-03-17 |
| **Status** | Complete |

---

## Summary & Timeline

| Phase | Description | Tasks | Est. Effort |
| :--- | :--- | :--- | :--- |
| 1 | Workspace Restructure | 1.1 – 1.6 | 2–3 days |
| 2 | Skill System Integration | 2.1 – 2.4 | 1–2 days |
| 3 | Channel Adapters | 3.1 – 3.5 | 2–3 days |
| 4 | Comprehensive Testing & Polish | 4.1 – 4.5 | 2–3 days |
| **Total** | | **20 tasks** | **~8–11 days** |

---

## Phase 1: Workspace Restructure

### Task 1.1: Create `alice-core` Crate Scaffold

> **Context:** `alice-core` is the innermost hexagonal layer — pure domain types, port traits, and service logic with zero adapter dependencies. Currently all memory logic lives in `crates/common`. The domain and ports modules move here.
>
> **Verification:** `cargo check -p alice-core` compiles. No external adapter dependencies in `Cargo.toml`.

- [x] Create `crates/alice-core/Cargo.toml` with workspace edition/version, deps: `thiserror`, `serde`, `serde_json`, `parking_lot` only
- [x] Create `crates/alice-core/src/lib.rs` with `pub mod memory`
- [x] Move `crates/common/src/memory/domain.rs` → `crates/alice-core/src/memory/domain.rs` (as-is)
- [x] Move `crates/common/src/memory/ports.rs` → `crates/alice-core/src/memory/ports.rs` (as-is)
- [x] Move `crates/common/src/memory/service.rs` → `crates/alice-core/src/memory/service.rs` (update imports)
- [x] Move `crates/common/src/memory/error.rs` → `crates/alice-core/src/memory/error.rs` (as-is)
- [x] Move `crates/common/src/memory/hybrid.rs` → `crates/alice-core/src/memory/hybrid.rs` (as-is)
- [x] Create `crates/alice-core/src/memory/mod.rs` re-exporting all memory submodules
- [x] Verification: `cargo check -p alice-core` succeeds with zero warnings — 🟢 DONE

### Task 1.2: Create `alice-adapters` Crate Scaffold

> **Context:** `alice-adapters` contains concrete implementations of core ports — SQLite memory store, channel adapters, skill adapter wrappers. The SQLite modules move here from `crates/common`.
>
> **Verification:** `cargo check -p alice-adapters` compiles. Depends on `alice-core` via path.

- [x] Create `crates/alice-adapters/Cargo.toml` with deps: `alice-core` (path), `rusqlite` (bundled), `sqlite-vec`, `serde`, `serde_json`, `tokio`, `tracing`, `parking_lot`
- [x] Create `crates/alice-adapters/src/lib.rs` with `pub mod memory`
- [x] Move `crates/common/src/memory/sqlite_schema.rs` → `crates/alice-adapters/src/memory/sqlite_schema.rs`
- [x] Move `crates/common/src/memory/sqlite_store.rs` → `crates/alice-adapters/src/memory/sqlite_store.rs` (update imports to `alice_core::memory::*`)
- [x] Create `crates/alice-adapters/src/memory/mod.rs` re-exporting `sqlite_schema` and `sqlite_store`
- [x] Move `crates/common/tests/memory_sqlite_integration.rs` → `crates/alice-adapters/tests/memory_sqlite_integration.rs` (update imports)
- [x] Verification: `cargo check -p alice-adapters` succeeds; `cargo test -p alice-adapters` passes (4 tests) — 🟢 DONE

### Task 1.3: Create `alice-runtime` Crate and Migrate Logic

> **Context:** `alice-runtime` owns the composition root, config, bootstrap, commands, and context. Currently these live in `bin/cli-app/src/`. Moving them to a library crate enables reuse by multiple binaries and test harnesses.
>
> **Verification:** `cargo check -p alice-runtime` compiles. Contains config, bootstrap, context, commands, memory_context modules.

- [x] Create `crates/alice-runtime/Cargo.toml` with deps: `alice-core` (path), `alice-adapters` (path), `bob-core`, `bob-runtime`, `bob-adapters` (features = ["skills-agent"]), `config`, `eyre`, `serde`, `tokio`, `tracing`, `async-trait`
- [x] Create `crates/alice-runtime/src/lib.rs` with module declarations and public API re-exports
- [x] Move `bin/cli-app/src/config.rs` → `crates/alice-runtime/src/config.rs` (extend with `SkillsConfig`, `ChannelsConfig`)
- [x] Move `bin/cli-app/src/bootstrap.rs` → `crates/alice-runtime/src/bootstrap.rs` (update imports to `alice_core`, `alice_adapters`)
- [x] Extract `AliceRuntimeContext` into `crates/alice-runtime/src/context.rs`
- [x] Move `bin/cli-app/src/memory_context.rs` → `crates/alice-runtime/src/memory_context.rs` (update imports)
- [x] Move `cmd_run`, `cmd_chat` from `bin/cli-app/src/lib.rs` → `crates/alice-runtime/src/commands.rs`
- [x] Verification: `cargo check -p alice-runtime` succeeds; unit tests in config/bootstrap modules pass — 🟢 DONE (4 tests)

### Task 1.4: Rename `bin/cli-app` to `bin/alice-cli`

> **Context:** The binary crate should be thin — only CLI parsing and dispatch. All logic now lives in `alice-runtime`. The crate rename also affects workspace member resolution.
>
> **Verification:** `cargo run -p alice-cli -- --help` works. Binary name is `alice`.

- [x] Rename `bin/cli-app/` directory to `bin/alice-cli/`
- [x] Update `bin/alice-cli/Cargo.toml`: `name = "alice-cli"`, `[[bin]] name = "alice"`, deps: `alice-runtime` (path), `clap`, `eyre`, `tokio`, `tracing-subscriber`
- [x] Rewrite `bin/alice-cli/src/main.rs` to be thin: clap parsing, tracing init, delegate to `alice_runtime::commands::*`
- [x] Rewrite `bin/alice-cli/src/lib.rs` to minimally re-export from `alice-runtime` (or remove if unnecessary)
- [x] Move `bin/cli-app/tests/alice_once_smoke.rs` → `bin/alice-cli/tests/` (update imports to `alice_runtime`)
- [x] Verification: `cargo build -p alice-cli`, `cargo test -p alice-cli` both pass — 🟢 DONE (2 tests)

### Task 1.5: Remove `crates/common` and Update Workspace

> **Context:** All code from `crates/common` has been migrated to `alice-core` and `alice-adapters`. The crate can be removed.
>
> **Verification:** `cargo check --workspace` succeeds. No references to `common` remain.

- [x] Remove `crates/common/` directory entirely
- [x] Update root `Cargo.toml` workspace members — ensure `bin/*` and `crates/*` glob still picks up the new crates
- [x] Run `cargo check --workspace` to verify no broken references
- [x] Verification: `just lint` passes (no dead code warnings about `common`) — 🟢 DONE

### Task 1.6: Post-Migration Verification

> **Context:** All migrations are done. Run the full quality gate to ensure nothing broke.
>
> **Verification:** All three commands pass cleanly.

- [x] `just format` — no changes
- [x] `just lint` — zero warnings/errors
- [x] `just test` — all existing tests pass (15 tests across all crates)
- [x] Verification: all three just commands succeed with clean output — 🟢 DONE

---

## Phase 2: Skill System Integration

### Task 2.1: Enable Bob `skills-agent` Feature and Add Skill Config

> **Context:** Bob 0.2.0 provides `bob_adapters::skills_agent` behind a feature gate. We need to enable it and add the `SkillsConfig` section to Alice's configuration. Task 1.3 already added the config types; this task wires the feature flag and validates config loading.
>
> **Verification:** `cargo check -p alice-adapters` with `skills-agent` feature compiles. Config with `[skills]` section parses correctly.

- [x] Add `bob-adapters = { workspace = true, features = ["skills-agent"] }` to `crates/alice-adapters/Cargo.toml` (if not already set) — skills-agent is a default feature, already available
- [x] Verify `use bob_adapters::skills_agent::*` compiles in `alice-adapters` — confirmed via alice-runtime
- [x] Add unit test: parse `alice.toml` with `[skills]` section, verify `SkillsConfig` defaults and overrides — in config.rs parse_full_config
- [x] Add unit test: parse `alice.toml` without `[skills]` section, verify defaults (enabled=true, max_selected=3, token_budget=1800) — in config.rs parse_minimal_config
- [x] Verification: `cargo test -p alice-runtime` passes config tests — 🟢 DONE

### Task 2.2: Implement Skill Wiring in Bootstrap

> **Context:** The bootstrap needs to build a `SkillPromptComposer` from config and store it in `AliceRuntimeContext`. The composer is stateless and created once at startup.
>
> **Verification:** Bootstrap with skill sources builds successfully. Stub test with temp skill directories verifies composer creation.

- [x] Create `crates/alice-runtime/src/skill_wiring.rs` with `build_skill_composer(cfg: &SkillsConfig) -> eyre::Result<Option<SkillPromptComposer>>`
- [x] Add `skill_composer: Option<SkillPromptComposer>` and `skill_token_budget: usize` fields to `AliceRuntimeContext`
- [x] Wire `build_skill_composer` call in `bootstrap::build_runtime()`
- [x] Add unit test: bootstrap with empty skill sources → `skill_composer` is None (3 unit tests in skill_wiring)
- [x] Add integration test: create temp dir with a `SKILL.md` fixture, bootstrap → verify composer has 1 skill loaded (deferred to 2.4)
- [x] Verification: `cargo test -p alice-runtime` passes skill wiring tests — 🟢 DONE (7 tests)

### Task 2.3: Implement Per-Turn Skill Injection

> **Context:** For each natural language input, the skill composer selects relevant skills, renders a prompt fragment, and provides tool policy constraints. This needs to be injected into the `AgentRequest.context` before calling `runtime.run()`. See design.md section 4.3 for the `handle_input_with_skills` approach.
>
> **Verification:** Skill-augmented turn includes skill names in response context. Mock test verifies prompt augmentation.

- [x] Create `inject_skills_context()` function in `skill_wiring.rs` that calls `composer.render_bundle_for_input_with_policy()`
- [x] Implement `handle_input_with_skills()` in `handle_input.rs` + `output_to_text()` helper
- [x] Update `cmd_chat` to use `handle_input_with_skills()` for slash commands + skill-augmented NL
- [x] Skills injected in `memory_context::run_turn_with_memory()` — composes system prompt from memory + skills, populates `selected_skills` and `tool_policy`
- [x] Verification: `cargo test -p alice-runtime` passes all tests — 🟢 DONE

### Task 2.4: Skill System Integration Tests

> **Context:** End-to-end skill flow: load skills from fixture directories → select for input → verify prompt rendering → verify tool policy. Also test graceful degradation when no skills match.
>
> **Verification:** All skill integration tests pass. Coverage of: skill loading, selection, prompt rendering, no-match fallback, invalid source handling.

- [x] Create `crates/alice-runtime/tests/` directory
- [x] Add integration test: load 3 fixture skills → input that matches one → verify only that skill selected
- [x] Add integration test: input that matches no skills → verify empty selection, no skill prompt injected
- [x] Add integration test: skill with `allowed_tools` → verify `RequestToolPolicy.allow_tools` populated
- [x] Add integration test: invalid skill source path → verify graceful error, agent operates without skills
- [x] Create `tests/fixtures/skills/` directory with 2-3 test `SKILL.md` files for integration tests
- [x] Verification: `cargo test -p alice-runtime` — all skill integration tests pass (7 integration tests) — 🟢 DONE

---

## Phase 3: Channel Adapters

### Task 3.1: Implement `CliReplChannel`

> **Context:** Extract the current REPL read/write logic from `cmd_chat` into a proper `Channel` implementation. This is the simplest channel and validates the pattern before Discord/Telegram.
>
> **Verification:** `cmd_chat` refactored to use `CliReplChannel` + channel runner. Interactive REPL works as before.

- [x] Create `crates/alice-adapters/src/channel/mod.rs` declaring `cli_repl`, `discord`, `telegram` submodules
- [x] Create `crates/alice-adapters/src/channel/cli_repl.rs` implementing `CliReplChannel`:
  - `recv()`: read line from stdin, return `ChannelMessage { text, session_id, sender: None }`
  - `send()`: print to stdout (normal) or stderr (error)
  - Return `None` on EOF/Ctrl-D
- [x] Add unit test: `CliReplChannel` with piped input → returns expected `ChannelMessage` sequence → `None` on EOF (deferred to 3.5)
- [x] Verification: `cargo check -p alice-adapters` compiles — 🟢 DONE

### Task 3.2: Implement Channel Runner

> **Context:** The channel runner manages one or more `Channel` instances concurrently. Each channel gets its own tokio task. Messages flow: `Channel.recv()` → `handle_input_with_skills()` → `Channel.send()`. See design.md section 4.4.4.
>
> **Verification:** Channel runner with mock channel processes messages correctly. Multiple channels run concurrently.

- [x] Create `crates/alice-runtime/src/channel_runner.rs` with `run_channels(ctx, channels)`:
  - Spawn one tokio task per channel
  - Each task: loop `recv → handle → send` until `recv` returns None or `Quit`
  - Graceful shutdown on channel error (log, continue with remaining channels)
- [x] Implement `cmd_channel` in `commands.rs`:
  - Construct enabled channels based on `ChannelsConfig`
  - Always include `CliReplChannel`
  - Start channel runner with Discord/Telegram wiring behind feature flags
- [x] Add unit test: mock channel with 3 messages → verify all 3 processed and responded (deferred to 3.5)
- [x] Update `cmd_chat` to use `CliReplChannel` + channel runner instead of inline REPL loop
- [x] Verification: `cargo check -p alice-runtime` compiles, all existing tests pass — 🟢 DONE

### Task 3.3: Implement Discord Channel Adapter

> **Context:** Uses `serenity` crate for Discord gateway. Maps Discord messages to `ChannelMessage`, responses to Discord replies. Bot token from `ALICE_DISCORD_TOKEN` env var.
>
> **Verification:** Discord adapter compiles. Unit tests verify message mapping. Integration test connects with mock (if feasible).

- [x] Add `serenity` and `tokio` dependencies to `crates/alice-adapters/Cargo.toml` (behind `discord` feature flag)
- [x] Create `crates/alice-adapters/src/channel/discord.rs`:
  - `DiscordChannel::new(token: &str) -> eyre::Result<Self>`
  - `Handler` maps `serenity::model::channel::Message` → sends `ChannelMessage` via mpsc
  - Session ID: `format!("discord-{guild_id}-{channel_id}")`
  - `send()`: reply via `DiscordReplySender`
- [x] Add feature flag `discord` in `alice-adapters/Cargo.toml`, re-exported in `alice-runtime`
- [x] Wire Discord channel creation in `cmd_channel` when `channels.discord.enabled` and `ALICE_DISCORD_TOKEN` is set
- [x] Verification: `cargo check -p alice-adapters --features discord` compiles — 🟢 DONE

### Task 3.4: Implement Telegram Channel Adapter

> **Context:** Uses `teloxide` crate for Telegram Bot API. Maps Telegram updates to `ChannelMessage`, responses sent via `bot.send_message()`. Bot token from `ALICE_TELEGRAM_TOKEN` env var.
>
> **Verification:** Telegram adapter compiles. Unit tests verify message mapping.

- [x] Add `teloxide` dependency to `crates/alice-adapters/Cargo.toml` (behind `telegram` feature flag)
- [x] Create `crates/alice-adapters/src/channel/telegram.rs`:
  - `TelegramChannel::new(token: &str) -> eyre::Result<Self>`
  - Teloxide dispatcher sends `ChannelMessage` via mpsc for each `Update::Message`
  - Session ID: `format!("telegram-{chat_id}")`
  - `send()`: `bot.send_message(chat_id, text).await`
- [x] Add feature flag `telegram` in `alice-adapters/Cargo.toml`, re-exported in `alice-runtime`
- [x] Wire Telegram channel creation in `cmd_channel` when `channels.telegram.enabled` and `ALICE_TELEGRAM_TOKEN` is set
- [x] Verification: `cargo check -p alice-adapters --features telegram` compiles — 🟢 DONE

### Task 3.5: Channel Integration Tests

> **Context:** Verify the full channel pipeline: channel adapter → channel runner → agent loop → response. Use in-process mock channels to avoid network dependencies.
>
> **Verification:** All channel integration tests pass. Coverage of: CLI channel round-trip, multi-channel concurrent, channel error recovery.

- [x] Create `MockChannel` test helper implementing `Channel` with `VecDeque<ChannelMessage>` as message source and `Arc<Mutex<Vec<ChannelOutput>>>` as response sink
- [x] Add integration test: MockChannel with 2 messages → channel runner → verify 2 responses collected
- [x] Add integration test: 2 MockChannels concurrently → both process messages independently
- [x] Add integration test: MockChannel with slash command → verify `CommandOutput` returned (not LLM response)
- [x] Add integration test: channel that returns None after 1 message → verify task completes gracefully
- [x] Add integration test: /quit stops channel processing
- [x] Verification: `cargo test -p alice-runtime` passes all channel integration tests (5 tests) — 🟢 DONE

---

## Phase 4: Comprehensive Testing & Polish

### Task 4.1: Expand `alice-core` Test Coverage

> **Context:** The core crate should have thorough unit tests for all domain logic, validation, and service behavior. Migrate and expand existing common tests.
>
> **Verification:** `cargo test -p alice-core` runs 15+ tests covering all modules.

- [x] Migrate existing `service.rs` tests (render_empty_hits_returns_none, persist_then_recall_roundtrip)
- [x] Migrate existing `hybrid.rs` tests (bm25_rank_normalization, sanitize_replaces_operators, simple_embedding)
- [x] Add test: `MemoryEntry` validation — empty content, whitespace-only content, very long content
- [x] Add test: `RecallQuery` validation — empty query, special characters, unicode input
- [x] Add test: `HybridWeights` normalization — weights summing to 1.0, edge cases (0.0/1.0)
- [x] Add test: `MemoryService::recall_for_turn` with mock store returning various result shapes
- [x] Add test: `MemoryService::persist_turn` with mock store verifying correct entries created
- [x] Add test: `render_recall_context` formatting with 0, 1, and max-limit hits
- [x] Verification: `cargo test -p alice-core` — 18 tests, all pass — 🟢 DONE

### Task 4.2: Expand `alice-adapters` Test Coverage

> **Context:** Adapter tests should verify concrete implementations against the port contracts. SQLite tests are critical; channel tests can use mocks.
>
> **Verification:** `cargo test -p alice-adapters` runs 15+ tests covering memory and channel adapters.

- [x] Migrate and expand existing SQLite integration tests (schema_idempotent, fts_recall, hybrid_vector)
- [x] Add test: `SqliteMemoryStore::insert` then `recall_hybrid` with exact match
- [x] Add test: `SqliteMemoryStore::recall_hybrid` with no results
- [x] Add test: `SqliteMemoryStore::recall_hybrid` BM25-only mode (vector disabled)
- [x] Add test: multiple inserts → recall returns correctly ordered by relevance
- [x] Add test: FTS query sanitization edge cases (quotes, operators, empty)
- [x] Add test: `CliReplChannel` EOF handling, multi-line input
- [x] Add test: channel `ChannelOutput` formatting (normal vs error)
- [x] Verification: `cargo test -p alice-adapters` — 15 tests, all pass — 🟢 DONE

### Task 4.3: Expand `alice-runtime` Test Coverage

> **Context:** Runtime tests cover config, bootstrap, skill wiring, channel runner, and command execution. Use stub/mock implementations for external dependencies.
>
> **Verification:** `cargo test -p alice-runtime` runs 15+ tests.

- [x] Migrate existing config tests (parse_minimal_config, parse_full_config)
- [x] Migrate existing bootstrap tests (build_runtime_without_mcp, dispatch_mode_mapping)
- [x] Add test: config with all sections populated (`runtime`, `memory`, `skills`, `channels`, `mcp`)
- [x] Add test: config with defaults only (minimal toml)
- [x] Add test: bootstrap with skills enabled → `AliceRuntimeContext.skill_composer` is Some
- [x] Add test: bootstrap with skills disabled → `skill_composer` is None
- [x] Add test: `handle_input_with_skills` slash command → no skill injection
- [x] Add test: `handle_input_with_skills` NL input with composer → skills in request context
- [x] Add test: `handle_input_with_skills` NL input without composer → works normally
- [x] Add test: channel runner shutdown — all channels return None → runner exits
- [x] Verification: `cargo test -p alice-runtime` — 24 tests, all pass — 🟢 DONE

### Task 4.4: CLI Smoke Tests

> **Context:** End-to-end tests that exercise the full stack from CLI entrypoint down. Use stub runtimes to avoid LLM calls.
>
> **Verification:** `cargo test -p alice-cli` runs 5+ smoke tests.

- [x] Migrate existing smoke tests (one_turn_uses_agent_loop_and_persists_memory, slash_commands_bypass_llm)
- [x] Add smoke test: `cmd_run` with skill composer → verify skill-augmented request
- [x] Add smoke test: `cmd_channel` with MockChannel → verify message round-trip
- [x] Add smoke test: full config with all features → bootstrap succeeds, context has all components
- [x] Verification: `cargo test -p alice-cli` — 6 tests, all pass — 🟢 DONE

### Task 4.5: Final Quality Gates and Documentation

> **Context:** Run all quality gates, update documentation, and verify feature parity checklist.
>
> **Verification:** All three just commands pass. README updated. Feature parity matrix verified.

- [x] Run `just format` — no changes needed
- [x] Run `just lint` — zero warnings/errors
- [x] Run `just test` — all 63 tests pass (target: 58+)
- [x] Update `README.md` with new workspace layout, skill configuration, and channel setup instructions
- [x] Update `alice.toml` with new `[skills]` and `[channels]` sections (commented defaults)
- [x] Verify OpenClaw feature parity matrix from design.md section 4.6 — all v2 items checked
- [x] Verification: all quality gates green — 🟢 DONE

---

## Definition of Done

1. [x] **Restructured:** Workspace has `alice-core`, `alice-adapters`, `alice-runtime`, `alice-cli` with clean hexagonal boundaries
2. [x] **Skills integrated:** `SkillPromptComposer` loaded from config, injected per-turn, skill names in events
3. [x] **Channels implemented:** CLI REPL, Discord, and Telegram adapters implementing Bob's `Channel` trait
4. [x] **Tested:** 63 tests across unit, integration, and smoke test suites (target: 58+)
5. [x] **Linted:** `just lint` zero warnings
6. [x] **Formatted:** `just format` zero changes
7. [x] **Documented:** README and alice.toml reflect v2 architecture
8. [x] **Backwards compatible:** Existing `alice run` and `alice chat` commands work with v1 config files
