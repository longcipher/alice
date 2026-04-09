# alice-runtime

Runtime wiring layer for the [Alice](https://github.com/longcipher/alice) AI agent.

## Overview

This crate is the composition root that wires all components together:

- **Configuration** — TOML-based config with `alice.toml` via the `config` crate
- **Bootstrap** — builds the full runtime context from configuration
- **Agent backends** — pluggable `AgentBackend` trait with two implementations:
  - `BobAgentBackend` — built-in Bob runtime with liter-llm adapter
  - `AcpAgentBackend` — external agent via Agent Client Protocol (feature-gated)
- **Chat adapter runner** — event loop for CLI, Discord, and Telegram channels
- **Channel dispatcher** — background-safe posting back into active channel threads
- **Skill system** — dynamic SKILL.md loading and per-turn skill injection
- **Memory integration** — recall-before, persist-after turn execution
- **ACP orchestration** — optional manager/worker fan-out for ordinary NL turns

## Features

| Feature      | Description                              | Dependencies                   |
|--------------|------------------------------------------|-------------------------------|
| `acp-agent`  | ACP agent backend support                | agent-client-protocol, tokio-util |
| `discord`    | Discord channel adapter                  | (via alice-adapters/discord)  |
| `telegram`   | Telegram channel adapter                 | (via alice-adapters/telegram) |

## Usage

```rust
use alice_runtime::{config::load_config, bootstrap::build_runtime};

// Load configuration
let cfg = load_config("alice.toml")?;

// Build the full runtime context
let context = build_runtime(&cfg).await?;

// Run a one-shot prompt
alice_runtime::commands::cmd_run(&context, "session-1", None, "hello").await?;
```

### Agent Backend Configuration

```toml
# Built-in Bob agent (default)
[agent]
backend = "bob"

# External ACP agent
[agent]
backend = "acp"
auto_orchestrate = true
acp_command = "opencode"
acp_args = ["serve", "--acp"]
acp_working_dir = "/path/to/project"

[agent.acp_profiles.manager]
command = "opencode"
args = ["serve", "--acp"]
working_dir = "/path/to/project"
```

### Scheduler

The runtime exposes a reusable scheduler tick executor plus a background Tokio worker. Long-running CLI modes can spawn the worker when `[scheduler].enabled = true`. When a global user has an active channel lease with a concrete thread id and the channel adapter is registered, scheduler results are posted back into that thread.

### Identity Continuity

Use `alice_runtime::identity::IdentityResolver` to issue bind tokens, consume `/bind` commands, and resolve a stable `global_user_id` into the active session lease used by CLI and channel turns.

### Auto Orchestration

When `agent.auto_orchestrate = true` and multiple ACP profiles are configured, `handle_input_with_skills` routes ordinary natural-language turns through the runtime orchestrator while leaving slash commands on `AgentLoop`.

## License

Apache-2.0
