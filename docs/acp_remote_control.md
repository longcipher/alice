# Design: ACP Remote Control for Alice

| Metadata | Details |
| :--- | :--- |
| **Status** | Proposed |
| **Created** | 2026-04-01 |
| **Scope** | Remote control + local/remote unification |
| **References** | `aj47/acp-remote`, `MattKotsenas/uplink`, ACP docs, Cloudflare Tunnel docs |

---

## 1. Executive Summary

The proposed direction is **feasible**, but the exact shape in the initial sketch is **not the best final architecture for Alice**.

An `uplink`-style design, where a browser PWA speaks raw ACP JSON-RPC over WebSocket to a very thin stdio bridge, is excellent for a fast prototype. It is small, auditable, and works well when the product goal is "remote UI for one ACP CLI." It is **not** the best long-term fit for Alice, because Alice already has its own runtime, memory pipeline, skill injection, multi-channel entrypoints, and backend abstraction.

For Alice, the recommended target architecture is:

1. **Alice owns the interactive session core.**
2. **Local CLI chat and remote PWA both use the same session core.**
3. **Alice exposes an ACP-compatible WebSocket facade to the PWA**, instead of letting the browser talk directly to the external ACP subprocess.
4. **Alice keeps the external agent connection on stdio**, using the existing `acp` backend for arbitrary ACP-compatible agents.
5. **Cloudflared persistent tunnels + Cloudflare Access** are the production path. Quick tunnels are development-only.

This preserves Alice's value-add, supports both `bob` and `acp` backends, and avoids creating a second control plane that bypasses the current runtime.

---

## 2. Objective Assessment of the Initial Proposal

### 2.1 What Is Correct and Feasible

The original proposal gets several important things right:

1. **ACP is the correct protocol anchor** when the target is "control arbitrary ACP-compatible coding agents."
2. **Cloudflared is a good transport choice** for secure remote access to a localhost service.
3. **A PWA is a good product choice** for phone control, because installation friction is low and there is no app-store dependency.
4. **Leptos is viable** for a Rust-native client-side UI, especially when SEO is irrelevant and the UI is highly interactive.
5. **A WebSocket transport is appropriate** for streaming updates, permission requests, and long-lived sessions.

### 2.2 What Is Only Partially Reasonable

The initial sketch is weaker in the following places:

1. **It treats the browser as the ACP session owner.**
   This is fine for a pure ACP bridge product, but Alice is not only a bridge. Alice also owns memory recall, skill injection, command routing, and backend selection.

2. **It assumes "ACP over WebSocket" is a settled interoperability surface.**
   ACP documentation explicitly supports local and remote scenarios, but remote support is still evolving. The safe assumption today is that **stdio is the most mature interoperability path**, while WebSocket is a transport binding that an application can define for its own client/server pair.

3. **It uses a dumb pipe as the final architecture.**
   A dumb pipe is ideal when the server should do almost nothing. Alice needs more than that: session ownership, policy enforcement, replay, security boundaries, and backend normalization.

4. **It duplicates state logic between server and PWA.**
   If the PWA owns the ACP state machine, Alice still needs its own internal state to support local control, multi-channel control, memory persistence, and reconnect handling. That means two state machines for one product.

5. **It overstates the value of a subprocess-per-socket bridge.**
   For Alice, the session should outlive a browser tab. The browser is a control surface, not the canonical owner of the agent session.

### 2.3 Better Framing

The right framing for Alice is not:

> "Build a PWA that remotely talks to an ACP process."

The right framing is:

> "Build a unified Alice interactive session layer that can be driven locally or remotely, while still delegating to arbitrary ACP-compatible agents when configured to do so."

---

## 3. Recommended Approach Compared to Alternatives

### 3.1 Approach A: Uplink-Style Dumb ACP Tunnel

**Summary:**
Browser speaks ACP JSON-RPC over WebSocket to a thin bridge. Bridge forwards frames to `stdin/stdout` of an ACP subprocess.

**Pros:**

1. Smallest implementation.
2. Closest to `MattKotsenas/uplink`.
3. Easy to reason about for ACP-only use cases.
4. High protocol fidelity with little server logic.

**Cons:**

1. ACP-only; does not naturally cover Alice's `bob` backend.
2. Puts session truth in the browser.
3. Bypasses Alice's current local control path.
4. Makes reconnect, replay, and approval ownership awkward.
5. Turns Alice into a transport wrapper instead of a runtime.

**Assessment for Alice:**
Good prototype, poor final shape.

### 3.2 Approach B: ACP Remote-Style Custom Remote API

**Summary:**
Desktop/host owns ACP sessions and exposes a higher-level remote API to a thin client.

**Pros:**

1. Better control over security and UX.
2. Session survives client reconnects cleanly.
3. Easier to normalize multiple backends.
4. Easier to merge with local control.

**Cons:**

1. Custom protocol becomes another maintenance surface.
2. Less reusable by generic ACP clients.
3. Higher initial design complexity.

**Assessment for Alice:**
Architecturally sound, but slightly too far from ACP if interoperability matters.

### 3.3 Approach C: Recommended Hybrid

**Summary:**
Alice owns the interactive session core and exposes an **ACP-compatible remote facade over WebSocket**. Internally, Alice routes to either the `bob` backend or the `acp` backend. Local CLI uses the same core directly in-process.

**Pros:**

1. One session core for local and remote control.
2. Works with both `bob` and `acp` backends.
3. Preserves Alice memory/skills/runtime semantics.
4. Still lets the PWA speak ACP-style JSON-RPC.
5. Avoids direct exposure of arbitrary child-process stdio to the internet.

**Cons:**

1. More server logic than `uplink`.
2. Requires an internal event model.
3. Requires fixing current ACP session lifetime limitations first.

**Assessment for Alice:**
Best long-term architecture.

---

## 4. Current-State Findings in This Repository

Before remote control is added, the current runtime has several facts that matter.

### 4.1 Good News

The repository already has the right high-level seams:

1. `alice-runtime` already owns runtime composition.
2. `AgentBackend` already abstracts `bob` vs `acp`.
3. `handle_input_with_skills()` already centralizes slash-command vs natural-language routing.
4. `run_turn_with_memory()` already centralizes memory recall, skill injection, and persistence.
5. `ChatAdapter`-driven entrypoints already exist for local and chat-channel surfaces.

This means Alice does **not** need a second application core.

### 4.2 Existing Gaps That Must Be Addressed

The current `acp` path is not yet a valid foundation for remote control.

#### Gap 1: Session lifetime is currently per turn, not truly persistent

`run_turn_with_memory()` currently calls `context.backend.create_session_with_id(session_id)` on every turn.

That is acceptable only if the backend implementation can cheaply restore the same live session. The current `AcpAgentBackend` cannot. It creates a fresh ACP runtime thread and starts a fresh ACP session each time.

**Consequence:**

1. Multi-turn ACP continuity is not reliable.
2. Reconnect and session resume are not meaningful.
3. Spawning cost is paid on every user turn.

#### Gap 2: ACP streaming notifications are not surfaced

`AcpAgentBackend` implements `session_notification()` but currently discards notifications.

**Consequence:**

1. No streaming text.
2. No tool call progress UI.
3. No plan updates.
4. No session replay.

#### Gap 3: ACP permission requests are auto-approved inside the backend

The current ACP client implementation auto-selects an allow option.

**Consequence:**

1. Unsafe for remote operation.
2. Incompatible with phone-side approval UX.
3. Prevents unified approval policy across local and remote surfaces.

#### Gap 4: ACP prompt completion returns empty content

`handle_prompt()` currently returns an `AgentResponse` with `content: String::new()`.

**Consequence:**

1. Final assistant text is not materially returned to Alice.
2. Even local ACP chat is incomplete.

### 4.3 Architectural Conclusion from Current State

The first design task is **not** "build a PWA."

The first design task is:

> Make Alice own a long-lived interactive session abstraction with event streaming and approval brokerage.

Without that, remote control will either duplicate state or sit on top of an incomplete ACP implementation.

---

## 5. Design Goals

### 5.1 Functional Goals

1. Remote control Alice from a mobile PWA.
2. Keep local control available from the CLI.
3. Support both `bob` and `acp` backends behind one control model.
4. Preserve memory recall, skill injection, and tool policy behavior.
5. Support streaming text, tool-call visibility, plan updates, cancellation, and reconnect.
6. Support explicit user approval for sensitive tool actions.
7. Keep the system able to drive arbitrary ACP-compatible agent subprocesses.

### 5.2 Non-Functional Goals

1. Remote access must be secure by default.
2. Browser reconnect should not kill the active agent session.
3. The server should be the canonical owner of session state.
4. The transport layer should remain thin and testable.
5. The implementation should reuse current crate boundaries wherever possible.

### 5.3 Non-Goals

1. Native mobile apps.
2. Push notifications in v1.
3. Multi-user collaboration in the same session.
4. Direct browser-to-agent subprocess communication.
5. Full browser-side file system or terminal proxying.

---

## 6. Recommended Target Architecture

```text
Phone PWA (Leptos CSR)
    │
    │ HTTPS / WSS
    ▼
Cloudflare Tunnel + Access
    │
    ▼
Alice Remote Server (Axum)
    │
    ├── ACP-compatible WS facade
    ├── Session registry
    ├── Permission broker
    ├── Replay buffer / snapshots
    └── Static asset serving for PWA
            │
            ▼
Unified Alice Interactive Session Core
            │
            ├── Bob interactive session
            └── ACP interactive session
                     │
                     ▼
           External ACP-compatible agent subprocess

Local CLI chat
    │
    └──────────────► same interactive session core (in-process)
```

### 6.1 Key Architectural Rule

**Unify at the session core, not at the transport.**

That means:

1. Local CLI does **not** need to talk to WebSocket just to be "consistent."
2. Remote PWA does **not** need direct ownership of the backend session.
3. Both surfaces share the same session orchestration code in-process.

### 6.2 Recommended Browser-Side Protocol Contract

The PWA should speak **ACP-compatible JSON-RPC over WebSocket**, but with one important implementation decision:

1. **Use one JSON-RPC message per WebSocket frame.**
2. **Do not preserve NDJSON on the browser link.**

Rationale:

1. NDJSON is necessary on stdio because stdio is a byte stream.
2. WebSocket already provides message framing.
3. A browser client should not need to care about newline delimiting.

Internally, the Alice server can still use NDJSON for stdio communication with the external ACP subprocess.

---

## 7. Unifying Local and Remote Control in Code

### 7.1 New Internal Abstraction: Interactive Session

The current `AgentBackend` / `AgentSession` shape is too one-shot for remote control.

Alice should introduce a new long-lived abstraction, conceptually like this:

```rust
pub trait InteractiveSession: Send + Sync {
    fn session_id(&self) -> &str;
    fn subscribe(&self) -> broadcast::Receiver<SessionEvent>;
    async fn prompt(&self, input: UserPrompt, context: RequestContext) -> eyre::Result<()>;
    async fn cancel(&self, turn_id: Option<String>) -> eyre::Result<()>;
    async fn answer_permission(
        &self,
        request_id: &str,
        decision: PermissionDecision,
    ) -> eyre::Result<()>;
    async fn snapshot(&self) -> SessionSnapshot;
}
```

This does not need to replace every existing API immediately, but it must become the foundation for:

1. local interactive CLI,
2. remote WebSocket control,
3. future session replay and session list support.

### 7.2 New Internal Type: SessionEvent

Alice needs an internal event model that is richer than a final `String` response.

Recommended event categories:

1. `SessionStarted`
2. `SessionLoaded`
3. `TurnStarted`
4. `TextDelta`
5. `TextFinal`
6. `ToolCallUpdated`
7. `PlanUpdated`
8. `PermissionRequested`
9. `PermissionResolved`
10. `TurnCompleted`
11. `TurnFailed`
12. `SessionWarning`

This internal model is the pivot point.

It should be mapped:

1. to terminal rendering for local CLI,
2. to ACP notifications for the WebSocket API,
3. to persistent replay buffers for reconnects.

### 7.3 New Internal Component: Session Registry

Alice should own a `SessionRegistry` keyed by Alice session ID.

Responsibilities:

1. create or reuse long-lived sessions,
2. hold replay buffers,
3. track active subscribers,
4. keep session metadata for listing and reconnection,
5. determine whether a session is local-only or remotely controlled.

### 7.4 New Internal Component: Permission Broker

ACP permission requests and Alice-side approval policies should not live inside the ACP backend implementation.

Instead, a dedicated broker should:

1. receive permission requests from the backend session,
2. publish them as `SessionEvent::PermissionRequested`,
3. wait for a local or remote UI response,
4. apply timeout/default-deny behavior,
5. resume the backend when a decision arrives.

This is the correct place to implement policies such as:

1. auto-approve read-only actions locally,
2. require explicit approval remotely,
3. deny when the control surface disconnects mid-request.

---

## 8. Backend Design Changes

### 8.1 Bob Backend

The `bob` backend should be adapted into a long-lived interactive session.

Preferred model:

1. keep Alice memory/skills composition in front of the backend,
2. run the backend through a streaming-capable path when available,
3. publish tool and lifecycle events into `SessionEvent`.

If full streaming is not immediately available for `bob`, phase 1 may emit coarse-grained events only:

1. `TurnStarted`,
2. final response text,
3. tool transcript summary,
4. `TurnCompleted`.

That is acceptable for incremental delivery, but the event model should still be introduced now.

### 8.2 ACP Backend

The `acp` backend must be promoted from minimal request/response glue into a real long-lived session.

Required changes:

1. create the ACP subprocess **once per Alice session**, not once per user turn,
2. initialize the ACP connection once,
3. create or load the ACP session once,
4. forward notifications into `SessionEvent`,
5. route permission requests through the permission broker,
6. accumulate final text and session snapshot state,
7. support cancellation and session load if the agent exposes it.

### 8.3 Preserve Alice as the Control Owner

Even when the backend is an external ACP agent, the remote client should still talk to Alice, not directly to the subprocess.

That gives Alice control over:

1. memory recall/persist,
2. skill injection,
3. unified approval policy,
4. local/remote parity,
5. reconnect and replay,
6. backend substitution.

---

## 9. Remote Server Design

### 9.1 Placement in the Workspace

The most practical fit is:

1. keep remote server logic in `alice-runtime`,
2. add a new `serve` subcommand to `alice-cli`,
3. keep the PWA as a separate Rust frontend crate.

Recommended structure:

```text
bin/alice-cli/
  src/main.rs               # add `serve` subcommand

crates/alice-runtime/src/
  control/
    mod.rs
    session.rs
    registry.rs
    events.rs
    permission.rs
    snapshot.rs
  remote/
    mod.rs
    server.rs
    ws.rs
    acp_api.rs
    auth.rs

crates/alice-remote-ui/
  src/
  public/
  Cargo.toml
```

This is a justified boundary:

1. `alice-runtime` remains the composition/runtime crate,
2. the Leptos UI is a distinct WASM target with its own dependencies and build pipeline.

### 9.2 Command-Line Entry Point

Add a new CLI command:

```text
alice serve --bind 127.0.0.1:3000
```

The server command should:

1. build the same runtime context as local CLI,
2. start the session registry and permission broker,
3. serve WebSocket ACP API,
4. serve the PWA shell and assets,
5. optionally print pairing information and tunnel instructions.

### 9.3 WebSocket API Shape

Recommended endpoints:

1. `GET /` or `/app/*` for static PWA assets,
2. `GET /ws` for ACP-compatible JSON-RPC over WebSocket,
3. `GET /healthz` for local/tunnel health checks,
4. `GET /api/sessions` as an optional convenience API for non-ACP session listing.

### 9.4 Session Ownership Rules

Recommended v1 rules:

1. multiple sessions per server are allowed,
2. only one primary interactive client per session,
3. reconnecting client can reclaim its session,
4. stale disconnected sessions remain alive for a configurable TTL,
5. newest interactive controller wins for the same session.

This is simpler than full multi-user collaboration and good enough for phone control.

---

## 10. Leptos PWA Design

### 10.1 Why Leptos CSR Is the Right Default

For this product, **CSR is better than SSR for v1**.

Reasons:

1. the app is authenticated/private,
2. SEO is irrelevant,
3. most value is in live session state and WebSocket updates,
4. the UI can be served as static assets by Axum,
5. operational complexity is lower.

SSR can be added later if needed, but it is not necessary for the remote-control use case.

### 10.2 PWA Capabilities

The PWA should provide:

1. chat transcript with streaming markdown,
2. tool-call cards with status and affected paths,
3. approval sheet for permission requests,
4. session list / resume view,
5. connection banner and reconnect state,
6. installable manifest and service worker,
7. mobile-first layout.

### 10.3 PWA Component Model

Recommended main components:

1. `AppShell`
2. `ConversationView`
3. `Composer`
4. `ToolCallPanel`
5. `ApprovalSheet`
6. `SessionDrawer`
7. `ConnectionBanner`
8. `SettingsSheet`

### 10.4 Client-Side State Ownership

The PWA should own **view state**, not canonical agent state.

The client keeps:

1. current transcript snapshot,
2. pending approval UI state,
3. reconnect status,
4. last known session metadata.

The server keeps:

1. live session state,
2. permission waiters,
3. replay buffer,
4. backend handles,
5. authoritative session lifecycle.

### 10.5 PWA Build and Serving

Recommended deployment model:

1. build Leptos CSR app to static assets,
2. serve those assets from the Axum server,
3. keep the PWA and WebSocket API under the same origin.

Benefits:

1. no CORS complexity,
2. clean cookie/auth story with Cloudflare Access,
3. simpler installation and caching.

---

## 11. ACP on the Remote Surface

### 11.1 What the Remote Surface Should Promise

Alice should expose **ACP-compatible JSON-RPC methods** to remote clients.

Recommended baseline methods:

1. `initialize`
2. `session/new`
3. `session/load`
4. `session/prompt`
5. `session/cancel`
6. `session/list` as an Alice extension if needed

Recommended notifications/requests:

1. `session/update`
2. `session/request_permission`

### 11.2 Important Caveat

This browser-facing WebSocket binding should be documented as:

> "Alice ACP-compatible WebSocket transport"

not as:

> "the official ACP WebSocket standard"

because ACP remote transport is still evolving.

### 11.3 Why This Is Still the Right Choice

Using ACP-compatible method names and message shapes gives three advantages:

1. the PWA stays close to the agent ecosystem's vocabulary,
2. future interoperability is easier,
3. the external API remains understandable to users already familiar with ACP.

---

## 12. Security and Cloudflare Design

### 12.1 Quick Tunnel vs Persistent Tunnel

Recommended policy:

1. **Quick Tunnel** only for development and demos,
2. **Persistent Cloudflare Tunnel + Access** for real use.

Rationale:

1. Quick Tunnels are documented by Cloudflare as development/testing oriented.
2. They use random hostnames and have product limitations.
3. Production remote control needs stable DNS, explicit Access policy, and predictable ownership.

### 12.2 Recommended Cloudflare Setup

Production path:

1. create a persistent tunnel outside Alice,
2. bind a stable hostname such as `alice.example.com`,
3. configure a Cloudflare Access self-hosted application,
4. restrict access by email or SSO group,
5. enable token validation via `cloudflared` or origin validation,
6. run Alice bound to `127.0.0.1` only.

This keeps Alice off the public internet except through Cloudflare.

### 12.3 Application-Level Security on Top of Access

Cloudflare Access should be the outer gate, but Alice should still add a lightweight app-level handshake.

Recommended v1 pattern:

1. Access authenticates the browser,
2. Alice generates a short-lived session pairing token,
3. the PWA sends the token during WebSocket handshake,
4. Alice binds the token to one controller session.

This is useful for:

1. local-only operation without Access,
2. QR-based pairing flows,
3. reducing accidental cross-session attachment.

### 12.4 Approval Safety Rules

Recommended approval rules:

1. default deny on timeout,
2. default deny when the controlling client disconnects mid-approval,
3. configurable allowlists for obviously safe read-only operations,
4. never hard-code "allow always" inside the ACP backend.

---

## 13. Phased Delivery Plan

### Phase 0: Fix the Existing ACP Foundation

Must happen first.

1. make ACP sessions persistent across turns,
2. surface ACP streaming notifications,
3. remove backend-level auto-approval,
4. return final assistant content correctly,
5. add tests with a mock ACP subprocess.

### Phase 1: Introduce the Interactive Session Core

1. add `SessionEvent`, `SessionSnapshot`, and `PermissionBroker`,
2. add `SessionRegistry`,
3. adapt local CLI `chat` to use the session core,
4. keep `run` as a convenience wrapper on top of the same core.

### Phase 2: Add the Remote Server

1. add Axum server in `alice-runtime::remote`,
2. add `alice serve`,
3. add ACP-compatible WebSocket API,
4. add replay and reconnect behavior,
5. add local health/status endpoints.

### Phase 3: Add the Leptos PWA

1. create `alice-remote-ui` Leptos CSR crate,
2. implement chat/tool/approval/session UI,
3. add manifest and service worker,
4. serve assets from the Axum server.

### Phase 4: Harden Deployment

1. document persistent tunnel deployment,
2. add Access-aware pairing flow,
3. define approval timeout and reconnection semantics,
4. add structured telemetry and export.

### Phase 5: Optional Enhancements

1. multi-agent profile switching,
2. richer model/mode selection UI,
3. read-only observer clients,
4. per-session persisted metadata,
5. push notifications or background summaries.

---

## 14. Testing Strategy

### 14.1 Unit Tests

Add unit coverage for:

1. session registry lifecycle,
2. permission broker timeout behavior,
3. event-to-ACP mapping,
4. replay buffer logic,
5. reconnect state transitions.

### 14.2 Integration Tests

Add integration coverage for:

1. mock ACP subprocess -> Alice ACP backend,
2. WebSocket client -> Alice remote server -> mock backend,
3. reconnect with session replay,
4. permission request/response round-trip,
5. cancellation.

### 14.3 Browser Tests

For the PWA, add end-to-end flows covering:

1. initial connect,
2. send prompt,
3. stream updates,
4. approve tool,
5. disconnect and reconnect,
6. restore active session.

---

## 15. Final Recommendation

### 15.1 Feasibility Verdict

The original idea is **feasible**.

### 15.2 Reasonableness Verdict

As a **prototype**, it is reasonable.

As the **final Alice architecture**, it is not ideal, because it would duplicate control logic and underuse the runtime structure that already exists.

### 15.3 Recommended Final Direction

Build:

1. **Alice-owned interactive session core**,
2. **ACP-compatible WebSocket facade** for the PWA,
3. **Leptos CSR PWA** served by Alice,
4. **Cloudflared persistent tunnel + Cloudflare Access** for production remote access.

This gives Alice a clean answer to both requirements:

1. **local control** through CLI using the same session core,
2. **remote control** through PWA using the same session core,
3. **arbitrary ACP-compatible agent support** through the existing `acp` backend,
4. **one unified code path where it matters**: session orchestration, policy, replay, approvals, and memory/skills integration.

### 15.4 One-Sentence Architectural Decision

> Alice should not become a thin ACP tunnel; it should become a unified interactive runtime that can present an ACP-compatible remote surface.
