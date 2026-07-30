use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use super::{StorageError, StorageResult};

struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

const INITIAL_SCHEMA: &str = r#"
CREATE TABLE providers (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(id)) > 0),
    display_name TEXT NOT NULL CHECK (length(trim(display_name)) > 0)
);

CREATE TABLE accounts (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(id)) > 0),
    provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    display_name TEXT NOT NULL CHECK (length(trim(display_name)) > 0)
);

CREATE TABLE quota_pools (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(id)) > 0),
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    display_name TEXT NOT NULL CHECK (length(trim(display_name)) > 0),
    unit TEXT NOT NULL CHECK (length(trim(unit)) > 0)
);

CREATE TABLE quota_windows (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(id)) > 0),
    pool_id TEXT NOT NULL REFERENCES quota_pools(id) ON DELETE CASCADE,
    starts_at INTEGER NOT NULL,
    ends_at INTEGER NOT NULL,
    capacity INTEGER NOT NULL CHECK (capacity >= 0),
    CHECK (ends_at > starts_at)
);

CREATE TABLE scopes (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(id)) > 0),
    parent_id TEXT REFERENCES scopes(id) ON DELETE RESTRICT,
    kind TEXT NOT NULL CHECK (kind IN ('project', 'repository', 'task', 'reserve')),
    display_name TEXT NOT NULL CHECK (length(trim(display_name)) > 0),
    CHECK (parent_id IS NULL OR parent_id <> id)
);

CREATE TABLE allocations (
    scope_id TEXT NOT NULL REFERENCES scopes(id) ON DELETE CASCADE,
    window_id TEXT NOT NULL REFERENCES quota_windows(id) ON DELETE CASCADE,
    amount INTEGER NOT NULL CHECK (amount >= 0),
    PRIMARY KEY (scope_id, window_id)
);

CREATE TABLE reservations (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(id)) > 0),
    scope_id TEXT NOT NULL REFERENCES scopes(id) ON DELETE RESTRICT,
    window_id TEXT NOT NULL REFERENCES quota_windows(id) ON DELETE RESTRICT,
    amount INTEGER NOT NULL CHECK (amount > 0),
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('active', 'released', 'consumed', 'expired')),
    CHECK (expires_at > created_at)
);

CREATE TABLE usage_events (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(id)) > 0),
    window_id TEXT NOT NULL REFERENCES quota_windows(id) ON DELETE RESTRICT,
    scope_id TEXT REFERENCES scopes(id) ON DELETE RESTRICT,
    amount INTEGER NOT NULL CHECK (amount > 0),
    observed_at INTEGER NOT NULL,
    source TEXT NOT NULL CHECK (
        source IN ('provider_confirmed', 'provider_observed', 'local_measured', 'estimated')
    ),
    confidence TEXT NOT NULL CHECK (
        confidence IN ('confirmed', 'observed', 'inferred', 'estimated')
    )
);

CREATE INDEX idx_accounts_provider_id ON accounts(provider_id);
CREATE INDEX idx_quota_pools_account_id ON quota_pools(account_id);
CREATE INDEX idx_quota_windows_pool_id ON quota_windows(pool_id);
CREATE INDEX idx_scopes_parent_id ON scopes(parent_id);
CREATE INDEX idx_allocations_window_id ON allocations(window_id);
CREATE INDEX idx_reservations_scope_window_status
    ON reservations(scope_id, window_id, status);
CREATE INDEX idx_usage_events_scope_window
    ON usage_events(scope_id, window_id);

CREATE TRIGGER usage_events_are_immutable_on_update
BEFORE UPDATE ON usage_events
BEGIN
    SELECT RAISE(ABORT, 'usage events are immutable');
END;

CREATE TRIGGER usage_events_are_immutable_on_delete
BEFORE DELETE ON usage_events
BEGIN
    SELECT RAISE(ABORT, 'usage events are immutable');
END;
"#;

const PROVIDER_QUOTA_SNAPSHOTS: &str = r#"
CREATE TABLE provider_quota_snapshots (
    window_id TEXT PRIMARY KEY NOT NULL REFERENCES quota_windows(id) ON DELETE CASCADE,
    adapter TEXT NOT NULL CHECK (length(trim(adapter)) > 0),
    remote_limit_id TEXT NOT NULL CHECK (length(trim(remote_limit_id)) > 0),
    remote_window_kind TEXT NOT NULL CHECK (length(trim(remote_window_kind)) > 0),
    used INTEGER NOT NULL CHECK (used >= 0),
    observed_at INTEGER NOT NULL,
    resets_at INTEGER NOT NULL
);
"#;

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_schema",
        sql: INITIAL_SCHEMA,
    },
    Migration {
        version: 2,
        name: "provider_quota_snapshots",
        sql: PROVIDER_QUOTA_SNAPSHOTS,
    },
];

pub(crate) fn migrate(connection: &mut Connection) -> StorageResult<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY NOT NULL,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );",
    )?;

    let current = connection
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get::<_, Option<i64>>(0)
        })
        .optional()?
        .flatten()
        .unwrap_or(0);
    let supported = MIGRATIONS.last().map_or(0, |migration| migration.version);

    if current > supported {
        return Err(StorageError::UnsupportedSchemaVersion { current, supported });
    }

    for migration in MIGRATIONS
        .iter()
        .filter(|migration| migration.version > current)
    {
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute_batch(migration.sql)?;
        transaction.execute(
            "INSERT INTO schema_migrations (version, name) VALUES (?1, ?2)",
            params![migration.version, migration.name],
        )?;
        transaction.commit()?;
    }

    Ok(())
}

#[cfg(test)]
pub(crate) fn latest_version() -> i64 {
    MIGRATIONS.last().map_or(0, |migration| migration.version)
}
