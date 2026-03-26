# alice-adapters

Adapter implementations for the [Alice](https://github.com/longcipher/alice) AI agent.

## Overview

Concrete implementations of core ports:

- **SQLite memory store** — `SqliteMemoryStore` with FTS5 full-text search and sqlite-vec vector search
- **CLI REPL adapter** — interactive terminal chat via stdin/stdout
- **Discord adapter** — serenity-based gateway (feature-gated on `discord`)
- **Telegram adapter** — teloxide-based bot (feature-gated on `telegram`)

## Features

| Feature    | Description                          | Dependencies         |
|------------|--------------------------------------|----------------------|
| `discord`  | Discord channel adapter via serenity | serenity, eyre       |
| `telegram` | Telegram channel adapter via teloxide| teloxide, eyre       |

## Usage

```rust
use alice_adapters::memory::sqlite_store::SqliteMemoryStore;

// Open a file-backed memory store
let store = SqliteMemoryStore::open("./memory.db", 384, true)?;

// Or an in-memory store for tests
let store = SqliteMemoryStore::in_memory(384, false)?;
```

```rust
use alice_adapters::channel::cli_repl::CliReplChatAdapter;

// Create a CLI REPL adapter
let adapter = CliReplChatAdapter::new("my-session".to_string());
```

## License

Apache-2.0
