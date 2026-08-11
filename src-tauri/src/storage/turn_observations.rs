use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};

use crate::domain::{
    Confidence, QuotaAmount, QuotaUnit, ScopeId, UnixMillis, UsageAttribution, UsageEvent,
    UsageEventId, UsageSource, WindowId,
};

use super::{error::from_sql_integer, ledger::insert_usage_event, StorageError, StorageResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderTurnObservation {
    session_id: String,
    turn_id: String,
    adapter: String,
    canonical_path: String,
    scope_id: Option<ScopeId>,
    window_id: WindowId,
    baseline_used: u64,
    started_at: UnixMillis,
    contended: bool,
}

impl ProviderTurnObservation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        session_id: String,
        turn_id: String,
        adapter: String,
        canonical_path: String,
        scope_id: Option<ScopeId>,
        window_id: WindowId,
        baseline_used: u64,
        started_at: UnixMillis,
    ) -> Self {
        Self {
            session_id,
            turn_id,
            adapter,
            canonical_path,
            scope_id,
            window_id,
            baseline_used,
            started_at,
            contended: false,
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn turn_id(&self) -> &str {
        &self.turn_id
    }

    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    pub fn canonical_path(&self) -> &str {
        &self.canonical_path
    }

    pub fn scope_id(&self) -> Option<&ScopeId> {
        self.scope_id.as_ref()
    }

    pub fn window_id(&self) -> &WindowId {
        &self.window_id
    }

    pub fn baseline_used(&self) -> u64 {
        self.baseline_used
    }

    pub fn started_at(&self) -> UnixMillis {
        self.started_at
    }

    pub fn contended(&self) -> bool {
        self.contended
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeginObservationStatus {
    Started,
    AlreadyStarted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BeginObservationResult {
    pub status: BeginObservationStatus,
    pub contended: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconcileObservationResult {
    Attributed {
        amount: u64,
        scope_id: ScopeId,
        window_id: WindowId,
    },
    NoUsage,
    Ambiguous,
    Unmapped,
    WindowRolledOver,
    SnapshotUnavailable,
    Missing,
}

pub struct TurnObservationRepository<'connection> {
    connection: &'connection mut Connection,
}

impl<'connection> TurnObservationRepository<'connection> {
    pub(crate) fn new(connection: &'connection mut Connection) -> Self {
        Self { connection }
    }

    pub fn begin(
        &mut self,
        observation: &ProviderTurnObservation,
        stale_before: UnixMillis,
    ) -> StorageResult<BeginObservationResult> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let stale_overlap_count: i64 = transaction.query_row(
            "SELECT COUNT(*)
             FROM provider_turn_observations
             WHERE adapter = ?1 AND window_id = ?2 AND started_at < ?3",
            params![
                observation.adapter(),
                observation.window_id().as_str(),
                stale_before.value()
            ],
            |row| row.get(0),
        )?;
        transaction.execute(
            "DELETE FROM provider_turn_observations WHERE started_at < ?1",
            [stale_before.value()],
        )?;

        if let Some(existing) = get_with_connection(
            &transaction,
            observation.session_id(),
            observation.turn_id(),
        )? {
            transaction.commit()?;
            return Ok(BeginObservationResult {
                status: BeginObservationStatus::AlreadyStarted,
                contended: existing.contended(),
            });
        }

        let active_count: i64 = transaction.query_row(
            "SELECT COUNT(*)
             FROM provider_turn_observations
             WHERE adapter = ?1 AND window_id = ?2",
            params![observation.adapter(), observation.window_id().as_str()],
            |row| row.get(0),
        )?;
        let active_managed_count: i64 = transaction.query_row(
            "SELECT COUNT(*)
             FROM managed_sessions
             WHERE adapter = ?1
               AND window_id = ?2
               AND status IN ('starting', 'running')",
            params![observation.adapter(), observation.window_id().as_str()],
            |row| row.get(0),
        )?;
        let contended = active_count > 0 || active_managed_count > 0 || stale_overlap_count > 0;
        if active_count > 0 {
            transaction.execute(
                "UPDATE provider_turn_observations
                 SET contended = 1
                 WHERE adapter = ?1 AND window_id = ?2",
                params![observation.adapter(), observation.window_id().as_str()],
            )?;
        }
        if active_managed_count > 0 {
            transaction.execute(
                "UPDATE managed_sessions
                 SET contended = 1
                 WHERE adapter = ?1
                   AND window_id = ?2
                   AND status IN ('starting', 'running')",
                params![observation.adapter(), observation.window_id().as_str()],
            )?;
        }

        transaction.execute(
            "INSERT INTO provider_turn_observations (
                 session_id, turn_id, adapter, canonical_path, scope_id,
                 window_id, baseline_used, started_at, contended
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                observation.session_id(),
                observation.turn_id(),
                observation.adapter(),
                observation.canonical_path(),
                observation.scope_id().map(ScopeId::as_str),
                observation.window_id().as_str(),
                i64::try_from(observation.baseline_used()).map_err(|_| {
                    StorageError::NumericOutOfRange {
                        field: "turn baseline usage",
                        value: observation.baseline_used(),
                    }
                })?,
                observation.started_at().value(),
                contended
            ],
        )?;
        transaction.commit()?;

        Ok(BeginObservationResult {
            status: BeginObservationStatus::Started,
            contended,
        })
    }

    pub fn get(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> StorageResult<Option<ProviderTurnObservation>> {
        get_with_connection(self.connection, session_id, turn_id)
    }

    pub fn latest_for_session_except(
        &self,
        session_id: &str,
        excluded_turn_id: &str,
    ) -> StorageResult<Option<ProviderTurnObservation>> {
        let turn_id = self
            .connection
            .query_row(
                "SELECT turn_id
                 FROM provider_turn_observations
                 WHERE session_id = ?1 AND turn_id <> ?2
                 ORDER BY started_at DESC
                 LIMIT 1",
                params![session_id, excluded_turn_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        turn_id
            .map(|turn_id| get_with_connection(self.connection, session_id, &turn_id))
            .transpose()
            .map(Option::flatten)
    }

    pub fn abandon(&mut self, session_id: &str, turn_id: &str) -> StorageResult<bool> {
        Ok(self.connection.execute(
            "DELETE FROM provider_turn_observations
             WHERE session_id = ?1 AND turn_id = ?2",
            params![session_id, turn_id],
        )? == 1)
    }

    pub fn abandon_session(&mut self, session_id: &str) -> StorageResult<usize> {
        Ok(self.connection.execute(
            "DELETE FROM provider_turn_observations WHERE session_id = ?1",
            [session_id],
        )?)
    }

    pub fn reconcile(
        &mut self,
        session_id: &str,
        turn_id: &str,
        current_window_id: &WindowId,
        usage_event_id: UsageEventId,
        observed_at: UnixMillis,
    ) -> StorageResult<ReconcileObservationResult> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let Some(observation) = get_with_connection(&transaction, session_id, turn_id)? else {
            transaction.commit()?;
            return Ok(ReconcileObservationResult::Missing);
        };

        transaction.execute(
            "DELETE FROM provider_turn_observations
             WHERE session_id = ?1 AND turn_id = ?2",
            params![session_id, turn_id],
        )?;

        let result = reconcile_in_transaction(
            &transaction,
            observation,
            current_window_id,
            usage_event_id,
            observed_at,
        )?;
        transaction.commit()?;
        Ok(result)
    }
}

fn reconcile_in_transaction(
    transaction: &Transaction<'_>,
    observation: ProviderTurnObservation,
    current_window_id: &WindowId,
    usage_event_id: UsageEventId,
    observed_at: UnixMillis,
) -> StorageResult<ReconcileObservationResult> {
    if observation.contended() {
        return Ok(ReconcileObservationResult::Ambiguous);
    }
    let Some(scope_id) = observation.scope_id().cloned() else {
        return Ok(ReconcileObservationResult::Unmapped);
    };
    if observation.window_id() != current_window_id {
        return Ok(ReconcileObservationResult::WindowRolledOver);
    }

    let snapshot_used = transaction
        .query_row(
            "SELECT used
             FROM provider_quota_snapshots
             WHERE window_id = ?1",
            [current_window_id.as_str()],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let Some(snapshot_used) = snapshot_used else {
        return Ok(ReconcileObservationResult::SnapshotUnavailable);
    };
    let snapshot_used = from_sql_integer(snapshot_used, "provider snapshot usage")?;
    let Some(amount) = snapshot_used.checked_sub(observation.baseline_used()) else {
        return Ok(ReconcileObservationResult::Ambiguous);
    };
    if amount == 0 {
        return Ok(ReconcileObservationResult::NoUsage);
    }

    let unit = transaction
        .query_row(
            "SELECT p.unit
             FROM quota_windows w
             JOIN quota_pools p ON p.id = w.pool_id
             WHERE w.id = ?1",
            [current_window_id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| StorageError::NotFound {
            entity: "quota window",
            id: current_window_id.to_string(),
        })?;
    let event = UsageEvent::new(
        usage_event_id,
        current_window_id.clone(),
        UsageAttribution::Scope(scope_id.clone()),
        QuotaAmount::new(amount, QuotaUnit::new(unit)?),
        observed_at,
        UsageSource::ProviderObserved,
        Confidence::Inferred,
    )?;
    insert_usage_event(transaction, &event)?;

    Ok(ReconcileObservationResult::Attributed {
        amount,
        scope_id,
        window_id: current_window_id.clone(),
    })
}

fn get_with_connection(
    connection: &Connection,
    session_id: &str,
    turn_id: &str,
) -> StorageResult<Option<ProviderTurnObservation>> {
    let row = connection
        .query_row(
            "SELECT adapter, canonical_path, scope_id, window_id,
                    baseline_used, started_at, contended
             FROM provider_turn_observations
             WHERE session_id = ?1 AND turn_id = ?2",
            params![session_id, turn_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, bool>(6)?,
                ))
            },
        )
        .optional()?;

    row.map(
        |(adapter, canonical_path, scope_id, window_id, baseline_used, started_at, contended)| {
            Ok(ProviderTurnObservation {
                session_id: session_id.to_owned(),
                turn_id: turn_id.to_owned(),
                adapter,
                canonical_path,
                scope_id: scope_id.map(ScopeId::new).transpose()?,
                window_id: WindowId::new(window_id)?,
                baseline_used: from_sql_integer(baseline_used, "turn baseline usage")?,
                started_at: UnixMillis::new(started_at),
                contended,
            })
        },
    )
    .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{
        test_support::{allocation, points, seeded_database},
        ProviderQuotaSnapshot,
    };

    fn observation(session: &str, turn: &str, scope: Option<&str>) -> ProviderTurnObservation {
        ProviderTurnObservation::new(
            session.to_owned(),
            turn.to_owned(),
            "codex_app_server".to_owned(),
            "/code/project-a".to_owned(),
            scope.map(|value| ScopeId::new(value).unwrap()),
            WindowId::new("week-1").unwrap(),
            10,
            UnixMillis::new(2_000),
        )
    }

    fn set_snapshot(database: &mut super::super::Database, used: u64) {
        database
            .sync_provider_quota(
                &WindowId::new("week-1").unwrap(),
                &crate::domain::QuotaWindow::new(
                    WindowId::new("week-1").unwrap(),
                    crate::domain::QuotaPoolId::new("codex-weekly").unwrap(),
                    UnixMillis::new(1_000),
                    UnixMillis::new(10_000),
                    points(100),
                )
                .unwrap(),
                &ProviderQuotaSnapshot::new(
                    WindowId::new("week-1").unwrap(),
                    "codex_app_server".to_owned(),
                    "codex".to_owned(),
                    "secondary".to_owned(),
                    used,
                    UnixMillis::new(3_000),
                    UnixMillis::new(10_000),
                ),
                None,
            )
            .unwrap();
    }

    #[test]
    fn a_single_turn_reconciles_provider_delta_to_its_scope() {
        let mut database = seeded_database();
        database
            .allocations()
            .set(&allocation("project-a", 100))
            .unwrap();
        set_snapshot(&mut database, 10);
        database
            .turn_observations()
            .begin(
                &observation("session-1", "turn-1", Some("project-a")),
                UnixMillis::new(0),
            )
            .unwrap();
        set_snapshot(&mut database, 14);

        let result = database
            .turn_observations()
            .reconcile(
                "session-1",
                "turn-1",
                &WindowId::new("week-1").unwrap(),
                UsageEventId::new("codex-turn-session-1-turn-1").unwrap(),
                UnixMillis::new(4_000),
            )
            .unwrap();

        assert!(matches!(
            result,
            ReconcileObservationResult::Attributed { amount: 4, .. }
        ));
        let events = database
            .ledger()
            .list_usage_for_window(&WindowId::new("week-1").unwrap())
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].amount().value(), 4);
        assert_eq!(events[0].confidence(), Confidence::Inferred);
    }

    #[test]
    fn overlapping_turns_are_both_kept_unattributed() {
        let mut database = seeded_database();
        set_snapshot(&mut database, 10);
        let first = database
            .turn_observations()
            .begin(
                &observation("session-1", "turn-1", Some("project-a")),
                UnixMillis::new(0),
            )
            .unwrap();
        let second = database
            .turn_observations()
            .begin(
                &observation("session-2", "turn-2", Some("project-b")),
                UnixMillis::new(0),
            )
            .unwrap();
        assert!(!first.contended);
        assert!(second.contended);
        set_snapshot(&mut database, 15);

        for (session, turn) in [("session-1", "turn-1"), ("session-2", "turn-2")] {
            assert_eq!(
                database
                    .turn_observations()
                    .reconcile(
                        session,
                        turn,
                        &WindowId::new("week-1").unwrap(),
                        UsageEventId::new(format!("usage-{turn}")).unwrap(),
                        UnixMillis::new(4_000),
                    )
                    .unwrap(),
                ReconcileObservationResult::Ambiguous
            );
        }
        assert!(database
            .ledger()
            .list_usage_for_window(&WindowId::new("week-1").unwrap())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn duplicate_begin_does_not_reset_the_baseline() {
        let mut database = seeded_database();
        set_snapshot(&mut database, 10);
        let first = database
            .turn_observations()
            .begin(
                &observation("session-1", "turn-1", Some("project-a")),
                UnixMillis::new(0),
            )
            .unwrap();
        let duplicate = database
            .turn_observations()
            .begin(
                &observation("session-1", "turn-1", Some("project-a")),
                UnixMillis::new(0),
            )
            .unwrap();

        assert_eq!(first.status, BeginObservationStatus::Started);
        assert_eq!(duplicate.status, BeginObservationStatus::AlreadyStarted);
        assert_eq!(
            database
                .turn_observations()
                .get("session-1", "turn-1")
                .unwrap()
                .unwrap()
                .baseline_used(),
            10
        );
    }

    #[test]
    fn rollover_never_attributes_across_windows() {
        let mut database = seeded_database();
        set_snapshot(&mut database, 10);
        database
            .turn_observations()
            .begin(
                &observation("session-1", "turn-1", Some("project-a")),
                UnixMillis::new(0),
            )
            .unwrap();

        assert_eq!(
            database
                .turn_observations()
                .reconcile(
                    "session-1",
                    "turn-1",
                    &WindowId::new("week-2").unwrap(),
                    UsageEventId::new("usage-rollover").unwrap(),
                    UnixMillis::new(11_000),
                )
                .unwrap(),
            ReconcileObservationResult::WindowRolledOver
        );
    }

    #[test]
    fn stale_recovery_keeps_the_next_turn_conservative() {
        let mut database = seeded_database();
        set_snapshot(&mut database, 10);
        database
            .turn_observations()
            .begin(
                &observation("stale-session", "stale-turn", Some("project-a")),
                UnixMillis::new(0),
            )
            .unwrap();
        let next = ProviderTurnObservation::new(
            "session-2".to_owned(),
            "turn-2".to_owned(),
            "codex_app_server".to_owned(),
            "/code/project-a".to_owned(),
            Some(ScopeId::new("project-a").unwrap()),
            WindowId::new("week-1").unwrap(),
            10,
            UnixMillis::new(50_000),
        );

        let result = database
            .turn_observations()
            .begin(&next, UnixMillis::new(40_000))
            .unwrap();

        assert!(result.contended);
        assert!(database
            .turn_observations()
            .get("stale-session", "stale-turn")
            .unwrap()
            .is_none());
    }
}
