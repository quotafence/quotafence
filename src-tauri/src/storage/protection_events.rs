use rusqlite::{params, Connection, OptionalExtension};

use super::StorageResult;

const RETAINED_EVENT_COUNT: u64 = 100;
const RETAINED_RECEIPT_COUNT: u64 = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCodexDesktopConfirmation {
    pub id: String,
    pub session_id: String,
    pub turn_id: String,
    pub scope_id: String,
    pub window_id: String,
    pub canonical_path: String,
    pub requested_at: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexDesktopConfirmation {
    pub id: String,
    pub workspace_name: String,
    pub canonical_path: String,
    pub requested_at: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexHookReceiptStatus {
    Received,
    Decision,
    Skipped,
    Failed,
}

impl CodexHookReceiptStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Received => "received",
            Self::Decision => "decision",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
        }
    }

    fn parse(value: &str) -> StorageResult<Self> {
        match value {
            "received" => Ok(Self::Received),
            "decision" => Ok(Self::Decision),
            "skipped" => Ok(Self::Skipped),
            "failed" => Ok(Self::Failed),
            other => Err(super::StorageError::InvalidState {
                message: format!(
                    "database contains an unsupported Codex hook receipt status: {other}"
                ),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexHookReceipt {
    pub session_id: String,
    pub turn_id: String,
    pub event_name: String,
    pub canonical_path: String,
    pub status: CodexHookReceiptStatus,
    pub issue: Option<String>,
    pub occurred_at: i64,
}

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

    pub fn last_bound_workspace_for_session(
        &self,
        session_id: &str,
    ) -> StorageResult<Option<String>> {
        self.connection
            .query_row(
                "SELECT event.canonical_path
                 FROM codex_protection_events event
                 WHERE event.session_id = ?1
                   AND event.scope_id IS NOT NULL
                 ORDER BY event.occurred_at DESC
                 LIMIT 1",
                [session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn record_hook_received(&self, receipt: &CodexHookReceipt) -> StorageResult<()> {
        self.connection.execute(
            "INSERT INTO codex_hook_receipts (
                session_id, turn_id, event_name, canonical_path, status, issue, occurred_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(session_id, turn_id, event_name) DO UPDATE SET
                canonical_path = excluded.canonical_path,
                status = excluded.status,
                issue = excluded.issue,
                occurred_at = excluded.occurred_at",
            params![
                receipt.session_id,
                receipt.turn_id,
                receipt.event_name,
                receipt.canonical_path,
                receipt.status.as_str(),
                receipt.issue,
                receipt.occurred_at,
            ],
        )?;
        self.connection.execute(
            "DELETE FROM codex_hook_receipts
             WHERE rowid NOT IN (
                SELECT rowid
                FROM codex_hook_receipts
                ORDER BY occurred_at DESC
                LIMIT ?1
             )",
            [i64::try_from(RETAINED_RECEIPT_COUNT).unwrap_or(i64::MAX)],
        )?;
        Ok(())
    }

    pub fn update_hook_receipt(
        &self,
        session_id: &str,
        turn_id: &str,
        event_name: &str,
        status: CodexHookReceiptStatus,
        issue: Option<&str>,
    ) -> StorageResult<()> {
        self.connection.execute(
            "UPDATE codex_hook_receipts
             SET status = ?4, issue = ?5
             WHERE session_id = ?1 AND turn_id = ?2 AND event_name = ?3",
            params![session_id, turn_id, event_name, status.as_str(), issue],
        )?;
        Ok(())
    }

    pub fn latest_hook_receipt(&self, event_name: &str) -> StorageResult<Option<CodexHookReceipt>> {
        let mut statement = self.connection.prepare(
            "SELECT session_id, turn_id, event_name, canonical_path, status, issue, occurred_at
             FROM codex_hook_receipts
             WHERE event_name = ?1
             ORDER BY occurred_at DESC
             LIMIT 1",
        )?;
        let receipt = statement
            .query_row([event_name], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            })
            .optional()?;
        receipt
            .map(
                |(session_id, turn_id, event_name, canonical_path, status, issue, occurred_at)| {
                    Ok(CodexHookReceipt {
                        session_id,
                        turn_id,
                        event_name,
                        canonical_path,
                        status: CodexHookReceiptStatus::parse(&status)?,
                        issue,
                        occurred_at,
                    })
                },
            )
            .transpose()
    }

    pub fn consume_desktop_approval(
        &self,
        session_id: &str,
        scope_id: &str,
        window_id: &str,
        now: i64,
    ) -> StorageResult<bool> {
        let updated = self.connection.execute(
            "UPDATE codex_desktop_confirmations
             SET status = 'consumed', resolved_at = ?4
             WHERE id = (
                SELECT id FROM codex_desktop_confirmations
                WHERE session_id = ?1 AND scope_id = ?2 AND window_id = ?3
                  AND status = 'approved' AND expires_at > ?4
                ORDER BY resolved_at DESC LIMIT 1
             )",
            params![session_id, scope_id, window_id, now],
        )?;
        Ok(updated == 1)
    }

    pub fn request_desktop_confirmation(
        &self,
        request: &NewCodexDesktopConfirmation,
    ) -> StorageResult<()> {
        self.connection.execute(
            "UPDATE codex_desktop_confirmations
             SET status = 'expired', resolved_at = ?1
             WHERE status IN ('pending', 'approved') AND expires_at <= ?1",
            [request.requested_at],
        )?;
        self.connection.execute(
            "INSERT INTO codex_desktop_confirmations (
                id, session_id, turn_id, scope_id, window_id, canonical_path,
                status, requested_at, expires_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                turn_id = excluded.turn_id,
                canonical_path = excluded.canonical_path,
                status = 'pending',
                requested_at = excluded.requested_at,
                resolved_at = NULL,
                expires_at = excluded.expires_at
             WHERE codex_desktop_confirmations.status IN ('denied', 'consumed', 'expired')",
            params![
                request.id,
                request.session_id,
                request.turn_id,
                request.scope_id,
                request.window_id,
                request.canonical_path,
                request.requested_at,
                request.expires_at,
            ],
        )?;
        Ok(())
    }

    pub fn list_pending_desktop_confirmations(
        &self,
        now: i64,
    ) -> StorageResult<Vec<CodexDesktopConfirmation>> {
        self.connection.execute(
            "UPDATE codex_desktop_confirmations
             SET status = 'expired', resolved_at = ?1
             WHERE status = 'pending' AND expires_at <= ?1",
            [now],
        )?;
        let mut statement = self.connection.prepare(
            "SELECT confirmation.id, scope.display_name, confirmation.canonical_path,
                    confirmation.requested_at, confirmation.expires_at
             FROM codex_desktop_confirmations confirmation
             JOIN scopes scope ON scope.id = confirmation.scope_id
             WHERE confirmation.status = 'pending' AND confirmation.expires_at > ?1
             ORDER BY confirmation.requested_at ASC",
        )?;
        let rows = statement.query_map([now], |row| {
            Ok(CodexDesktopConfirmation {
                id: row.get(0)?,
                workspace_name: row.get(1)?,
                canonical_path: row.get(2)?,
                requested_at: row.get(3)?,
                expires_at: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn resolve_desktop_confirmation(
        &self,
        id: &str,
        approved: bool,
        resolved_at: i64,
    ) -> StorageResult<bool> {
        let status = if approved { "approved" } else { "denied" };
        Ok(self.connection.execute(
            "UPDATE codex_desktop_confirmations
             SET status = ?2, resolved_at = ?3
             WHERE id = ?1 AND status = 'pending' AND expires_at > ?3",
            params![id, status, resolved_at],
        )? == 1)
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

    #[test]
    fn hook_receipt_tracks_delivery_separately_from_policy_decisions() {
        let database = seeded_database();
        let repository = database.codex_protection_events();
        let receipt = CodexHookReceipt {
            session_id: "session-1".to_owned(),
            turn_id: "turn-1".to_owned(),
            event_name: "UserPromptSubmit".to_owned(),
            canonical_path: "/code/a".to_owned(),
            status: CodexHookReceiptStatus::Received,
            issue: None,
            occurred_at: 4_000,
        };

        repository.record_hook_received(&receipt).unwrap();
        repository
            .update_hook_receipt(
                "session-1",
                "turn-1",
                "UserPromptSubmit",
                CodexHookReceiptStatus::Skipped,
                Some("Provider refresh unavailable."),
            )
            .unwrap();

        let stored = repository
            .latest_hook_receipt("UserPromptSubmit")
            .unwrap()
            .unwrap();
        assert_eq!(stored.status, CodexHookReceiptStatus::Skipped);
        assert_eq!(
            stored.issue.as_deref(),
            Some("Provider refresh unavailable.")
        );
        assert_eq!(
            database.codex_protection_events().list_recent(5).unwrap(),
            vec![]
        );
    }

    #[test]
    fn desktop_approval_is_scoped_expires_and_is_consumed_once() {
        let database = seeded_database();
        let repository = database.codex_protection_events();
        repository
            .request_desktop_confirmation(&NewCodexDesktopConfirmation {
                id: "confirmation-1".to_owned(),
                session_id: "session-1".to_owned(),
                turn_id: "turn-1".to_owned(),
                scope_id: "feature-a".to_owned(),
                window_id: "week-1".to_owned(),
                canonical_path: "/code/a".to_owned(),
                requested_at: 2_000,
                expires_at: 3_000,
            })
            .unwrap();

        assert_eq!(
            repository
                .list_pending_desktop_confirmations(2_100)
                .unwrap()
                .len(),
            1
        );
        assert!(repository
            .resolve_desktop_confirmation("confirmation-1", true, 2_200)
            .unwrap());
        assert!(!repository
            .consume_desktop_approval("other-session", "feature-a", "week-1", 2_300)
            .unwrap());
        assert!(repository
            .consume_desktop_approval("session-1", "feature-a", "week-1", 2_300)
            .unwrap());
        assert!(!repository
            .consume_desktop_approval("session-1", "feature-a", "week-1", 2_400)
            .unwrap());

        repository
            .request_desktop_confirmation(&NewCodexDesktopConfirmation {
                id: "confirmation-expired".to_owned(),
                session_id: "session-2".to_owned(),
                turn_id: "turn-2".to_owned(),
                scope_id: "feature-a".to_owned(),
                window_id: "week-1".to_owned(),
                canonical_path: "/code/a".to_owned(),
                requested_at: 3_000,
                expires_at: 4_000,
            })
            .unwrap();
        assert!(repository
            .list_pending_desktop_confirmations(4_001)
            .unwrap()
            .is_empty());
        assert!(!repository
            .resolve_desktop_confirmation("confirmation-expired", true, 4_001)
            .unwrap());
    }
}
