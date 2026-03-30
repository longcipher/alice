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
- **Skill system** — dynamic SKILL.md loading and per-turn skill injection
- **Memory integration** — recall-before, persist-after turn execution

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
alice_runtime::commands::cmd_run(&context, "session-1", "hello").await?;
```

### Agent Backend Configuration

```toml
# Built-in Bob agent (default)
[agent]
backend = "bob"

# External ACP agent
[agent]
backend = "acp"
acp_command = "opencode"
acp_args = ["serve", "--acp"]
acp_working_dir = "/path/to/project"
```

## License

Apache-2.0
