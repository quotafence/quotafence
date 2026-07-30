# Local Storage

Agent Quota Manager uses SQLite inside the Rust backend. The database stores
quota configuration and attribution metadata locally; prompts, source code, and
provider credentials are not part of the schema.

## Current implementation

The `src-tauri/src/storage` module provides:

- database opening with a caller-supplied path;
- isolated in-memory databases for tests;
- ordered, transactional schema migrations;
- catalog repositories for providers, accounts, pools, windows, and scopes;
- atomic quota-source onboarding;
- provider-source identity checks and soft archival;
- canonical folder to workspace-scope bindings;
- atomic workspace, initial allocation, and binding creation;
- transactional folder-allocation writes;
- capacity-checked reservations;
- atomic usage recording and reservation consumption; and
- append-only usage events.

Application startup now resolves Tauri's operating-system-specific app-data
directory and opens `agent-quota-manager.sqlite3` inside it. The storage layer
still receives that path from its caller rather than guessing a filesystem
location.

## Connection configuration

Each initialized connection enables:

- foreign-key enforcement;
- a five-second busy timeout;
- `NORMAL` synchronous mode; and
- write-ahead logging for file-backed databases.

The application layer must serialize access to the connection when it becomes
Tauri managed state. Storage operations do not expose a connection to the
frontend.

## Schema

```text
providers
└── accounts
    └── quota_pools
        └── quota_windows
            ├── provider_quota_snapshots
            ├── allocations ── scopes
            ├── reservations ─ scopes
            └── usage_events ─ scopes (optional)

workspace_bindings ── workspace scopes
```

Amounts are stored as non-negative SQLite integers. Their unit is defined by the
quota pool and checked when domain values cross the storage boundary.
Timestamps are Unix milliseconds.

Usage events allow a null scope for unattributed provider consumption. Update
and delete triggers make those events append-only, and restrictive foreign keys
prevent parent deletion from erasing ledger history.

Provider quota snapshots are different from ledger events: each window has at
most one absolute provider reading, and a later sync replaces it. Dashboard
reconciliation takes the greater of that reading and locally observed usage, so
refreshing cannot double-count the provider total.

Detected sources receive a normalized identity derived from the adapter,
provider limit, and window kind. Only one active quota pool may own that
identity. Archiving sets a timestamp on the pool rather than cascading deletes,
which allows the source to be added again while preserving its historical
windows and ledger records. The active source list selects one current, or
otherwise latest, window per pool so rollover history is not presented as a
second source.

Workspace bindings are separate from scopes. The canonical folder path is
unique, and a workspace scope can be bound only once. Folder selection,
top-level allocation, and binding are committed atomically. Restrictive foreign
keys prevent a bound scope from being deleted behind the mapping.

Migration 5 renames the original `repository_bindings` table and path column.
Existing Git-root bindings remain valid folder bindings; Git is no longer read
or required.

## Transactions

Allocation writes acquire an immediate transaction before checking capacity:

- folder allocations share the quota-window capacity; and
- an over-capacity write rolls back without creating a partial workspace or
  binding.

The existing parent column and hierarchical checks remain readable for
pre-folder-only databases. The current UI and Tauri command boundary create
only top-level workspace allocations.

Reservation admission also uses an immediate transaction. It subtracts existing
attributed usage and active reservations before inserting a new reservation.

Recording usage and consuming its reservation happen in one transaction. If the
reservation is missing, inactive, or belongs to another scope/window, the usage
insert is rolled back.

## Migrations

Applied migrations are recorded in `schema_migrations`. Startup applies pending
migrations in order and rejects a database whose schema version is newer than
the running application supports.

Migration rules:

- never edit a migration after release;
- add a new monotonically increasing version;
- preserve ledger history;
- make destructive changes explicit and recoverable; and
- test both a fresh database and upgrades from supported versions.

## Not implemented yet

- encryption at rest;
- backup, export, or restore;
- automated retention or pruning policy.
