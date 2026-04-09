# Alice Agent Evolution Phase 2 Implementation Plan

> For agentic workers: use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task.

## Overview

**Goal:** add cross-channel global identity binding, background scheduled tasks, and multi-ACP subagent orchestration to Alice without breaking the existing hexagonal runtime.

**Architecture:** introduce a dedicated runtime-state persistence layer for bindings, active sessions, and schedules; route all channel and CLI traffic through that state layer; then layer a scheduler worker and ACP orchestration router on top. Keep execution boundaries explicit: channels resolve identity, runtime resolves session and schedules, ACP backends own subprocess orchestration.

**Tech stack:** Rust workspace, SQLite via `rusqlite`, Tokio runtime/tasks, existing Alice runtime/core/adapters crates, ACP backend.

## Task 1: Runtime-State Domain And SQLite Persistence

**Files:**

- Create: `crates/alice-core/src/runtime_state/mod.rs`
- Create: `crates/alice-core/src/runtime_state/domain.rs`
- Create: `crates/alice-core/src/runtime_state/error.rs`
- Create: `crates/alice-core/src/runtime_state/ports.rs`
- Create: `crates/alice-core/src/runtime_state/service.rs`
- Modify: `crates/alice-core/src/lib.rs`
- Create: `crates/alice-adapters/src/runtime_state/mod.rs`
- Create: `crates/alice-adapters/src/runtime_state/sqlite_schema.rs`
- Create: `crates/alice-adapters/src/runtime_state/sqlite_store.rs`
- Modify: `crates/alice-adapters/src/lib.rs`
- Test: `crates/alice-adapters/tests/runtime_state_sqlite_integration.rs`

**Step 1:** Write failing core tests for bindings, active sessions, and scheduled tasks.

Target behaviors:

- binding lookup resolves `(provider, external_user_id) -> global_user_id`
- one-time bind tokens can be created, consumed, and expire
- active session lease can be stored and reused
- scheduled tasks persist `task_id`, `global_user_id`, `channel`, `schedule`, `prompt`, `next_run_epoch_ms`, and `enabled`

Run:

```bash
cargo test -p alice-core runtime_state -- --nocapture
```

Expected: compile or test failure because `runtime_state` does not exist yet.

**Step 2:** Add the `runtime_state` domain model and service API in `alice-core`.

Required public types:

- `GlobalIdentityBinding`
- `BindToken`
- `ActiveSessionLease`
- `ScheduledTask`
- `ScheduleKind`
- `RuntimeStateStorePort`
- `RuntimeStateService`

Required service operations:

- `issue_bind_token`
- `consume_bind_token`
- `bind_identity`
- `resolve_global_user_id`
- `upsert_active_session`
- `get_active_session`
- `insert_scheduled_task`
- `list_due_tasks`
- `mark_task_executed`

**Step 3:** Write failing SQLite adapter integration tests.

Run:

```bash
cargo test -p alice-adapters runtime_state_sqlite_integration -- --nocapture
```

Expected: failure because the SQLite runtime-state adapter does not exist yet.

**Step 4:** Implement the SQLite schema and store.

Create tables:

- `identity_bindings`
- `bind_tokens`
- `active_sessions`
- `scheduled_tasks`

Implementation constraints:

- use the same database path pattern as memory (`WAL` mode is fine)
- serialize structured schedule metadata as JSON only when needed
- keep row-to-domain mapping in the adapter

**Step 5:** Verify core and adapter tests pass.

Run:

```bash
cargo test -p alice-core runtime_state -- --nocapture
cargo test -p alice-adapters runtime_state_sqlite_integration -- --nocapture
```

Expected: both test targets pass.

## Task 2: Cross-Channel Global Identity Binding And Session Resolution

**Files:**

- Create: `crates/alice-runtime/src/identity.rs`
- Modify: `crates/alice-runtime/src/context.rs`
- Modify: `crates/alice-runtime/src/bootstrap.rs`
- Modify: `crates/alice-runtime/src/config.rs`
- Modify: `crates/alice-runtime/src/chatbot_runner.rs`
- Modify: `crates/alice-runtime/src/commands.rs`
- Modify: `crates/alice-runtime/src/handle_input.rs`
- Modify: `crates/alice-runtime/src/memory_context.rs`
- Modify: `bin/alice-cli/src/main.rs`
- Modify: `crates/alice-adapters/src/channel/telegram.rs`
- Modify: `crates/alice-adapters/src/channel/discord.rs`
- Test: `crates/alice-runtime/tests/identity_integration.rs`

**Step 1:** Write failing integration tests for identity resolution.

Required scenarios:

- CLI `run/chat` with explicit `global_user_id` reuses the same active session
- Telegram/Discord `/bind <token>` associates provider user ids with the same global user
- after binding, channel messages resolve to the stored `global_user_id`
- active session reuse prefers the global session lease over per-channel thread id

Run:

```bash
cargo test -p alice-runtime identity_integration -- --nocapture
```

Expected: failure because identity resolution and bind handling are missing.

**Step 2:** Add a runtime identity resolver.

Implement an `IdentityResolver` or similarly named runtime helper that:

- issues bind tokens for CLI users
- consumes channel bind commands
- resolves `global_user_id`
- resolves the effective session id from `active_sessions`
- updates the active session lease after successful turns

**Step 3:** Extend CLI and channel entrypoints.

Required behavior:

- add CLI flags or subcommands to set `global_user_id`
- expose a CLI path to issue bind tokens
- intercept `/bind <token>` before normal agent execution in Telegram/Discord
- ensure the resolved `global_user_id` flows into `handle_input_with_skills`

**Step 4:** Verify identity continuity end-to-end.

Run:

```bash
cargo test -p alice-runtime identity_integration -- --nocapture
cargo test -p alice-runtime channel_integration -- --nocapture
cargo test -p alice-cli alice_once_smoke -- --nocapture
```

Expected: the new identity test passes and existing channel/CLI tests remain green.

## Task 3: Background Scheduler And Scheduled Task Execution

**Files:**

- Create: `crates/alice-runtime/src/scheduler.rs`
- Modify: `crates/alice-runtime/src/context.rs`
- Modify: `crates/alice-runtime/src/bootstrap.rs`
- Modify: `crates/alice-runtime/src/config.rs`
- Modify: `crates/alice-runtime/src/commands.rs`
- Modify: `README.md`
- Modify: `alice.toml`
- Test: `crates/alice-runtime/tests/scheduler_integration.rs`

**Step 1:** Write failing scheduler integration tests.

Required scenarios:

- scheduler polls due tasks and runs them through the existing turn pipeline
- scheduled task execution uses the stored `global_user_id` and preferred channel
- task execution advances `next_run_epoch_ms`
- disabled tasks are skipped

Run:

```bash
cargo test -p alice-runtime scheduler_integration -- --nocapture
```

Expected: failure because the scheduler worker does not exist.

**Step 2:** Implement scheduler runtime.

Required behavior:

- spawn a Tokio background loop from `bootstrap`
- poll runtime-state storage for due tasks
- execute tasks using hidden turns through existing memory and identity plumbing
- post results to the selected adapter channel when possible

**Step 3:** Add task creation and inspection commands.

Required first increment:

- CLI command to add a scheduled task
- CLI command to list scheduled tasks
- config gate to enable or disable scheduler polling

Natural language parsing can be limited to a documented small set of patterns in this increment, but the persisted schedule representation must remain explicit and testable.

**Step 4:** Verify scheduler behavior.

Run:

```bash
cargo test -p alice-runtime scheduler_integration -- --nocapture
```

Expected: due tasks execute and reschedule correctly.

## Task 4: Multi-ACP Router And Manager/Worker Subagent Orchestration

**Files:**

- Create: `crates/alice-runtime/src/orchestration.rs`
- Modify: `crates/alice-runtime/src/agent_backend/mod.rs`
- Modify: `crates/alice-runtime/src/agent_backend/acp_backend.rs`
- Modify: `crates/alice-runtime/src/config.rs`
- Modify: `crates/alice-runtime/src/bootstrap.rs`
- Modify: `README.md`
- Test: `crates/alice-runtime/tests/acp_orchestration_integration.rs`

**Step 1:** Write failing ACP orchestration tests.

Required scenarios:

- runtime can configure more than one ACP backend profile
- manager route selects a primary backend and can fan out worker requests
- worker ACP sessions remain isolated from the manager session
- orchestrated result aggregation returns a single final response

Run:

```bash
cargo test -p alice-runtime acp_orchestration_integration --features acp-agent -- --nocapture
```

Expected: failure because multi-ACP orchestration is not implemented.

**Step 2:** Introduce ACP profile configuration.

Required config support:

- one primary ACP profile
- zero or more named worker ACP profiles
- profile-specific command, args, and working directory

**Step 3:** Implement orchestration router.

Required behavior:

- manager creates worker sessions from named ACP profiles
- worker requests run with isolated session ids
- aggregation is explicit in runtime code, not implicit string concatenation in adapters

**Step 4:** Expose an orchestration entrypoint.

First increment is enough if:

- orchestration is available to the built-in runtime for complex tasks
- tests prove multiple ACP subprocesses can be managed concurrently

**Step 5:** Verify ACP orchestration.

Run:

```bash
cargo test -p alice-runtime acp_orchestration_integration --features acp-agent -- --nocapture
```

Expected: orchestration tests pass without regressing existing ACP behavior.

## Task 5: Full Verification And Documentation

**Files:**

- Modify: `README.md`
- Modify: `alice.toml`
- Modify: `crates/alice-runtime/README.md`

**Step 1:** Document identity binding, scheduler, and ACP orchestration.

Required docs:

- how to issue and consume bind tokens
- how to configure scheduled tasks
- how to configure multiple ACP profiles

**Step 2:** Run full verification.

Run:

```bash
just format
just lint
just test
```

Expected: all commands succeed.
