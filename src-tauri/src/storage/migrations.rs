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

const QUOTA_SOURCE_LIFECYCLE: &str = r#"
ALTER TABLE quota_pools ADD COLUMN source_key TEXT;
ALTER TABLE quota_pools ADD COLUMN archived_at INTEGER;

CREATE UNIQUE INDEX active_quota_pool_source_key
ON quota_pools(source_key)
WHERE source_key IS NOT NULL AND archived_at IS NULL;

CREATE INDEX active_quota_pools
ON quota_pools(archived_at, display_name, id);
"#;

const REPOSITORY_BINDINGS: &str = r#"
CREATE TABLE repository_bindings (
    canonical_root TEXT PRIMARY KEY NOT NULL CHECK (length(trim(canonical_root)) > 0),
    scope_id TEXT NOT NULL UNIQUE REFERENCES scopes(id) ON DELETE RESTRICT,
    bound_at INTEGER NOT NULL
);
"#;

const WORKSPACE_BINDINGS: &str = r#"
ALTER TABLE repository_bindings RENAME TO workspace_bindings;
ALTER TABLE workspace_bindings RENAME COLUMN canonical_root TO canonical_path;
"#;

const PROVIDER_TURN_OBSERVATIONS: &str = r#"
CREATE TABLE provider_turn_observations (
    session_id TEXT NOT NULL CHECK (length(trim(session_id)) > 0),
    turn_id TEXT NOT NULL CHECK (length(trim(turn_id)) > 0),
    adapter TEXT NOT NULL CHECK (length(trim(adapter)) > 0),
    canonical_path TEXT NOT NULL CHECK (length(trim(canonical_path)) > 0),
    scope_id TEXT REFERENCES scopes(id) ON DELETE RESTRICT,
    window_id TEXT NOT NULL REFERENCES quota_windows(id) ON DELETE CASCADE,
    baseline_used INTEGER NOT NULL CHECK (baseline_used >= 0),
    started_at INTEGER NOT NULL,
    contended INTEGER NOT NULL DEFAULT 0 CHECK (contended IN (0, 1)),
    PRIMARY KEY (session_id, turn_id)
);

CREATE INDEX idx_provider_turn_observations_window
    ON provider_turn_observations(adapter, window_id, started_at);
CREATE INDEX idx_provider_turn_observations_session
    ON provider_turn_observations(session_id);
"#;

const MANAGED_SESSIONS: &str = r#"
CREATE TABLE managed_sessions (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(id)) > 0),
    adapter TEXT NOT NULL CHECK (length(trim(adapter)) > 0),
    pool_id TEXT NOT NULL REFERENCES quota_pools(id) ON DELETE RESTRICT,
    window_id TEXT NOT NULL REFERENCES quota_windows(id) ON DELETE RESTRICT,
    scope_id TEXT NOT NULL REFERENCES scopes(id) ON DELETE RESTRICT,
    reservation_id TEXT NOT NULL UNIQUE REFERENCES reservations(id) ON DELETE RESTRICT,
    canonical_path TEXT NOT NULL CHECK (length(trim(canonical_path)) > 0),
    status TEXT NOT NULL CHECK (
        status IN ('starting', 'running', 'completed', 'failed', 'interrupted')
    ),
    reconciliation_status TEXT NOT NULL DEFAULT 'pending' CHECK (
        reconciliation_status IN ('pending', 'reconciled', 'unavailable')
    ),
    supervisor_pid INTEGER NOT NULL CHECK (supervisor_pid > 0),
    child_pid INTEGER CHECK (child_pid IS NULL OR child_pid > 0),
    created_at INTEGER NOT NULL,
    started_at INTEGER,
    finished_at INTEGER,
    exit_code INTEGER,
    CHECK (
        (status = 'starting' AND started_at IS NULL AND finished_at IS NULL)
        OR (status = 'running' AND started_at IS NOT NULL AND finished_at IS NULL)
        OR (
            status IN ('completed', 'failed', 'interrupted')
            AND finished_at IS NOT NULL
        )
    )
);

CREATE UNIQUE INDEX one_active_managed_session_per_pool
    ON managed_sessions(pool_id)
    WHERE status IN ('starting', 'running');
CREATE INDEX idx_managed_sessions_status
    ON managed_sessions(status, created_at);
"#;

const MANAGED_SESSION_RECONCILIATION: &str = r#"
ALTER TABLE managed_sessions
    ADD COLUMN baseline_used INTEGER CHECK (baseline_used IS NULL OR baseline_used >= 0);
ALTER TABLE managed_sessions
    ADD COLUMN baseline_observed_at INTEGER;
ALTER TABLE managed_sessions
    ADD COLUMN contended INTEGER NOT NULL DEFAULT 0 CHECK (contended IN (0, 1));
ALTER TABLE managed_sessions
    ADD COLUMN reconciled_amount INTEGER CHECK (
        reconciled_amount IS NULL OR reconciled_amount >= 0
    );
ALTER TABLE managed_sessions
    ADD COLUMN reconciled_at INTEGER;
ALTER TABLE managed_sessions
    ADD COLUMN reconciliation_outcome TEXT CHECK (
        reconciliation_outcome IS NULL OR reconciliation_outcome IN (
            'attributed',
            'no_usage',
            'ambiguous',
            'window_rolled_over',
            'snapshot_unavailable'
        )
    );

UPDATE managed_sessions
SET reconciliation_status = 'unavailable',
    reconciliation_outcome = 'snapshot_unavailable'
WHERE status IN ('completed', 'failed', 'interrupted')
  AND reconciliation_status = 'pending';
"#;

const WORKSPACE_POLICIES: &str = r#"
CREATE TABLE workspace_policies (
    scope_id TEXT PRIMARY KEY NOT NULL REFERENCES scopes(id) ON DELETE RESTRICT,
    warn_at_basis_points INTEGER CHECK (
        warn_at_basis_points IS NULL
        OR warn_at_basis_points BETWEEN 1 AND 10000
    ),
    confirm_at_basis_points INTEGER CHECK (
        confirm_at_basis_points IS NULL
        OR confirm_at_basis_points BETWEEN 1 AND 10000
    ),
    stop_at_basis_points INTEGER CHECK (
        stop_at_basis_points IS NULL
        OR stop_at_basis_points BETWEEN 1 AND 10000
    ),
    updated_at INTEGER NOT NULL,
    CHECK (
        warn_at_basis_points IS NULL
        OR confirm_at_basis_points IS NULL
        OR warn_at_basis_points <= confirm_at_basis_points
    ),
    CHECK (
        confirm_at_basis_points IS NULL
        OR stop_at_basis_points IS NULL
        OR confirm_at_basis_points <= stop_at_basis_points
    ),
    CHECK (
        warn_at_basis_points IS NULL
        OR stop_at_basis_points IS NULL
        OR warn_at_basis_points <= stop_at_basis_points
    )
);

CREATE TABLE managed_session_policy_overrides (
    session_id TEXT PRIMARY KEY NOT NULL
        REFERENCES managed_sessions(id) ON DELETE RESTRICT,
    scope_id TEXT NOT NULL REFERENCES scopes(id) ON DELETE RESTRICT,
    window_id TEXT NOT NULL REFERENCES quota_windows(id) ON DELETE RESTRICT,
    decision TEXT NOT NULL CHECK (decision = 'require_confirmation'),
    accepted_at INTEGER NOT NULL
);

CREATE INDEX idx_managed_session_policy_overrides_scope
    ON managed_session_policy_overrides(scope_id, accepted_at);
"#;

const CODEX_DESKTOP_ACTIVITY: &str = r#"
CREATE TABLE codex_desktop_thread_cursors (
    pool_id TEXT NOT NULL REFERENCES quota_pools(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL CHECK (length(trim(thread_id)) > 0),
    canonical_path TEXT NOT NULL CHECK (length(trim(canonical_path)) > 0),
    last_tokens INTEGER NOT NULL CHECK (last_tokens >= 0),
    pending_tokens INTEGER NOT NULL DEFAULT 0 CHECK (pending_tokens >= 0),
    observed_at INTEGER NOT NULL,
    PRIMARY KEY (pool_id, thread_id)
);

CREATE INDEX idx_codex_desktop_pending_activity
    ON codex_desktop_thread_cursors(pool_id, pending_tokens);
"#;

const ALLOCATION_PRIORITIES: &str = r#"
CREATE TABLE allocation_priorities (
    pool_id TEXT NOT NULL REFERENCES quota_pools(id) ON DELETE CASCADE,
    scope_id TEXT NOT NULL REFERENCES scopes(id) ON DELETE CASCADE,
    rank INTEGER NOT NULL CHECK (rank >= 0),
    PRIMARY KEY (pool_id, scope_id),
    UNIQUE (pool_id, rank)
);
"#;

const CODEX_PROTECTION_EVENTS: &str = r#"
CREATE TABLE codex_protection_events (
    session_id TEXT NOT NULL CHECK (length(trim(session_id)) > 0),
    turn_id TEXT NOT NULL CHECK (length(trim(turn_id)) > 0),
    canonical_path TEXT NOT NULL CHECK (length(trim(canonical_path)) > 0),
    scope_id TEXT REFERENCES scopes(id) ON DELETE SET NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('allowed', 'blocked')),
    reason TEXT NOT NULL CHECK (length(trim(reason)) > 0),
    occurred_at INTEGER NOT NULL,
    PRIMARY KEY (session_id, turn_id)
);

CREATE INDEX idx_codex_protection_events_occurred_at
    ON codex_protection_events(occurred_at DESC);
"#;

const PROVIDER_QUOTA_HISTORY: &str = r#"
CREATE TABLE provider_quota_history (
    window_id TEXT NOT NULL REFERENCES quota_windows(id) ON DELETE CASCADE,
    used INTEGER NOT NULL CHECK (used >= 0),
    observed_at INTEGER NOT NULL,
    PRIMARY KEY (window_id, observed_at)
);

CREATE INDEX idx_provider_quota_history_window_observed
    ON provider_quota_history(window_id, observed_at);

INSERT OR IGNORE INTO provider_quota_history (window_id, used, observed_at)
SELECT window_id, used, observed_at
FROM provider_quota_snapshots;
"#;

const CODEX_HOOK_RECEIPTS: &str = r#"
CREATE TABLE codex_hook_receipts (
    session_id TEXT NOT NULL CHECK (length(trim(session_id)) > 0),
    turn_id TEXT NOT NULL CHECK (length(trim(turn_id)) > 0),
    event_name TEXT NOT NULL CHECK (length(trim(event_name)) > 0),
    canonical_path TEXT NOT NULL CHECK (length(trim(canonical_path)) > 0),
    status TEXT NOT NULL CHECK (status IN ('received', 'decision', 'skipped', 'failed')),
    issue TEXT,
    occurred_at INTEGER NOT NULL,
    PRIMARY KEY (session_id, turn_id, event_name)
);

CREATE INDEX idx_codex_hook_receipts_event_occurred
    ON codex_hook_receipts(event_name, occurred_at DESC);
"#;

const PROVIDER_SYNC_HEALTH: &str = r#"
CREATE TABLE provider_sync_health (
    window_id TEXT PRIMARY KEY NOT NULL REFERENCES quota_windows(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN ('synced', 'not_applicable', 'unavailable')),
    message TEXT,
    checked_at INTEGER NOT NULL
);
"#;

const CODEX_DESKTOP_CONFIRMATIONS: &str = r#"
CREATE TABLE codex_desktop_confirmations (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(trim(id)) > 0),
    session_id TEXT NOT NULL CHECK (length(trim(session_id)) > 0),
    turn_id TEXT NOT NULL CHECK (length(trim(turn_id)) > 0),
    scope_id TEXT NOT NULL REFERENCES scopes(id) ON DELETE CASCADE,
    window_id TEXT NOT NULL REFERENCES quota_windows(id) ON DELETE CASCADE,
    canonical_path TEXT NOT NULL CHECK (length(trim(canonical_path)) > 0),
    status TEXT NOT NULL CHECK (
        status IN ('pending', 'approved', 'denied', 'consumed', 'expired')
    ),
    requested_at INTEGER NOT NULL,
    resolved_at INTEGER,
    expires_at INTEGER NOT NULL,
    CHECK (expires_at > requested_at)
);

CREATE INDEX idx_codex_desktop_confirmations_pending
    ON codex_desktop_confirmations(status, expires_at, requested_at);
CREATE INDEX idx_codex_desktop_confirmations_scope
    ON codex_desktop_confirmations(session_id, scope_id, window_id, status);
"#;

const WEEKLY_WORKSPACE_ALLOCATIONS: &str = r#"
DELETE FROM allocation_priorities
WHERE EXISTS (
    SELECT 1
    FROM allocations short_allocation
    JOIN quota_windows short_window ON short_window.id = short_allocation.window_id
    JOIN quota_pools short_pool ON short_pool.id = short_window.pool_id
    JOIN accounts short_account ON short_account.id = short_pool.account_id
    JOIN providers short_provider ON short_provider.id = short_account.provider_id
    JOIN provider_quota_snapshots short_snapshot
      ON short_snapshot.window_id = short_window.id
    WHERE short_allocation.scope_id = allocation_priorities.scope_id
      AND short_pool.id = allocation_priorities.pool_id
      AND lower(short_pool.display_name) NOT LIKE '%weekly%'
      AND EXISTS (
          SELECT 1
          FROM allocations weekly_allocation
          JOIN quota_windows weekly_window ON weekly_window.id = weekly_allocation.window_id
          JOIN quota_pools weekly_pool ON weekly_pool.id = weekly_window.pool_id
          JOIN accounts weekly_account ON weekly_account.id = weekly_pool.account_id
          JOIN providers weekly_provider ON weekly_provider.id = weekly_account.provider_id
          JOIN provider_quota_snapshots weekly_snapshot
            ON weekly_snapshot.window_id = weekly_window.id
          WHERE weekly_allocation.scope_id = short_allocation.scope_id
            AND lower(weekly_pool.display_name) LIKE '%weekly%'
            AND lower(weekly_provider.display_name) = lower(short_provider.display_name)
            AND lower(weekly_account.display_name) = lower(short_account.display_name)
      )
);

DELETE FROM allocations
WHERE rowid IN (
    SELECT short_allocation.rowid
    FROM allocations short_allocation
    JOIN quota_windows short_window ON short_window.id = short_allocation.window_id
    JOIN quota_pools short_pool ON short_pool.id = short_window.pool_id
    JOIN accounts short_account ON short_account.id = short_pool.account_id
    JOIN providers short_provider ON short_provider.id = short_account.provider_id
    JOIN provider_quota_snapshots short_snapshot
      ON short_snapshot.window_id = short_window.id
    WHERE lower(short_pool.display_name) NOT LIKE '%weekly%'
      AND EXISTS (
          SELECT 1
          FROM allocations weekly_allocation
          JOIN quota_windows weekly_window ON weekly_window.id = weekly_allocation.window_id
          JOIN quota_pools weekly_pool ON weekly_pool.id = weekly_window.pool_id
          JOIN accounts weekly_account ON weekly_account.id = weekly_pool.account_id
          JOIN providers weekly_provider ON weekly_provider.id = weekly_account.provider_id
          JOIN provider_quota_snapshots weekly_snapshot
            ON weekly_snapshot.window_id = weekly_window.id
          WHERE weekly_allocation.scope_id = short_allocation.scope_id
            AND lower(weekly_pool.display_name) LIKE '%weekly%'
            AND lower(weekly_provider.display_name) = lower(short_provider.display_name)
            AND lower(weekly_account.display_name) = lower(short_account.display_name)
      )
);
"#;

const CODEX_DESKTOP_MODELS: &str = r#"
ALTER TABLE codex_desktop_thread_cursors ADD COLUMN model TEXT;
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
    Migration {
        version: 3,
        name: "quota_source_lifecycle",
        sql: QUOTA_SOURCE_LIFECYCLE,
    },
    Migration {
        version: 4,
        name: "repository_bindings",
        sql: REPOSITORY_BINDINGS,
    },
    Migration {
        version: 5,
        name: "workspace_bindings",
        sql: WORKSPACE_BINDINGS,
    },
    Migration {
        version: 6,
        name: "provider_turn_observations",
        sql: PROVIDER_TURN_OBSERVATIONS,
    },
    Migration {
        version: 7,
        name: "managed_sessions",
        sql: MANAGED_SESSIONS,
    },
    Migration {
        version: 8,
        name: "managed_session_reconciliation",
        sql: MANAGED_SESSION_RECONCILIATION,
    },
    Migration {
        version: 9,
        name: "workspace_policies",
        sql: WORKSPACE_POLICIES,
    },
    Migration {
        version: 10,
        name: "managed_session_reconciliation_schema_repair",
        sql: "",
    },
    Migration {
        version: 11,
        name: "codex_desktop_activity",
        sql: CODEX_DESKTOP_ACTIVITY,
    },
    Migration {
        version: 12,
        name: "allocation_priorities",
        sql: ALLOCATION_PRIORITIES,
    },
    Migration {
        version: 13,
        name: "codex_protection_events",
        sql: CODEX_PROTECTION_EVENTS,
    },
    Migration {
        version: 14,
        name: "provider_quota_history",
        sql: PROVIDER_QUOTA_HISTORY,
    },
    Migration {
        version: 15,
        name: "codex_hook_receipts",
        sql: CODEX_HOOK_RECEIPTS,
    },
    Migration {
        version: 16,
        name: "provider_sync_health",
        sql: PROVIDER_SYNC_HEALTH,
    },
    Migration {
        version: 17,
        name: "codex_desktop_confirmations",
        sql: CODEX_DESKTOP_CONFIRMATIONS,
    },
    Migration {
        version: 18,
        name: "weekly_workspace_allocations",
        sql: WEEKLY_WORKSPACE_ALLOCATIONS,
    },
    Migration {
        version: 19,
        name: "codex_desktop_models",
        sql: CODEX_DESKTOP_MODELS,
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
        if migration.version == 10 {
            repair_managed_session_reconciliation_schema(&transaction)?;
        } else {
            transaction.execute_batch(migration.sql)?;
        }
        transaction.execute(
            "INSERT INTO schema_migrations (version, name) VALUES (?1, ?2)",
            params![migration.version, migration.name],
        )?;
        transaction.commit()?;
    }

    Ok(())
}

fn repair_managed_session_reconciliation_schema(
    transaction: &rusqlite::Transaction<'_>,
) -> StorageResult<()> {
    let mut statement = transaction.prepare("PRAGMA table_info(managed_sessions)")?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    let mut has_reconciliation_outcome = false;
    for column in columns {
        if column? == "reconciliation_outcome" {
            has_reconciliation_outcome = true;
            break;
        }
    }

    if has_reconciliation_outcome {
        return Ok(());
    }

    transaction.execute_batch(
        "ALTER TABLE managed_sessions
            ADD COLUMN reconciliation_outcome TEXT CHECK (
                reconciliation_outcome IS NULL OR reconciliation_outcome IN (
                    'attributed',
                    'no_usage',
                    'ambiguous',
                    'window_rolled_over',
                    'snapshot_unavailable'
                )
            );

         UPDATE managed_sessions
         SET reconciliation_outcome = CASE
             WHEN reconciliation_status = 'reconciled'
                  AND reconciled_amount = 0
                 THEN 'no_usage'
             WHEN reconciliation_status = 'reconciled'
                  AND contended = 1
                  AND reconciled_amount > 0
                 THEN 'ambiguous'
             WHEN reconciliation_status = 'reconciled'
                  AND contended = 0
                  AND reconciled_amount > 0
                 THEN 'attributed'
             WHEN reconciliation_status = 'unavailable'
                 THEN 'snapshot_unavailable'
             ELSE NULL
         END
         WHERE reconciliation_outcome IS NULL;",
    )?;
    Ok(())
}

#[cfg(test)]
pub(crate) fn latest_version() -> i64 {
    MIGRATIONS.last().map_or(0, |migration| migration.version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekly_allocation_migration_removes_only_the_mirrored_short_window_copy() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                 CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL,
                    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                 );",
            )
            .unwrap();
        for migration in MIGRATIONS
            .iter()
            .filter(|migration| migration.version <= 17)
        {
            let transaction = connection.transaction().unwrap();
            if migration.version == 10 {
                repair_managed_session_reconciliation_schema(&transaction).unwrap();
            } else {
                transaction.execute_batch(migration.sql).unwrap();
            }
            transaction
                .execute(
                    "INSERT INTO schema_migrations (version, name) VALUES (?1, ?2)",
                    params![migration.version, migration.name],
                )
                .unwrap();
            transaction.commit().unwrap();
        }
        connection
            .execute_batch(
                "INSERT INTO providers (id, display_name)
                 VALUES ('codex-weekly-provider', 'Codex'), ('codex-short-provider', 'Codex');
                 INSERT INTO accounts (id, provider_id, display_name)
                 VALUES
                    ('codex-weekly-account', 'codex-weekly-provider', 'Subscription'),
                    ('codex-short-account', 'codex-short-provider', 'Subscription');
                 INSERT INTO quota_pools (id, account_id, display_name, unit)
                 VALUES
                    ('codex-weekly', 'codex-weekly-account', 'Weekly allowance', 'percent'),
                    ('codex-short', 'codex-short-account', '5-hour allowance', 'percent');
                 INSERT INTO quota_windows (id, pool_id, starts_at, ends_at, capacity)
                 VALUES
                    ('codex-weekly-window', 'codex-weekly', 1000, 10000, 100),
                    ('codex-short-window', 'codex-short', 1000, 5000, 100);
                 INSERT INTO provider_quota_snapshots (
                    window_id, adapter, remote_limit_id, remote_window_kind,
                    used, observed_at, resets_at
                 ) VALUES
                    ('codex-weekly-window', 'codex_app_server', 'codex', 'secondary', 10, 2000, 10000),
                    ('codex-short-window', 'codex_app_server', 'codex', 'primary', 20, 2000, 5000);
                 INSERT INTO scopes (id, parent_id, kind, display_name)
                 VALUES ('workspace-a', NULL, 'repository', 'Workspace A');
                 INSERT INTO allocations (scope_id, window_id, amount)
                 VALUES
                    ('workspace-a', 'codex-weekly-window', 80),
                    ('workspace-a', 'codex-short-window', 80);
                 INSERT INTO allocation_priorities (pool_id, scope_id, rank)
                 VALUES
                    ('codex-weekly', 'workspace-a', 0),
                    ('codex-short', 'workspace-a', 0);",
            )
            .unwrap();

        migrate(&mut connection).unwrap();

        let allocations = connection
            .prepare("SELECT window_id, amount FROM allocations ORDER BY window_id")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(allocations, vec![("codex-weekly-window".to_owned(), 80)]);
        let priorities: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM allocation_priorities WHERE pool_id = 'codex-short'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(priorities, 0);
    }

    #[test]
    fn reconciliation_migration_preserves_existing_terminal_sessions() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                 CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL,
                    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                 );",
            )
            .unwrap();
        for migration in MIGRATIONS.iter().filter(|migration| migration.version <= 7) {
            connection.execute_batch(migration.sql).unwrap();
            connection
                .execute(
                    "INSERT INTO schema_migrations (version, name) VALUES (?1, ?2)",
                    params![migration.version, migration.name],
                )
                .unwrap();
        }
        connection
            .execute_batch(
                "INSERT INTO providers (id, display_name) VALUES ('codex', 'Codex');
                 INSERT INTO accounts (id, provider_id, display_name)
                 VALUES ('account', 'codex', 'Subscription');
                 INSERT INTO quota_pools (id, account_id, display_name, unit)
                 VALUES ('pool', 'account', 'Weekly', 'percent');
                 INSERT INTO quota_windows (id, pool_id, starts_at, ends_at, capacity)
                 VALUES ('window', 'pool', 1000, 10000, 100);
                 INSERT INTO scopes (id, parent_id, kind, display_name)
                 VALUES ('workspace', NULL, 'repository', 'Workspace');
                 INSERT INTO reservations (
                    id, scope_id, window_id, amount, created_at, expires_at, status
                 ) VALUES (
                    'reservation', 'workspace', 'window', 20, 2000, 8000, 'released'
                 );
                 INSERT INTO managed_sessions (
                    id, adapter, pool_id, window_id, scope_id, reservation_id,
                    canonical_path, status, reconciliation_status, supervisor_pid,
                    child_pid, created_at, started_at, finished_at, exit_code
                 ) VALUES (
                    'session', 'codex', 'pool', 'window', 'workspace', 'reservation',
                    '/code/workspace', 'completed', 'pending', 42,
                    84, 2000, 2100, 3000, 0
                 );",
            )
            .unwrap();

        migrate(&mut connection).unwrap();

        let migrated: (String, Option<i64>, bool, String) = connection
            .query_row(
                "SELECT reconciliation_status, baseline_used, contended,
                        reconciliation_outcome
                 FROM managed_sessions WHERE id = 'session'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            migrated,
            (
                "unavailable".to_owned(),
                None,
                false,
                "snapshot_unavailable".to_owned()
            )
        );
        assert_eq!(
            connection
                .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            latest_version()
        );
    }

    #[test]
    fn repair_migration_recovers_a_partially_applied_reconciliation_schema() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                 CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL,
                    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                 );",
            )
            .unwrap();
        for migration in MIGRATIONS.iter().filter(|migration| migration.version <= 7) {
            connection.execute_batch(migration.sql).unwrap();
            connection
                .execute(
                    "INSERT INTO schema_migrations (version, name) VALUES (?1, ?2)",
                    params![migration.version, migration.name],
                )
                .unwrap();
        }
        connection
            .execute_batch(
                "INSERT INTO providers (id, display_name) VALUES ('codex', 'Codex');
                 INSERT INTO accounts (id, provider_id, display_name)
                 VALUES ('account', 'codex', 'Subscription');
                 INSERT INTO quota_pools (id, account_id, display_name, unit)
                 VALUES ('pool', 'account', 'Weekly', 'percent');
                 INSERT INTO quota_windows (id, pool_id, starts_at, ends_at, capacity)
                 VALUES ('window', 'pool', 1000, 10000, 100);
                 INSERT INTO scopes (id, parent_id, kind, display_name)
                 VALUES ('workspace', NULL, 'repository', 'Workspace');
                 INSERT INTO reservations (
                    id, scope_id, window_id, amount, created_at, expires_at, status
                 ) VALUES (
                    'reservation', 'workspace', 'window', 20, 2000, 8000, 'released'
                 );
                 INSERT INTO managed_sessions (
                    id, adapter, pool_id, window_id, scope_id, reservation_id,
                    canonical_path, status, reconciliation_status, supervisor_pid,
                    child_pid, created_at, started_at, finished_at, exit_code
                 ) VALUES (
                    'session', 'codex', 'pool', 'window', 'workspace', 'reservation',
                    '/code/workspace', 'completed', 'pending', 42,
                    84, 2000, 2100, 3000, 0
                 );

                 ALTER TABLE managed_sessions
                    ADD COLUMN baseline_used INTEGER CHECK (
                        baseline_used IS NULL OR baseline_used >= 0
                    );
                 ALTER TABLE managed_sessions ADD COLUMN baseline_observed_at INTEGER;
                 ALTER TABLE managed_sessions
                    ADD COLUMN contended INTEGER NOT NULL DEFAULT 0 CHECK (
                        contended IN (0, 1)
                    );
                 ALTER TABLE managed_sessions
                    ADD COLUMN reconciled_amount INTEGER CHECK (
                        reconciled_amount IS NULL OR reconciled_amount >= 0
                    );
                 ALTER TABLE managed_sessions ADD COLUMN reconciled_at INTEGER;
                 UPDATE managed_sessions
                 SET reconciliation_status = 'unavailable'
                 WHERE status IN ('completed', 'failed', 'interrupted')
                   AND reconciliation_status = 'pending';
                 INSERT INTO schema_migrations (version, name)
                 VALUES (8, 'managed_session_reconciliation');",
            )
            .unwrap();
        connection.execute_batch(WORKSPACE_POLICIES).unwrap();
        connection
            .execute(
                "INSERT INTO schema_migrations (version, name) VALUES (9, 'workspace_policies')",
                [],
            )
            .unwrap();

        migrate(&mut connection).unwrap();

        let repaired: (String, String) = connection
            .query_row(
                "SELECT reconciliation_status, reconciliation_outcome
                 FROM managed_sessions WHERE id = 'session'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            repaired,
            ("unavailable".to_owned(), "snapshot_unavailable".to_owned())
        );
        assert_eq!(
            connection
                .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            latest_version()
        );
    }

    #[test]
    fn lifecycle_migration_accepts_preexisting_duplicate_sources() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL,
                    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );",
            )
            .unwrap();
        connection.execute_batch(INITIAL_SCHEMA).unwrap();
        connection.execute_batch(PROVIDER_QUOTA_SNAPSHOTS).unwrap();
        connection
            .execute_batch(
                "INSERT INTO schema_migrations (version, name)
                 VALUES (1, 'initial_schema'), (2, 'provider_quota_snapshots');

                 INSERT INTO providers (id, display_name)
                 VALUES ('provider-1', 'Codex'), ('provider-2', 'Codex');
                 INSERT INTO accounts (id, provider_id, display_name)
                 VALUES
                    ('account-1', 'provider-1', 'Subscription'),
                    ('account-2', 'provider-2', 'Subscription');
                 INSERT INTO quota_pools (id, account_id, display_name, unit)
                 VALUES
                    ('pool-1', 'account-1', 'Weekly allowance', 'percent'),
                    ('pool-2', 'account-2', 'Weekly allowance', 'percent');
                 INSERT INTO quota_windows (id, pool_id, starts_at, ends_at, capacity)
                 VALUES
                    ('window-1', 'pool-1', 1000, 2000, 100),
                    ('window-2', 'pool-2', 1000, 2000, 100);
                 INSERT INTO provider_quota_snapshots (
                    window_id, adapter, remote_limit_id, remote_window_kind,
                    used, observed_at, resets_at
                 )
                 VALUES
                    ('window-1', 'codex_app_server', 'codex', 'secondary', 10, 1500, 2000),
                    ('window-2', 'codex_app_server', 'codex', 'secondary', 10, 1500, 2000);",
            )
            .unwrap();

        migrate(&mut connection).unwrap();

        let active_pools: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM quota_pools WHERE archived_at IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(active_pools, 2);
        assert_eq!(
            connection
                .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            latest_version()
        );
    }

    #[test]
    fn workspace_migration_preserves_existing_git_root_bindings_as_folders() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                 CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL,
                    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                 );",
            )
            .unwrap();
        connection.execute_batch(INITIAL_SCHEMA).unwrap();
        connection.execute_batch(PROVIDER_QUOTA_SNAPSHOTS).unwrap();
        connection.execute_batch(QUOTA_SOURCE_LIFECYCLE).unwrap();
        connection.execute_batch(REPOSITORY_BINDINGS).unwrap();
        connection
            .execute_batch(
                "INSERT INTO schema_migrations (version, name)
                 VALUES
                    (1, 'initial_schema'),
                    (2, 'provider_quota_snapshots'),
                    (3, 'quota_source_lifecycle'),
                    (4, 'repository_bindings');
                 INSERT INTO scopes (id, parent_id, kind, display_name)
                 VALUES ('workspace-a', NULL, 'repository', 'Workspace A');
                 INSERT INTO repository_bindings (canonical_root, scope_id, bound_at)
                 VALUES ('/code/workspace-a', 'workspace-a', 1000);",
            )
            .unwrap();

        migrate(&mut connection).unwrap();

        let migrated: (String, String, i64) = connection
            .query_row(
                "SELECT canonical_path, scope_id, bound_at FROM workspace_bindings",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            migrated,
            (
                "/code/workspace-a".to_owned(),
                "workspace-a".to_owned(),
                1_000
            )
        );
    }
}
