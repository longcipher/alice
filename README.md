# Alice

Alice is a minimal, reusable AI agent application built with a hexagonal architecture on top of the [Bob](https://github.com/longcipher/bob) framework.

## What Alice Includes

- Bob runtime orchestration (`bob-core`, `bob-runtime`, `bob-adapters`)
- CLI interaction (`alice` command behavior in `bin/cli-app`)
- Local memory system in `crates/common` with:
  - SQLite persistence
  - FTS5 full-text recall
  - sqlite-vec vector search
  - hybrid ranking (BM25 + vector similarity)

## Workspace Layout

- `bin/cli-app`: Alice CLI composition root
- `crates/common`: shared memory domain/service/SQLite adapter
- `specs/`: design and task specs

## Quick Start

```bash
# Install dev tools
just setup

# Format / lint / tests
just format
just lint
just test

# Run Alice (interactive)
cargo run -p cli-app -- --config alice.toml

# Run one prompt and exit
cargo run -p cli-app -- --config alice.toml --once "summarize our current memory setup"
```

## Configuration

Use `alice.toml` in repo root as a starting point. Key sections:

- `[runtime]`: model and turn limits
- `[memory]`: sqlite path and hybrid recall weights
- `[[mcp.servers]]`: optional MCP tool servers

## Current Scope

Alice is intentionally CLI-first and local-first. Web gateways, multi-channel bots, and distributed memory are out of scope for this phase.
