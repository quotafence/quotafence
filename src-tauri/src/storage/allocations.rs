use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};

use crate::domain::{Allocation, DomainError, QuotaAmount, QuotaUnit, ScopeId, WindowId};

use super::{
    catalog::ensure_unit,
    error::{from_sql_integer, to_sql_integer},
    StorageError, StorageResult,
};

pub struct AllocationRepository<'connection> {
    connection: &'connection mut Connection,
}

impl<'connection> AllocationRepository<'connection> {
    pub(crate) fn new(connection: &'connection mut Connection) -> Self {
        Self { connection }
    }

    pub fn set(&mut self, allocation: &Allocation) -> StorageResult<()> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        set_in_transaction(&transaction, allocation)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn get(
        &self,
        scope_id: &ScopeId,
        window_id: &WindowId,
    ) -> StorageResult<Option<Allocation>> {
        let row = self
            .connection
            .query_row(
                "SELECT a.amount, p.unit
                 FROM allocations a
                 JOIN quota_windows w ON w.id = a.window_id
                 JOIN quota_pools p ON p.id = w.pool_id
                 WHERE a.scope_id = ?1 AND a.window_id = ?2",
                params![scope_id.as_str(), window_id.as_str()],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;

        row.map(|(amount, unit)| {
            Ok(Allocation::new(
                scope_id.clone(),
                window_id.clone(),
                QuotaAmount::new(
                    from_sql_integer(amount, "allocation amount")?,
                    QuotaUnit::new(unit)?,
                ),
            ))
        })
        .transpose()
    }

    pub fn list_for_window(&self, window_id: &WindowId) -> StorageResult<Vec<Allocation>> {
        let mut statement = self.connection.prepare(
            "SELECT a.scope_id, a.amount, p.unit
             FROM allocations a
             JOIN quota_windows w ON w.id = a.window_id
             JOIN quota_pools p ON p.id = w.pool_id
             LEFT JOIN allocation_priorities ap
               ON ap.pool_id = w.pool_id AND ap.scope_id = a.scope_id
             WHERE a.window_id = ?1
             ORDER BY
               CASE WHEN ap.rank IS NULL THEN 1 ELSE 0 END,
               ap.rank,
               a.scope_id",
        )?;
        let rows = statement.query_map([window_id.as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;

        rows.map(|row| {
            let (scope_id, amount, unit) = row?;
            Ok(Allocation::new(
                ScopeId::new(scope_id)?,
                window_id.clone(),
                QuotaAmount::new(
                    from_sql_integer(amount, "allocation amount")?,
                    QuotaUnit::new(unit)?,
                ),
            ))
        })
        .collect()
    }

    pub fn set_priority_order(
        &mut self,
        window_id: &WindowId,
        ordered_scope_ids: &[ScopeId],
    ) -> StorageResult<()> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let pool_id = transaction
            .query_row(
                "SELECT pool_id FROM quota_windows WHERE id = ?1",
                [window_id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| StorageError::NotFound {
                entity: "quota window",
                id: window_id.to_string(),
            })?;

        let mut statement = transaction.prepare(
            "SELECT a.scope_id
             FROM allocations a
             JOIN scopes s ON s.id = a.scope_id
             WHERE a.window_id = ?1 AND s.parent_id IS NULL
             ORDER BY a.scope_id",
        )?;
        let rows = statement.query_map([window_id.as_str()], |row| row.get::<_, String>(0))?;
        let mut expected = rows.collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        let mut received = ordered_scope_ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        expected.sort();
        received.sort();
        received.dedup();
        if received != expected || ordered_scope_ids.len() != expected.len() {
            return Err(StorageError::InvalidState {
                message: "priority order must contain every root allocation exactly once"
                    .to_owned(),
            });
        }

        transaction.execute(
            "DELETE FROM allocation_priorities WHERE pool_id = ?1",
            [&pool_id],
        )?;
        for (rank, scope_id) in ordered_scope_ids.iter().enumerate() {
            let rank = i64::try_from(rank).map_err(|_| StorageError::NumericOutOfRange {
                field: "allocation priority",
                value: u64::try_from(rank).unwrap_or(u64::MAX),
            })?;
            transaction.execute(
                "INSERT INTO allocation_priorities (pool_id, scope_id, rank)
                 VALUES (?1, ?2, ?3)",
                params![pool_id, scope_id.as_str(), rank],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }
}

pub(crate) fn set_in_transaction(
    transaction: &Transaction<'_>,
    allocation: &Allocation,
) -> StorageResult<()> {
    let (window_capacity, unit) = window_capacity_and_unit(transaction, allocation.window_id())?;
    ensure_unit(allocation.limit().unit(), &unit)?;

    let parent_id = transaction
        .query_row(
            "SELECT parent_id FROM scopes WHERE id = ?1",
            [allocation.scope_id().as_str()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .ok_or_else(|| StorageError::NotFound {
            entity: "scope",
            id: allocation.scope_id().to_string(),
        })?;

    let limit = match parent_id.as_deref() {
        Some(parent_id) => transaction
            .query_row(
                "SELECT amount FROM allocations WHERE scope_id = ?1 AND window_id = ?2",
                params![parent_id, allocation.window_id().as_str()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .ok_or_else(|| StorageError::NotFound {
                entity: "parent allocation",
                id: format!("{parent_id}/{}", allocation.window_id()),
            })?,
        None => window_capacity,
    };

    let allocated_elsewhere = match parent_id.as_deref() {
        Some(parent_id) => transaction.query_row(
            "SELECT COALESCE(SUM(a.amount), 0)
             FROM allocations a
             JOIN scopes s ON s.id = a.scope_id
             WHERE a.window_id = ?1
               AND s.parent_id = ?2
               AND a.scope_id <> ?3",
            params![
                allocation.window_id().as_str(),
                parent_id,
                allocation.scope_id().as_str()
            ],
            |row| row.get::<_, i64>(0),
        )?,
        None => transaction.query_row(
            "SELECT COALESCE(SUM(a.amount), 0)
             FROM allocations a
             JOIN scopes s ON s.id = a.scope_id
             WHERE a.window_id = ?1
               AND s.parent_id IS NULL
               AND a.scope_id <> ?2",
            params![
                allocation.window_id().as_str(),
                allocation.scope_id().as_str()
            ],
            |row| row.get::<_, i64>(0),
        )?,
    };

    let amount = to_sql_integer(allocation.limit().value(), "allocation amount")?;
    let allocated_to_children: i64 = transaction.query_row(
        "SELECT COALESCE(SUM(a.amount), 0)
         FROM allocations a
         JOIN scopes s ON s.id = a.scope_id
         WHERE a.window_id = ?1 AND s.parent_id = ?2",
        params![
            allocation.window_id().as_str(),
            allocation.scope_id().as_str()
        ],
        |row| row.get(0),
    )?;

    if allocated_to_children > amount {
        return Err(DomainError::AllocationExceeded {
            limit: allocation.limit().value(),
            allocated: from_sql_integer(allocated_to_children, "child allocation total")?,
            unit: unit.to_string(),
        }
        .into());
    }

    let total = allocated_elsewhere
        .checked_add(amount)
        .ok_or(StorageError::Domain(DomainError::ArithmeticOverflow))?;

    if total > limit {
        return Err(DomainError::AllocationExceeded {
            limit: from_sql_integer(limit, "allocation limit")?,
            allocated: from_sql_integer(total, "allocated total")?,
            unit: unit.to_string(),
        }
        .into());
    }

    transaction.execute(
        "INSERT INTO allocations (scope_id, window_id, amount)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(scope_id, window_id)
         DO UPDATE SET amount = excluded.amount",
        params![
            allocation.scope_id().as_str(),
            allocation.window_id().as_str(),
            amount
        ],
    )?;
    Ok(())
}

pub(crate) fn window_capacity_and_unit(
    connection: &Transaction<'_>,
    window_id: &WindowId,
) -> StorageResult<(i64, QuotaUnit)> {
    let row = connection
        .query_row(
            "SELECT w.capacity, p.unit
             FROM quota_windows w
             JOIN quota_pools p ON p.id = w.pool_id
             WHERE w.id = ?1",
            [window_id.as_str()],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .ok_or_else(|| StorageError::NotFound {
            entity: "quota window",
            id: window_id.to_string(),
        })?;

    Ok((row.0, QuotaUnit::new(row.1)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_support::{allocation, seeded_database};

    #[test]
    fn top_level_allocations_are_transactionally_bounded_by_window_capacity() {
        let mut database = seeded_database();
        let mut repository = database.allocations();

        repository.set(&allocation("project-a", 60)).unwrap();
        let error = repository.set(&allocation("project-b", 50)).unwrap_err();

        assert!(matches!(
            error,
            StorageError::Domain(DomainError::AllocationExceeded {
                limit: 100,
                allocated: 110,
                ..
            })
        ));
        assert!(repository
            .get(
                &ScopeId::new("project-b").unwrap(),
                &WindowId::new("week-1").unwrap()
            )
            .unwrap()
            .is_none());
    }

    #[test]
    fn priority_order_is_persisted_per_pool_and_controls_listing() {
        let mut database = seeded_database();
        let mut repository = database.allocations();
        repository.set(&allocation("project-a", 40)).unwrap();
        repository.set(&allocation("project-b", 40)).unwrap();

        repository
            .set_priority_order(
                &WindowId::new("week-1").unwrap(),
                &[
                    ScopeId::new("project-b").unwrap(),
                    ScopeId::new("project-a").unwrap(),
                ],
            )
            .unwrap();

        let allocations = repository
            .list_for_window(&WindowId::new("week-1").unwrap())
            .unwrap();
        assert_eq!(allocations[0].scope_id().as_str(), "project-b");
        assert_eq!(allocations[1].scope_id().as_str(), "project-a");
    }

    #[test]
    fn priority_order_rejects_missing_or_duplicate_root_allocations() {
        let mut database = seeded_database();
        let mut repository = database.allocations();
        repository.set(&allocation("project-a", 40)).unwrap();
        repository.set(&allocation("project-b", 40)).unwrap();

        let error = repository
            .set_priority_order(
                &WindowId::new("week-1").unwrap(),
                &[ScopeId::new("project-a").unwrap()],
            )
            .unwrap_err();

        assert!(matches!(error, StorageError::InvalidState { .. }));
    }

    #[test]
    fn child_allocations_require_and_respect_the_parent_allocation() {
        let mut database = seeded_database();
        let mut repository = database.allocations();

        assert!(matches!(
            repository.set(&allocation("feature-a", 10)),
            Err(StorageError::NotFound {
                entity: "parent allocation",
                ..
            })
        ));

        repository.set(&allocation("project-a", 70)).unwrap();
        repository.set(&allocation("feature-a", 50)).unwrap();

        let stored = repository
            .get(
                &ScopeId::new("feature-a").unwrap(),
                &WindowId::new("week-1").unwrap(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(stored.limit().value(), 50);
    }

    #[test]
    fn parent_allocation_cannot_be_reduced_below_existing_children() {
        let mut database = seeded_database();
        let mut repository = database.allocations();

        repository.set(&allocation("project-a", 70)).unwrap();
        repository.set(&allocation("feature-a", 50)).unwrap();
        let error = repository.set(&allocation("project-a", 40)).unwrap_err();

        assert!(matches!(
            error,
            StorageError::Domain(DomainError::AllocationExceeded {
                limit: 40,
                allocated: 50,
                ..
            })
        ));
        assert_eq!(
            repository
                .get(
                    &ScopeId::new("project-a").unwrap(),
                    &WindowId::new("week-1").unwrap()
                )
                .unwrap()
                .unwrap()
                .limit()
                .value(),
            70
        );
    }
}
