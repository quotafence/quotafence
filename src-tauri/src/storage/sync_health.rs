use rusqlite::{params, Connection, OptionalExtension};

use super::StorageResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSyncHealth {
    pub status: String,
    pub message: Option<String>,
    pub checked_at: i64,
}

pub struct ProviderSyncHealthRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> ProviderSyncHealthRepository<'connection> {
    pub(crate) fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub fn put(
        &self,
        window_id: &str,
        status: &str,
        message: Option<&str>,
        checked_at: i64,
    ) -> StorageResult<()> {
        self.connection.execute(
            "INSERT INTO provider_sync_health (window_id, status, message, checked_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(window_id) DO UPDATE SET
                status = excluded.status, message = excluded.message, checked_at = excluded.checked_at",
            params![window_id, status, message, checked_at],
        )?;
        Ok(())
    }

    pub fn get(&self, window_id: &str) -> StorageResult<Option<ProviderSyncHealth>> {
        Ok(self
            .connection
            .query_row(
                "SELECT status, message, checked_at FROM provider_sync_health WHERE window_id = ?1",
                [window_id],
                |row| {
                    Ok(ProviderSyncHealth {
                        status: row.get(0)?,
                        message: row.get(1)?,
                        checked_at: row.get(2)?,
                    })
                },
            )
            .optional()?)
    }
}

#[cfg(test)]
mod tests {
    use crate::storage::test_support::seeded_database;

    #[test]
    fn success_replaces_the_previous_sync_issue() {
        let database = seeded_database();
        let repository = database.provider_sync_health();
        repository
            .put("week-1", "unavailable", Some("offline"), 2_000)
            .unwrap();
        repository.put("week-1", "synced", None, 3_000).unwrap();
        let health = repository.get("week-1").unwrap().unwrap();
        assert_eq!(health.status, "synced");
        assert_eq!(health.message, None);
        assert_eq!(health.checked_at, 3_000);
    }
}
