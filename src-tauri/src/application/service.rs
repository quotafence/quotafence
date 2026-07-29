use std::{collections::HashMap, path::Path};

use crate::{
    domain::{
        Account, AccountId, Allocation, EnforcementPolicy, Provider, ProviderId, QuotaAmount,
        QuotaPool, QuotaPoolId, QuotaUnit, QuotaWindow, Reservation, ReservationId, Scope, ScopeId,
        UnixMillis, UsageAttribution, UsageEvent, UsageEventId, WindowId,
    },
    storage::Database,
};

use super::{
    error::to_view_integer, AllocationSnapshot, ApplicationError, ApplicationResult, CreateAccount,
    CreateProvider, CreateQuotaPool, CreateQuotaWindow, CreateScope, GetQuotaDashboard,
    QuotaDashboard, RecordUsage, ReleaseReservation, ReserveQuota, SetAllocation, WindowSummary,
};

pub struct QuotaService {
    database: Database,
    policy: EnforcementPolicy,
}

impl QuotaService {
    pub fn open(path: impl AsRef<Path>) -> ApplicationResult<Self> {
        Ok(Self::new(Database::open(path)?))
    }

    pub fn new(database: Database) -> Self {
        Self::with_policy(database, EnforcementPolicy::standard())
    }

    pub fn with_policy(database: Database, policy: EnforcementPolicy) -> Self {
        Self { database, policy }
    }

    pub fn create_provider(&mut self, command: CreateProvider) -> ApplicationResult<()> {
        let provider = Provider::new(ProviderId::new(command.id)?, command.display_name)?;
        self.database.catalog().insert_provider(&provider)?;
        Ok(())
    }

    pub fn create_account(&mut self, command: CreateAccount) -> ApplicationResult<()> {
        let account = Account::new(
            AccountId::new(command.id)?,
            ProviderId::new(command.provider_id)?,
            command.display_name,
        )?;
        self.database.catalog().insert_account(&account)?;
        Ok(())
    }

    pub fn create_quota_pool(&mut self, command: CreateQuotaPool) -> ApplicationResult<()> {
        let pool = QuotaPool::new(
            QuotaPoolId::new(command.id)?,
            AccountId::new(command.account_id)?,
            command.display_name,
            QuotaUnit::new(command.unit)?,
        )?;
        self.database.catalog().insert_quota_pool(&pool)?;
        Ok(())
    }

    pub fn create_quota_window(&mut self, command: CreateQuotaWindow) -> ApplicationResult<()> {
        let window = QuotaWindow::new(
            WindowId::new(command.id)?,
            QuotaPoolId::new(command.pool_id)?,
            UnixMillis::new(command.starts_at),
            UnixMillis::new(command.ends_at),
            QuotaAmount::new(command.capacity, QuotaUnit::new(command.unit)?),
        )?;
        self.database.catalog().insert_quota_window(&window)?;
        Ok(())
    }

    pub fn create_scope(&mut self, command: CreateScope) -> ApplicationResult<()> {
        let scope = Scope::new(
            ScopeId::new(command.id)?,
            command.parent_id.map(ScopeId::new).transpose()?,
            command.kind,
            command.display_name,
        )?;
        self.database.catalog().insert_scope(&scope)?;
        Ok(())
    }

    pub fn set_allocation(&mut self, command: SetAllocation) -> ApplicationResult<()> {
        let allocation = Allocation::new(
            ScopeId::new(command.scope_id)?,
            WindowId::new(command.window_id)?,
            QuotaAmount::new(command.amount, QuotaUnit::new(command.unit)?),
        );
        self.database.allocations().set(&allocation)?;
        Ok(())
    }

    pub fn reserve_quota(&mut self, command: ReserveQuota) -> ApplicationResult<()> {
        let admitted_at = UnixMillis::new(command.admitted_at);
        let reservation = Reservation::new(
            ReservationId::new(command.id)?,
            ScopeId::new(command.scope_id)?,
            WindowId::new(command.window_id)?,
            QuotaAmount::new(command.amount, QuotaUnit::new(command.unit)?),
            admitted_at,
            UnixMillis::new(command.expires_at),
        )?;
        self.database.ledger().reserve(&reservation, admitted_at)?;
        Ok(())
    }

    pub fn release_reservation(&mut self, command: ReleaseReservation) -> ApplicationResult<()> {
        self.database
            .ledger()
            .release_reservation(&ReservationId::new(command.id)?)?;
        Ok(())
    }

    pub fn record_usage(&mut self, command: RecordUsage) -> ApplicationResult<()> {
        let event = UsageEvent::new(
            UsageEventId::new(command.id)?,
            WindowId::new(command.window_id)?,
            match command.scope_id {
                Some(scope_id) => UsageAttribution::Scope(ScopeId::new(scope_id)?),
                None => UsageAttribution::Unattributed,
            },
            QuotaAmount::new(command.amount, QuotaUnit::new(command.unit)?),
            UnixMillis::new(command.observed_at),
            command.source,
            command.confidence,
        )?;
        let reservation_id = command.reservation_id.map(ReservationId::new).transpose()?;

        self.database
            .ledger()
            .record_usage_and_consume(&event, reservation_id.as_ref())?;
        Ok(())
    }

    pub fn dashboard(&mut self, command: GetQuotaDashboard) -> ApplicationResult<QuotaDashboard> {
        let window_id = WindowId::new(command.window_id)?;
        let window = self
            .database
            .catalog()
            .get_quota_window(&window_id)?
            .ok_or_else(|| ApplicationError::NotFound {
                resource: "quota window",
                id: window_id.to_string(),
            })?;
        let scopes: HashMap<_, _> = self
            .database
            .catalog()
            .list_scopes()?
            .into_iter()
            .map(|scope| (scope.id().clone(), scope))
            .collect();
        let allocations = self.database.allocations().list_for_window(&window_id)?;
        let unattributed = self
            .database
            .ledger()
            .unattributed_usage_for_window(&window_id)?;
        let mut snapshots = Vec::with_capacity(allocations.len());
        let mut allocated_to_root_scopes = 0_u64;
        let mut root_committed = 0_u64;

        for allocation in allocations {
            let scope = scopes.get(allocation.scope_id()).ok_or_else(|| {
                ApplicationError::InconsistentData {
                    message: format!(
                        "allocation references missing scope {}",
                        allocation.scope_id()
                    ),
                }
            })?;
            let balance = self.database.ledger().allocation_balance(
                allocation.scope_id(),
                &window_id,
                UnixMillis::new(command.at),
            )?;

            if scope.parent_id().is_none() {
                allocated_to_root_scopes = allocated_to_root_scopes
                    .checked_add(allocation.limit().value())
                    .ok_or_else(arithmetic_overflow)?;
                root_committed = root_committed
                    .checked_add(balance.committed())
                    .ok_or_else(arithmetic_overflow)?;
            }

            snapshots.push(AllocationSnapshot {
                scope_id: scope.id().to_string(),
                parent_id: scope.parent_id().map(ToString::to_string),
                scope_kind: scope.kind(),
                display_name: scope.display_name().to_owned(),
                unit: allocation.limit().unit().to_string(),
                limit: allocation.limit().value(),
                attributed_usage: balance.attributed_usage().value(),
                active_reservations: balance.active_reservations().value(),
                remaining: to_view_integer(balance.remaining(), "allocation remaining")?,
                spendable: balance.spendable().value(),
                decision: self.policy.evaluate(&balance),
            });
        }

        let provider_committed = root_committed
            .checked_add(unattributed.value())
            .ok_or_else(arithmetic_overflow)?;
        let provider_remaining =
            i128::from(window.capacity().value()) - i128::from(provider_committed);
        let provider_spendable = u64::try_from(provider_remaining).unwrap_or(0);

        Ok(QuotaDashboard {
            window: WindowSummary {
                id: window.id().to_string(),
                pool_id: window.pool_id().to_string(),
                starts_at: window.starts_at().value(),
                ends_at: window.ends_at().value(),
                unit: window.capacity().unit().to_string(),
                capacity: window.capacity().value(),
                allocated_to_root_scopes,
                unallocated: window
                    .capacity()
                    .value()
                    .saturating_sub(allocated_to_root_scopes),
                unattributed_usage: unattributed.value(),
                provider_remaining: to_view_integer(provider_remaining, "provider remaining")?,
                provider_spendable,
            },
            allocations: snapshots,
        })
    }
}

fn arithmetic_overflow() -> ApplicationError {
    crate::domain::DomainError::ArithmeticOverflow.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::{
            CreateAccount, CreateProvider, CreateQuotaPool, CreateQuotaWindow, CreateScope,
            GetQuotaDashboard, RecordUsage, ReserveQuota, SetAllocation,
        },
        domain::{Confidence, EnforcementDecision, ScopeKind, UsageSource},
        storage::{Database, StorageError},
    };

    fn configured_service() -> QuotaService {
        let mut service = QuotaService::new(Database::open_in_memory().unwrap());

        service
            .create_provider(CreateProvider {
                id: "codex".to_owned(),
                display_name: "Codex".to_owned(),
            })
            .unwrap();
        service
            .create_account(CreateAccount {
                id: "codex-default".to_owned(),
                provider_id: "codex".to_owned(),
                display_name: "Default account".to_owned(),
            })
            .unwrap();
        service
            .create_quota_pool(CreateQuotaPool {
                id: "codex-weekly".to_owned(),
                account_id: "codex-default".to_owned(),
                display_name: "Weekly quota".to_owned(),
                unit: "quota_points".to_owned(),
            })
            .unwrap();
        service
            .create_quota_window(CreateQuotaWindow {
                id: "week-1".to_owned(),
                pool_id: "codex-weekly".to_owned(),
                starts_at: 1_000,
                ends_at: 10_000,
                capacity: 100,
                unit: "quota_points".to_owned(),
            })
            .unwrap();
        service
            .create_scope(CreateScope {
                id: "project-a".to_owned(),
                parent_id: None,
                kind: ScopeKind::Project,
                display_name: "Project A".to_owned(),
            })
            .unwrap();
        service
            .create_scope(CreateScope {
                id: "feature-a".to_owned(),
                parent_id: Some("project-a".to_owned()),
                kind: ScopeKind::Task,
                display_name: "Feature A".to_owned(),
            })
            .unwrap();
        service
            .set_allocation(SetAllocation {
                scope_id: "project-a".to_owned(),
                window_id: "week-1".to_owned(),
                amount: 70,
                unit: "quota_points".to_owned(),
            })
            .unwrap();
        service
            .set_allocation(SetAllocation {
                scope_id: "feature-a".to_owned(),
                window_id: "week-1".to_owned(),
                amount: 50,
                unit: "quota_points".to_owned(),
            })
            .unwrap();

        service
    }

    fn snapshot<'a>(dashboard: &'a QuotaDashboard, scope_id: &str) -> &'a AllocationSnapshot {
        dashboard
            .allocations
            .iter()
            .find(|snapshot| snapshot.scope_id == scope_id)
            .unwrap()
    }

    #[test]
    fn dashboard_reports_hierarchical_reservations_and_window_capacity() {
        let mut service = configured_service();
        service
            .reserve_quota(ReserveQuota {
                id: "reservation-1".to_owned(),
                scope_id: "feature-a".to_owned(),
                window_id: "week-1".to_owned(),
                amount: 20,
                unit: "quota_points".to_owned(),
                admitted_at: 2_000,
                expires_at: 8_000,
            })
            .unwrap();

        let dashboard = service
            .dashboard(GetQuotaDashboard {
                window_id: "week-1".to_owned(),
                at: 3_000,
            })
            .unwrap();
        let project = snapshot(&dashboard, "project-a");
        let feature = snapshot(&dashboard, "feature-a");

        assert_eq!(dashboard.window.capacity, 100);
        assert_eq!(dashboard.window.allocated_to_root_scopes, 70);
        assert_eq!(dashboard.window.unallocated, 30);
        assert_eq!(dashboard.window.provider_remaining, 80);
        assert_eq!(project.active_reservations, 20);
        assert_eq!(project.remaining, 50);
        assert_eq!(feature.active_reservations, 20);
        assert_eq!(feature.remaining, 30);
    }

    #[test]
    fn recording_usage_consumes_its_reservation_atomically() {
        let mut service = configured_service();
        service
            .reserve_quota(ReserveQuota {
                id: "reservation-1".to_owned(),
                scope_id: "feature-a".to_owned(),
                window_id: "week-1".to_owned(),
                amount: 20,
                unit: "quota_points".to_owned(),
                admitted_at: 2_000,
                expires_at: 8_000,
            })
            .unwrap();
        service
            .record_usage(RecordUsage {
                id: "usage-1".to_owned(),
                window_id: "week-1".to_owned(),
                scope_id: Some("feature-a".to_owned()),
                amount: 18,
                unit: "quota_points".to_owned(),
                observed_at: 3_000,
                source: UsageSource::LocalMeasured,
                confidence: Confidence::Observed,
                reservation_id: Some("reservation-1".to_owned()),
            })
            .unwrap();

        let dashboard = service
            .dashboard(GetQuotaDashboard {
                window_id: "week-1".to_owned(),
                at: 3_000,
            })
            .unwrap();
        let feature = snapshot(&dashboard, "feature-a");

        assert_eq!(feature.attributed_usage, 18);
        assert_eq!(feature.active_reservations, 0);
        assert_eq!(feature.remaining, 32);
        assert_eq!(dashboard.window.provider_remaining, 82);
    }

    #[test]
    fn unattributed_usage_reduces_provider_capacity_without_debiting_a_scope() {
        let mut service = configured_service();
        service
            .record_usage(RecordUsage {
                id: "external-usage".to_owned(),
                window_id: "week-1".to_owned(),
                scope_id: None,
                amount: 12,
                unit: "quota_points".to_owned(),
                observed_at: 3_000,
                source: UsageSource::ProviderConfirmed,
                confidence: Confidence::Confirmed,
                reservation_id: None,
            })
            .unwrap();

        let dashboard = service
            .dashboard(GetQuotaDashboard {
                window_id: "week-1".to_owned(),
                at: 3_000,
            })
            .unwrap();

        assert_eq!(dashboard.window.unattributed_usage, 12);
        assert_eq!(dashboard.window.provider_remaining, 88);
        assert_eq!(snapshot(&dashboard, "project-a").attributed_usage, 0);
    }

    #[test]
    fn dashboard_exposes_the_effective_enforcement_decision() {
        let mut service = configured_service();
        service
            .record_usage(RecordUsage {
                id: "usage-1".to_owned(),
                window_id: "week-1".to_owned(),
                scope_id: Some("feature-a".to_owned()),
                amount: 40,
                unit: "quota_points".to_owned(),
                observed_at: 3_000,
                source: UsageSource::LocalMeasured,
                confidence: Confidence::Observed,
                reservation_id: None,
            })
            .unwrap();

        let dashboard = service
            .dashboard(GetQuotaDashboard {
                window_id: "week-1".to_owned(),
                at: 3_000,
            })
            .unwrap();

        assert_eq!(
            snapshot(&dashboard, "feature-a").decision,
            EnforcementDecision::Warn
        );
    }

    #[test]
    fn invalid_commands_fail_before_reaching_storage() {
        let mut service = QuotaService::new(Database::open_in_memory().unwrap());

        let error = service
            .create_provider(CreateProvider {
                id: " ".to_owned(),
                display_name: "Codex".to_owned(),
            })
            .unwrap_err();

        assert!(matches!(error, ApplicationError::Validation(_)));
        assert!(!matches!(
            error,
            ApplicationError::Storage(StorageError::Database(_))
        ));
    }

    #[test]
    fn service_can_be_held_behind_synchronized_tauri_state() {
        fn assert_send<T: Send>() {}

        assert_send::<QuotaService>();
    }

    #[test]
    fn dashboard_serializes_with_frontend_friendly_field_names() {
        let mut service = configured_service();
        let dashboard = service
            .dashboard(GetQuotaDashboard {
                window_id: "week-1".to_owned(),
                at: 3_000,
            })
            .unwrap();

        let json = serde_json::to_value(dashboard).unwrap();
        let allocations = json["allocations"].as_array().unwrap();

        assert_eq!(json["window"]["providerRemaining"], 100);
        assert!(allocations
            .iter()
            .any(|allocation| allocation["scopeId"] == "feature-a"));
        assert!(json["window"].get("provider_remaining").is_none());
    }
}
