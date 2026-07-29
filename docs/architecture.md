# Architecture

This document describes the intended architecture. The repository currently
contains the provider-neutral domain, local SQLite storage, and application
service layers, while Tauri commands and provider adapters remain planned.

## Goals

- Allocate subscription quota to projects, repositories, and tasks.
- Attribute managed coding-agent usage to those scopes.
- Enforce policy only to the degree supported by a provider integration.
- Keep configuration and usage history local by default.
- Add providers without leaking their terminology into the core domain.

## Non-goals

- Metering API-key billing or replacing a provider's billing system.
- Circumventing provider limits or terms of service.
- Claiming exact token accounting when a subscription exposes only a percentage
  or time-window allowance.
- Acting as a cloud proxy for prompts or source code.

## Initial shape: a modular monolith

The desktop process owns the UI boundary, domain services, local persistence,
and provider adapters. This keeps installation and debugging simple while the
domain is still evolving.

```text
src/                         React presentation and view state
src-tauri/src/
  commands/                  Narrow Tauri command boundary
  domain/                    Quota, allocation, scope, policy, usage event
  application/               Use cases and orchestration
  storage/                   Local persistence and migrations
  providers/
    codex/                   First provider adapter
```

The `domain/`, `storage/`, and `application/` modules now exist. The remaining
paths are targets, not a reason to create empty modules in advance.

## Component responsibilities

### Desktop UI

Displays allocations, remaining quota, data confidence, provider capabilities,
and managed-session state. It must not infer enforcement guarantees from a
provider name.

### Tauri command boundary

Validates requests from the webview and exposes small application use cases. It
must not expose arbitrary shell execution or unrestricted filesystem access.
Transport and storage DTOs must be converted through domain constructors so
deserialization cannot bypass domain invariants.

### Quota core

Owns provider-neutral rules:

- allocation and rollover;
- hierarchical debiting;
- reservations for in-flight work;
- warning and stop policies; and
- reconciliation of attributed and unattributed usage.

The core works with provider-native quota units plus confidence metadata. It
does not pretend that quota from different providers is fungible.

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

The application layer will supply a path inside Tauri's app-data directory.
Tests use isolated in-memory databases. See [Storage](storage.md).

### Provider adapters

Adapters translate provider-specific quota windows, usage signals, and session
controls into the core model. Each adapter reports capabilities at runtime; see
[Provider adapters](provider-adapters.md).

## Managed-session flow

1. Resolve the current repository or selected project to a quota scope.
2. Read the allocation, confirmed usage, reservations, and policy.
3. Refuse, warn, or reserve capacity before starting work.
4. Start the provider through a supported local integration.
5. Observe usage and append immutable attribution events.
6. Release the reservation and reconcile against the provider's latest total.

Usage outside a managed session can reduce the provider's total without having
a known project. That delta is recorded as **unattributed usage**, not assigned
to a convenient project.

## Enforcement modes

- **Managed hard stop:** the adapter controls the session and can stop new work.
- **Managed warning:** the app can observe or estimate usage but cannot safely
  interrupt the provider.
- **Observed only:** the app reports budget state but cannot attribute or enforce
  individual sessions.

The effective mode is derived from adapter capabilities and current health, not
only from user preference.

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
