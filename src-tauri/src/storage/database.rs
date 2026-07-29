use std::{path::Path, time::Duration};

use rusqlite::Connection;

use super::{migrations, AllocationRepository, CatalogRepository, LedgerRepository, StorageResult};

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

    pub fn schema_version(&self) -> StorageResult<i64> {
        Ok(self.connection.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{
            Confidence, UnixMillis, UsageAttribution, UsageEvent, UsageEventId, UsageSource,
            WindowId,
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
}
