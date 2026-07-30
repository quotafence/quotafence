use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::domain::{ScopeId, UnixMillis};

use super::StorageResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceBinding {
    canonical_path: String,
    scope_id: ScopeId,
    bound_at: UnixMillis,
}

impl WorkspaceBinding {
    pub fn new(canonical_path: String, scope_id: ScopeId, bound_at: UnixMillis) -> Self {
        Self {
            canonical_path,
            scope_id,
            bound_at,
        }
    }

    pub fn canonical_path(&self) -> &str {
        &self.canonical_path
    }

    pub fn scope_id(&self) -> &ScopeId {
        &self.scope_id
    }

    pub fn bound_at(&self) -> UnixMillis {
        self.bound_at
    }
}

pub struct WorkspaceBindingRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> WorkspaceBindingRepository<'connection> {
    pub(crate) fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub fn insert(&self, binding: &WorkspaceBinding) -> StorageResult<()> {
        insert_with_connection(self.connection, binding)
    }

    pub fn get_by_path(&self, canonical_path: &str) -> StorageResult<Option<WorkspaceBinding>> {
        let row = self
            .connection
            .query_row(
                "SELECT canonical_path, scope_id, bound_at
                 FROM workspace_bindings
                 WHERE canonical_path = ?1",
                [canonical_path],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?;

        row.map(decode_binding).transpose()
    }

    pub fn list(&self) -> StorageResult<Vec<WorkspaceBinding>> {
        let mut statement = self.connection.prepare(
            "SELECT canonical_path, scope_id, bound_at
             FROM workspace_bindings
             ORDER BY canonical_path",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;

        rows.map(|row| decode_binding(row?)).collect()
    }
}

pub(crate) fn insert_in_transaction(
    transaction: &Transaction<'_>,
    binding: &WorkspaceBinding,
) -> StorageResult<()> {
    insert_with_connection(transaction, binding)
}

fn insert_with_connection(
    connection: &Connection,
    binding: &WorkspaceBinding,
) -> StorageResult<()> {
    connection.execute(
        "INSERT INTO workspace_bindings (canonical_path, scope_id, bound_at)
         VALUES (?1, ?2, ?3)",
        params![
            binding.canonical_path(),
            binding.scope_id().as_str(),
            binding.bound_at().value()
        ],
    )?;
    Ok(())
}

fn decode_binding(
    (canonical_path, scope_id, bound_at): (String, String, i64),
) -> StorageResult<WorkspaceBinding> {
    Ok(WorkspaceBinding::new(
        canonical_path,
        ScopeId::new(scope_id)?,
        UnixMillis::new(bound_at),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{test_support::seeded_database, StorageError};

    #[test]
    fn bindings_round_trip_and_reject_duplicate_paths_or_scopes() {
        let database = seeded_database();
        let bindings = database.workspace_bindings();
        bindings
            .insert(&WorkspaceBinding::new(
                "/code/project-a".to_owned(),
                ScopeId::new("project-a").unwrap(),
                UnixMillis::new(1_000),
            ))
            .unwrap();

        let binding = bindings.get_by_path("/code/project-a").unwrap().unwrap();
        assert_eq!(binding.scope_id().as_str(), "project-a");
        assert_eq!(binding.bound_at().value(), 1_000);
        assert!(matches!(
            bindings.insert(&WorkspaceBinding::new(
                "/code/project-a".to_owned(),
                ScopeId::new("project-b").unwrap(),
                UnixMillis::new(2_000),
            )),
            Err(StorageError::Database(_))
        ));
        assert!(matches!(
            bindings.insert(&WorkspaceBinding::new(
                "/code/another".to_owned(),
                ScopeId::new("project-a").unwrap(),
                UnixMillis::new(2_000),
            )),
            Err(StorageError::Database(_))
        ));
    }
}
