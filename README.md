# Alice

[![DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/longcipher/alice)
[![Context7](https://img.shields.io/badge/Website-context7.com-blue)](https://context7.com/longcipher/alice)
[![crates.io](https://img.shields.io/crates/v/alice-core.svg)](https://crates.io/crates/alice-core)
[![docs.rs](https://docs.rs/alice-core/badge.svg)](https://docs.rs/alice-core)

![alice](https://socialify.git.ci/longcipher/alice/image?font=Source+Code+Pro&language=1&name=1&owner=1&pattern=Circuit+Board&theme=Auto)

A configurable AI agent application with pluggable backends, built with hexagonal architecture on top of the [Bob](https://github.com/longcipher/bob) framework.

Alice combines short-term turn memory with two longer-lived learning layers:

- User profiles: Alice distills durable user preferences and project context into a profile that is injected into future turns.
- Learned skills: when reflection is enabled, Alice can synthesize reusable `SKILL.md` files from successful sessions and save them into a configured skills directory.
- Global identity bindings: CLI, Telegram, and Discord users can be linked to one global user id so active sessions survive channel switches.
- Scheduled tasks: Alice can persist recurring background prompts in SQLite, execute them from a Tokio scheduler loop, and push results back into the active bound channel thread when available.

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
| `bob` (default) | Built-in Bob runtime with liter-llm adapter and MCP tools | Self-contained agent, no external process needed |
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

# Issue a bind token for a global user id
cargo run -p alice-cli -- --config alice.toml bind-token alice-user-1 --provider telegram

# Create a background scheduled task
cargo run -p alice-cli -- --config alice.toml schedule add \
  --global-user-id alice-user-1 \
  --prompt "summarize recent alerts" \
  --every-minutes 60

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

[reflection]
enabled = false
learned_skills_dir = "./skills/learned"

[scheduler]
enabled = false
poll_interval_ms = 30000
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

# Multi-profile ACP orchestration
[agent]
backend = "acp"
auto_orchestrate = true
primary_profile = "manager"

[agent.acp_profiles.manager]
command = "opencode"
args = ["serve", "--acp"]
working_dir = "/path/to/project"

[agent.acp_profiles.writer]
command = "codex"
args = ["--acp"]
working_dir = "/path/to/project"
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

When `[reflection]` is enabled, point `learned_skills_dir` under one of your configured skill source roots, for example `./skills/learned`. Alice reloads skill sources on each turn, so reflected skills can be selected without restarting the process.

### Long-Term Profiles

Alice stores long-lived user profiles alongside the memory index in SQLite. These profiles are updated from durable self-descriptions in user turns, such as preferences, project constraints, and repository context, then injected into future prompts as "Known user profile" context.

### Global Identity Binding

Use `--global-user-id` on CLI `run` or `chat` sessions to anchor them to a stable user identity:

```bash
cargo run -p alice-cli -- --config alice.toml chat --global-user-id alice-user-1
```

Then issue a bind token and consume it from Telegram or Discord with `/bind <token>`:

```bash
cargo run -p alice-cli -- --config alice.toml bind-token alice-user-1 --provider telegram
```

Once a channel identity is bound, Alice reuses the latest active session lease for that global user whenever possible.
For long-running channel sessions, Alice also records the active thread id so background scheduler results can be posted back to the same Telegram or Discord conversation.

### Learned Skill Reflection

Enable post-turn reflection to have Alice run a hidden reflection pass after successful responses:

```toml
[reflection]
enabled = true
learned_skills_dir = "./skills/learned"
```

The reflector writes learned workflows as `./skills/learned/<skill-name>/SKILL.md`. If a turn does not teach a reusable workflow, nothing is written.

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

To link a Telegram account to an existing CLI identity, issue a bind token from the CLI and send `/bind <token>` to the bot.

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

# Example with environment variables:
[[mcp.servers]]
id = "github"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]

[mcp.servers.env]
GITHUB_TOKEN = "your-github-token"
```

### Scheduler

Enable the background scheduler loop in long-running `chat` or `channel` sessions:

```toml
[scheduler]
enabled = true
poll_interval_ms = 30000
```

Create and inspect scheduled tasks from the CLI:

```bash
# Every 30 minutes
cargo run -p alice-cli -- --config alice.toml schedule add \
  --global-user-id alice-user-1 \
  --prompt "summarize pending PR reviews" \
  --every-minutes 30

# Daily at 08:15
cargo run -p alice-cli -- --config alice.toml schedule add \
  --global-user-id alice-user-1 \
  --prompt "prepare the morning project brief" \
  --daily-hour 8 \
  --daily-minute 15

cargo run -p alice-cli -- --config alice.toml schedule list
```

Scheduled tasks execute through the normal memory-aware turn pipeline. When the owning global user has an active bound channel/thread lease and that channel adapter is live, Alice posts the task result back into that same thread.

### ACP Orchestration

When multiple ACP profiles are configured, you can run an explicit manager/worker orchestration flow:

```bash
cargo run -p alice-cli --features acp-agent -- --config alice.toml orchestrate \
  --session-id multi-agent-run \
  --manager-prompt "Plan how to refactor the auth subsystem." \
  --worker planner "Outline the migration steps." \
  --worker writer "Draft the concrete code changes."
```

If `agent.auto_orchestrate = true`, ordinary natural-language chat turns also fan out through the configured non-primary ACP profiles and return the aggregated orchestration summary to the user.

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
