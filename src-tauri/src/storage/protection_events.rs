use rusqlite::{params, Connection};

use super::StorageResult;

const RETAINED_EVENT_COUNT: u64 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexProtectionEventOutcome {
    Allowed,
    Blocked,
}

impl CodexProtectionEventOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::Blocked => "blocked",
        }
    }

    fn parse(value: &str) -> StorageResult<Self> {
        match value {
            "allowed" => Ok(Self::Allowed),
            "blocked" => Ok(Self::Blocked),
            other => Err(super::StorageError::InvalidState {
                message: format!(
                    "database contains an unsupported Codex protection event outcome: {other}"
                ),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCodexProtectionEvent {
    pub session_id: String,
    pub turn_id: String,
    pub canonical_path: String,
    pub scope_id: Option<String>,
    pub outcome: CodexProtectionEventOutcome,
    pub reason: String,
    pub occurred_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexProtectionEvent {
    pub canonical_path: String,
    pub scope_id: Option<String>,
    pub workspace_name: Option<String>,
    pub outcome: CodexProtectionEventOutcome,
    pub reason: String,
    pub occurred_at: i64,
}

pub struct CodexProtectionEventRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> CodexProtectionEventRepository<'connection> {
    pub(crate) fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub fn record(&self, event: &NewCodexProtectionEvent) -> StorageResult<()> {
        self.connection.execute(
            "INSERT OR IGNORE INTO codex_protection_events (
                session_id,
                turn_id,
                canonical_path,
                scope_id,
                outcome,
                reason,
                occurred_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                event.session_id,
                event.turn_id,
                event.canonical_path,
                event.scope_id,
                event.outcome.as_str(),
                event.reason,
                event.occurred_at,
            ],
        )?;
        self.connection.execute(
            "DELETE FROM codex_protection_events
             WHERE rowid NOT IN (
                SELECT rowid
                FROM codex_protection_events
                ORDER BY occurred_at DESC
                LIMIT ?1
             )",
            [i64::try_from(RETAINED_EVENT_COUNT).unwrap_or(i64::MAX)],
        )?;
        Ok(())
    }

    pub fn list_recent(&self, limit: u64) -> StorageResult<Vec<CodexProtectionEvent>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut statement = self.connection.prepare(
            "SELECT
                event.canonical_path,
                event.scope_id,
                scope.display_name,
                event.outcome,
                event.reason,
                event.occurred_at
             FROM codex_protection_events event
             LEFT JOIN scopes scope ON scope.id = event.scope_id
             ORDER BY event.occurred_at DESC
             LIMIT ?1",
        )?;
        let rows = statement.query_map([limit], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?;
        rows.map(|row| {
            let (canonical_path, scope_id, workspace_name, outcome, reason, occurred_at) = row?;
            Ok(CodexProtectionEvent {
                canonical_path,
                scope_id,
                workspace_name,
                outcome: CodexProtectionEventOutcome::parse(&outcome)?,
                reason,
                occurred_at,
            })
        })
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{ScopeId, UnixMillis},
        storage::{test_support::seeded_database, WorkspaceBinding},
    };

    #[test]
    fn recent_events_are_ordered_and_duplicate_hook_delivery_is_idempotent() {
        let database = seeded_database();
        let scope_id = ScopeId::new("feature-a").unwrap();
        database
            .workspace_bindings()
            .insert(&WorkspaceBinding::new(
                "/code/a".to_owned(),
                scope_id,
                UnixMillis::new(1_000),
            ))
            .unwrap();
        let repository = database.codex_protection_events();
        let first = NewCodexProtectionEvent {
            session_id: "session-1".to_owned(),
            turn_id: "turn-1".to_owned(),
            canonical_path: "/code/a".to_owned(),
            scope_id: Some("feature-a".to_owned()),
            outcome: CodexProtectionEventOutcome::Allowed,
            reason: "Prompt admitted.".to_owned(),
            occurred_at: 2_000,
        };
        repository.record(&first).unwrap();
        repository.record(&first).unwrap();
        repository
            .record(&NewCodexProtectionEvent {
                session_id: "session-2".to_owned(),
                turn_id: "turn-2".to_owned(),
                canonical_path: "/code/unmapped".to_owned(),
                scope_id: None,
                outcome: CodexProtectionEventOutcome::Blocked,
                reason: "No allocation.".to_owned(),
                occurred_at: 3_000,
            })
            .unwrap();

        let events = repository.list_recent(5).unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].outcome, CodexProtectionEventOutcome::Blocked);
        assert_eq!(events[1].workspace_name.as_deref(), Some("Feature A"));
    }
}
