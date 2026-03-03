# Alice

Alice is a minimal, reusable AI agent application built with a hexagonal architecture on top of the [Bob](https://github.com/longcipher/bob) framework.

## What Alice Includes

- Bob runtime orchestration (`bob-core`, `bob-runtime`, `bob-adapters`)
- Hexagonal architecture with clean layer boundaries
- Skill system integration — auto-selects relevant `SKILL.md` files per turn
- Multi-channel adapters: CLI REPL, Discord, and Telegram
- Local memory system with:
  - SQLite persistence
  - FTS5 full-text recall
  - sqlite-vec vector search
  - hybrid ranking (BM25 + vector similarity)

## Workspace Layout

- `bin/alice-cli`: thin CLI binary — clap parsing and dispatch
- `crates/alice-core`: innermost layer — domain types, port traits, service logic (zero adapter deps)
- `crates/alice-adapters`: concrete implementations — SQLite memory store, CLI/Discord/Telegram channel adapters
- `crates/alice-runtime`: composition root — config, bootstrap, commands, skill wiring, channel runner
- `specs/`: design and task specs

## Quick Start

```bash
# Install dev tools
just setup

# Format / lint / tests
just format
just lint
just test

# Run Alice (interactive chat)
cargo run -p alice-cli -- --config alice.toml chat

# Run one prompt and exit
cargo run -p alice-cli -- --config alice.toml run "summarize our current memory setup"

# Run with multi-channel support (CLI + enabled adapters)
cargo run -p alice-cli -- --config alice.toml channel
```

## Configuration

Use `alice.toml` in repo root as a starting point. Key sections:

- `[runtime]`: model, turn limits, dispatch mode
- `[memory]`: sqlite path and hybrid recall weights
- `[skills]`: skill system — enable/disable, source directories, token budget
- `[channels.discord]` / `[channels.telegram]`: channel adapters (require env vars)
- `[[mcp.servers]]`: optional MCP tool servers

### Skills

Place `SKILL.md` files in a directory and configure the path in `alice.toml`:

```toml
[skills]
enabled = true
max_selected = 3
token_budget = 1800

[[skills.sources]]
path = "./skills"
recursive = true
```

### Channels

Enable Discord and/or Telegram alongside the default CLI REPL:

```toml
[channels.discord]
enabled = true

[channels.telegram]
enabled = true
```

Set the corresponding environment variables:

```bash
export ALICE_DISCORD_TOKEN="your-discord-bot-token"
export ALICE_TELEGRAM_TOKEN="your-telegram-bot-token"
```

## Building with Channel Features

Discord and Telegram adapters are behind feature flags:

```bash
# Build with Discord support
cargo build -p alice-cli --features discord

# Build with Telegram support
cargo build -p alice-cli --features telegram

# Build with both
cargo build -p alice-cli --features discord,telegram
```
