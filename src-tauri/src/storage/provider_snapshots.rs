use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::domain::{UnixMillis, WindowId};

use super::{
    error::{from_sql_integer, to_sql_integer},
    StorageResult,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderQuotaSnapshot {
    window_id: WindowId,
    adapter: String,
    remote_limit_id: String,
    remote_window_kind: String,
    used: u64,
    observed_at: UnixMillis,
    resets_at: UnixMillis,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderQuotaHistoryPoint {
    pub used: u64,
    pub observed_at: UnixMillis,
}

impl ProviderQuotaSnapshot {
    pub fn new(
        window_id: WindowId,
        adapter: String,
        remote_limit_id: String,
        remote_window_kind: String,
        used: u64,
        observed_at: UnixMillis,
        resets_at: UnixMillis,
    ) -> Self {
        Self {
            window_id,
            adapter,
            remote_limit_id,
            remote_window_kind,
            used,
            observed_at,
            resets_at,
        }
    }

    pub fn window_id(&self) -> &WindowId {
        &self.window_id
    }

    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    pub fn remote_limit_id(&self) -> &str {
        &self.remote_limit_id
    }

    pub fn remote_window_kind(&self) -> &str {
        &self.remote_window_kind
    }

    pub fn used(&self) -> u64 {
        self.used
    }

    pub fn observed_at(&self) -> UnixMillis {
        self.observed_at
    }

    pub fn resets_at(&self) -> UnixMillis {
        self.resets_at
    }
}

pub(crate) fn upsert(
    transaction: &Transaction<'_>,
    snapshot: &ProviderQuotaSnapshot,
) -> StorageResult<()> {
    transaction.execute(
        "INSERT INTO provider_quota_history (window_id, used, observed_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(window_id, observed_at) DO UPDATE SET
             used = excluded.used",
        params![
            snapshot.window_id().as_str(),
            to_sql_integer(snapshot.used(), "provider history usage")?,
            snapshot.observed_at().value(),
        ],
    )?;
    transaction.execute(
        "INSERT INTO provider_quota_snapshots (
             window_id, adapter, remote_limit_id, remote_window_kind,
             used, observed_at, resets_at
         )
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(window_id) DO UPDATE SET
             adapter = excluded.adapter,
             remote_limit_id = excluded.remote_limit_id,
             remote_window_kind = excluded.remote_window_kind,
             used = excluded.used,
             observed_at = excluded.observed_at,
             resets_at = excluded.resets_at",
        params![
            snapshot.window_id().as_str(),
            snapshot.adapter(),
            snapshot.remote_limit_id(),
            snapshot.remote_window_kind(),
            to_sql_integer(snapshot.used(), "provider snapshot usage")?,
            snapshot.observed_at().value(),
            snapshot.resets_at().value(),
        ],
    )?;
    Ok(())
}

pub(crate) fn list_history(
    connection: &Connection,
    window_id: &WindowId,
) -> StorageResult<Vec<ProviderQuotaHistoryPoint>> {
    let mut statement = connection.prepare(
        "SELECT used, observed_at
         FROM provider_quota_history
         WHERE window_id = ?1
         ORDER BY observed_at",
    )?;
    let rows = statement.query_map([window_id.as_str()], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
    })?;

    rows.map(|row| {
        let (used, observed_at) = row?;
        Ok(ProviderQuotaHistoryPoint {
            used: from_sql_integer(used, "provider history usage")?,
            observed_at: UnixMillis::new(observed_at),
        })
    })
    .collect()
}

pub(crate) fn get(
    connection: &Connection,
    window_id: &WindowId,
) -> StorageResult<Option<ProviderQuotaSnapshot>> {
    let row = connection
        .query_row(
            "SELECT adapter, remote_limit_id, remote_window_kind,
                    used, observed_at, resets_at
             FROM provider_quota_snapshots
             WHERE window_id = ?1",
            [window_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        )
        .optional()?;

    row.map(
        |(adapter, remote_limit_id, remote_window_kind, used, observed_at, resets_at)| {
            Ok(ProviderQuotaSnapshot::new(
                window_id.clone(),
                adapter,
                remote_limit_id,
                remote_window_kind,
                from_sql_integer(used, "provider snapshot usage")?,
                UnixMillis::new(observed_at),
                UnixMillis::new(resets_at),
            ))
        },
    )
    .transpose()
}
