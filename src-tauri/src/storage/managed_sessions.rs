use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::domain::{ReservationId, ScopeId, WindowId};

use super::{StorageError, StorageResult};

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
                    child_pid, created_at, started_at, finished_at, exit_code
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

    transaction.execute(
        "INSERT INTO managed_sessions (
            id, adapter, pool_id, window_id, scope_id, reservation_id,
            canonical_path, status, reconciliation_status, supervisor_pid, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'starting', 'pending', ?8, ?9)",
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

pub(crate) fn finish_and_release(
    transaction: &Transaction<'_>,
    id: &str,
    status: ManagedSessionStatus,
    finished_at: i64,
    exit_code: Option<i32>,
) -> StorageResult<()> {
    if !status.is_terminal() {
        return Err(StorageError::InvalidState {
            message: "managed session can only finish in a terminal state".to_owned(),
        });
    }
    let reservation_id: Option<String> = transaction
        .query_row(
            "SELECT reservation_id FROM managed_sessions
             WHERE id = ?1 AND status IN ('starting', 'running')",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    let reservation_id = reservation_id.ok_or_else(|| StorageError::InvalidState {
        message: format!("managed session {id} does not exist or is already terminal"),
    })?;

    let updated = transaction.execute(
        "UPDATE managed_sessions
         SET status = ?2, finished_at = ?3, exit_code = ?4
         WHERE id = ?1 AND status IN ('starting', 'running')",
        params![id, status.as_str(), finished_at, exit_code],
    )?;
    ensure_single_transition(updated, id, "active", status.as_str())?;

    let released = transaction.execute(
        "UPDATE reservations SET status = 'released'
         WHERE id = ?1 AND status = 'active'",
        [reservation_id],
    )?;
    if released != 1 {
        return Err(StorageError::InvalidState {
            message: format!("managed session {id} does not have an active reservation to release"),
        });
    }
    Ok(())
}

fn get(connection: &Connection, id: &str) -> StorageResult<Option<ManagedSession>> {
    connection
        .query_row(
            "SELECT id, adapter, pool_id, window_id, scope_id, reservation_id,
                    canonical_path, status, reconciliation_status, supervisor_pid,
                    child_pid, created_at, started_at, finished_at, exit_code
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
