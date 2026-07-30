# Architecture

This document describes the intended architecture. The repository currently
contains the provider-neutral domain, local SQLite storage, application
services, Tauri command boundary, and a Codex discovery/synchronization adapter.
Repository mapping and managed-session execution remain planned.

## Goals

- Protect capacity for high-priority repository and task work.
- Attribute managed coding-agent usage to those scopes automatically.
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
- Claiming hard enforcement for sessions that AQM did not launch and control.

## Initial shape: a modular monolith

The repository remains a modular monolith. The desktop and the planned CLI
should share domain, application, storage, and adapter code. This keeps
installation and debugging simple while the managed-session contract evolves.

```text
src/                         React presentation and view state
src-tauri/src/
  commands/                  Narrow Tauri command boundary
  domain/                    Quota, allocation, scope, policy, usage event
  application/               Use cases and orchestration
  storage/                   Local persistence and migrations
  providers/
    codex/                   First provider adapter
  bin/aqm.rs                 Planned lightweight CLI entry point
```

The CLI path is a target, not a reason to create an empty binary in advance.

## Component responsibilities

### Desktop UI

Displays setup, allocations, remaining capacity, confidence, provider
capabilities, and eventually managed-session state. The current local MVP
implements onboarding, multi-source window selection, project/task allocations,
allocation updates, and manual usage recording. It does not infer enforcement
guarantees from a provider name. See [Local MVP](local-mvp.md).

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
- hierarchical debiting;
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

The current Codex adapter discovers and synchronizes aggregate quota. It does
not yet launch a user session or expose session-level consumption.

### CLI wrapper

The first managed workflow should be `aqm run codex`. The CLI resolves the
current repository, calls the same application services as the desktop, and
owns the child process lifecycle. It should not duplicate policy or storage
logic in command handlers.

## Managed-session flow

1. Resolve the current Git root through an explicit repository binding.
2. Refresh the relevant provider checkpoint.
3. Read the allocation, usage, reservations, policy, and adapter capabilities.
4. Allow, warn, request confirmation, or refuse admission.
5. Persist a session record and reserve capacity before spawning.
6. Start and supervise Codex in the repository working directory.
7. Persist the exit outcome, refresh the provider checkpoint, and reconcile.
8. Consume or release the reservation and append immutable attribution events.

An aggregate provider delta is not automatically proof that one session caused
it. Usage outside a managed session or concurrent work can reduce the same
total. Ambiguous consumption remains **unattributed**, and managed attribution
must carry an honest confidence level.

## Enforcement modes

- **Managed hard stop:** the adapter controls the session and can stop new work.
- **Managed warning:** the app can observe or estimate usage but cannot safely
  interrupt the provider.
- **Observed only:** the app reports budget state but cannot attribute or enforce
  individual sessions.

The effective mode is derived from adapter capabilities and current health, not
only from user preference.

For the initial Codex slice, a stop can safely mean refusing to launch an
AQM-managed process. Live termination is a separate capability requiring both
process ownership and a timely provider signal. It must not be inferred merely
because AQM can kill a child process.

## Trust boundaries

- The webview is untrusted input to Rust commands.
- Repository paths and metadata are untrusted.
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
