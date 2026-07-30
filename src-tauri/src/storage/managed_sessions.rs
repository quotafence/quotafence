use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::domain::{
    Confidence, QuotaAmount, QuotaUnit, ReservationId, ScopeId, UnixMillis, UsageAttribution,
    UsageEvent, UsageEventId, UsageSource, WindowId,
};

use super::{
    error::{from_sql_integer, to_sql_integer},
    ledger::insert_usage_event,
    StorageError, StorageResult,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedSessionStatus {
    Starting,
    Running,
    Completed,
    Failed,
    Interrupted,
}

impl ManagedSessionStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Interrupted)
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }

    fn from_str(value: &str) -> StorageResult<Self> {
        match value {
            "starting" => Ok(Self::Starting),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "interrupted" => Ok(Self::Interrupted),
            _ => Err(StorageError::InvalidState {
                message: format!("database contains unknown managed session status {value:?}"),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconciliationStatus {
    Pending,
    Reconciled,
    Unavailable,
}

impl ReconciliationStatus {
    fn from_str(value: &str) -> StorageResult<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "reconciled" => Ok(Self::Reconciled),
            "unavailable" => Ok(Self::Unavailable),
            _ => Err(StorageError::InvalidState {
                message: format!("database contains unknown reconciliation status {value:?}"),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewManagedSession {
    pub id: String,
    pub adapter: String,
    pub pool_id: String,
    pub window_id: String,
    pub scope_id: String,
    pub reservation_id: String,
    pub canonical_path: String,
    pub supervisor_pid: u32,
    pub created_at: i64,
    pub baseline_used: u64,
    pub baseline_observed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedSession {
    pub id: String,
    pub adapter: String,
    pub pool_id: String,
    pub window_id: String,
    pub scope_id: String,
    pub reservation_id: String,
    pub canonical_path: String,
    pub status: ManagedSessionStatus,
    pub reconciliation_status: ReconciliationStatus,
    pub supervisor_pid: u32,
    pub child_pid: Option<u32>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub exit_code: Option<i32>,
    pub baseline_used: Option<u64>,
    pub baseline_observed_at: Option<i64>,
    pub contended: bool,
    pub reconciled_amount: Option<u64>,
    pub reconciled_at: Option<i64>,
    pub reconciliation_outcome: Option<ManagedSessionReconciliationOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagedSessionReconciliationResult {
    Attributed { amount: u64, scope_id: ScopeId },
    NoUsage,
    Ambiguous { amount: u64 },
    WindowRolledOver,
    SnapshotUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedSessionReconciliationOutcome {
    Attributed,
    NoUsage,
    Ambiguous,
    WindowRolledOver,
    SnapshotUnavailable,
}

impl ManagedSessionReconciliationOutcome {
    fn as_str(self) -> &'static str {
        match self {
            Self::Attributed => "attributed",
            Self::NoUsage => "no_usage",
            Self::Ambiguous => "ambiguous",
            Self::WindowRolledOver => "window_rolled_over",
            Self::SnapshotUnavailable => "snapshot_unavailable",
        }
    }

    fn from_str(value: &str) -> StorageResult<Self> {
        match value {
            "attributed" => Ok(Self::Attributed),
            "no_usage" => Ok(Self::NoUsage),
            "ambiguous" => Ok(Self::Ambiguous),
            "window_rolled_over" => Ok(Self::WindowRolledOver),
            "snapshot_unavailable" => Ok(Self::SnapshotUnavailable),
            _ => Err(StorageError::InvalidState {
                message: format!(
                    "database contains unknown managed reconciliation outcome {value:?}"
                ),
            }),
        }
    }
}

pub struct ManagedSessionRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> ManagedSessionRepository<'connection> {
    pub(crate) fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub fn get(&self, id: &str) -> StorageResult<Option<ManagedSession>> {
        get(self.connection, id)
    }

    pub fn list_active(&self) -> StorageResult<Vec<ManagedSession>> {
        let mut statement = self.connection.prepare(
            "SELECT id, adapter, pool_id, window_id, scope_id, reservation_id,
                    canonical_path, status, reconciliation_status, supervisor_pid,
                    child_pid, created_at, started_at, finished_at, exit_code,
                    baseline_used, baseline_observed_at, contended,
                    reconciled_amount, reconciled_at, reconciliation_outcome
             FROM managed_sessions
             WHERE status IN ('starting', 'running')
             ORDER BY created_at, id",
        )?;
        let rows = statement.query_map([], decode_row)?;
        rows.map(|row| row.map_err(StorageError::from))
            .collect::<StorageResult<Vec<_>>>()?
            .into_iter()
            .map(validate_row)
            .collect()
    }

    pub fn list_reconciled_for_window(
        &self,
        window_id: &WindowId,
    ) -> StorageResult<Vec<ManagedSession>> {
        let mut statement = self.connection.prepare(
            "SELECT id, adapter, pool_id, window_id, scope_id, reservation_id,
                    canonical_path, status, reconciliation_status, supervisor_pid,
                    child_pid, created_at, started_at, finished_at, exit_code,
                    baseline_used, baseline_observed_at, contended,
                    reconciled_amount, reconciled_at, reconciliation_outcome
             FROM managed_sessions
             WHERE window_id = ?1
               AND status IN ('completed', 'failed', 'interrupted')
               AND reconciliation_status = 'reconciled'
               AND reconciliation_outcome IN ('attributed', 'no_usage', 'ambiguous')
             ORDER BY reconciled_at, id",
        )?;
        let rows = statement.query_map([window_id.as_str()], decode_row)?;
        rows.map(|row| row.map_err(StorageError::from))
            .collect::<StorageResult<Vec<_>>>()?
            .into_iter()
            .map(validate_row)
            .collect()
    }
}

pub(crate) fn insert_starting(
    transaction: &Transaction<'_>,
    session: &NewManagedSession,
) -> StorageResult<()> {
    let active: Option<String> = transaction
        .query_row(
            "SELECT id FROM managed_sessions
             WHERE pool_id = ?1 AND status IN ('starting', 'running')
             LIMIT 1",
            [&session.pool_id],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(active) = active {
        return Err(StorageError::InvalidState {
            message: format!(
                "managed session {active} is already active for pool {}",
                session.pool_id
            ),
        });
    }

    let contended = transaction.query_row(
        "SELECT EXISTS (
             SELECT 1 FROM provider_turn_observations
             WHERE adapter = ?1 AND window_id = ?2
         )",
        params![session.adapter, session.window_id],
        |row| row.get::<_, bool>(0),
    )?;
    transaction.execute(
        "INSERT INTO managed_sessions (
            id, adapter, pool_id, window_id, scope_id, reservation_id,
            canonical_path, status, reconciliation_status, supervisor_pid, created_at,
            baseline_used, baseline_observed_at, contended
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, 'starting', 'pending', ?8, ?9, ?10, ?11, ?12
         )",
        params![
            session.id,
            session.adapter,
            session.pool_id,
            session.window_id,
            session.scope_id,
            session.reservation_id,
            session.canonical_path,
            i64::from(session.supervisor_pid),
            session.created_at,
            to_sql_integer(session.baseline_used, "managed session baseline usage")?,
            session.baseline_observed_at,
            contended,
        ],
    )?;
    Ok(())
}

pub(crate) fn mark_running(
    connection: &Connection,
    id: &str,
    child_pid: u32,
    started_at: i64,
) -> StorageResult<()> {
    let updated = connection.execute(
        "UPDATE managed_sessions
         SET status = 'running', child_pid = ?2, started_at = ?3
         WHERE id = ?1 AND status = 'starting'",
        params![id, i64::from(child_pid), started_at],
    )?;
    ensure_single_transition(updated, id, "starting", "running")
}

pub(crate) fn finish_and_reconcile(
    transaction: &Transaction<'_>,
    id: &str,
    status: ManagedSessionStatus,
    finished_at: i64,
    exit_code: Option<i32>,
    current_window_id: Option<&WindowId>,
) -> StorageResult<ManagedSessionReconciliationResult> {
    if !status.is_terminal() {
        return Err(StorageError::InvalidState {
            message: "managed session can only finish in a terminal state".to_owned(),
        });
    }
    if let Some(existing) = get(transaction, id)? {
        if existing.status.is_terminal() {
            return persisted_reconciliation(&existing);
        }
    } else {
        return Err(StorageError::NotFound {
            entity: "managed session",
            id: id.to_owned(),
        });
    }
    let active: Option<(String, String, String, Option<i64>, bool)> = transaction
        .query_row(
            "SELECT reservation_id, window_id, scope_id, baseline_used, contended
             FROM managed_sessions
             WHERE id = ?1 AND status IN ('starting', 'running')",
            [id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()?;
    let (reservation_id, baseline_window_id, scope_id, baseline_used, contended) = active
        .ok_or_else(|| StorageError::InvalidState {
            message: format!("managed session {id} does not exist or is already terminal"),
        })?;

    let baseline_window_id = WindowId::new(baseline_window_id)?;
    let scope_id = ScopeId::new(scope_id)?;
    let (
        reconciliation,
        reconciliation_status,
        reconciliation_outcome,
        reconciled_amount,
        reservation_status,
    ) = reconcile_usage(
        transaction,
        id,
        &baseline_window_id,
        &scope_id,
        baseline_used,
        contended,
        current_window_id,
        UnixMillis::new(finished_at),
    )?;

    let updated = transaction.execute(
        "UPDATE managed_sessions
         SET status = ?2,
             finished_at = ?3,
             exit_code = ?4,
             reconciliation_status = ?5,
             reconciled_amount = ?6,
             reconciled_at = ?3,
             reconciliation_outcome = ?7
         WHERE id = ?1 AND status IN ('starting', 'running')",
        params![
            id,
            status.as_str(),
            finished_at,
            exit_code,
            reconciliation_status,
            reconciled_amount
                .map(|amount| to_sql_integer(amount, "managed session reconciled usage"))
                .transpose()?,
            reconciliation_outcome.as_str(),
        ],
    )?;
    ensure_single_transition(updated, id, "active", status.as_str())?;

    let released = transaction.execute(
        "UPDATE reservations SET status = ?2
         WHERE id = ?1 AND status = 'active'",
        params![reservation_id, reservation_status],
    )?;
    if released != 1 {
        return Err(StorageError::InvalidState {
            message: format!(
                "managed session {id} does not have an active reservation to reconcile"
            ),
        });
    }
    Ok(reconciliation)
}

#[allow(clippy::too_many_arguments)]
fn reconcile_usage(
    transaction: &Transaction<'_>,
    session_id: &str,
    baseline_window_id: &WindowId,
    scope_id: &ScopeId,
    baseline_used: Option<i64>,
    contended: bool,
    current_window_id: Option<&WindowId>,
    observed_at: UnixMillis,
) -> StorageResult<(
    ManagedSessionReconciliationResult,
    &'static str,
    ManagedSessionReconciliationOutcome,
    Option<u64>,
    &'static str,
)> {
    let Some(current_window_id) = current_window_id else {
        return Ok((
            ManagedSessionReconciliationResult::SnapshotUnavailable,
            "unavailable",
            ManagedSessionReconciliationOutcome::SnapshotUnavailable,
            None,
            "released",
        ));
    };
    if current_window_id != baseline_window_id {
        return Ok((
            ManagedSessionReconciliationResult::WindowRolledOver,
            "unavailable",
            ManagedSessionReconciliationOutcome::WindowRolledOver,
            None,
            "released",
        ));
    }
    let Some(baseline_used) = baseline_used else {
        return Ok((
            ManagedSessionReconciliationResult::SnapshotUnavailable,
            "unavailable",
            ManagedSessionReconciliationOutcome::SnapshotUnavailable,
            None,
            "released",
        ));
    };
    let baseline_used = from_sql_integer(baseline_used, "managed session baseline usage")?;
    let snapshot_used = transaction
        .query_row(
            "SELECT used FROM provider_quota_snapshots WHERE window_id = ?1",
            [current_window_id.as_str()],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let Some(snapshot_used) = snapshot_used else {
        return Ok((
            ManagedSessionReconciliationResult::SnapshotUnavailable,
            "unavailable",
            ManagedSessionReconciliationOutcome::SnapshotUnavailable,
            None,
            "released",
        ));
    };
    let snapshot_used = from_sql_integer(snapshot_used, "provider snapshot usage")?;
    let Some(amount) = snapshot_used.checked_sub(baseline_used) else {
        return Ok((
            ManagedSessionReconciliationResult::SnapshotUnavailable,
            "unavailable",
            ManagedSessionReconciliationOutcome::SnapshotUnavailable,
            None,
            "released",
        ));
    };
    if amount == 0 {
        return Ok((
            ManagedSessionReconciliationResult::NoUsage,
            "reconciled",
            ManagedSessionReconciliationOutcome::NoUsage,
            Some(0),
            "released",
        ));
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
        UsageEventId::new(format!("managed-session:{session_id}"))?,
        current_window_id.clone(),
        if contended {
            UsageAttribution::Unattributed
        } else {
            UsageAttribution::Scope(scope_id.clone())
        },
        QuotaAmount::new(amount, QuotaUnit::new(unit)?),
        observed_at,
        UsageSource::ProviderObserved,
        Confidence::Observed,
    )?;
    insert_usage_event(transaction, &event)?;

    if contended {
        Ok((
            ManagedSessionReconciliationResult::Ambiguous { amount },
            "reconciled",
            ManagedSessionReconciliationOutcome::Ambiguous,
            Some(amount),
            "released",
        ))
    } else {
        Ok((
            ManagedSessionReconciliationResult::Attributed {
                amount,
                scope_id: scope_id.clone(),
            },
            "reconciled",
            ManagedSessionReconciliationOutcome::Attributed,
            Some(amount),
            "consumed",
        ))
    }
}

fn persisted_reconciliation(
    session: &ManagedSession,
) -> StorageResult<ManagedSessionReconciliationResult> {
    match session.reconciliation_outcome {
        Some(ManagedSessionReconciliationOutcome::Attributed) => {
            let amount = positive_reconciled_amount(session)?;
            Ok(ManagedSessionReconciliationResult::Attributed {
                amount,
                scope_id: ScopeId::new(session.scope_id.clone())?,
            })
        }
        Some(ManagedSessionReconciliationOutcome::NoUsage) => {
            if session.reconciled_amount != Some(0) {
                return Err(invalid_persisted_reconciliation(session));
            }
            Ok(ManagedSessionReconciliationResult::NoUsage)
        }
        Some(ManagedSessionReconciliationOutcome::Ambiguous) => {
            Ok(ManagedSessionReconciliationResult::Ambiguous {
                amount: positive_reconciled_amount(session)?,
            })
        }
        Some(ManagedSessionReconciliationOutcome::WindowRolledOver) => {
            Ok(ManagedSessionReconciliationResult::WindowRolledOver)
        }
        Some(ManagedSessionReconciliationOutcome::SnapshotUnavailable) => {
            Ok(ManagedSessionReconciliationResult::SnapshotUnavailable)
        }
        None => Err(invalid_persisted_reconciliation(session)),
    }
}

fn positive_reconciled_amount(session: &ManagedSession) -> StorageResult<u64> {
    session
        .reconciled_amount
        .filter(|amount| *amount > 0)
        .ok_or_else(|| invalid_persisted_reconciliation(session))
}

fn invalid_persisted_reconciliation(session: &ManagedSession) -> StorageError {
    StorageError::InvalidState {
        message: format!(
            "managed session {} has incomplete persisted reconciliation state",
            session.id
        ),
    }
}

fn get(connection: &Connection, id: &str) -> StorageResult<Option<ManagedSession>> {
    connection
        .query_row(
            "SELECT id, adapter, pool_id, window_id, scope_id, reservation_id,
                    canonical_path, status, reconciliation_status, supervisor_pid,
                    child_pid, created_at, started_at, finished_at, exit_code,
                    baseline_used, baseline_observed_at, contended,
                    reconciled_amount, reconciled_at, reconciliation_outcome
             FROM managed_sessions WHERE id = ?1",
            [id],
            decode_row,
        )
        .optional()?
        .map(validate_row)
        .transpose()
}

type ManagedSessionRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    i64,
    Option<i64>,
    i64,
    Option<i64>,
    Option<i64>,
    Option<i32>,
    Option<i64>,
    Option<i64>,
    bool,
    Option<i64>,
    Option<i64>,
    Option<String>,
);

fn decode_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ManagedSessionRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
        row.get(12)?,
        row.get(13)?,
        row.get(14)?,
        row.get(15)?,
        row.get(16)?,
        row.get(17)?,
        row.get(18)?,
        row.get(19)?,
        row.get(20)?,
    ))
}

fn validate_row(row: ManagedSessionRow) -> StorageResult<ManagedSession> {
    let (
        id,
        adapter,
        pool_id,
        window_id,
        scope_id,
        reservation_id,
        canonical_path,
        status,
        reconciliation_status,
        supervisor_pid,
        child_pid,
        created_at,
        started_at,
        finished_at,
        exit_code,
        baseline_used,
        baseline_observed_at,
        contended,
        reconciled_amount,
        reconciled_at,
        reconciliation_outcome,
    ) = row;
    let supervisor_pid = u32::try_from(supervisor_pid).map_err(|_| StorageError::InvalidState {
        message: format!("managed session {id} has invalid supervisor PID {supervisor_pid}"),
    })?;
    let child_pid = child_pid
        .map(|pid| {
            u32::try_from(pid).map_err(|_| StorageError::InvalidState {
                message: format!("managed session {id} has invalid child PID {pid}"),
            })
        })
        .transpose()?;
    let baseline_used = baseline_used
        .map(|value| from_sql_integer(value, "managed session baseline usage"))
        .transpose()?;
    let reconciled_amount = reconciled_amount
        .map(|value| from_sql_integer(value, "managed session reconciled usage"))
        .transpose()?;
    let reconciliation_outcome = reconciliation_outcome
        .map(|value| ManagedSessionReconciliationOutcome::from_str(&value))
        .transpose()?;

    ScopeId::new(scope_id.clone())?;
    WindowId::new(window_id.clone())?;
    ReservationId::new(reservation_id.clone())?;

    Ok(ManagedSession {
        id,
        adapter,
        pool_id,
        window_id,
        scope_id,
        reservation_id,
        canonical_path,
        status: ManagedSessionStatus::from_str(&status)?,
        reconciliation_status: ReconciliationStatus::from_str(&reconciliation_status)?,
        supervisor_pid,
        child_pid,
        created_at,
        started_at,
        finished_at,
        exit_code,
        baseline_used,
        baseline_observed_at,
        contended,
        reconciled_amount,
        reconciled_at,
        reconciliation_outcome,
    })
}

fn ensure_single_transition(updated: usize, id: &str, from: &str, to: &str) -> StorageResult<()> {
    if updated == 1 {
        return Ok(());
    }
    Err(StorageError::InvalidState {
        message: format!("managed session {id} cannot transition from {from} to {to}"),
    })
}
