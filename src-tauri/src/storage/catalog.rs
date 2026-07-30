use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{
    Account, AccountId, Provider, ProviderId, QuotaAmount, QuotaPool, QuotaPoolId, QuotaUnit,
    QuotaWindow, Scope, ScopeId, UnixMillis, WindowId,
};

use super::{
    codec::{scope_kind_from_str, scope_kind_to_str},
    error::{from_sql_integer, to_sql_integer},
    StorageError, StorageResult,
};

pub struct CatalogRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> CatalogRepository<'connection> {
    pub(crate) fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub fn insert_provider(&self, provider: &Provider) -> StorageResult<()> {
        self.connection.execute(
            "INSERT INTO providers (id, display_name) VALUES (?1, ?2)",
            params![provider.id().as_str(), provider.display_name()],
        )?;
        Ok(())
    }

    pub fn get_provider(&self, id: &ProviderId) -> StorageResult<Option<Provider>> {
        let row = self
            .connection
            .query_row(
                "SELECT id, display_name FROM providers WHERE id = ?1",
                [id.as_str()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;

        row.map(|(id, display_name)| {
            Provider::new(ProviderId::new(id)?, display_name).map_err(StorageError::from)
        })
        .transpose()
    }

    pub fn list_providers(&self) -> StorageResult<Vec<Provider>> {
        let mut statement = self
            .connection
            .prepare("SELECT id, display_name FROM providers ORDER BY display_name, id")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        rows.map(|row| {
            let (id, display_name) = row?;
            Provider::new(ProviderId::new(id)?, display_name).map_err(StorageError::from)
        })
        .collect()
    }

    pub fn insert_account(&self, account: &Account) -> StorageResult<()> {
        self.connection.execute(
            "INSERT INTO accounts (id, provider_id, display_name) VALUES (?1, ?2, ?3)",
            params![
                account.id().as_str(),
                account.provider_id().as_str(),
                account.display_name()
            ],
        )?;
        Ok(())
    }

    pub fn get_account(&self, id: &AccountId) -> StorageResult<Option<Account>> {
        let row = self
            .connection
            .query_row(
                "SELECT id, provider_id, display_name FROM accounts WHERE id = ?1",
                [id.as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;

        row.map(|(id, provider_id, display_name)| {
            Account::new(
                AccountId::new(id)?,
                ProviderId::new(provider_id)?,
                display_name,
            )
            .map_err(StorageError::from)
        })
        .transpose()
    }

    pub fn list_accounts(&self) -> StorageResult<Vec<Account>> {
        let mut statement = self.connection.prepare(
            "SELECT id, provider_id, display_name
             FROM accounts
             ORDER BY display_name, id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;

        rows.map(|row| {
            let (id, provider_id, display_name) = row?;
            Account::new(
                AccountId::new(id)?,
                ProviderId::new(provider_id)?,
                display_name,
            )
            .map_err(StorageError::from)
        })
        .collect()
    }

    pub fn insert_quota_pool(&self, pool: &QuotaPool) -> StorageResult<()> {
        self.connection.execute(
            "INSERT INTO quota_pools (id, account_id, display_name, unit)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                pool.id().as_str(),
                pool.account_id().as_str(),
                pool.display_name(),
                pool.unit().as_str()
            ],
        )?;
        Ok(())
    }

    pub fn get_quota_pool(&self, id: &QuotaPoolId) -> StorageResult<Option<QuotaPool>> {
        let row = self
            .connection
            .query_row(
                "SELECT id, account_id, display_name, unit FROM quota_pools WHERE id = ?1",
                [id.as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?;

        row.map(|(id, account_id, display_name, unit)| {
            QuotaPool::new(
                QuotaPoolId::new(id)?,
                AccountId::new(account_id)?,
                display_name,
                QuotaUnit::new(unit)?,
            )
            .map_err(StorageError::from)
        })
        .transpose()
    }

    pub fn list_quota_pools(&self) -> StorageResult<Vec<QuotaPool>> {
        let mut statement = self.connection.prepare(
            "SELECT id, account_id, display_name, unit
             FROM quota_pools
             ORDER BY display_name, id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;

        rows.map(|row| {
            let (id, account_id, display_name, unit) = row?;
            QuotaPool::new(
                QuotaPoolId::new(id)?,
                AccountId::new(account_id)?,
                display_name,
                QuotaUnit::new(unit)?,
            )
            .map_err(StorageError::from)
        })
        .collect()
    }

    pub fn insert_quota_window(&self, window: &QuotaWindow) -> StorageResult<()> {
        let expected_unit = self.pool_unit(window.pool_id())?;
        ensure_unit(window.capacity().unit(), &expected_unit)?;

        self.connection.execute(
            "INSERT INTO quota_windows (id, pool_id, starts_at, ends_at, capacity)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                window.id().as_str(),
                window.pool_id().as_str(),
                window.starts_at().value(),
                window.ends_at().value(),
                to_sql_integer(window.capacity().value(), "window capacity")?
            ],
        )?;
        Ok(())
    }

    pub fn get_quota_window(&self, id: &WindowId) -> StorageResult<Option<QuotaWindow>> {
        let row = self
            .connection
            .query_row(
                "SELECT w.id, w.pool_id, w.starts_at, w.ends_at, w.capacity, p.unit
                 FROM quota_windows w
                 JOIN quota_pools p ON p.id = w.pool_id
                 WHERE w.id = ?1",
                [id.as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()?;

        row.map(|(id, pool_id, starts_at, ends_at, capacity, unit)| {
            QuotaWindow::new(
                WindowId::new(id)?,
                QuotaPoolId::new(pool_id)?,
                UnixMillis::new(starts_at),
                UnixMillis::new(ends_at),
                QuotaAmount::new(
                    from_sql_integer(capacity, "window capacity")?,
                    QuotaUnit::new(unit)?,
                ),
            )
            .map_err(StorageError::from)
        })
        .transpose()
    }

    pub fn list_quota_windows(&self) -> StorageResult<Vec<QuotaWindow>> {
        let mut statement = self.connection.prepare(
            "SELECT w.id, w.pool_id, w.starts_at, w.ends_at, w.capacity, p.unit
             FROM quota_windows w
             JOIN quota_pools p ON p.id = w.pool_id
             ORDER BY w.ends_at DESC, w.starts_at DESC, w.id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;

        rows.map(|row| {
            let (id, pool_id, starts_at, ends_at, capacity, unit) = row?;
            QuotaWindow::new(
                WindowId::new(id)?,
                QuotaPoolId::new(pool_id)?,
                UnixMillis::new(starts_at),
                UnixMillis::new(ends_at),
                QuotaAmount::new(
                    from_sql_integer(capacity, "window capacity")?,
                    QuotaUnit::new(unit)?,
                ),
            )
            .map_err(StorageError::from)
        })
        .collect()
    }

    pub fn insert_scope(&self, scope: &Scope) -> StorageResult<()> {
        self.connection.execute(
            "INSERT INTO scopes (id, parent_id, kind, display_name) VALUES (?1, ?2, ?3, ?4)",
            params![
                scope.id().as_str(),
                scope.parent_id().map(ScopeId::as_str),
                scope_kind_to_str(scope.kind()),
                scope.display_name()
            ],
        )?;
        Ok(())
    }

    pub fn get_scope(&self, id: &ScopeId) -> StorageResult<Option<Scope>> {
        let row = self
            .connection
            .query_row(
                "SELECT id, parent_id, kind, display_name FROM scopes WHERE id = ?1",
                [id.as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?;

        row.map(|(id, parent_id, kind, display_name)| {
            Scope::new(
                ScopeId::new(id)?,
                parent_id.map(ScopeId::new).transpose()?,
                scope_kind_from_str(&kind)?,
                display_name,
            )
            .map_err(StorageError::from)
        })
        .transpose()
    }

    pub fn list_scopes(&self) -> StorageResult<Vec<Scope>> {
        let mut statement = self.connection.prepare(
            "SELECT id, parent_id, kind, display_name
             FROM scopes
             ORDER BY parent_id IS NOT NULL, parent_id, display_name, id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;

        rows.map(|row| {
            let (id, parent_id, kind, display_name) = row?;
            Scope::new(
                ScopeId::new(id)?,
                parent_id.map(ScopeId::new).transpose()?,
                scope_kind_from_str(&kind)?,
                display_name,
            )
            .map_err(StorageError::from)
        })
        .collect()
    }

    fn pool_unit(&self, id: &QuotaPoolId) -> StorageResult<QuotaUnit> {
        let unit = self
            .connection
            .query_row(
                "SELECT unit FROM quota_pools WHERE id = ?1",
                [id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| StorageError::NotFound {
                entity: "quota pool",
                id: id.to_string(),
            })?;

        Ok(QuotaUnit::new(unit)?)
    }
}

pub(crate) fn ensure_unit(actual: &QuotaUnit, expected: &QuotaUnit) -> StorageResult<()> {
    if actual != expected {
        return Err(crate::domain::DomainError::UnitMismatch {
            expected: expected.to_string(),
            actual: actual.to_string(),
        }
        .into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ScopeKind;
    use crate::storage::test_support::seeded_database;

    #[test]
    fn catalog_entities_round_trip_through_domain_constructors() {
        let database = seeded_database();
        let catalog = database.catalog();

        let provider = catalog
            .get_provider(&ProviderId::new("codex").unwrap())
            .unwrap()
            .unwrap();
        let account = catalog
            .get_account(&AccountId::new("codex-default").unwrap())
            .unwrap()
            .unwrap();
        let pool = catalog
            .get_quota_pool(&QuotaPoolId::new("codex-weekly").unwrap())
            .unwrap()
            .unwrap();
        let window = catalog
            .get_quota_window(&WindowId::new("week-1").unwrap())
            .unwrap()
            .unwrap();

        assert_eq!(provider.display_name(), "Codex");
        assert_eq!(account.provider_id().as_str(), "codex");
        assert_eq!(pool.unit().as_str(), "quota_points");
        assert_eq!(window.capacity().value(), 100);
        assert_eq!(catalog.list_providers().unwrap().len(), 1);
        assert_eq!(catalog.list_accounts().unwrap().len(), 1);
        assert_eq!(catalog.list_quota_pools().unwrap().len(), 1);
        assert_eq!(catalog.list_quota_windows().unwrap().len(), 1);
    }

    #[test]
    fn scope_hierarchy_and_kind_round_trip() {
        let database = seeded_database();
        let catalog = database.catalog();

        let scope = catalog
            .get_scope(&ScopeId::new("feature-a").unwrap())
            .unwrap()
            .unwrap();

        assert_eq!(scope.parent_id().unwrap().as_str(), "project-a");
        assert_eq!(scope.kind(), ScopeKind::Task);
        assert_eq!(catalog.list_scopes().unwrap().len(), 3);
    }
}
