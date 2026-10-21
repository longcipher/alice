# Alice Agent Instructions

## Non-negotiables

- Surgical edits only: every changed line must trace to the request. No drive-by refactors, no reformatting untouched code, no deleting pre-existing dead code (mention it instead).
- Fix root causes. Never silence a warning, weaken a check, or patch a symptom to make a gate pass.
- Never report completion while any `just` gate below fails. Report the failure verbatim and stop.
- Stop and ask when a request is ambiguous, conflicts with existing conventions, or requires credentials, publishing, or destructive operations.
- When a correction reveals a missing rule, add one specific rule here ("always X for Y", never "be careful with Y"); delete rules that never catch real errors.

## Scope

- Alice is a configurable AI agent built on hexagonal architecture over the Bob framework.
- Layers, innermost first: `crates/alice-core` (domain types, port traits, memory and runtime-state services, no adapter dependencies), `crates/alice-adapters` (SQLite stores, CLI REPL, Discord and Telegram channels), `crates/alice-runtime` (config, bootstrap, commands, scheduler, orchestration, reflection, skill wiring), `bin/alice-cli` (clap parsing and dispatch).
- Alice consumes the published `bob-core`, `bob-chat`, `bob-runtime`, `bob-adapters` and `bob-skills` crates from crates.io. They are not workspace members; do not patch them from here.
- Storage is SQLite through `rusqlite` plus `sqlite-vec` for vector recall. There is no `sqlx` in this workspace.
- Two agent backends: `bob` (default, in-tree Bob runtime) and `acp` (an external agent over the Agent Client Protocol, behind the `acp-agent` feature).
- `.alice/memory.db` is runtime state, not source. Never commit it.
- English only in code, comments, docs, and commit messages.

## Execution

- Parallelize aggressively: dispatch subagents for independent research, exploration, and implementation.
- NEVER run two `cargo` commands at once; `target/` is file-locked. Finish edits first, then run cargo.
- NEVER use `git worktree`. Edit the current branch in place. The repository contains a stale `.worktrees/` directory; ignore it.
- TDD for behavior changes: failing test first in the affected crate, smallest change to pass, then run `just test`.

## Verification loop

After each feature or bug fix, in order; any failure means the task is not done:

```bash
just format
just lint
just test
```

`just lint` is the gate: `typos`, `rumdl check`, `taplo fmt --check` for TOML, `cargo +nightly fmt --check`, `cargo +nightly clippy -D warnings` and `cargo machete` for unused dependencies. CI installs exactly `just`, `cargo-machete`, `cargo-nextest`, `taplo-cli`, `typos-cli` and `rumdl`, so `just lint` must not call anything else.

## Rust agent rules

- Never hand-type dependency versions. Use `cargo add <crate> -p <crate>`. Shared crates live in the root `[workspace.dependencies]` with numeric versions and no features, and are referenced as `dep = { workspace = true }`.
- Sub-crates inherit `version`, `edition`, `license`, `repository`, `homepage`, `authors`, `description`, `keywords` and `categories` with `workspace = true`.
- `.rustfmt.toml` sets `unstable_features = true`, so formatting requires nightly: use `cargo +nightly fmt` and `cargo +nightly clippy`.
- `.taplo.toml` is the TOML contract: `taplo fmt` in `format`, `taplo fmt --check` in `lint`. Do not reorder the `package` table.
- Workspace lints deny `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented`, `dbg_macro` and `allow_attributes`. A bare `#[allow(...)]` on a rustc lint is a compile error; use `#[expect(lint, reason = "...")]`.
- `print_stdout` and `print_stderr` are denied in library code. Write terminal output with `writeln!(std::io::stdout(), ..)` instead of `println!`, so the lint never fires.
- `cargo machete` needs the ignore list in `crates/alice-runtime/Cargo.toml` (`[package.metadata.cargo-machete]`) kept accurate when a feature-gated dependency is added.

## Dependency choices

- Errors: `thiserror` in `crates/*`, `eyre` in `bin/alice-cli` and in the adapter layer. Never `anyhow`.
- Logging: `tracing` with `tracing-subscriber` in the CLI. Never `log`.
- Async: `tokio` with `tokio::sync` primitives. Release `parking_lot` guards before every `.await`; use `tokio::sync::Mutex` only when a guard must span an `.await`.
- Channels: `tokio::sync::mpsc`/`broadcast` for async, `crossbeam-channel` for sync MPMC. Never `std::sync::mpsc`.
- Feature-gated integrations stay optional: Discord uses `serenity`, Telegram uses `teloxide`, ACP uses `agent-client-protocol` plus `tokio-util`.
- Never `reqwest`, never `dashmap`, never `RwLock<HashMap<..>>` on a hot path.

## Code conventions

- Configuration is TOML through the `config` crate; `alice.toml` in the repository root is the reference file. `[runtime]`, `[memory]`, `[skills]`, `[reflection]`, `[channels.*]` and `[scheduler]` are the known sections.
- Secrets come from the environment, never from the TOML file: `ALICE_DISCORD_TOKEN` for Discord, `ALICE_TELEGRAM_TOKEN` for Telegram.
- The memory service is hybrid: BM25 over the FTS5 index plus vector similarity, weighted by `[memory] bm25_weight` and `vector_weight`. Keep both sides in the same SQLite file and keep the weights summing to one.
- Ports live in `alice-core` (`memory/ports.rs`, `runtime_state/ports.rs`); adapters implement them in `alice-adapters`. A port must never depend on `rusqlite`, `serenity` or `teloxide`.
- `alice-core/src/memory/service.rs` intentionally lists Chinese-language markers so the agent can detect CJK turns. That is data, not prose; keep it out of the English-only check.
- Unit tests are colocated in `#[cfg(test)]` modules; integration tests live in each crate's `tests/` directory.
- Skill files are `SKILL.md` documents with YAML frontmatter. The parser lives in `bob-skills`; do not re-implement frontmatter parsing here.
