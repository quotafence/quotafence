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
- atomic scope-and-initial-allocation creation;
- transactional hierarchical allocation writes;
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
            ├── allocations ── scopes
            ├── reservations ─ scopes
            └── usage_events ─ scopes (optional)
```

Amounts are stored as non-negative SQLite integers. Their unit is defined by the
quota pool and checked when domain values cross the repository boundary.
Timestamps are Unix milliseconds.

Usage events allow a null scope for unattributed provider consumption. Update
and delete triggers make those events append-only, and restrictive foreign keys
prevent parent deletion from erasing ledger history.

## Transactions

Allocation writes acquire an immediate transaction before checking capacity:

- top-level allocations share the quota-window capacity;
- child allocations share their parent allocation; and
- a missing parent allocation rejects child allocation.

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
- reconciliation checkpoints; and
- pruning or archival policy.
