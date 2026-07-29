use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};

use crate::domain::{
    QuotaAmount, QuotaUnit, Reservation, ReservationId, ReservationStatus, ScopeId, UnixMillis,
    UsageAttribution, UsageEvent, UsageEventId, WindowId,
};

use super::{
    catalog::ensure_unit,
    codec::{
        confidence_from_str, confidence_to_str, reservation_status_from_str,
        reservation_status_to_str, usage_source_from_str, usage_source_to_str,
    },
    error::{from_sql_integer, to_sql_integer},
    StorageError, StorageResult,
};

pub struct LedgerRepository<'connection> {
    connection: &'connection mut Connection,
}

impl<'connection> LedgerRepository<'connection> {
    pub(crate) fn new(connection: &'connection mut Connection) -> Self {
        Self { connection }
    }

    pub fn reserve(&mut self, reservation: &Reservation, at: UnixMillis) -> StorageResult<()> {
        if reservation.status() != ReservationStatus::Active || !reservation.is_active_at(at) {
            return Err(StorageError::InvalidState {
                message: format!(
                    "reservation {} must be active at the admission timestamp",
                    reservation.id()
                ),
            });
        }

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let requested = to_sql_integer(reservation.amount().value(), "reservation amount")?;
        let lineage = scope_lineage(&transaction, reservation.scope_id())?;

        for budget_scope_id in lineage {
            let (allocation, unit) = allocation_amount_and_unit(
                &transaction,
                &budget_scope_id,
                reservation.window_id(),
            )?;
            ensure_unit(reservation.amount().unit(), &unit)?;

            let usage = attributed_usage_for_scope_tree(
                &transaction,
                &budget_scope_id,
                reservation.window_id(),
            )?;
            let active_reservations = active_reservations_for_scope_tree(
                &transaction,
                &budget_scope_id,
                reservation.window_id(),
                at,
            )?;
            let committed = usage
                .checked_add(active_reservations)
                .ok_or_else(arithmetic_overflow)?;
            let available = allocation.saturating_sub(committed);

            if requested > available {
                return Err(StorageError::InsufficientCapacity {
                    scope_id: budget_scope_id.to_string(),
                    requested: reservation.amount().value(),
                    available: from_sql_integer(available, "available capacity")?,
                    unit: unit.to_string(),
                });
            }
        }

        insert_reservation(&transaction, reservation)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn get_reservation(&self, id: &ReservationId) -> StorageResult<Option<Reservation>> {
        let row = self
            .connection
            .query_row(
                "SELECT r.id, r.scope_id, r.window_id, r.amount, r.created_at, r.expires_at,
                        r.status, p.unit
                 FROM reservations r
                 JOIN quota_windows w ON w.id = r.window_id
                 JOIN quota_pools p ON p.id = w.pool_id
                 WHERE r.id = ?1",
                [id.as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                    ))
                },
            )
            .optional()?;

        row.map(
            |(id, scope_id, window_id, amount, created_at, expires_at, status, unit)| {
                Reservation::restore(
                    ReservationId::new(id)?,
                    ScopeId::new(scope_id)?,
                    WindowId::new(window_id)?,
                    QuotaAmount::new(
                        from_sql_integer(amount, "reservation amount")?,
                        QuotaUnit::new(unit)?,
                    ),
                    UnixMillis::new(created_at),
                    UnixMillis::new(expires_at),
                    reservation_status_from_str(&status)?,
                )
                .map_err(StorageError::from)
            },
        )
        .transpose()
    }

    pub fn release_reservation(&mut self, id: &ReservationId) -> StorageResult<()> {
        let updated = self.connection.execute(
            "UPDATE reservations SET status = 'released'
             WHERE id = ?1 AND status = 'active'",
            [id.as_str()],
        )?;

        if updated != 1 {
            return Err(StorageError::InvalidState {
                message: format!("reservation {id} does not exist or is not active"),
            });
        }

        Ok(())
    }

    pub fn record_usage(&mut self, event: &UsageEvent) -> StorageResult<()> {
        self.record_usage_and_consume(event, None)
    }

    pub fn record_usage_and_consume(
        &mut self,
        event: &UsageEvent,
        reservation_id: Option<&ReservationId>,
    ) -> StorageResult<()> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let unit = window_unit(&transaction, event.window_id())?;
        ensure_unit(event.amount().unit(), &unit)?;
        insert_usage_event(&transaction, event)?;

        if let Some(reservation_id) = reservation_id {
            let UsageAttribution::Scope(scope_id) = event.attribution() else {
                return Err(StorageError::InvalidState {
                    message: "unattributed usage cannot consume a scoped reservation".to_owned(),
                });
            };

            let updated = transaction.execute(
                "UPDATE reservations SET status = 'consumed'
                 WHERE id = ?1
                   AND status = 'active'
                   AND scope_id = ?2
                   AND window_id = ?3",
                params![
                    reservation_id.as_str(),
                    scope_id.as_str(),
                    event.window_id().as_str()
                ],
            )?;

            if updated != 1 {
                return Err(StorageError::InvalidState {
                    message: format!(
                        "reservation {reservation_id} is not active or does not match the usage event"
                    ),
                });
            }
        }

        transaction.commit()?;
        Ok(())
    }

    pub fn list_usage_for_window(&self, window_id: &WindowId) -> StorageResult<Vec<UsageEvent>> {
        let mut statement = self.connection.prepare(
            "SELECT u.id, u.scope_id, u.amount, u.observed_at, u.source, u.confidence, p.unit
             FROM usage_events u
             JOIN quota_windows w ON w.id = u.window_id
             JOIN quota_pools p ON p.id = w.pool_id
             WHERE u.window_id = ?1
             ORDER BY u.observed_at, u.id",
        )?;
        let rows = statement.query_map([window_id.as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?;

        rows.map(|row| {
            let (id, scope_id, amount, observed_at, source, confidence, unit) = row?;
            UsageEvent::new(
                UsageEventId::new(id)?,
                window_id.clone(),
                match scope_id {
                    Some(scope_id) => UsageAttribution::Scope(ScopeId::new(scope_id)?),
                    None => UsageAttribution::Unattributed,
                },
                QuotaAmount::new(
                    from_sql_integer(amount, "usage amount")?,
                    QuotaUnit::new(unit)?,
                ),
                UnixMillis::new(observed_at),
                usage_source_from_str(&source)?,
                confidence_from_str(&confidence)?,
            )
            .map_err(StorageError::from)
        })
        .collect()
    }
}

fn allocation_amount_and_unit(
    transaction: &Transaction<'_>,
    scope_id: &ScopeId,
    window_id: &WindowId,
) -> StorageResult<(i64, QuotaUnit)> {
    let row = transaction
        .query_row(
            "SELECT a.amount, p.unit
             FROM allocations a
             JOIN quota_windows w ON w.id = a.window_id
             JOIN quota_pools p ON p.id = w.pool_id
             WHERE a.scope_id = ?1 AND a.window_id = ?2",
            params![scope_id.as_str(), window_id.as_str()],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .ok_or_else(|| StorageError::NotFound {
            entity: "allocation",
            id: format!("{scope_id}/{window_id}"),
        })?;

    Ok((row.0, QuotaUnit::new(row.1)?))
}

fn scope_lineage(transaction: &Transaction<'_>, scope_id: &ScopeId) -> StorageResult<Vec<ScopeId>> {
    let mut statement = transaction.prepare(
        "WITH RECURSIVE lineage(id, parent_id) AS (
             SELECT id, parent_id FROM scopes WHERE id = ?1
             UNION ALL
             SELECT s.id, s.parent_id
             FROM scopes s
             JOIN lineage l ON s.id = l.parent_id
         )
         SELECT id FROM lineage",
    )?;
    let rows = statement.query_map([scope_id.as_str()], |row| row.get::<_, String>(0))?;
    let lineage: Vec<_> = rows
        .map(|row| Ok(ScopeId::new(row?)?))
        .collect::<StorageResult<_>>()?;

    if lineage.is_empty() {
        return Err(StorageError::NotFound {
            entity: "scope",
            id: scope_id.to_string(),
        });
    }

    Ok(lineage)
}

fn attributed_usage_for_scope_tree(
    transaction: &Transaction<'_>,
    scope_id: &ScopeId,
    window_id: &WindowId,
) -> StorageResult<i64> {
    Ok(transaction.query_row(
        "WITH RECURSIVE descendants(id) AS (
             SELECT id FROM scopes WHERE id = ?1
             UNION ALL
             SELECT s.id FROM scopes s JOIN descendants d ON s.parent_id = d.id
         )
         SELECT COALESCE(SUM(u.amount), 0)
         FROM usage_events u
         JOIN descendants d ON d.id = u.scope_id
         WHERE u.window_id = ?2",
        params![scope_id.as_str(), window_id.as_str()],
        |row| row.get(0),
    )?)
}

fn active_reservations_for_scope_tree(
    transaction: &Transaction<'_>,
    scope_id: &ScopeId,
    window_id: &WindowId,
    at: UnixMillis,
) -> StorageResult<i64> {
    Ok(transaction.query_row(
        "WITH RECURSIVE descendants(id) AS (
             SELECT id FROM scopes WHERE id = ?1
             UNION ALL
             SELECT s.id FROM scopes s JOIN descendants d ON s.parent_id = d.id
         )
         SELECT COALESCE(SUM(r.amount), 0)
         FROM reservations r
         JOIN descendants d ON d.id = r.scope_id
         WHERE r.window_id = ?2
           AND r.status = 'active'
           AND r.created_at <= ?3
           AND r.expires_at > ?3",
        params![scope_id.as_str(), window_id.as_str(), at.value()],
        |row| row.get(0),
    )?)
}

fn window_unit(transaction: &Transaction<'_>, window_id: &WindowId) -> StorageResult<QuotaUnit> {
    let unit = transaction
        .query_row(
            "SELECT p.unit
             FROM quota_windows w
             JOIN quota_pools p ON p.id = w.pool_id
             WHERE w.id = ?1",
            [window_id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| StorageError::NotFound {
            entity: "quota window",
            id: window_id.to_string(),
        })?;

    Ok(QuotaUnit::new(unit)?)
}

fn insert_reservation(
    transaction: &Transaction<'_>,
    reservation: &Reservation,
) -> StorageResult<()> {
    transaction.execute(
        "INSERT INTO reservations
         (id, scope_id, window_id, amount, created_at, expires_at, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            reservation.id().as_str(),
            reservation.scope_id().as_str(),
            reservation.window_id().as_str(),
            to_sql_integer(reservation.amount().value(), "reservation amount")?,
            reservation.created_at().value(),
            reservation.expires_at().value(),
            reservation_status_to_str(reservation.status())
        ],
    )?;
    Ok(())
}

fn insert_usage_event(transaction: &Transaction<'_>, event: &UsageEvent) -> StorageResult<()> {
    let scope_id = match event.attribution() {
        UsageAttribution::Scope(scope_id) => Some(scope_id.as_str()),
        UsageAttribution::Unattributed => None,
    };

    transaction.execute(
        "INSERT INTO usage_events
         (id, window_id, scope_id, amount, observed_at, source, confidence)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            event.id().as_str(),
            event.window_id().as_str(),
            scope_id,
            to_sql_integer(event.amount().value(), "usage amount")?,
            event.observed_at().value(),
            usage_source_to_str(event.source()),
            confidence_to_str(event.confidence())
        ],
    )?;
    Ok(())
}

fn arithmetic_overflow() -> StorageError {
    crate::domain::DomainError::ArithmeticOverflow.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{Confidence, UsageSource},
        storage::test_support::{allocation, points, seeded_database},
    };

    fn reservation(id: &str, scope_id: &str, value: u64) -> Reservation {
        Reservation::new(
            ReservationId::new(id).unwrap(),
            ScopeId::new(scope_id).unwrap(),
            WindowId::new("week-1").unwrap(),
            points(value),
            UnixMillis::new(2_000),
            UnixMillis::new(8_000),
        )
        .unwrap()
    }

    fn usage(id: &str, scope_id: &str, value: u64) -> UsageEvent {
        UsageEvent::new(
            UsageEventId::new(id).unwrap(),
            WindowId::new("week-1").unwrap(),
            UsageAttribution::Scope(ScopeId::new(scope_id).unwrap()),
            points(value),
            UnixMillis::new(3_000),
            UsageSource::LocalMeasured,
            Confidence::Observed,
        )
        .unwrap()
    }

    #[test]
    fn reservations_cannot_oversubscribe_an_allocation() {
        let mut database = seeded_database();
        database
            .allocations()
            .set(&allocation("project-a", 100))
            .unwrap();
        let mut ledger = database.ledger();

        ledger
            .reserve(
                &reservation("reservation-1", "project-a", 60),
                UnixMillis::new(2_000),
            )
            .unwrap();
        let error = ledger
            .reserve(
                &reservation("reservation-2", "project-a", 50),
                UnixMillis::new(2_000),
            )
            .unwrap_err();

        assert!(matches!(
            error,
            StorageError::InsufficientCapacity {
                requested: 50,
                available: 40,
                ..
            }
        ));
        assert!(ledger
            .get_reservation(&ReservationId::new("reservation-2").unwrap())
            .unwrap()
            .is_none());
    }

    #[test]
    fn usage_and_reservation_consumption_commit_together() {
        let mut database = seeded_database();
        database
            .allocations()
            .set(&allocation("project-a", 100))
            .unwrap();
        let mut ledger = database.ledger();
        let reservation_id = ReservationId::new("reservation-1").unwrap();

        ledger
            .reserve(
                &reservation("reservation-1", "project-a", 40),
                UnixMillis::new(2_000),
            )
            .unwrap();
        ledger
            .record_usage_and_consume(&usage("usage-1", "project-a", 35), Some(&reservation_id))
            .unwrap();

        let stored_reservation = ledger.get_reservation(&reservation_id).unwrap().unwrap();
        let events = ledger
            .list_usage_for_window(&WindowId::new("week-1").unwrap())
            .unwrap();

        assert_eq!(stored_reservation.status(), ReservationStatus::Consumed);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].amount().value(), 35);
    }

    #[test]
    fn failed_reservation_consumption_rolls_back_the_usage_event() {
        let mut database = seeded_database();
        database
            .allocations()
            .set(&allocation("project-a", 100))
            .unwrap();
        let mut ledger = database.ledger();
        let reservation_id = ReservationId::new("reservation-1").unwrap();

        ledger
            .reserve(
                &reservation("reservation-1", "project-a", 40),
                UnixMillis::new(2_000),
            )
            .unwrap();
        let result = ledger
            .record_usage_and_consume(&usage("usage-1", "project-b", 35), Some(&reservation_id));

        assert!(matches!(result, Err(StorageError::InvalidState { .. })));
        assert!(ledger
            .list_usage_for_window(&WindowId::new("week-1").unwrap())
            .unwrap()
            .is_empty());
        assert_eq!(
            ledger
                .get_reservation(&reservation_id)
                .unwrap()
                .unwrap()
                .status(),
            ReservationStatus::Active
        );
    }

    #[test]
    fn unattributed_usage_round_trips_without_a_scope() {
        let mut database = seeded_database();
        let event = UsageEvent::new(
            UsageEventId::new("external-usage").unwrap(),
            WindowId::new("week-1").unwrap(),
            UsageAttribution::Unattributed,
            points(5),
            UnixMillis::new(4_000),
            UsageSource::ProviderConfirmed,
            Confidence::Confirmed,
        )
        .unwrap();

        database.ledger().record_usage(&event).unwrap();
        let events = database
            .ledger()
            .list_usage_for_window(&WindowId::new("week-1").unwrap())
            .unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].attribution(), &UsageAttribution::Unattributed);
    }

    #[test]
    fn child_reservations_also_consume_parent_capacity() {
        let mut database = seeded_database();
        {
            let mut allocations = database.allocations();
            allocations.set(&allocation("project-a", 70)).unwrap();
            allocations.set(&allocation("feature-a", 60)).unwrap();
        }
        let mut ledger = database.ledger();

        ledger
            .reserve(
                &reservation("project-work", "project-a", 30),
                UnixMillis::new(2_000),
            )
            .unwrap();
        let error = ledger
            .reserve(
                &reservation("feature-work", "feature-a", 50),
                UnixMillis::new(2_000),
            )
            .unwrap_err();

        assert!(matches!(
            error,
            StorageError::InsufficientCapacity {
                scope_id,
                requested: 50,
                available: 40,
                ..
            } if scope_id == "project-a"
        ));
        assert!(ledger
            .get_reservation(&ReservationId::new("feature-work").unwrap())
            .unwrap()
            .is_none());
    }
}
