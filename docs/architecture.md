# Architecture

This document describes the intended architecture. The repository currently
contains the provider-neutral domain, local SQLite storage, application
services, Tauri command boundary, and a Codex discovery/synchronization adapter.
Folder-based workspace mapping and a lightweight context CLI are implemented;
provider-refreshing admission dry runs, passive Codex Desktop attribution, and
an optional trusted lifecycle-hook admission gate are also implemented.
`qfence codex` owns one admitted child process and reservation and reconciles
terminal usage.

## Goals

- Protect capacity for high-priority folder workspaces.
- Attribute managed coding-agent usage to those workspaces automatically.
- Put admission and policy enforcement in the managed execution path.
- Forecast depletion from trustworthy reconciled history.
- Keep configuration and usage history local by default.
- Preserve a path toward capability-aware routing without adding providers
  early.

## Non-goals

- Replacing a provider's billing, quota, or account-wide enforcement system.
- Circumventing provider limits or terms of service.
- Claiming exact token accounting when a subscription exposes only a percentage
  or time-window allowance.
- Acting as a cloud proxy for prompts or source code.
- Cloud sync, team workspaces, RBAC, billing, or organization governance in v0.1.
- Claiming that QuotaFence can terminate a Codex Desktop turn it did not launch.

## Initial shape: a modular monolith

The repository remains a modular monolith. The desktop and CLI
should share domain, application, storage, and adapter code. This keeps
installation and debugging simple while the managed-session contract evolves.

```text
src/                         React presentation and view state
src-tauri/src/
  commands/                  Narrow Tauri command boundary
  domain/                    Quota, allocation, scope, policy, usage event
  entitlements.rs            Central Free/paid capability boundary
  application/               Use cases and orchestration
  storage/                   Local persistence and migrations
  providers/
    codex.rs                 Quota discovery and synchronization
    codex_desktop.rs         Read-only local activity metadata scan
    codex_hooks.rs           Desktop prompt admission, observation, hook config
  bin/quotafence.rs                 Context, admission, and Codex hook entrypoint
```

## Component responsibilities

### Desktop UI

Displays setup, allocations, remaining capacity, confidence, provider
capabilities, and eventually managed-session state. The current local MVP
implements onboarding, multi-source window selection, folder selection,
workspace allocations, and allocation updates. It does not accept manual usage
estimates or infer enforcement guarantees from a provider name. See
[Local MVP](local-mvp.md).

### Tauri command boundary

Validates requests from the webview and exposes small application use cases. It
must not expose arbitrary shell execution or unrestricted filesystem access.
Transport and storage DTOs must be converted through domain constructors so
deserialization cannot bypass domain invariants. The current boundary owns the
application-data path, synchronized service state, and stable IPC error mapping;
see [Tauri commands](tauri-commands.md).

### Quota core

Owns provider-neutral rules:

- allocation and rollover;
- priority-ordered funding of folder targets from current provider capacity;
- folder-level debiting;
- reservations for in-flight work;
- warning and stop policies; and
- reconciliation of attributed and unattributed usage.

The core works with provider-native quota units plus confidence metadata. It
does not pretend that quota from different providers is fungible.

The current pool/window model is designed for consumable capacity. Future
resource types have different semantics: concurrency is instantaneous, USD
should use integer minor units, and priority/deadline belong to workload policy
rather than an amount. See [Quota model](quota-model.md). No general resource
rewrite is required for the Codex slice.

### Entitlements

Entitlements are a boundary beside the quota core, not part of it. The backend
returns a capability snapshot containing the complete Free core plus any
capabilities from a verified, unexpired grant. The UI and future commercial
modules ask for individual capabilities instead of reading a plan name.

The current adapter always returns Free. It performs no network request and
stores no license. Future payment, signature-verification, and cached-license
adapters must feed the resolver without introducing a dependency from the
domain or storage layers to a cloud service. Expired grants resolve to Free;
they do not mutate or delete user data. See
[Product and monetization direction](product-and-monetization.md).

### Application services

Application services validate command DTOs through domain constructors,
orchestrate repositories, and produce serializable dashboard snapshots. They do
not depend on Tauri, webview state, or a provider implementation. See
[Application services](application-services.md).

### Local storage

The Rust backend owns a local SQLite database for configuration, allocations,
reservations, and usage events. Migrations are versioned, foreign keys are
enabled, allocation and ledger writes use immediate transactions, and usage
events are append-only. Provider credentials are not part of the schema.

The Tauri boundary supplies a path inside the operating system's app-data
directory. Tests use isolated in-memory databases. See [Storage](storage.md).

### Provider adapters

Adapters translate provider-specific quota windows, usage signals, and session
controls into the core model. Each adapter reports capabilities at runtime; see
[Provider adapters](provider-adapters.md).

The current Codex adapter discovers and synchronizes aggregate quota. Its
checkpoint application is shared by desktop refresh and CLI admission, and can
be tested with a fabricated detection result without spawning Codex. It does
not launch user work through the App Server; the CLI wrapper starts the resolved
Codex executable directly. A passive desktop scanner correlates minimal local
thread activity metadata with provider checkpoint movement. An optional
lifecycle-hook adapter checks a Desktop prompt before it starts, then brackets
allowed turns with provider checkpoints and records an inferred scoped delta
only when that turn is the sole active observation for the window.

## Passive Codex Desktop attribution

1. Desktop startup or explicit refresh reads the provider checkpoint.
2. The adapter opens Codex's newest local state database read-only and selects
   only thread identity, working folder, cumulative token counter, and update
   time.
3. The first scan records cursors without attributing historical usage.
4. Later counter movement accumulates as pending activity by canonical folder.
5. When the provider percentage advances, nearest-ancestor bindings resolve
   those folders.
6. The provider delta becomes one inferred workspace event only when every
   pending folder resolves to the same workspace.
7. Multiple workspaces, unmapped activity, rollover, unavailable scans, or
   refreshes from another interface invalidate or retain no scoped guess; the
   aggregate provider snapshot remains the source of truth.

Token counters are correlation evidence, not the quota unit displayed or
debited by QuotaFence. No prompt, response, title, preview, transcript, credential, or
workspace-file content crosses this adapter boundary.

### CLI wrapper

The CLI resolves and explicitly binds the current folder through the shared
application and storage layers. `qfence admit codex` refreshes the relevant
checkpoint and evaluates the effective admission boundary without launching a
process. `qfence codex` reuses that application boundary, persists and reserves
before spawn, supervises the child, and commits its terminal outcome with
reservation release. `qfence hook codex` is a lifecycle entrypoint used by
installed Codex hooks; explicit allocation decisions may block a new prompt,
while integration failures remain fail-open. It is not a managed launch. See
[CLI](cli.md).

## Observed Codex desktop turn

1. `UserPromptSubmit` supplies session ID, turn ID, and working folder.
2. QuotaFence resolves the nearest folder binding. Once protection is enabled by at
   least one Codex allocation, an unmapped folder is rejected before the turn
   starts.
3. For a mapped folder QuotaFence refreshes the selected Codex checkpoint and applies
   the workspace allocation policy. Exhausted or confirmation-boundary work is
   rejected.
4. An allowed prompt stores a minimal active-turn row containing the baseline,
   window, scope, and
   whether another turn overlaps it.
5. `Stop` refreshes the checkpoint again.
6. The end checkpoint minus the baseline becomes an immutable scoped usage
   event only when the window is unchanged, the folder is mapped, and no turn
   overlapped.
7. Zero deltas create no event. Rollover, concurrency, missing checkpoints, and
   unmapped folders remain represented by the aggregate provider total rather
   than fabricated attribution.
8. Stale unfinished rows are pruned at the next turn start.

The trusted prompt hook is a pre-turn admission gate, not process ownership: it
can reject a new prompt but cannot terminate an already-running turn. Codex
supplies a complete lifecycle event on stdin, but QuotaFence deserializes and persists
only session/turn identity, event type, and folder metadata. It does not
deserialize the prompt, assistant response, or transcript path.

## Managed-session flow

1. Canonicalize the current folder and resolve its nearest workspace binding.
2. Refresh the relevant provider checkpoint.
3. Read the allocation, usage, reservations, policy, and adapter capabilities.
4. Allow, warn, request confirmation, or refuse admission.
5. Persist a session record and reserve capacity before spawning.
6. Start and supervise Codex in the workspace working directory.
7. Persist the exit outcome, refresh the provider checkpoint, and reconcile.
8. Consume or release the reservation and append immutable attribution events.

Steps 1–8 are implemented for one aggregate Codex quota pool. The baseline is
persisted before spawn, and terminal state, immutable usage, reconciliation
state, and the reservation transition commit together. Missing or cross-window
checkpoints release the reservation without inventing usage.

An aggregate provider delta is not proof that one session caused it. Visible
concurrent work marks both observations contended and keeps the managed delta
**unattributed**. Usage outside QuotaFence without a lifecycle signal cannot be
detected, so managed attribution remains `observed`, never provider-confirmed.

## Depletion forecast

The forecast is a read model calculated from persisted managed-session
reconciliation, not provider-dashboard refreshes. For the active window QuotaFence
uses the interval from the earliest included managed launch to the explicit
query time. It requires at least two trustworthy reconciliations, at least one
hour of observation, and managed attribution covering at least half of observed
provider usage before exposing a daily burn rate or depletion time.

Ambiguous managed deltas count as managed history but not attributed evidence.
Sparse history, dominant unattributed usage, pre-window queries, and expired
windows do not produce a precise ETA. Because Codex exposes an aggregate
integer percentage, current forecast confidence is capped at medium.

## Enforcement modes

- **Managed hard stop:** the adapter controls the session and can stop new work.
- **Trusted prompt gate:** a reviewed provider hook can refuse a new prompt but
  cannot terminate an already-running turn.
- **Managed warning:** the app can observe or estimate usage but cannot safely
  interrupt the provider.
- **Observed only:** the app reports budget state but cannot attribute or enforce
  individual sessions.

The effective mode is derived from adapter capabilities and current health, not
only from user preference.

For the initial Codex slice, a stop means refusing a QuotaFence-managed process or a
new trusted-hook prompt. Live termination is a separate capability requiring
both process ownership and a timely provider signal. It must not be inferred
merely because QuotaFence can kill a child process.

The effective policy resolves from a persisted workspace-scope override and
then the application default. This keeps thresholds stable across provider
window rollover and gives the desktop, dry-run admission, and managed launch
one precedence rule. At a Desktop confirmation boundary, the hook creates a
short-lived, workspace/window-scoped request and blocks the original prompt.
The user can approve it once in QuotaFence and retry; the next matching prompt consumes
that approval atomically. Managed CLI confirmation remains explicit and is
audited when a managed session and reservation are committed.

## Trust boundaries

- The webview is untrusted input to Rust commands.
- Workspace paths and metadata are untrusted.
- Provider output may be malformed, incomplete, or change between versions.
- Logs and exports may reveal private project names or usage patterns.
- External provider authentication remains outside the app whenever practical.

Tauri permissions should be added per feature. A broad shell or filesystem
capability is not an acceptable shortcut.

## When to extract a daemon

A background service becomes justified when one of these is implemented:

- sessions must remain governed after the desktop window exits;
- both a CLI and desktop UI need concurrent access to one ledger;
- multiple repositories need long-running observation; or
- operating-system launch and lifecycle behavior becomes a product requirement.

Until then, an internal daemon would add deployment and security complexity
without improving the core model.

The milestone sequence and recommended lifecycle decisions live in the
[Codex-first roadmap](roadmap.md).
