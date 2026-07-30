use std::{path::Path, time::Duration};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::domain::{Account, Allocation, Provider, QuotaPool, QuotaWindow, Scope, WindowId};

use super::{
    allocations::set_in_transaction, ledger::reserve_in_transaction, managed_sessions, migrations,
    policies, provider_snapshots,
    workspace_bindings::insert_in_transaction as insert_workspace_binding_in_transaction,
    AllocationRepository, CatalogRepository, LedgerRepository, ManagedSessionReconciliationResult,
    ManagedSessionRepository, ManagedSessionStatus, NewManagedSession, ProviderQuotaSnapshot,
    StorageResult, WorkspaceBinding,
};

pub struct Database {
    connection: Connection,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> StorageResult<Self> {
        let connection = Connection::open(path)?;
        Self::initialize(connection, true)
    }

    pub fn open_in_memory() -> StorageResult<Self> {
        let connection = Connection::open_in_memory()?;
        Self::initialize(connection, false)
    }

    fn initialize(mut connection: Connection, use_wal: bool) -> StorageResult<Self> {
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.pragma_update(None, "foreign_keys", true)?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;

        if use_wal {
            connection.pragma_update(None, "journal_mode", "WAL")?;
        }

        migrations::migrate(&mut connection)?;
        Ok(Self { connection })
    }

    pub fn catalog(&self) -> CatalogRepository<'_> {
        CatalogRepository::new(&self.connection)
    }

    pub fn allocations(&mut self) -> AllocationRepository<'_> {
        AllocationRepository::new(&mut self.connection)
    }

    pub fn ledger(&mut self) -> LedgerRepository<'_> {
        LedgerRepository::new(&mut self.connection)
    }

    pub fn workspace_bindings(&self) -> super::WorkspaceBindingRepository<'_> {
        super::WorkspaceBindingRepository::new(&self.connection)
    }

    pub fn turn_observations(&mut self) -> super::TurnObservationRepository<'_> {
        super::TurnObservationRepository::new(&mut self.connection)
    }

    pub fn managed_sessions(&self) -> ManagedSessionRepository<'_> {
        ManagedSessionRepository::new(&self.connection)
    }

    pub fn workspace_policies(&self) -> super::WorkspacePolicyRepository<'_> {
        super::WorkspacePolicyRepository::new(&self.connection)
    }

    pub fn schema_version(&self) -> StorageResult<i64> {
        Ok(self.connection.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?)
    }

    pub fn insert_quota_source(
        &mut self,
        provider: &Provider,
        account: &Account,
        pool: &QuotaPool,
        window: &QuotaWindow,
    ) -> StorageResult<()> {
        self.insert_quota_source_with_snapshot(provider, account, pool, window, None)
    }

    pub fn insert_allocated_workspace(
        &mut self,
        scope: &crate::domain::Scope,
        allocation: &Allocation,
        binding: &WorkspaceBinding,
    ) -> StorageResult<()> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        CatalogRepository::new(&transaction).insert_scope(scope)?;
        set_in_transaction(&transaction, allocation)?;
        insert_workspace_binding_in_transaction(&transaction, binding)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn start_managed_session(
        &mut self,
        reservation: &crate::domain::Reservation,
        session: &NewManagedSession,
        admitted_at: crate::domain::UnixMillis,
        confirmation_override_accepted: bool,
    ) -> StorageResult<()> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        reserve_in_transaction(&transaction, reservation, admitted_at)?;
        managed_sessions::insert_starting(&transaction, session)?;
        if confirmation_override_accepted {
            policies::insert_override_in_transaction(
                &transaction,
                &session.id,
                reservation.scope_id(),
                reservation.window_id(),
                admitted_at,
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn mark_managed_session_running(
        &self,
        id: &str,
        child_pid: u32,
        started_at: i64,
    ) -> StorageResult<()> {
        managed_sessions::mark_running(&self.connection, id, child_pid, started_at)
    }

    pub fn finish_managed_session(
        &mut self,
        id: &str,
        status: ManagedSessionStatus,
        finished_at: i64,
        exit_code: Option<i32>,
        current_window_id: Option<&WindowId>,
    ) -> StorageResult<ManagedSessionReconciliationResult> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let result = managed_sessions::finish_and_reconcile(
            &transaction,
            id,
            status,
            finished_at,
            exit_code,
            current_window_id,
        )?;
        transaction.commit()?;
        Ok(result)
    }

    pub fn insert_quota_source_with_snapshot(
        &mut self,
        provider: &Provider,
        account: &Account,
        pool: &QuotaPool,
        window: &QuotaWindow,
        snapshot: Option<&ProviderQuotaSnapshot>,
    ) -> StorageResult<()> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let source_key = snapshot.map(provider_source_key);
        if let (Some(source_key), Some(snapshot)) = (source_key.as_deref(), snapshot) {
            let existing: Option<String> = transaction
                .query_row(
                    "SELECT p.id
                     FROM quota_pools p
                     JOIN quota_windows w ON w.pool_id = p.id
                     JOIN provider_quota_snapshots s ON s.window_id = w.id
                     WHERE p.archived_at IS NULL
                       AND lower(trim(s.adapter)) = lower(trim(?1))
                       AND lower(trim(s.remote_limit_id)) = lower(trim(?2))
                       AND lower(trim(s.remote_window_kind)) = lower(trim(?3))
                    LIMIT 1",
                    params![
                        snapshot.adapter(),
                        snapshot.remote_limit_id(),
                        snapshot.remote_window_kind()
                    ],
                    |row| row.get(0),
                )
                .optional()?;
            if existing.is_some() {
                return Err(super::StorageError::DuplicateSource {
                    source_key: source_key.to_owned(),
                });
            }
        }
        {
            let catalog = CatalogRepository::new(&transaction);
            catalog.insert_provider(provider)?;
            catalog.insert_account(account)?;
            catalog.insert_quota_pool(pool)?;
            if let Some(source_key) = source_key.as_deref() {
                transaction.execute(
                    "UPDATE quota_pools SET source_key = ?2 WHERE id = ?1",
                    params![pool.id().as_str(), source_key],
                )?;
            }
            catalog.insert_quota_window(window)?;
        }
        if let Some(snapshot) = snapshot {
            provider_snapshots::upsert(&transaction, snapshot)?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn provider_quota_snapshot(
        &self,
        window_id: &WindowId,
    ) -> StorageResult<Option<ProviderQuotaSnapshot>> {
        provider_snapshots::get(&self.connection, window_id)
    }

    pub fn sync_provider_quota(
        &mut self,
        current_window_id: &WindowId,
        target_window: &QuotaWindow,
        snapshot: &ProviderQuotaSnapshot,
    ) -> StorageResult<()> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        if target_window.id() != current_window_id {
            if CatalogRepository::new(&transaction)
                .get_quota_window(target_window.id())?
                .is_none()
            {
                CatalogRepository::new(&transaction).insert_quota_window(target_window)?;
            }
            transaction.execute(
                "INSERT OR IGNORE INTO allocations (scope_id, window_id, amount)
                 SELECT scope_id, ?1, amount
                 FROM allocations
                 WHERE window_id = ?2",
                [target_window.id().as_str(), current_window_id.as_str()],
            )?;
        }

        provider_snapshots::upsert(&transaction, snapshot)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn insert_allocated_scope(
        &mut self,
        scope: &Scope,
        allocation: &Allocation,
    ) -> StorageResult<()> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        CatalogRepository::new(&transaction).insert_scope(scope)?;
        set_in_transaction(&transaction, allocation)?;
        transaction.commit()?;
        Ok(())
    }
}

fn provider_source_key(snapshot: &ProviderQuotaSnapshot) -> String {
    format!(
        "{}:{}:{}",
        snapshot.adapter().trim().to_lowercase(),
        snapshot.remote_limit_id().trim().to_lowercase(),
        snapshot.remote_window_kind().trim().to_lowercase()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{
            Account, AccountId, Confidence, Provider, ProviderId, QuotaAmount, QuotaPool,
            QuotaPoolId, QuotaUnit, QuotaWindow, UnixMillis, UsageAttribution, UsageEvent,
            UsageEventId, UsageSource, WindowId,
        },
        storage::test_support::{points, seeded_database},
    };

    #[test]
    fn in_memory_database_applies_all_migrations() {
        let database = Database::open_in_memory().unwrap();

        assert_eq!(
            database.schema_version().unwrap(),
            migrations::latest_version()
        );
    }

    #[test]
    fn migration_is_idempotent_for_an_existing_connection() {
        let mut connection = Connection::open_in_memory().unwrap();

        migrations::migrate(&mut connection).unwrap();
        migrations::migrate(&mut connection).unwrap();

        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, migrations::latest_version());
    }

    #[test]
    fn foreign_keys_are_enabled() {
        let database = Database::open_in_memory().unwrap();
        let enabled: bool = database
            .connection
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .unwrap();

        assert!(enabled);
    }

    #[test]
    fn usage_events_are_immutable_in_the_database() {
        let mut database = seeded_database();
        let event = UsageEvent::new(
            UsageEventId::new("usage-1").unwrap(),
            WindowId::new("week-1").unwrap(),
            UsageAttribution::Unattributed,
            points(5),
            UnixMillis::new(2_000),
            UsageSource::ProviderConfirmed,
            Confidence::Confirmed,
        )
        .unwrap();
        database.ledger().record_usage(&event).unwrap();

        let update = database.connection.execute(
            "UPDATE usage_events SET amount = 10 WHERE id = 'usage-1'",
            [],
        );
        let delete = database
            .connection
            .execute("DELETE FROM usage_events WHERE id = 'usage-1'", []);

        assert!(update.is_err());
        assert!(delete.is_err());
    }

    #[test]
    fn quota_source_insert_rolls_back_the_full_chain_on_conflict() {
        let mut database = Database::open_in_memory().unwrap();
        let catalog = database.catalog();
        catalog
            .insert_provider(
                &Provider::new(ProviderId::new("existing").unwrap(), "Existing").unwrap(),
            )
            .unwrap();
        catalog
            .insert_account(
                &Account::new(
                    AccountId::new("shared-account").unwrap(),
                    ProviderId::new("existing").unwrap(),
                    "Existing account",
                )
                .unwrap(),
            )
            .unwrap();

        let provider = Provider::new(ProviderId::new("codex").unwrap(), "Codex").unwrap();
        let account = Account::new(
            AccountId::new("shared-account").unwrap(),
            provider.id().clone(),
            "Subscription",
        )
        .unwrap();
        let pool = QuotaPool::new(
            QuotaPoolId::new("codex-weekly").unwrap(),
            account.id().clone(),
            "Weekly allowance",
            QuotaUnit::new("percent").unwrap(),
        )
        .unwrap();
        let window = QuotaWindow::new(
            WindowId::new("week-1").unwrap(),
            pool.id().clone(),
            UnixMillis::new(1_000),
            UnixMillis::new(10_000),
            QuotaAmount::new(100, QuotaUnit::new("percent").unwrap()),
        )
        .unwrap();

        assert!(database
            .insert_quota_source(&provider, &account, &pool, &window)
            .is_err());
        assert!(database
            .catalog()
            .get_provider(provider.id())
            .unwrap()
            .is_none());
    }
}
