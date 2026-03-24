# bob-chat Channels Integration — Tasks

| Metadata | Details |
| :--- | :--- |
| **Design Doc** | specs/2026-03-03-02-bob-chat-channels/design.md |
| **Status** | Complete |

---

## Summary & Timeline

| Phase | Description | Tasks | Status |
| :--- | :--- | :--- | :--- |
| 1 — Foundation | Add `bob-chat` dependency, module scaffolding | 1.1, 1.2 | Done |
| 2 — Core Adapters | Implement `ChatAdapter` for CLI, Discord, Telegram | 2.1, 2.2, 2.3 | Done |
| 3 — Runner & Wiring | ChatBot runner, update commands, update lib.rs | 3.1, 3.2 | Done |
| 4 — Testing & Polish | Unit tests, integration tests, build verification | 4.1, 4.2, 4.3 | Done |

---

## Phase 1 — Foundation

### Task 1.1: Add `bob-chat` Dependency to Crates

- [x] Added `bob-chat.workspace = true` to alice-adapters and alice-runtime
- [x] Removed unused `bob-core` from alice-adapters (detected by cargo-machete)
- [x] Added `futures-util` to alice-runtime for stream combinators
- [x] Verification: `cargo check -p alice-adapters -p alice-runtime` passes

### Task 1.2: Create Module Scaffolding

- [x] Replaced `cli_repl.rs`, `discord.rs`, `telegram.rs` in-place with ChatAdapter impls
- [x] Created `chatbot_runner.rs` replacing `channel_runner.rs`
- [x] Updated `lib.rs` module declaration
- [x] Verification: `cargo check --workspace` passes

---

## Phase 2 — Core Adapters

### Task 2.1: Implement `CliReplChatAdapter`

- [x] `CliReplChatAdapter` with `BufReader<Stdin>`, session_id, AtomicU64 counter
- [x] Full `ChatAdapter` trait impl (recv_event, post_message, render_card, etc.)
- [x] 5 unit tests: name_is_cli, render_message_text/markdown, render_card, next_id_increments
- [x] Verification: `cargo test -p alice-adapters` passes

### Task 2.2: Implement `DiscordChatAdapter`

- [x] Serenity gateway with mpsc forwarding, `Arc<Mutex<Option<ChannelId>>>`
- [x] Maps serenity Message to ChatEvent::Message/Mention
- [x] Uses `EditMessage::new().content()` builder (serenity 0.12.5)
- [x] Verification: `cargo check -p alice-adapters --features discord` passes

### Task 2.3: Implement `TelegramChatAdapter`

- [x] Teloxide dispatcher with mpsc forwarding
- [x] Uses `chrono::DateTime::to_rfc3339()` for timestamp (not `time` crate)
- [x] Verification: `cargo check -p alice-adapters --features telegram` passes

---

## Phase 3 — Runner & Wiring

### Task 3.1: Implement `chatbot_runner.rs`

- [x] Manual adapter polling via `futures_util::stream::select_all` with tagged streams
- [x] Each adapter wrapped in `Arc<tokio::sync::Mutex>` for shared recv/post access
- [x] Directly calls `adapter.post_message()` instead of `ThreadHandle::post()`
  (workaround: bob-chat 0.2.1 `make_thread_handle` uses NullAdapter)
- [x] Verification: `cargo check -p alice-runtime` passes

### Task 3.2: Update `commands.rs` to Use ChatBot Runner

- [x] `cmd_chat` creates `Vec<Box<dyn ChatAdapter>>` with CliReplChatAdapter
- [x] `cmd_channel` builds adapter vec with CLI + optional Discord/Telegram
- [x] Feature-gated `cfg_attr` for unused_mut when no platform features enabled
- [x] Verification: `cargo check --workspace` passes

---

## Phase 4 — Testing & Polish

### Task 4.1: Unit Tests for Adapters

- [x] CLI adapter: 5 inline unit tests
- [x] Discord adapter: name_is_discord test
- [x] Telegram adapter: name_is_telegram test
- [x] Verification: `cargo test -p alice-adapters` — 8 tests pass

### Task 4.2: Integration Tests — MockChatAdapter

- [x] `channel_integration.rs`: MockChatAdapter with 4 tests
  (mock_adapter_processes_messages, two_adapters_process_concurrently,
  slash_command_returns_command_output, adapter_returns_none_completes_gracefully)
- [x] `alice_once_smoke.rs`: chatbot_runner_with_mock_adapter test
- [x] Verification: all integration tests pass

### Task 4.3: Full Build & Lint Verification

- [x] `just format` — clean
- [x] `just lint` — clean (typos, rumdl, taplo, fmt, clippy pedantic, cargo-machete)
- [x] `just test` — 69 tests pass, 0 failures, 0 warnings
- [x] Verification: all commands exit 0

---

## Definition of Done

- [x] `bob-chat` 0.2.1 dependency added to `alice-adapters` and `alice-runtime`
- [x] `CliReplChatAdapter` implements `ChatAdapter` with working stdin/stdout I/O
- [x] `DiscordChatAdapter` implements `ChatAdapter` using serenity gateway (feature-gated)
- [x] `TelegramChatAdapter` implements `ChatAdapter` using teloxide (feature-gated)
- [x] `chatbot_runner::run_chatbot` orchestrates adapters via manual stream polling
- [x] `cmd_chat` and `cmd_channel` use the new `chatbot_runner` instead of `channel_runner`
- [x] Legacy `Channel` implementations removed (per user request: not preserved)
- [x] Unit tests for all adapter `name()`, `render_card()` methods pass
- [x] Integration test with `MockChatAdapter` passes
- [x] `just format && just lint && just test` all pass
