use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{ScopeId, UnixMillis};

use super::StorageResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryBinding {
    canonical_root: String,
    scope_id: ScopeId,
    bound_at: UnixMillis,
}

impl RepositoryBinding {
    pub fn new(canonical_root: String, scope_id: ScopeId, bound_at: UnixMillis) -> Self {
        Self {
            canonical_root,
            scope_id,
            bound_at,
        }
    }

    pub fn canonical_root(&self) -> &str {
        &self.canonical_root
    }

    pub fn scope_id(&self) -> &ScopeId {
        &self.scope_id
    }

    pub fn bound_at(&self) -> UnixMillis {
        self.bound_at
    }
}

pub struct RepositoryBindingRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> RepositoryBindingRepository<'connection> {
    pub(crate) fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub fn insert(&self, binding: &RepositoryBinding) -> StorageResult<()> {
        self.connection.execute(
            "INSERT INTO repository_bindings (canonical_root, scope_id, bound_at)
             VALUES (?1, ?2, ?3)",
            params![
                binding.canonical_root(),
                binding.scope_id().as_str(),
                binding.bound_at().value()
            ],
        )?;
        Ok(())
    }

    pub fn get_by_root(&self, canonical_root: &str) -> StorageResult<Option<RepositoryBinding>> {
        let row = self
            .connection
            .query_row(
                "SELECT canonical_root, scope_id, bound_at
                 FROM repository_bindings
                 WHERE canonical_root = ?1",
                [canonical_root],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?;

        row.map(|(canonical_root, scope_id, bound_at)| {
            Ok(RepositoryBinding::new(
                canonical_root,
                ScopeId::new(scope_id)?,
                UnixMillis::new(bound_at),
            ))
        })
        .transpose()
    }

    pub fn list(&self) -> StorageResult<Vec<RepositoryBinding>> {
        let mut statement = self.connection.prepare(
            "SELECT canonical_root, scope_id, bound_at
             FROM repository_bindings
             ORDER BY canonical_root",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;

        rows.map(|row| {
            let (canonical_root, scope_id, bound_at) = row?;
            Ok(RepositoryBinding::new(
                canonical_root,
                ScopeId::new(scope_id)?,
                UnixMillis::new(bound_at),
            ))
        })
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{test_support::seeded_database, StorageError};

    #[test]
    fn bindings_round_trip_and_reject_duplicate_roots_or_scopes() {
        let database = seeded_database();
        let repository = database.repository_bindings();
        repository
            .insert(&RepositoryBinding::new(
                "/code/project-a".to_owned(),
                ScopeId::new("project-a").unwrap(),
                UnixMillis::new(1_000),
            ))
            .unwrap();

        let binding = repository.get_by_root("/code/project-a").unwrap().unwrap();
        assert_eq!(binding.scope_id().as_str(), "project-a");
        assert_eq!(binding.bound_at().value(), 1_000);
        assert!(matches!(
            repository.insert(&RepositoryBinding::new(
                "/code/project-a".to_owned(),
                ScopeId::new("project-b").unwrap(),
                UnixMillis::new(2_000),
            )),
            Err(StorageError::Database(_))
        ));
        assert!(matches!(
            repository.insert(&RepositoryBinding::new(
                "/code/another".to_owned(),
                ScopeId::new("project-a").unwrap(),
                UnixMillis::new(2_000),
            )),
            Err(StorageError::Database(_))
        ));
    }
}
