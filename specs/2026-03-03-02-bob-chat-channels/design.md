# Design: bob-chat Channels Integration

| Metadata | Details |
| :--- | :--- |
| **Status** | Implemented |
| **Created** | 2026-03-03 |
| **Scope** | Full |

---

## 1. Executive Summary

Alice currently implements channel adapters directly against `bob_core::channel::Channel` — a minimal two-method trait (`recv`/`send`). The `bob-chat` 0.2.1 crate introduces a richer `ChatAdapter` trait with a `ChatBot` orchestrator, structured event types (`ChatEvent`), rich message primitives (`IncomingMessage`, `PostableMessage`, `CardElement`), streaming support, and per-handler event dispatch. This design migrates Alice's channel subsystem from the low-level `Channel` trait to the `bob_chat::ChatAdapter` trait and `ChatBot` orchestrator, enabling CLI REPL, Telegram, and Discord platforms to benefit from the full `bob-chat` feature set (reactions, ephemeral messages, modals, streaming responses, structured events).

---

## 2. Requirements & Goals

### 2.1 Functional Goals

| ID | Requirement | Source |
| :--- | :--- | :--- |
| FR-01 | Implement `ChatAdapter` for CLI REPL (stdin/stdout) | User req |
| FR-02 | Implement `ChatAdapter` for Telegram (via `teloxide`) | User req |
| FR-03 | Implement `ChatAdapter` for Discord (via `serenity`) | User req |
| FR-04 | Replace current `bob_core::channel::Channel` usage with `bob_chat::ChatBot` orchestrator | User req |
| FR-05 | Update `channel_runner` to drive `ChatBot::run()` instead of manual `recv/send` loops | User req |
| FR-06 | Wire agent loop into `ChatBot` message/mention handlers via `ThreadHandle` | User req |
| FR-07 | Update alice-runtime `commands.rs` (`cmd_chat`, `cmd_channel`) to use new `ChatBot`-based runner | User req |
| FR-08 | Maintain backward-compatible `alice.toml` channel configuration | User req |
| FR-09 | Preserve existing integration tests using `MockChannel` (adapt to `ChatAdapter`) | User req |

### 2.2 Non-Functional Goals

- **Performance:** Adapter event polling must be non-blocking. `ChatBot::run()` already uses `futures_util::stream::select_all` for concurrent adapter polling.
- **Reliability:** Graceful degradation — if a platform adapter fails to start, log and continue with remaining adapters.
- **Security:** Bot tokens remain in environment variables, never in config or code.
- **Maintainability:** Each adapter is a self-contained module. The old `Channel` trait re-exports from `bob-chat` remain available for backward compatibility.
- **Testability:** `ChatAdapter` is object-safe; integration tests can use a `MockChatAdapter`.

### 2.3 Assumptions

- **A1:** The `bob_core::channel::Channel` trait remains available (bob-chat re-exports it). Existing code that depends on `Channel` still compiles. The migration adds `ChatAdapter` implementations alongside, eventually deprecating direct `Channel` usage.
- **A2:** `bob_chat::ChatBot` handles event dispatch and concurrent adapter polling, so the current manual `channel_runner::run_channels` loop is replaced by `ChatBot::run()`.
- **A3:** `ThreadHandle` (passed to handlers by `ChatBot`) is the primary mechanism for sending replies. The agent loop response is posted via `thread.post(text)`.

### 2.4 Out of Scope

- Slack adapter (no existing adapter code, no `slackbot` crate dep today).
- Rich card/modal support in handlers — initial integration uses plain text messages only; rich message support can be layered later.
- Streaming response support — initial handlers post complete responses; streaming via `TextStream` is a follow-up.

---

## 3. Architecture Overview

### 3.1 System Context

```text
┌─────────────────────────────────────────────────────┐
│                    alice-cli                          │
│  main.rs → cmd_chat / cmd_channel                    │
└───────────────┬─────────────────────────────────────┘
                │ builds & calls
┌───────────────▼─────────────────────────────────────┐
│               alice-runtime                          │
│  chatbot_runner.rs: build_chatbot() → ChatBot::run() │
│  commands.rs: cmd_chat, cmd_channel                  │
│  handle_input.rs: handle_input_with_skills()         │
└───────────────┬─────────────────────────────────────┘
                │ uses
┌───────────────▼─────────────────────────────────────┐
│               alice-adapters                         │
│  channel/cli_repl_adapter.rs  (ChatAdapter)          │
│  channel/discord_adapter.rs   (ChatAdapter)          │
│  channel/telegram_adapter.rs  (ChatAdapter)          │
│  channel/cli_repl.rs          (Channel, legacy)      │
└───────────────┬─────────────────────────────────────┘
                │ implements
┌───────────────▼─────────────────────────────────────┐
│               bob-chat 0.2.1                         │
│  ChatAdapter trait                                   │
│  ChatBot orchestrator                                │
│  ChatEvent, IncomingMessage, ThreadHandle            │
└─────────────────────────────────────────────────────┘
```

### 3.2 Key Design Principles

1. **Adapter pattern:** Each platform gets a struct implementing `bob_chat::ChatAdapter`. Required methods: `name`, `post_message`, `edit_message`, `delete_message`, `render_card`, `render_message`, `recv_event`. Provided methods with default `NotSupported` are left as-is initially.
2. **Orchestrator-driven:** Replace the manual `channel_runner` loop with `ChatBot::run()`. Register appropriate handlers (`on_message`, `on_mention`) that bridge to `handle_input_with_skills` via the `AliceRuntimeContext`.
3. **Incremental migration:** Keep legacy `Channel` implementations and `channel_runner.rs` intact initially. Add new `chatbot_runner.rs` module. Wire `cmd_channel` to the new runner; `cmd_chat` can use either approach.

### 3.3 Existing Components to Reuse

| Component | Location | Reuse Strategy |
| :--- | :--- | :--- |
| `CliReplChannel` | `alice-adapters/src/channel/cli_repl.rs` | Reference for stdin/stdout I/O patterns; new `CliReplChatAdapter` wraps same logic |
| `DiscordChannel` | `alice-adapters/src/channel/discord.rs` | Reference for serenity gateway wiring; new adapter reuses `mpsc`-based event forwarding |
| `TelegramChannel` | `alice-adapters/src/channel/telegram.rs` | Reference for teloxide dispatcher; new adapter reuses `mpsc` pattern |
| `channel_runner.rs` | `alice-runtime/src/channel_runner.rs` | Preserved as legacy; new `chatbot_runner.rs` replaces it |
| `handle_input_with_skills` | `alice-runtime/src/handle_input.rs` | Called from `ChatBot` handlers |
| `AliceRuntimeContext` | `alice-runtime/src/context.rs` | Shared into handlers via `Arc<AliceRuntimeContext>` |
| `ChannelsConfig` | `alice-runtime/src/config.rs` | No changes needed — same config structure |

---

## 4. Detailed Design

### 4.1 Dependency Changes

**Workspace `Cargo.toml`:** `bob-chat = "0.2.1"` already declared.

**`alice-adapters/Cargo.toml`:** Add `bob-chat` dependency:

```toml
[dependencies]
bob-chat.workspace = true
```

**`alice-runtime/Cargo.toml`:** Add `bob-chat` dependency:

```toml
[dependencies]
bob-chat.workspace = true
```

### 4.2 CLI REPL Chat Adapter

New file: `alice-adapters/src/channel/cli_repl_adapter.rs`

```rust
use bob_chat::adapter::ChatAdapter;
use bob_chat::event::ChatEvent;
use bob_chat::message::{
    AdapterPostableMessage, Author, IncomingMessage, SentMessage,
};
use bob_chat::error::ChatError;
use bob_chat::card::CardElement;

pub struct CliReplChatAdapter {
    stdin: tokio::io::BufReader<tokio::io::Stdin>,
    session_id: String,
    msg_counter: std::sync::atomic::AtomicU64,
}

#[async_trait::async_trait]
impl ChatAdapter for CliReplChatAdapter {
    fn name(&self) -> &str { "cli" }

    async fn recv_event(&mut self) -> Option<ChatEvent> {
        // Read line from stdin, return ChatEvent::Message
        // Return None on EOF
    }

    async fn post_message(
        &self, _thread_id: &str, message: &AdapterPostableMessage,
    ) -> Result<SentMessage, ChatError> {
        // Print rendered text to stdout
    }

    async fn edit_message(
        &self, _thread_id: &str, _message_id: &str,
        _message: &AdapterPostableMessage,
    ) -> Result<SentMessage, ChatError> {
        // CLI doesn't support editing — just post again
    }

    async fn delete_message(
        &self, _thread_id: &str, _message_id: &str,
    ) -> Result<(), ChatError> {
        Err(ChatError::NotSupported)
    }

    fn render_card(&self, card: &CardElement) -> String {
        bob_chat::render_card_as_text(card)
    }

    fn render_message(&self, message: &AdapterPostableMessage) -> String {
        // Use plain-text rendering
    }
}
```

Key behaviors:

- `recv_event` prompts to stderr (`"> "`), reads a line, returns `ChatEvent::Message` with an `IncomingMessage`.
- `post_message` writes rendered text to stdout and returns a `SentMessage` with a monotonic message id.
- `edit_message` re-prints (CLI has no edit capability) — alternatively returns `NotSupported`.
- `delete_message` returns `ChatError::NotSupported`.
- `render_card` delegates to `bob_chat::render_card_as_text` (plain-text fallback).

### 4.3 Discord Chat Adapter

New file: `alice-adapters/src/channel/discord_adapter.rs`

```rust
pub struct DiscordChatAdapter {
    rx: tokio::sync::mpsc::Receiver<ChatEvent>,
    http: Arc<serenity::http::Http>,
    last_channel_id: parking_lot::Mutex<Option<serenity::all::ChannelId>>,
}
```

Key behaviors:

- Constructor starts serenity gateway in a spawned task, forwarding `Message` events through `mpsc` as `ChatEvent::Message` (and `ChatEvent::Mention` when bot is mentioned).
- `recv_event` reads from `mpsc::Receiver`.
- `post_message` sends via `channel_id.say(&http, text)`.
- `edit_message` uses serenity's `channel_id.edit_message()`.
- `delete_message` uses serenity's `channel_id.delete_message()`.
- `render_card` uses `render_card_as_text` (Discord-native cards are a future enhancement).
- Maps serenity `Message` to `IncomingMessage` (id, text, author, thread_id = channel_id, is_mention from content parsing).

### 4.4 Telegram Chat Adapter

New file: `alice-adapters/src/channel/telegram_adapter.rs`

```rust
pub struct TelegramChatAdapter {
    rx: tokio::sync::mpsc::Receiver<ChatEvent>,
    bot: teloxide::Bot,
    last_chat_id: Arc<parking_lot::Mutex<Option<teloxide::types::ChatId>>>,
}
```

Key behaviors:

- Constructor starts teloxide dispatcher in a spawned task, forwarding updates as `ChatEvent::Message`.
- `recv_event` reads from `mpsc::Receiver`.
- `post_message` sends via `bot.send_message(chat_id, text)`.
- `edit_message` uses `bot.edit_message_text()`.
- `delete_message` uses `bot.delete_message()`.
- `render_card` uses `render_card_as_text` for now.
- Maps teloxide `Message` to `IncomingMessage`.

### 4.5 ChatBot Runner

New file: `alice-runtime/src/chatbot_runner.rs`

Replaces the role of `channel_runner.rs` for `bob-chat`-based execution:

```rust
use std::sync::Arc;
use bob_chat::{ChatBot, ChatBotConfig};
use bob_chat::adapter::ChatAdapter;

use crate::context::AliceRuntimeContext;
use crate::handle_input::{handle_input_with_skills, output_to_text};

/// Build a ChatBot, register adapters and handlers, then run.
pub async fn run_chatbot(
    ctx: Arc<AliceRuntimeContext>,
    adapters: Vec<Box<dyn ChatAdapter>>,
) -> eyre::Result<()> {
    let mut bot = ChatBot::new(ChatBotConfig::default());

    for adapter in adapters {
        bot.add_adapter(adapter);
    }

    let ctx_clone = Arc::clone(&ctx);
    bot.on_message(None, move |thread, message| {
        let ctx = Arc::clone(&ctx_clone);
        async move {
            let session_id = format!("{}-{}", thread.adapter_name(), message.thread_id);
            match handle_input_with_skills(&ctx, &session_id, &message.text).await {
                Ok(output) => {
                    if let Some(text) = output_to_text(&output) {
                        if !text.is_empty() {
                            let _ = thread.post(text.to_string()).await;
                        }
                    }
                }
                Err(e) => {
                    let _ = thread.post(format!("Error: {e}")).await;
                }
            }
        }
    });

    let ctx_clone2 = Arc::clone(&ctx);
    bot.on_mention(move |thread, message| {
        let ctx = Arc::clone(&ctx_clone2);
        async move {
            let session_id = format!("{}-{}", thread.adapter_name(), message.thread_id);
            match handle_input_with_skills(&ctx, &session_id, &message.text).await {
                Ok(output) => {
                    if let Some(text) = output_to_text(&output) {
                        if !text.is_empty() {
                            let _ = thread.post(text.to_string()).await;
                        }
                    }
                }
                Err(e) => {
                    let _ = thread.post(format!("Error: {e}")).await;
                }
            }
        }
    });

    bot.run().await.map_err(|e| eyre::eyre!("chatbot run failed: {e}"))?;
    Ok(())
}
```

### 4.6 Updated Commands

**`cmd_chat`** — Creates a `CliReplChatAdapter` and runs through `run_chatbot`.

**`cmd_channel`** — Builds adapters based on config (always CLI REPL + optionally Discord/Telegram), then calls `run_chatbot`.

### 4.7 Module Structure Changes

```text
alice-adapters/src/channel/
  mod.rs                    # add new adapter modules
  cli_repl.rs               # existing (preserved)
  cli_repl_adapter.rs       # NEW — ChatAdapter for CLI
  discord.rs                # existing (preserved)
  discord_adapter.rs        # NEW — ChatAdapter for Discord
  telegram.rs               # existing (preserved)
  telegram_adapter.rs       # NEW — ChatAdapter for Telegram

alice-runtime/src/
  lib.rs                    # add chatbot_runner module
  chatbot_runner.rs         # NEW — ChatBot-based runner
  channel_runner.rs         # existing (preserved as legacy)
  commands.rs               # MODIFIED — use chatbot_runner
```

### 4.8 Error Handling

- Adapter construction errors (e.g., missing token, gateway connection failure) are logged via `tracing::warn` and the adapter is skipped. Remaining adapters continue.
- `ChatBot::run()` returns `Err(ChatError::Closed)` if no adapters registered — this maps to `eyre::eyre` in `run_chatbot`.
- In-handler errors from `handle_input_with_skills` are caught and posted back as error messages via `thread.post()`.

### 4.9 Configuration

No changes to `alice.toml` schema. The existing `ChannelsConfig` with `discord.enabled` and `telegram.enabled` booleans and `ALICE_DISCORD_TOKEN` / `ALICE_TELEGRAM_TOKEN` env vars are reused as-is.

---

## 5. Verification & Testing Strategy

### 5.1 Unit Tests

| Test | Module | Verification |
| :--- | :--- | :--- |
| CLI adapter recv/send | `cli_repl_adapter` | Feed known input via test stdin, verify `ChatEvent::Message` output |
| CLI adapter post_message | `cli_repl_adapter` | Post message, verify output and `SentMessage` returned |
| CLI adapter render_card | `cli_repl_adapter` | Verify `render_card_as_text` produces non-empty string |
| Discord adapter name | `discord_adapter` | `adapter.name() == "discord"` |
| Telegram adapter name | `telegram_adapter` | `adapter.name() == "telegram"` |

### 5.2 Integration Tests

| Test | Module | Verification |
| :--- | :--- | :--- |
| Mock adapter + ChatBot | `chatbot_runner` | Register `MockChatAdapter`, send events, verify handler invocation |
| chatbot_runner with mock adapter | `alice-cli/tests` | Similar to existing `channel_runner_with_mock_channel` but using `ChatAdapter` |
| cmd_channel with disabled adapters | `commands` | Only CLI adapter added when discord/telegram disabled |

### 5.3 Backward Compatibility

- Existing `channel_runner.rs` and `Channel` implementations remain compilable and functional.
- Existing tests in `alice_once_smoke.rs` using `MockChannel` continue to pass.

---

## 6. Implementation Plan

| Phase | Tasks | Dependency |
| :--- | :--- | :--- |
| 1. Foundation | Add `bob-chat` dependency to `alice-adapters` and `alice-runtime` | None |
| 2. Core Adapters | Implement `CliReplChatAdapter`, `DiscordChatAdapter`, `TelegramChatAdapter` | Phase 1 |
| 3. Runner & Wiring | Create `chatbot_runner.rs`, update `commands.rs` | Phase 2 |
| 4. Testing & Polish | Unit tests, integration tests, verify full build | Phase 3 |
