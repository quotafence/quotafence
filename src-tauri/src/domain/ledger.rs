use super::{
    Allocation, DomainError, DomainResult, QuotaAmount, QuotaBalance, QuotaWindow, Reservation,
    UnixMillis, UsageAttribution, UsageEvent,
};

pub fn validate_window_allocations(
    window: &QuotaWindow,
    allocations: &[Allocation],
) -> DomainResult<QuotaAmount> {
    validate_allocation_sum(window.id().as_str(), window.capacity(), allocations)
}

pub fn validate_child_allocations(
    parent: &Allocation,
    children: &[Allocation],
) -> DomainResult<QuotaAmount> {
    validate_allocation_sum(parent.window_id().as_str(), parent.limit(), children)
}

pub fn calculate_allocation_balance(
    allocation: &Allocation,
    usage_events: &[UsageEvent],
    reservations: &[Reservation],
    at: UnixMillis,
) -> DomainResult<QuotaBalance> {
    let mut attributed_usage = QuotaAmount::new(0, allocation.limit().unit().clone());
    let mut active_reservations = QuotaAmount::new(0, allocation.limit().unit().clone());

    for event in usage_events {
        ensure_window(allocation, event.window_id().as_str())?;

        match event.attribution() {
            UsageAttribution::Scope(scope_id) if scope_id == allocation.scope_id() => {}
            UsageAttribution::Scope(scope_id) => {
                return Err(DomainError::ScopeMismatch {
                    expected: allocation.scope_id().to_string(),
                    actual: scope_id.to_string(),
                });
            }
            UsageAttribution::Unattributed => {
                return Err(DomainError::ScopeMismatch {
                    expected: allocation.scope_id().to_string(),
                    actual: "unattributed".to_owned(),
                });
            }
        }

        attributed_usage = attributed_usage.checked_add(event.amount())?;
    }

    for reservation in reservations {
        ensure_window(allocation, reservation.window_id().as_str())?;

        if reservation.scope_id() != allocation.scope_id() {
            return Err(DomainError::ScopeMismatch {
                expected: allocation.scope_id().to_string(),
                actual: reservation.scope_id().to_string(),
            });
        }

        if reservation.is_active_at(at) {
            active_reservations = active_reservations.checked_add(reservation.amount())?;
        }
    }

    QuotaBalance::new(
        allocation.limit().clone(),
        attributed_usage,
        active_reservations,
    )
}

fn validate_allocation_sum(
    expected_window_id: &str,
    limit: &QuotaAmount,
    allocations: &[Allocation],
) -> DomainResult<QuotaAmount> {
    let mut total = QuotaAmount::new(0, limit.unit().clone());

    for allocation in allocations {
        if allocation.window_id().as_str() != expected_window_id {
            return Err(DomainError::WindowMismatch {
                expected: expected_window_id.to_owned(),
                actual: allocation.window_id().to_string(),
            });
        }

        total = total.checked_add(allocation.limit())?;
    }

    if total.value() > limit.value() {
        return Err(DomainError::AllocationExceeded {
            limit: limit.value(),
            allocated: total.value(),
            unit: limit.unit().to_string(),
        });
    }

    Ok(total)
}

fn ensure_window(allocation: &Allocation, actual_window_id: &str) -> DomainResult<()> {
    if allocation.window_id().as_str() != actual_window_id {
        return Err(DomainError::WindowMismatch {
            expected: allocation.window_id().to_string(),
            actual: actual_window_id.to_owned(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        Confidence, QuotaPoolId, QuotaUnit, ReservationId, ScopeId, UsageEventId, UsageSource,
        WindowId,
    };

    fn points(value: u64) -> QuotaAmount {
        QuotaAmount::new(value, QuotaUnit::new("quota_points").unwrap())
    }

    fn allocation() -> Allocation {
        Allocation::new(
            ScopeId::new("project-a").unwrap(),
            WindowId::new("week-1").unwrap(),
            points(100),
        )
    }

    fn usage(value: u64) -> UsageEvent {
        UsageEvent::new(
            UsageEventId::new(format!("usage-{value}")).unwrap(),
            WindowId::new("week-1").unwrap(),
            UsageAttribution::Scope(ScopeId::new("project-a").unwrap()),
            points(value),
            UnixMillis::new(1_100),
            UsageSource::LocalMeasured,
            Confidence::Observed,
        )
        .unwrap()
    }

    fn reservation(id: &str, value: u64, created_at: i64, expires_at: i64) -> Reservation {
        Reservation::new(
            ReservationId::new(id).unwrap(),
            ScopeId::new("project-a").unwrap(),
            WindowId::new("week-1").unwrap(),
            points(value),
            UnixMillis::new(created_at),
            UnixMillis::new(expires_at),
        )
        .unwrap()
    }

    #[test]
    fn available_capacity_subtracts_usage_and_active_reservations() {
        let balance = calculate_allocation_balance(
            &allocation(),
            &[usage(30)],
            &[reservation("reservation-1", 20, 1_000, 2_000)],
            UnixMillis::new(1_500),
        )
        .unwrap();

        assert_eq!(balance.attributed_usage().value(), 30);
        assert_eq!(balance.active_reservations().value(), 20);
        assert_eq!(balance.remaining(), 50);
        assert_eq!(balance.spendable().value(), 50);
    }

    #[test]
    fn expired_and_released_reservations_do_not_reduce_capacity() {
        let expired = reservation("expired", 20, 1_000, 1_200);
        let mut released = reservation("released", 25, 1_000, 2_000);
        released.release().unwrap();

        let balance = calculate_allocation_balance(
            &allocation(),
            &[],
            &[expired, released],
            UnixMillis::new(1_500),
        )
        .unwrap();

        assert_eq!(balance.active_reservations().value(), 0);
        assert_eq!(balance.remaining(), 100);
    }

    #[test]
    fn input_from_another_scope_is_rejected() {
        let event = UsageEvent::new(
            UsageEventId::new("usage-other").unwrap(),
            WindowId::new("week-1").unwrap(),
            UsageAttribution::Scope(ScopeId::new("project-b").unwrap()),
            points(10),
            UnixMillis::new(1_100),
            UsageSource::LocalMeasured,
            Confidence::Observed,
        )
        .unwrap();

        assert_eq!(
            calculate_allocation_balance(&allocation(), &[event], &[], UnixMillis::new(1_500)),
            Err(DomainError::ScopeMismatch {
                expected: "project-a".to_owned(),
                actual: "project-b".to_owned(),
            })
        );
    }

    #[test]
    fn input_from_another_window_is_rejected() {
        let event = UsageEvent::new(
            UsageEventId::new("usage-old").unwrap(),
            WindowId::new("week-0").unwrap(),
            UsageAttribution::Scope(ScopeId::new("project-a").unwrap()),
            points(10),
            UnixMillis::new(100),
            UsageSource::ProviderConfirmed,
            Confidence::Confirmed,
        )
        .unwrap();

        assert_eq!(
            calculate_allocation_balance(&allocation(), &[event], &[], UnixMillis::new(1_500)),
            Err(DomainError::WindowMismatch {
                expected: "week-1".to_owned(),
                actual: "week-0".to_owned(),
            })
        );
    }

    #[test]
    fn window_allocations_cannot_exceed_capacity() {
        let window = QuotaWindow::new(
            WindowId::new("week-1").unwrap(),
            QuotaPoolId::new("codex-weekly").unwrap(),
            UnixMillis::new(1_000),
            UnixMillis::new(2_000),
            points(100),
        )
        .unwrap();
        let allocations = [
            Allocation::new(
                ScopeId::new("project-a").unwrap(),
                WindowId::new("week-1").unwrap(),
                points(60),
            ),
            Allocation::new(
                ScopeId::new("project-b").unwrap(),
                WindowId::new("week-1").unwrap(),
                points(50),
            ),
        ];

        assert_eq!(
            validate_window_allocations(&window, &allocations),
            Err(DomainError::AllocationExceeded {
                limit: 100,
                allocated: 110,
                unit: "quota_points".to_owned(),
            })
        );
    }

    #[test]
    fn child_allocations_fit_within_the_parent_limit() {
        let parent = Allocation::new(
            ScopeId::new("project-a").unwrap(),
            WindowId::new("week-1").unwrap(),
            points(70),
        );
        let children = [
            Allocation::new(
                ScopeId::new("feature").unwrap(),
                WindowId::new("week-1").unwrap(),
                points(40),
            ),
            Allocation::new(
                ScopeId::new("maintenance").unwrap(),
                WindowId::new("week-1").unwrap(),
                points(20),
            ),
        ];

        let total = validate_child_allocations(&parent, &children).unwrap();

        assert_eq!(total.value(), 60);
    }
}
