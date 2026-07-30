use serde::{Deserialize, Serialize};

use super::{
    AccountId, DomainError, DomainResult, ProviderId, QuotaAmount, QuotaPoolId, QuotaUnit,
    ReservationId, ScopeId, UsageEventId, WindowId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnixMillis(i64);

impl UnixMillis {
    pub fn new(value: i64) -> Self {
        Self(value)
    }

    pub fn value(self) -> i64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Provider {
    id: ProviderId,
    display_name: String,
}

impl Provider {
    pub fn new(id: ProviderId, display_name: impl Into<String>) -> DomainResult<Self> {
        Ok(Self {
            id,
            display_name: required_text(display_name, "provider display name")?,
        })
    }

    pub fn id(&self) -> &ProviderId {
        &self.id
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Account {
    id: AccountId,
    provider_id: ProviderId,
    display_name: String,
}

impl Account {
    pub fn new(
        id: AccountId,
        provider_id: ProviderId,
        display_name: impl Into<String>,
    ) -> DomainResult<Self> {
        Ok(Self {
            id,
            provider_id,
            display_name: required_text(display_name, "account display name")?,
        })
    }

    pub fn id(&self) -> &AccountId {
        &self.id
    }

    pub fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QuotaPool {
    id: QuotaPoolId,
    account_id: AccountId,
    display_name: String,
    unit: QuotaUnit,
}

impl QuotaPool {
    pub fn new(
        id: QuotaPoolId,
        account_id: AccountId,
        display_name: impl Into<String>,
        unit: QuotaUnit,
    ) -> DomainResult<Self> {
        Ok(Self {
            id,
            account_id,
            display_name: required_text(display_name, "quota pool display name")?,
            unit,
        })
    }

    pub fn id(&self) -> &QuotaPoolId {
        &self.id
    }

    pub fn account_id(&self) -> &AccountId {
        &self.account_id
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn unit(&self) -> &QuotaUnit {
        &self.unit
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QuotaWindow {
    id: WindowId,
    pool_id: QuotaPoolId,
    starts_at: UnixMillis,
    ends_at: UnixMillis,
    capacity: QuotaAmount,
}

impl QuotaWindow {
    pub fn new(
        id: WindowId,
        pool_id: QuotaPoolId,
        starts_at: UnixMillis,
        ends_at: UnixMillis,
        capacity: QuotaAmount,
    ) -> DomainResult<Self> {
        if ends_at <= starts_at {
            return Err(DomainError::InvalidWindow {
                starts_at: starts_at.value(),
                ends_at: ends_at.value(),
            });
        }

        Ok(Self {
            id,
            pool_id,
            starts_at,
            ends_at,
            capacity,
        })
    }

    pub fn id(&self) -> &WindowId {
        &self.id
    }

    pub fn pool_id(&self) -> &QuotaPoolId {
        &self.pool_id
    }

    pub fn starts_at(&self) -> UnixMillis {
        self.starts_at
    }

    pub fn ends_at(&self) -> UnixMillis {
        self.ends_at
    }

    pub fn capacity(&self) -> &QuotaAmount {
        &self.capacity
    }

    pub fn contains(&self, timestamp: UnixMillis) -> bool {
        self.starts_at <= timestamp && timestamp < self.ends_at
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    Project,
    Workspace,
    Task,
    Reserve,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Scope {
    id: ScopeId,
    parent_id: Option<ScopeId>,
    kind: ScopeKind,
    display_name: String,
}

impl Scope {
    pub fn new(
        id: ScopeId,
        parent_id: Option<ScopeId>,
        kind: ScopeKind,
        display_name: impl Into<String>,
    ) -> DomainResult<Self> {
        if parent_id.as_ref() == Some(&id) {
            return Err(DomainError::SelfParent {
                scope_id: id.to_string(),
            });
        }

        Ok(Self {
            id,
            parent_id,
            kind,
            display_name: required_text(display_name, "scope display name")?,
        })
    }

    pub fn id(&self) -> &ScopeId {
        &self.id
    }

    pub fn parent_id(&self) -> Option<&ScopeId> {
        self.parent_id.as_ref()
    }

    pub fn kind(&self) -> ScopeKind {
        self.kind
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Allocation {
    scope_id: ScopeId,
    window_id: WindowId,
    limit: QuotaAmount,
}

impl Allocation {
    pub fn new(scope_id: ScopeId, window_id: WindowId, limit: QuotaAmount) -> Self {
        Self {
            scope_id,
            window_id,
            limit,
        }
    }

    pub fn scope_id(&self) -> &ScopeId {
        &self.scope_id
    }

    pub fn window_id(&self) -> &WindowId {
        &self.window_id
    }

    pub fn limit(&self) -> &QuotaAmount {
        &self.limit
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReservationStatus {
    Active,
    Released,
    Consumed,
    Expired,
}

impl ReservationStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Released => "released",
            Self::Consumed => "consumed",
            Self::Expired => "expired",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Reservation {
    id: ReservationId,
    scope_id: ScopeId,
    window_id: WindowId,
    amount: QuotaAmount,
    created_at: UnixMillis,
    expires_at: UnixMillis,
    status: ReservationStatus,
}

impl Reservation {
    pub fn new(
        id: ReservationId,
        scope_id: ScopeId,
        window_id: WindowId,
        amount: QuotaAmount,
        created_at: UnixMillis,
        expires_at: UnixMillis,
    ) -> DomainResult<Self> {
        Self::restore(
            id,
            scope_id,
            window_id,
            amount,
            created_at,
            expires_at,
            ReservationStatus::Active,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        id: ReservationId,
        scope_id: ScopeId,
        window_id: WindowId,
        amount: QuotaAmount,
        created_at: UnixMillis,
        expires_at: UnixMillis,
        status: ReservationStatus,
    ) -> DomainResult<Self> {
        if amount.value() == 0 {
            return Err(DomainError::ZeroAmount {
                context: "reservation",
            });
        }

        if expires_at <= created_at {
            return Err(DomainError::InvalidReservationExpiry {
                created_at: created_at.value(),
                expires_at: expires_at.value(),
            });
        }

        Ok(Self {
            id,
            scope_id,
            window_id,
            amount,
            created_at,
            expires_at,
            status,
        })
    }

    pub fn id(&self) -> &ReservationId {
        &self.id
    }

    pub fn scope_id(&self) -> &ScopeId {
        &self.scope_id
    }

    pub fn window_id(&self) -> &WindowId {
        &self.window_id
    }

    pub fn amount(&self) -> &QuotaAmount {
        &self.amount
    }

    pub fn created_at(&self) -> UnixMillis {
        self.created_at
    }

    pub fn expires_at(&self) -> UnixMillis {
        self.expires_at
    }

    pub fn status(&self) -> ReservationStatus {
        self.status
    }

    pub fn is_active_at(&self, timestamp: UnixMillis) -> bool {
        self.status == ReservationStatus::Active
            && self.created_at <= timestamp
            && timestamp < self.expires_at
    }

    pub fn release(&mut self) -> DomainResult<()> {
        self.transition_to(ReservationStatus::Released)
    }

    pub fn consume(&mut self) -> DomainResult<()> {
        self.transition_to(ReservationStatus::Consumed)
    }

    pub fn expire(&mut self, timestamp: UnixMillis) -> DomainResult<()> {
        if timestamp < self.expires_at {
            return Err(DomainError::InvalidReservationTransition {
                from: self.status.as_str(),
                to: ReservationStatus::Expired.as_str(),
            });
        }

        self.transition_to(ReservationStatus::Expired)
    }

    fn transition_to(&mut self, next: ReservationStatus) -> DomainResult<()> {
        if self.status != ReservationStatus::Active {
            return Err(DomainError::InvalidReservationTransition {
                from: self.status.as_str(),
                to: next.as_str(),
            });
        }

        self.status = next;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageAttribution {
    Scope(ScopeId),
    Unattributed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageSource {
    ProviderConfirmed,
    ProviderObserved,
    LocalMeasured,
    Estimated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Confirmed,
    Observed,
    Inferred,
    Estimated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UsageEvent {
    id: UsageEventId,
    window_id: WindowId,
    attribution: UsageAttribution,
    amount: QuotaAmount,
    observed_at: UnixMillis,
    source: UsageSource,
    confidence: Confidence,
}

impl UsageEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: UsageEventId,
        window_id: WindowId,
        attribution: UsageAttribution,
        amount: QuotaAmount,
        observed_at: UnixMillis,
        source: UsageSource,
        confidence: Confidence,
    ) -> DomainResult<Self> {
        if amount.value() == 0 {
            return Err(DomainError::ZeroAmount {
                context: "usage event",
            });
        }

        Ok(Self {
            id,
            window_id,
            attribution,
            amount,
            observed_at,
            source,
            confidence,
        })
    }

    pub fn id(&self) -> &UsageEventId {
        &self.id
    }

    pub fn window_id(&self) -> &WindowId {
        &self.window_id
    }

    pub fn attribution(&self) -> &UsageAttribution {
        &self.attribution
    }

    pub fn amount(&self) -> &QuotaAmount {
        &self.amount
    }

    pub fn observed_at(&self) -> UnixMillis {
        self.observed_at
    }

    pub fn source(&self) -> UsageSource {
        self.source
    }

    pub fn confidence(&self) -> Confidence {
        self.confidence
    }
}

fn required_text(value: impl Into<String>, field: &'static str) -> DomainResult<String> {
    let value = value.into();
    let value = value.trim();

    if value.is_empty() {
        return Err(DomainError::EmptyValue { field });
    }

    Ok(value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points(value: u64) -> QuotaAmount {
        QuotaAmount::new(value, QuotaUnit::new("quota_points").unwrap())
    }

    #[test]
    fn windows_are_half_open_intervals() {
        let window = QuotaWindow::new(
            WindowId::new("week-1").unwrap(),
            QuotaPoolId::new("codex-weekly").unwrap(),
            UnixMillis::new(1_000),
            UnixMillis::new(2_000),
            points(100),
        )
        .unwrap();

        assert!(window.contains(UnixMillis::new(1_000)));
        assert!(window.contains(UnixMillis::new(1_999)));
        assert!(!window.contains(UnixMillis::new(2_000)));
    }

    #[test]
    fn windows_must_have_positive_duration() {
        let result = QuotaWindow::new(
            WindowId::new("week-1").unwrap(),
            QuotaPoolId::new("codex-weekly").unwrap(),
            UnixMillis::new(2_000),
            UnixMillis::new(2_000),
            points(100),
        );

        assert_eq!(
            result,
            Err(DomainError::InvalidWindow {
                starts_at: 2_000,
                ends_at: 2_000,
            })
        );
    }

    #[test]
    fn a_scope_cannot_parent_itself() {
        let scope_id = ScopeId::new("project-a").unwrap();
        let result = Scope::new(
            scope_id.clone(),
            Some(scope_id),
            ScopeKind::Project,
            "Project A",
        );

        assert_eq!(
            result,
            Err(DomainError::SelfParent {
                scope_id: "project-a".to_owned(),
            })
        );
    }

    #[test]
    fn released_reservations_cannot_transition_again() {
        let mut reservation = Reservation::new(
            ReservationId::new("reservation-1").unwrap(),
            ScopeId::new("project-a").unwrap(),
            WindowId::new("week-1").unwrap(),
            points(10),
            UnixMillis::new(1_000),
            UnixMillis::new(2_000),
        )
        .unwrap();

        reservation.release().unwrap();

        assert_eq!(
            reservation.consume(),
            Err(DomainError::InvalidReservationTransition {
                from: "released",
                to: "consumed",
            })
        );
    }

    #[test]
    fn usage_events_must_debit_a_positive_amount() {
        let result = UsageEvent::new(
            UsageEventId::new("usage-1").unwrap(),
            WindowId::new("week-1").unwrap(),
            UsageAttribution::Unattributed,
            points(0),
            UnixMillis::new(1_000),
            UsageSource::Estimated,
            Confidence::Estimated,
        );

        assert_eq!(
            result,
            Err(DomainError::ZeroAmount {
                context: "usage event",
            })
        );
    }
}
