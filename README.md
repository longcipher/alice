# Alice

A configurable AI agent application with pluggable backends, built with hexagonal architecture on top of the [Bob](https://github.com/longcipher/bob) framework.

## Architecture

```text
┌─────────────────────────────────────────────────────────────┐
│                        alice-cli                            │
│                  (clap parsing + dispatch)                   │
└──────────────────────────┬──────────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────────┐
│                      alice-runtime                          │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐  │
│  │   Config     │  │  Bootstrap   │  │  Commands        │  │
│  │  alice.toml  │──│  build()     │──│  run/chat/channel│  │
│  └──────────────┘  └──────┬───────┘  └────────┬─────────┘  │
│                           │                    │            │
│  ┌────────────────────────▼────────────────────▼─────────┐  │
│  │              AgentBackend (trait)                      │  │
│  │  ┌───────────────────┐  ┌──────────────────────────┐  │  │
│  │  │  BobAgentBackend  │  │  AcpAgentBackend         │  │  │
│  │  │  bob Agent+Session│  │  subprocess via ACP      │  │  │
│  │  │  (built-in LLM)   │  │  (external agent binary) │  │  │
│  │  └───────────────────┘  └──────────────────────────┘  │  │
│  └────────────────────────────────────────────────────────┘  │
│                                                              │
│  ┌─────────────────┐  ┌──────────────┐  ┌────────────────┐  │
│  │ SkillComposer   │  │ MemorySvc    │  │ ChannelRunner  │  │
│  │ SKILL.md select │  │ SQLite+FTS5  │  │ CLI/Discord/TG │  │
│  └─────────────────┘  └──────────────┘  └────────────────┘  │
└─────────────────────────────────────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────────┐
│                      alice-core                             │
│  Domain types, port traits, memory service (zero deps)      │
└─────────────────────────────────────────────────────────────┘
                           │
┌──────────────────────────▼──────────────────────────────────┐
│                    alice-adapters                           │
│  SQLite memory store, CLI REPL, Discord, Telegram adapters  │
└─────────────────────────────────────────────────────────────┘
```

### Agent Backends

Alice supports two agent backends, selectable via configuration:

| Backend | Description | Use Case |
|---------|-------------|----------|
| `bob` (default) | Built-in Bob runtime with genai LLM adapter and MCP tools | Self-contained agent, no external process needed |
| `acp` | Delegates to an external agent via [Agent Client Protocol](https://agentclientprotocol.com) | Use any ACP-compatible agent (OpenCode, Claude Code, Codex, etc.) |

### Agent Client Protocol (ACP)

ACP is a standardized protocol for communication between clients and AI coding agents, similar to how LSP works for language servers. When using the `acp` backend, Alice spawns an external agent subprocess and communicates via stdin/stdout using the ACP protocol.

Supported ACP agents include any tool that implements `--acp` or equivalent ACP server mode.

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

# Run with multi-channel support
cargo run -p alice-cli -- --config alice.toml channel
```

## Configuration

Alice is configured via `alice.toml`. Copy and customize:

```toml
[runtime]
default_model = "openai:gpt-4o-mini"
max_steps = 12
turn_timeout_ms = 90000
dispatch_mode = "native_preferred"

[memory]
db_path = "./.alice/memory.db"
recall_limit = 6
bm25_weight = 0.3
vector_weight = 0.7
vector_dimensions = 384
enable_vector = true
```

### Agent Backend

Choose between the built-in Bob agent or an external ACP agent:

```toml
# Built-in Bob agent (default, no [agent] section needed)
# Or explicitly:
[agent]
backend = "bob"

# External ACP agent:
[agent]
backend = "acp"
acp_command = "opencode"
acp_args = ["serve", "--acp"]
acp_working_dir = "/path/to/project"
```

Build with ACP support:

```bash
cargo build -p alice-cli --features acp-agent
```

### Skills

Place `SKILL.md` files in a directory and configure the path:

```toml
[skills]
enabled = true
max_selected = 3
token_budget = 1800

[[skills.sources]]
path = "./skills"
recursive = true
```

### Telegram

Alice supports Telegram as a chat channel via the `teloxide` crate.

#### 1. Create a Telegram Bot

1. Open [@BotFather](https://t.me/BotFather) in Telegram
2. Send `/newbot` and follow the prompts
3. Copy the bot token you receive

#### 2. Configure Alice

Enable the Telegram channel in `alice.toml`:

```toml
[channels.telegram]
enabled = true
```

Set the bot token as an environment variable:

```bash
export ALICE_TELEGRAM_TOKEN="your-telegram-bot-token"
```

#### 3. Build and Run

```bash
# Build with Telegram support
cargo build -p alice-cli --features telegram

# Run with Telegram + CLI REPL
cargo run -p alice-cli --features telegram -- --config alice.toml channel
```

Messages sent to your bot will be processed by Alice and responses sent back. Each chat creates a unique session for memory continuity.

### Discord

```toml
[channels.discord]
enabled = true
```

```bash
export ALICE_DISCORD_TOKEN="your-discord-bot-token"
cargo build -p alice-cli --features discord
cargo run -p alice-cli --features discord -- --config alice.toml channel
```

### MCP Tool Servers

Add external tool servers via MCP:

```toml
[[mcp.servers]]
id = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "."]
tool_timeout_ms = 15000
```

## Example: Using OpenCode as an ACP Agent

[OpenCode](https://opencode.ai) is a terminal-based AI coding agent that supports ACP mode.

### 1. Install OpenCode

```bash
# Via npm
npm install -g opencode

# Or via Homebrew
brew install opencode
```

#### 2. Start the ACP Server

OpenCode can run as an ACP-compatible agent:

```bash
opencode serve --acp
```

This starts an ACP server that reads from stdin and writes to stdout.

#### 3. Configure Alice to Use OpenCode

```toml
[agent]
backend = "acp"
acp_command = "opencode"
acp_args = ["serve", "--acp"]
acp_working_dir = "/path/to/your/project"
```

#### 4. Run Alice

```bash
cargo run -p alice-cli --features acp-agent -- --config alice.toml chat
```

Alice will spawn an OpenCode subprocess per session and communicate via the ACP protocol. All tool execution, LLM calls, and session management are handled by OpenCode; Alice provides the chat interface, memory system, skill injection, and multi-channel support.

## Building with Features

```bash
# ACP agent backend
cargo build -p alice-cli --features acp-agent

# Telegram channel
cargo build -p alice-cli --features telegram

# Discord channel
cargo build -p alice-cli --features discord

# All features
cargo build -p alice-cli --features acp-agent,telegram,discord
```

## License

Apache-2.0
