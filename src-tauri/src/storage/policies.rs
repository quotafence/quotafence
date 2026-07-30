use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::domain::{BasisPoints, EnforcementPolicy, ScopeId, UnixMillis, WindowId};

use super::{StorageError, StorageResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePolicy {
    scope_id: ScopeId,
    policy: EnforcementPolicy,
    updated_at: UnixMillis,
}

impl WorkspacePolicy {
    pub fn new(scope_id: ScopeId, policy: EnforcementPolicy, updated_at: UnixMillis) -> Self {
        Self {
            scope_id,
            policy,
            updated_at,
        }
    }

    pub fn scope_id(&self) -> &ScopeId {
        &self.scope_id
    }

    pub fn policy(&self) -> &EnforcementPolicy {
        &self.policy
    }

    pub fn updated_at(&self) -> UnixMillis {
        self.updated_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyOverrideAudit {
    pub session_id: String,
    pub scope_id: ScopeId,
    pub window_id: WindowId,
    pub accepted_at: UnixMillis,
}

pub struct WorkspacePolicyRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> WorkspacePolicyRepository<'connection> {
    pub(crate) fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub fn get(&self, scope_id: &ScopeId) -> StorageResult<Option<WorkspacePolicy>> {
        self.connection
            .query_row(
                "SELECT warn_at_basis_points, confirm_at_basis_points,
                        stop_at_basis_points, updated_at
                 FROM workspace_policies
                 WHERE scope_id = ?1",
                [scope_id.as_str()],
                |row| {
                    Ok((
                        row.get::<_, Option<i64>>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()?
            .map(|row| decode_policy(scope_id.clone(), row))
            .transpose()
    }

    pub fn set(&self, workspace_policy: &WorkspacePolicy) -> StorageResult<()> {
        self.connection.execute(
            "INSERT INTO workspace_policies (
                scope_id, warn_at_basis_points, confirm_at_basis_points,
                stop_at_basis_points, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(scope_id) DO UPDATE SET
                warn_at_basis_points = excluded.warn_at_basis_points,
                confirm_at_basis_points = excluded.confirm_at_basis_points,
                stop_at_basis_points = excluded.stop_at_basis_points,
                updated_at = excluded.updated_at",
            params![
                workspace_policy.scope_id().as_str(),
                workspace_policy
                    .policy()
                    .warn_at()
                    .map(|value| i64::from(value.value())),
                workspace_policy
                    .policy()
                    .confirm_at()
                    .map(|value| i64::from(value.value())),
                workspace_policy
                    .policy()
                    .stop_at()
                    .map(|value| i64::from(value.value())),
                workspace_policy.updated_at().value(),
            ],
        )?;
        Ok(())
    }

    pub fn reset(&self, scope_id: &ScopeId) -> StorageResult<bool> {
        Ok(self.connection.execute(
            "DELETE FROM workspace_policies WHERE scope_id = ?1",
            [scope_id.as_str()],
        )? == 1)
    }

    pub fn list_override_audits(
        &self,
        scope_id: &ScopeId,
    ) -> StorageResult<Vec<PolicyOverrideAudit>> {
        let mut statement = self.connection.prepare(
            "SELECT session_id, window_id, accepted_at
             FROM managed_session_policy_overrides
             WHERE scope_id = ?1
             ORDER BY accepted_at, session_id",
        )?;
        let rows = statement.query_map([scope_id.as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (session_id, window_id, accepted_at) = row?;
            Ok(PolicyOverrideAudit {
                session_id,
                scope_id: scope_id.clone(),
                window_id: WindowId::new(window_id)?,
                accepted_at: UnixMillis::new(accepted_at),
            })
        })
        .collect()
    }
}

pub(crate) fn insert_override_in_transaction(
    transaction: &Transaction<'_>,
    session_id: &str,
    scope_id: &ScopeId,
    window_id: &WindowId,
    accepted_at: UnixMillis,
) -> StorageResult<()> {
    transaction.execute(
        "INSERT INTO managed_session_policy_overrides (
            session_id, scope_id, window_id, decision, accepted_at
         ) VALUES (?1, ?2, ?3, 'require_confirmation', ?4)",
        params![
            session_id,
            scope_id.as_str(),
            window_id.as_str(),
            accepted_at.value()
        ],
    )?;
    Ok(())
}

fn decode_policy(
    scope_id: ScopeId,
    row: (Option<i64>, Option<i64>, Option<i64>, i64),
) -> StorageResult<WorkspacePolicy> {
    let (warn_at, confirm_at, stop_at, updated_at) = row;
    Ok(WorkspacePolicy::new(
        scope_id,
        EnforcementPolicy::new(
            decode_basis_points(warn_at)?,
            decode_basis_points(confirm_at)?,
            decode_basis_points(stop_at)?,
        )?,
        UnixMillis::new(updated_at),
    ))
}

fn decode_basis_points(value: Option<i64>) -> StorageResult<Option<BasisPoints>> {
    value
        .map(|value| {
            u16::try_from(value)
                .map_err(|_| StorageError::InvalidState {
                    message: format!("database contains invalid policy threshold {value}"),
                })
                .and_then(|value| BasisPoints::new(value).map_err(StorageError::from))
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{Scope, ScopeKind},
        storage::Database,
    };

    #[test]
    fn workspace_policy_round_trips_and_can_be_reset() {
        let database = Database::open_in_memory().unwrap();
        let scope_id = ScopeId::new("workspace-a").unwrap();
        database
            .catalog()
            .insert_scope(
                &Scope::new(scope_id.clone(), None, ScopeKind::Workspace, "Workspace A").unwrap(),
            )
            .unwrap();
        let policy = EnforcementPolicy::new(
            Some(BasisPoints::new(7_500).unwrap()),
            Some(BasisPoints::new(9_000).unwrap()),
            None,
        )
        .unwrap();

        database
            .workspace_policies()
            .set(&WorkspacePolicy::new(
                scope_id.clone(),
                policy.clone(),
                UnixMillis::new(1_234),
            ))
            .unwrap();

        let stored = database
            .workspace_policies()
            .get(&scope_id)
            .unwrap()
            .unwrap();
        assert_eq!(stored.policy(), &policy);
        assert_eq!(stored.updated_at().value(), 1_234);
        assert!(database.workspace_policies().reset(&scope_id).unwrap());
        assert_eq!(database.workspace_policies().get(&scope_id).unwrap(), None);
        assert!(!database.workspace_policies().reset(&scope_id).unwrap());
    }
}
