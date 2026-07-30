use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use crate::{
    domain::{
        Account, AccountId, Allocation, EnforcementPolicy, Provider, ProviderId, QuotaAmount,
        QuotaBalance, QuotaPool, QuotaPoolId, QuotaUnit, QuotaWindow, Reservation, ReservationId,
        Scope, ScopeId, UnixMillis, UsageAttribution, UsageEvent, UsageEventId, WindowId,
    },
    storage::{
        BeginObservationStatus, Database, ProviderQuotaSnapshot, ProviderTurnObservation,
        ReconcileObservationResult, StorageError, WorkspaceBinding,
    },
    workspace::{contains_path, path_depth},
};

use super::{
    error::to_view_integer, AbandonProviderSessionObservations, AbandonProviderTurnObservation,
    AdmissionAssessment, AllocationSnapshot, ApplicationError, ApplicationResult,
    ArchiveQuotaSource, BeginProviderTurnObservation, BindWorkspace, CreateAccount,
    CreateAllocatedScope, CreateAllocatedWorkspace, CreateProvider, CreateQuotaPool,
    CreateQuotaSource, CreateQuotaWindow, CreateScope, EvaluateWorkspaceAdmission, GetLocalState,
    GetProviderTurnObservation, GetQuotaDashboard, GetWorkspaceContext, LocalState,
    ProviderTurnObservationSummary, QuotaDashboard, QuotaSourceSummary,
    ReconcileProviderTurnObservation, RecordUsage, ReleaseReservation, ReserveQuota, ScopeSummary,
    SetAllocation, SyncProviderQuota, SyncProviderQuotaResult, TurnObservationStartResult,
    TurnObservationStartStatus, TurnReconciliationResult, TurnReconciliationStatus, WindowSummary,
    WorkspaceAllocationContext, WorkspaceBindingSummary, WorkspaceContext,
};

const TURN_OBSERVATION_STALE_AFTER_MILLIS: i64 = 12 * 60 * 60 * 1_000;

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

    pub fn create_quota_source(&mut self, command: CreateQuotaSource) -> ApplicationResult<()> {
        let provider = Provider::new(
            ProviderId::new(command.provider_id)?,
            command.provider_display_name,
        )?;
        let account = Account::new(
            AccountId::new(command.account_id)?,
            provider.id().clone(),
            command.account_display_name,
        )?;
        let unit = QuotaUnit::new(command.unit)?;
        let pool = QuotaPool::new(
            QuotaPoolId::new(command.pool_id)?,
            account.id().clone(),
            command.pool_display_name,
            unit.clone(),
        )?;
        let window_id = WindowId::new(command.window_id)?;
        let snapshot = command.provider_snapshot.map(|snapshot| {
            ProviderQuotaSnapshot::new(
                window_id.clone(),
                snapshot.adapter,
                snapshot.remote_limit_id,
                snapshot.remote_window_kind,
                snapshot.used,
                UnixMillis::new(snapshot.observed_at),
                UnixMillis::new(snapshot.resets_at),
            )
        });
        let window = QuotaWindow::new(
            window_id,
            pool.id().clone(),
            UnixMillis::new(command.starts_at),
            UnixMillis::new(command.ends_at),
            QuotaAmount::new(command.capacity, unit),
        )?;

        self.database.insert_quota_source_with_snapshot(
            &provider,
            &account,
            &pool,
            &window,
            snapshot.as_ref(),
        )?;
        Ok(())
    }

    pub fn archive_quota_source(&mut self, command: ArchiveQuotaSource) -> ApplicationResult<()> {
        self.database.catalog().archive_quota_pool(
            &QuotaPoolId::new(command.pool_id)?,
            UnixMillis::new(command.archived_at),
        )?;
        Ok(())
    }

    pub fn sync_provider_quota(
        &mut self,
        command: SyncProviderQuota,
    ) -> ApplicationResult<SyncProviderQuotaResult> {
        let current_window_id = WindowId::new(command.current_window_id)?;
        let current_window = self
            .database
            .catalog()
            .get_quota_window(&current_window_id)?
            .ok_or_else(|| ApplicationError::NotFound {
                resource: "quota window",
                id: current_window_id.to_string(),
            })?;
        let unit = QuotaUnit::new(command.unit)?;
        if current_window.capacity().unit() != &unit {
            return Err(crate::domain::DomainError::UnitMismatch {
                expected: current_window.capacity().unit().to_string(),
                actual: unit.to_string(),
            }
            .into());
        }

        let rolled_over = current_window.starts_at().value() != command.starts_at
            || current_window.ends_at().value() != command.ends_at;
        let target_window_id = if rolled_over {
            WindowId::new(format!(
                "{}-window-{}",
                current_window.pool_id(),
                command.ends_at
            ))?
        } else {
            current_window_id.clone()
        };
        let target_window = QuotaWindow::new(
            target_window_id.clone(),
            current_window.pool_id().clone(),
            UnixMillis::new(command.starts_at),
            UnixMillis::new(command.ends_at),
            QuotaAmount::new(command.capacity, unit),
        )?;
        let snapshot = ProviderQuotaSnapshot::new(
            target_window_id.clone(),
            command.adapter,
            command.remote_limit_id,
            command.remote_window_kind,
            command.used,
            UnixMillis::new(command.observed_at),
            UnixMillis::new(command.ends_at),
        );

        self.database
            .sync_provider_quota(&current_window_id, &target_window, &snapshot)?;

        Ok(SyncProviderQuotaResult {
            window_id: target_window_id.to_string(),
            rolled_over,
        })
    }

    pub fn begin_provider_turn_observation(
        &mut self,
        command: BeginProviderTurnObservation,
    ) -> ApplicationResult<TurnObservationStartResult> {
        let session_id = required_request_text(command.session_id, "session ID")?;
        let turn_id = required_request_text(command.turn_id, "turn ID")?;
        let adapter = required_request_text(command.adapter, "provider adapter")?;
        let canonical_path = required_request_text(command.canonical_path, "workspace path")?;
        let window_id = WindowId::new(command.window_id)?;
        let scope_id = command.scope_id.map(ScopeId::new).transpose()?;
        let snapshot = self
            .database
            .provider_quota_snapshot(&window_id)?
            .ok_or_else(|| ApplicationError::InvalidRequest {
                message: format!(
                    "quota window {window_id} has no provider checkpoint for turn observation"
                ),
            })?;
        if !snapshot.adapter().eq_ignore_ascii_case(&adapter) {
            return Err(ApplicationError::InvalidRequest {
                message: format!(
                    "quota window {window_id} uses adapter {}, not {adapter}",
                    snapshot.adapter()
                ),
            });
        }

        if let Some(scope_id) = scope_id.as_ref() {
            let has_allocation = self
                .database
                .allocations()
                .list_for_window(&window_id)?
                .iter()
                .any(|allocation| allocation.scope_id() == scope_id);
            if !has_allocation {
                return Err(ApplicationError::InvalidRequest {
                    message: format!(
                        "scope {scope_id} has no allocation in quota window {window_id}"
                    ),
                });
            }
        }

        let observation = ProviderTurnObservation::new(
            session_id,
            turn_id,
            adapter,
            canonical_path,
            scope_id.clone(),
            window_id.clone(),
            snapshot.used(),
            UnixMillis::new(command.started_at),
        );
        let stale_before = UnixMillis::new(
            command
                .started_at
                .saturating_sub(TURN_OBSERVATION_STALE_AFTER_MILLIS),
        );
        let result = self
            .database
            .turn_observations()
            .begin(&observation, stale_before)?;

        Ok(TurnObservationStartResult {
            status: match result.status {
                BeginObservationStatus::Started => TurnObservationStartStatus::Started,
                BeginObservationStatus::AlreadyStarted => {
                    TurnObservationStartStatus::AlreadyStarted
                }
            },
            contended: result.contended,
            window_id: window_id.to_string(),
            scope_id: scope_id.map(|value| value.to_string()),
        })
    }

    pub fn provider_turn_observation(
        &mut self,
        command: GetProviderTurnObservation,
    ) -> ApplicationResult<Option<ProviderTurnObservationSummary>> {
        let session_id = required_request_text(command.session_id, "session ID")?;
        let turn_id = required_request_text(command.turn_id, "turn ID")?;
        Ok(self
            .database
            .turn_observations()
            .get(&session_id, &turn_id)?
            .map(|observation| ProviderTurnObservationSummary {
                canonical_path: observation.canonical_path().to_owned(),
                window_id: observation.window_id().to_string(),
                scope_id: observation.scope_id().map(ToString::to_string),
                contended: observation.contended(),
            }))
    }

    pub fn reconcile_provider_turn_observation(
        &mut self,
        command: ReconcileProviderTurnObservation,
    ) -> ApplicationResult<TurnReconciliationResult> {
        let session_id = required_request_text(command.session_id, "session ID")?;
        let turn_id = required_request_text(command.turn_id, "turn ID")?;
        let current_window_id = WindowId::new(command.current_window_id)?;
        let usage_event_id = UsageEventId::new(format!("codex-hook:{session_id}:{turn_id}"))?;
        let result = self.database.turn_observations().reconcile(
            &session_id,
            &turn_id,
            &current_window_id,
            usage_event_id,
            UnixMillis::new(command.observed_at),
        )?;

        Ok(match result {
            ReconcileObservationResult::Attributed {
                amount,
                scope_id,
                window_id,
            } => TurnReconciliationResult {
                status: TurnReconciliationStatus::Attributed,
                amount: Some(amount),
                scope_id: Some(scope_id.to_string()),
                window_id: Some(window_id.to_string()),
            },
            ReconcileObservationResult::NoUsage => {
                empty_reconciliation(TurnReconciliationStatus::NoUsage)
            }
            ReconcileObservationResult::Ambiguous => {
                empty_reconciliation(TurnReconciliationStatus::Ambiguous)
            }
            ReconcileObservationResult::Unmapped => {
                empty_reconciliation(TurnReconciliationStatus::Unmapped)
            }
            ReconcileObservationResult::WindowRolledOver => {
                empty_reconciliation(TurnReconciliationStatus::WindowRolledOver)
            }
            ReconcileObservationResult::SnapshotUnavailable => {
                empty_reconciliation(TurnReconciliationStatus::SnapshotUnavailable)
            }
            ReconcileObservationResult::Missing => {
                empty_reconciliation(TurnReconciliationStatus::Missing)
            }
        })
    }

    pub fn abandon_provider_turn_observation(
        &mut self,
        command: AbandonProviderTurnObservation,
    ) -> ApplicationResult<bool> {
        let session_id = required_request_text(command.session_id, "session ID")?;
        let turn_id = required_request_text(command.turn_id, "turn ID")?;
        Ok(self
            .database
            .turn_observations()
            .abandon(&session_id, &turn_id)?)
    }

    pub fn abandon_provider_session_observations(
        &mut self,
        command: AbandonProviderSessionObservations,
    ) -> ApplicationResult<usize> {
        let session_id = required_request_text(command.session_id, "session ID")?;
        Ok(self
            .database
            .turn_observations()
            .abandon_session(&session_id)?)
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

    pub fn bind_workspace(
        &mut self,
        command: BindWorkspace,
    ) -> ApplicationResult<WorkspaceBindingSummary> {
        let canonical_path = required_request_text(command.canonical_path, "workspace path")?;
        let scope_reference = required_request_text(command.scope_reference, "scope reference")?
            .trim()
            .to_owned();
        let scopes = self.database.catalog().list_scopes()?;
        let mut matches: Vec<_> = scopes
            .iter()
            .filter(|scope| {
                scope.parent_id().is_none()
                    && matches!(
                        scope.kind(),
                        crate::domain::ScopeKind::Workspace | crate::domain::ScopeKind::Project
                    )
                    && (scope.id().as_str() == scope_reference
                        || scope.display_name().eq_ignore_ascii_case(&scope_reference))
            })
            .collect();
        if matches.is_empty() {
            return Err(ApplicationError::NotFound {
                resource: "workspace scope",
                id: scope_reference,
            });
        }
        if matches.len() > 1 {
            return Err(ApplicationError::InvalidRequest {
                message: format!(
                    "workspace scope reference {scope_reference:?} is ambiguous; use the scope ID"
                ),
            });
        }
        let scope = matches.remove(0);
        if let Some(existing) = self
            .database
            .workspace_bindings()
            .get_by_path(&canonical_path)?
        {
            return Err(ApplicationError::InvalidRequest {
                message: format!(
                    "workspace {canonical_path} is already bound to scope {}",
                    existing.scope_id()
                ),
            });
        }
        if let Some(existing) = self
            .database
            .workspace_bindings()
            .list()?
            .into_iter()
            .find(|binding| binding.scope_id() == scope.id())
        {
            return Err(ApplicationError::InvalidRequest {
                message: format!(
                    "workspace scope {} is already bound to {}",
                    scope.id(),
                    existing.canonical_path()
                ),
            });
        }
        let binding = WorkspaceBinding::new(
            canonical_path.clone(),
            scope.id().clone(),
            UnixMillis::new(command.bound_at),
        );
        self.database.workspace_bindings().insert(&binding)?;

        Ok(WorkspaceBindingSummary {
            canonical_path,
            scope_id: scope.id().to_string(),
            scope_display_name: scope.display_name().to_owned(),
            bound_at: command.bound_at,
        })
    }

    pub fn workspace_context(
        &mut self,
        command: GetWorkspaceContext,
    ) -> ApplicationResult<WorkspaceContext> {
        let canonical_path = required_request_text(command.canonical_path, "workspace path")?;
        let scopes = self.database.catalog().list_scopes()?;
        let bindings = self.database.workspace_bindings().list()?;
        let binding = bindings
            .iter()
            .filter(|binding| contains_path(binding.canonical_path(), &canonical_path))
            .max_by_key(|binding| path_depth(binding.canonical_path()))
            .cloned();
        let bound_scope_ids: HashSet<_> = bindings
            .iter()
            .map(|binding| binding.scope_id().clone())
            .collect();
        let available_workspace_scopes = scopes
            .iter()
            .filter(|scope| {
                scope.parent_id().is_none()
                    && matches!(
                        scope.kind(),
                        crate::domain::ScopeKind::Workspace | crate::domain::ScopeKind::Project
                    )
                    && !bound_scope_ids.contains(scope.id())
            })
            .map(|scope| ScopeSummary {
                id: scope.id().to_string(),
                parent_id: scope.parent_id().map(ToString::to_string),
                kind: scope.kind(),
                display_name: scope.display_name().to_owned(),
                workspace_path: None,
            })
            .collect();

        let Some(binding) = binding else {
            return Ok(WorkspaceContext {
                canonical_path,
                binding: None,
                allocations: Vec::new(),
                available_workspace_scopes,
            });
        };
        let scope = scopes
            .iter()
            .find(|scope| scope.id() == binding.scope_id())
            .ok_or_else(|| {
                inconsistent_reference(
                    "workspace binding",
                    binding.canonical_path(),
                    "scope",
                    binding.scope_id(),
                )
            })?;
        let local_state = self.local_state(GetLocalState {
            selected_window_id: None,
            at: command.at,
        })?;
        let mut allocations = Vec::new();
        for source in local_state.sources {
            let dashboard = self.dashboard(GetQuotaDashboard {
                window_id: source.window_id.clone(),
                at: command.at,
            })?;
            if let Some(allocation) = dashboard
                .allocations
                .into_iter()
                .find(|allocation| allocation.scope_id == binding.scope_id().as_str())
            {
                let provider_decision = self.provider_decision(&dashboard.window)?;
                let allocation_decision = allocation.decision;
                allocations.push(WorkspaceAllocationContext {
                    provider_id: source.provider_id,
                    provider_display_name: source.provider_display_name,
                    pool_id: source.pool_id,
                    pool_display_name: source.pool_display_name,
                    window_id: source.window_id,
                    window_is_active: source.is_active,
                    unit: allocation.unit,
                    limit: allocation.limit,
                    remaining: allocation.remaining,
                    spendable: allocation.spendable,
                    provider_capacity: dashboard.window.capacity,
                    provider_remaining: dashboard.window.provider_remaining,
                    provider_spendable: dashboard.window.provider_spendable,
                    allocation_decision,
                    provider_decision,
                    decision: allocation_decision.max(provider_decision),
                });
            }
        }

        Ok(WorkspaceContext {
            canonical_path,
            binding: Some(WorkspaceBindingSummary {
                canonical_path: binding.canonical_path().to_owned(),
                scope_id: scope.id().to_string(),
                scope_display_name: scope.display_name().to_owned(),
                bound_at: binding.bound_at().value(),
            }),
            allocations,
            available_workspace_scopes,
        })
    }

    pub fn evaluate_workspace_admission(
        &mut self,
        command: EvaluateWorkspaceAdmission,
    ) -> ApplicationResult<AdmissionAssessment> {
        let provider_id = required_request_text(command.provider_id, "provider ID")?;
        let context = self.workspace_context(GetWorkspaceContext {
            canonical_path: command.canonical_path,
            at: command.at,
        })?;
        let binding = context
            .binding
            .ok_or_else(|| ApplicationError::InvalidRequest {
                message: format!(
                    "workspace {} is not bound; run `aqm bind --scope <name-or-id>` first",
                    context.canonical_path
                ),
            })?;
        let mut matching: Vec<_> = context
            .allocations
            .into_iter()
            .filter(|allocation| allocation.provider_id.eq_ignore_ascii_case(&provider_id))
            .collect();
        if matching.is_empty() {
            return Err(ApplicationError::InvalidRequest {
                message: format!(
                    "scope {} has no allocation for provider {provider_id}",
                    binding.scope_display_name
                ),
            });
        }
        if matching.len() > 1 {
            return Err(ApplicationError::InvalidRequest {
                message: format!(
                    "scope {} has multiple active allocations for provider {provider_id}; select a quota pool explicitly",
                    binding.scope_display_name
                ),
            });
        }
        let allocation = matching.remove(0);
        if !allocation.window_is_active {
            return Err(ApplicationError::InvalidRequest {
                message: format!(
                    "quota window {} is not active; refresh the provider checkpoint before admission",
                    allocation.window_id
                ),
            });
        }

        Ok(AdmissionAssessment {
            canonical_path: context.canonical_path,
            scope_id: binding.scope_id,
            scope_display_name: binding.scope_display_name,
            provider_id: allocation.provider_id,
            provider_display_name: allocation.provider_display_name,
            pool_id: allocation.pool_id,
            pool_display_name: allocation.pool_display_name,
            window_id: allocation.window_id,
            unit: allocation.unit,
            allocation_limit: allocation.limit,
            allocation_remaining: allocation.remaining,
            provider_capacity: allocation.provider_capacity,
            provider_remaining: allocation.provider_remaining,
            allocation_decision: allocation.allocation_decision,
            provider_decision: allocation.provider_decision,
            decision: allocation.decision,
        })
    }

    fn provider_decision(
        &self,
        window: &WindowSummary,
    ) -> ApplicationResult<crate::domain::EnforcementDecision> {
        let unit = QuotaUnit::new(window.unit.clone())?;
        let balance = QuotaBalance::new(
            QuotaAmount::new(window.capacity, unit.clone()),
            QuotaAmount::new(
                window.capacity.saturating_sub(window.provider_spendable),
                unit.clone(),
            ),
            QuotaAmount::new(0, unit),
        )?;
        Ok(self.policy.evaluate(&balance))
    }

    pub fn create_allocated_workspace(
        &mut self,
        command: CreateAllocatedWorkspace,
    ) -> ApplicationResult<()> {
        let scope = Scope::new(
            ScopeId::new(command.id)?,
            None,
            crate::domain::ScopeKind::Workspace,
            command.display_name,
        )?;
        let allocation = Allocation::new(
            scope.id().clone(),
            WindowId::new(command.window_id)?,
            QuotaAmount::new(command.amount, QuotaUnit::new(command.unit)?),
        );
        let binding = WorkspaceBinding::new(
            required_request_text(command.canonical_path, "workspace path")?,
            scope.id().clone(),
            UnixMillis::new(command.bound_at),
        );

        self.database
            .insert_allocated_workspace(&scope, &allocation, &binding)
            .map_err(map_input_storage_error)?;
        Ok(())
    }

    pub fn create_allocated_scope(
        &mut self,
        command: CreateAllocatedScope,
    ) -> ApplicationResult<()> {
        let scope = Scope::new(
            ScopeId::new(command.id)?,
            command.parent_id.map(ScopeId::new).transpose()?,
            command.kind,
            command.display_name,
        )?;
        let allocation = Allocation::new(
            scope.id().clone(),
            WindowId::new(command.window_id)?,
            QuotaAmount::new(command.amount, QuotaUnit::new(command.unit)?),
        );

        self.database
            .insert_allocated_scope(&scope, &allocation)
            .map_err(map_input_storage_error)?;
        Ok(())
    }

    pub fn set_allocation(&mut self, command: SetAllocation) -> ApplicationResult<()> {
        let allocation = Allocation::new(
            ScopeId::new(command.scope_id)?,
            WindowId::new(command.window_id)?,
            QuotaAmount::new(command.amount, QuotaUnit::new(command.unit)?),
        );
        self.database
            .allocations()
            .set(&allocation)
            .map_err(map_input_storage_error)?;
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
        let provider_snapshot = self.database.provider_quota_snapshot(&window_id)?;
        let unattributed = if provider_snapshot.is_some() {
            self.database
                .ledger()
                .local_unattributed_usage_for_window(&window_id)?
        } else {
            self.database
                .ledger()
                .unattributed_usage_for_window(&window_id)?
        };
        let mut snapshots = Vec::with_capacity(allocations.len());
        let mut allocated_to_root_scopes = 0_u64;
        let mut root_attributed_usage = 0_u64;
        let mut root_active_reservations = 0_u64;

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
                root_attributed_usage = root_attributed_usage
                    .checked_add(balance.attributed_usage().value())
                    .ok_or_else(arithmetic_overflow)?;
                root_active_reservations = root_active_reservations
                    .checked_add(balance.active_reservations().value())
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

        let local_observed_usage = root_attributed_usage
            .checked_add(unattributed.value())
            .ok_or_else(arithmetic_overflow)?;
        let effective_observed_usage = provider_snapshot
            .as_ref()
            .map_or(local_observed_usage, |snapshot| {
                snapshot.used().max(local_observed_usage)
            });
        let effective_unattributed_usage = effective_observed_usage
            .saturating_sub(root_attributed_usage)
            .max(unattributed.value());
        let provider_committed = effective_observed_usage
            .checked_add(root_active_reservations)
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
                unattributed_usage: effective_unattributed_usage,
                provider_remaining: to_view_integer(provider_remaining, "provider remaining")?,
                provider_spendable,
            },
            allocations: snapshots,
        })
    }

    pub fn local_state(&mut self, command: GetLocalState) -> ApplicationResult<LocalState> {
        let at = UnixMillis::new(command.at);
        let catalog = self.database.catalog();
        let providers: HashMap<_, _> = catalog
            .list_providers()?
            .into_iter()
            .map(|provider| (provider.id().clone(), provider))
            .collect();
        let accounts: HashMap<_, _> = catalog
            .list_accounts()?
            .into_iter()
            .map(|account| (account.id().clone(), account))
            .collect();
        let pools = catalog.list_active_quota_pools()?;
        let windows = catalog.list_quota_windows()?;
        let scopes = catalog.list_scopes()?;
        let workspace_paths: HashMap<_, _> = self
            .database
            .workspace_bindings()
            .list()?
            .into_iter()
            .map(|binding| {
                (
                    binding.scope_id().clone(),
                    binding.canonical_path().to_owned(),
                )
            })
            .collect();
        let mut sources = Vec::with_capacity(pools.len());

        for pool in pools {
            let window = windows
                .iter()
                .filter(|window| window.pool_id() == pool.id())
                .find(|window| window.contains(at))
                .or_else(|| {
                    windows
                        .iter()
                        .filter(|window| window.pool_id() == pool.id())
                        .max_by_key(|window| (window.ends_at().value(), window.starts_at().value()))
                });
            let Some(window) = window else {
                continue;
            };
            let account = accounts.get(pool.account_id()).ok_or_else(|| {
                inconsistent_reference("quota pool", pool.id(), "account", pool.account_id())
            })?;
            let provider = providers.get(account.provider_id()).ok_or_else(|| {
                inconsistent_reference("account", account.id(), "provider", account.provider_id())
            })?;

            let snapshot = self.database.provider_quota_snapshot(window.id())?;
            sources.push(QuotaSourceSummary {
                provider_id: provider.id().to_string(),
                provider_display_name: provider.display_name().to_owned(),
                account_id: account.id().to_string(),
                account_display_name: account.display_name().to_owned(),
                pool_id: pool.id().to_string(),
                pool_display_name: pool.display_name().to_owned(),
                window_id: window.id().to_string(),
                starts_at: window.starts_at().value(),
                ends_at: window.ends_at().value(),
                capacity: window.capacity().value(),
                unit: window.capacity().unit().to_string(),
                is_active: window.contains(at),
                provider_managed: snapshot.is_some(),
                last_synced_at: snapshot.map(|snapshot| snapshot.observed_at().value()),
            });
        }

        let requested_window_id = command.selected_window_id.map(WindowId::new).transpose()?;
        let selected_window_id = match requested_window_id {
            Some(requested) => {
                let selected_source = sources
                    .iter()
                    .find(|source| source.window_id == requested.as_str())
                    .or_else(|| {
                        let requested_pool_id = windows
                            .iter()
                            .find(|window| window.id() == &requested)
                            .map(|window| window.pool_id())?;
                        sources
                            .iter()
                            .find(|source| source.pool_id.as_str() == requested_pool_id.as_str())
                    })
                    .ok_or_else(|| ApplicationError::NotFound {
                        resource: "quota window",
                        id: requested.to_string(),
                    })?;
                Some(selected_source.window_id.clone())
            }
            None => sources
                .iter()
                .find(|source| source.is_active)
                .or_else(|| sources.first())
                .map(|source| source.window_id.clone()),
        };
        let dashboard = selected_window_id
            .as_ref()
            .map(|window_id| {
                self.dashboard(GetQuotaDashboard {
                    window_id: window_id.clone(),
                    at: command.at,
                })
            })
            .transpose()?;

        Ok(LocalState {
            sources,
            scopes: scopes
                .into_iter()
                .map(|scope| ScopeSummary {
                    id: scope.id().to_string(),
                    parent_id: scope.parent_id().map(ToString::to_string),
                    kind: scope.kind(),
                    display_name: scope.display_name().to_owned(),
                    workspace_path: workspace_paths.get(scope.id()).cloned(),
                })
                .collect(),
            selected_window_id,
            dashboard,
        })
    }
}

fn arithmetic_overflow() -> ApplicationError {
    crate::domain::DomainError::ArithmeticOverflow.into()
}

fn empty_reconciliation(status: TurnReconciliationStatus) -> TurnReconciliationResult {
    TurnReconciliationResult {
        status,
        amount: None,
        scope_id: None,
        window_id: None,
    }
}

fn required_request_text(value: String, field: &str) -> ApplicationResult<String> {
    if value.trim().is_empty() {
        return Err(ApplicationError::InvalidRequest {
            message: format!("{field} cannot be empty"),
        });
    }
    Ok(value)
}

fn map_input_storage_error(error: StorageError) -> ApplicationError {
    match error {
        StorageError::Domain(error) => ApplicationError::Validation(error),
        error => ApplicationError::Storage(error),
    }
}

fn inconsistent_reference(
    owner_kind: &str,
    owner_id: impl std::fmt::Display,
    missing_kind: &str,
    missing_id: impl std::fmt::Display,
) -> ApplicationError {
    ApplicationError::InconsistentData {
        message: format!("{owner_kind} {owner_id} references missing {missing_kind} {missing_id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::{
            ArchiveQuotaSource, BindWorkspace, CreateAccount, CreateAllocatedScope,
            CreateAllocatedWorkspace, CreateProvider, CreateQuotaPool, CreateQuotaSource,
            CreateQuotaWindow, CreateScope, EvaluateWorkspaceAdmission, GetLocalState,
            GetQuotaDashboard, GetWorkspaceContext, ProviderQuotaSnapshotInput, RecordUsage,
            ReserveQuota, SetAllocation, SyncProviderQuota,
        },
        domain::{Confidence, EnforcementDecision, ScopeKind, UsageSource},
        storage::Database,
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

    fn detected_codex_source(suffix: &str) -> CreateQuotaSource {
        CreateQuotaSource {
            provider_id: format!("codex-{suffix}"),
            provider_display_name: "Codex".to_owned(),
            account_id: format!("codex-subscription-{suffix}"),
            account_display_name: "Subscription".to_owned(),
            pool_id: format!("codex-weekly-{suffix}"),
            pool_display_name: "Weekly allowance".to_owned(),
            window_id: format!("week-{suffix}"),
            starts_at: 1_000,
            ends_at: 10_000,
            capacity: 100,
            unit: "percent".to_owned(),
            provider_snapshot: Some(ProviderQuotaSnapshotInput {
                adapter: "codex_app_server".to_owned(),
                remote_limit_id: "codex".to_owned(),
                remote_window_kind: "secondary".to_owned(),
                used: 24,
                observed_at: 2_000,
                resets_at: 10_000,
            }),
        }
    }

    fn add_workspace_allocation(service: &mut QuotaService) {
        service
            .create_allocated_scope(CreateAllocatedScope {
                id: "workspace-a".to_owned(),
                parent_id: None,
                kind: ScopeKind::Workspace,
                display_name: "Workspace A".to_owned(),
                window_id: "week-1".to_owned(),
                amount: 30,
                unit: "quota_points".to_owned(),
            })
            .unwrap();
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

    #[test]
    fn local_state_is_empty_before_onboarding() {
        let mut service = QuotaService::new(Database::open_in_memory().unwrap());

        let state = service
            .local_state(GetLocalState {
                selected_window_id: None,
                at: 3_000,
            })
            .unwrap();

        assert!(state.sources.is_empty());
        assert!(state.scopes.is_empty());
        assert_eq!(state.selected_window_id, None);
        assert_eq!(state.dashboard, None);
    }

    #[test]
    fn local_state_rehydrates_sources_scopes_and_the_active_dashboard() {
        let mut service = configured_service();

        let state = service
            .local_state(GetLocalState {
                selected_window_id: None,
                at: 3_000,
            })
            .unwrap();

        assert_eq!(state.sources.len(), 1);
        assert_eq!(state.sources[0].provider_display_name, "Codex");
        assert!(state.sources[0].is_active);
        assert_eq!(state.scopes.len(), 2);
        assert_eq!(state.selected_window_id.as_deref(), Some("week-1"));
        assert_eq!(state.dashboard.unwrap().window.capacity, 100);
    }

    #[test]
    fn workspace_context_is_unmapped_until_explicitly_bound() {
        let mut service = configured_service();
        add_workspace_allocation(&mut service);

        let context = service
            .workspace_context(GetWorkspaceContext {
                canonical_path: "/code/workspace-a".to_owned(),
                at: 3_000,
            })
            .unwrap();

        assert!(context.binding.is_none());
        assert!(context.allocations.is_empty());
        assert_eq!(context.available_workspace_scopes.len(), 2);
        assert!(context
            .available_workspace_scopes
            .iter()
            .any(|scope| scope.id == "workspace-a"));
    }

    #[test]
    fn workspace_binding_resolves_descendant_paths_and_active_allocations() {
        let mut service = configured_service();
        add_workspace_allocation(&mut service);

        let binding = service
            .bind_workspace(BindWorkspace {
                canonical_path: "/code/workspace-a".to_owned(),
                scope_reference: "Workspace A".to_owned(),
                bound_at: 2_000,
            })
            .unwrap();
        let context = service
            .workspace_context(GetWorkspaceContext {
                canonical_path: "/code/workspace-a/frontend/src".to_owned(),
                at: 3_000,
            })
            .unwrap();
        let state = service
            .local_state(GetLocalState {
                selected_window_id: None,
                at: 3_000,
            })
            .unwrap();

        assert_eq!(binding.scope_id, "workspace-a");
        assert_eq!(context.binding.unwrap().scope_id, "workspace-a");
        assert_eq!(context.allocations.len(), 1);
        assert_eq!(context.allocations[0].provider_display_name, "Codex");
        assert_eq!(context.allocations[0].provider_id, "codex");
        assert_eq!(context.allocations[0].limit, 30);
        assert_eq!(context.allocations[0].remaining, 30);
        assert_eq!(context.allocations[0].decision, EnforcementDecision::Allow);
        assert_eq!(
            state
                .scopes
                .iter()
                .find(|scope| scope.id == "workspace-a")
                .unwrap()
                .workspace_path
                .as_deref(),
            Some("/code/workspace-a")
        );
    }

    #[test]
    fn nearest_workspace_binding_wins_for_nested_folders() {
        let mut service = configured_service();
        add_workspace_allocation(&mut service);
        service
            .bind_workspace(BindWorkspace {
                canonical_path: "/code".to_owned(),
                scope_reference: "project-a".to_owned(),
                bound_at: 1_500,
            })
            .unwrap();
        service
            .bind_workspace(BindWorkspace {
                canonical_path: "/code/workspace-a".to_owned(),
                scope_reference: "workspace-a".to_owned(),
                bound_at: 2_000,
            })
            .unwrap();

        let context = service
            .workspace_context(GetWorkspaceContext {
                canonical_path: "/code/workspace-a/frontend".to_owned(),
                at: 3_000,
            })
            .unwrap();

        assert_eq!(context.binding.unwrap().scope_id, "workspace-a");
    }

    #[test]
    fn admission_uses_the_more_restrictive_provider_capacity_decision() {
        let mut service = configured_service();
        add_workspace_allocation(&mut service);
        service
            .bind_workspace(BindWorkspace {
                canonical_path: "/code/workspace-a".to_owned(),
                scope_reference: "workspace-a".to_owned(),
                bound_at: 2_000,
            })
            .unwrap();
        service
            .record_usage(RecordUsage {
                id: "external-usage".to_owned(),
                window_id: "week-1".to_owned(),
                scope_id: None,
                amount: 95,
                unit: "quota_points".to_owned(),
                observed_at: 2_500,
                source: UsageSource::ProviderConfirmed,
                confidence: Confidence::Confirmed,
                reservation_id: None,
            })
            .unwrap();

        let assessment = service
            .evaluate_workspace_admission(EvaluateWorkspaceAdmission {
                canonical_path: "/code/workspace-a/src".to_owned(),
                provider_id: "codex".to_owned(),
                at: 3_000,
            })
            .unwrap();

        assert_eq!(assessment.allocation_decision, EnforcementDecision::Allow);
        assert_eq!(
            assessment.provider_decision,
            EnforcementDecision::RequireConfirmation
        );
        assert_eq!(
            assessment.decision,
            EnforcementDecision::RequireConfirmation
        );
        assert_eq!(assessment.allocation_remaining, 30);
        assert_eq!(assessment.provider_remaining, 5);
    }

    #[test]
    fn admission_honors_workspace_usage_when_it_is_more_restrictive() {
        let mut service = configured_service();
        add_workspace_allocation(&mut service);
        service
            .bind_workspace(BindWorkspace {
                canonical_path: "/code/workspace-a".to_owned(),
                scope_reference: "workspace-a".to_owned(),
                bound_at: 2_000,
            })
            .unwrap();
        service
            .record_usage(RecordUsage {
                id: "workspace-usage".to_owned(),
                window_id: "week-1".to_owned(),
                scope_id: Some("workspace-a".to_owned()),
                amount: 24,
                unit: "quota_points".to_owned(),
                observed_at: 2_500,
                source: UsageSource::LocalMeasured,
                confidence: Confidence::Observed,
                reservation_id: None,
            })
            .unwrap();

        let assessment = service
            .evaluate_workspace_admission(EvaluateWorkspaceAdmission {
                canonical_path: "/code/workspace-a".to_owned(),
                provider_id: "codex".to_owned(),
                at: 3_000,
            })
            .unwrap();

        assert_eq!(assessment.allocation_decision, EnforcementDecision::Warn);
        assert_eq!(assessment.provider_decision, EnforcementDecision::Allow);
        assert_eq!(assessment.decision, EnforcementDecision::Warn);
    }

    #[test]
    fn admission_requires_an_explicit_workspace_binding() {
        let mut service = configured_service();
        add_workspace_allocation(&mut service);

        let error = service
            .evaluate_workspace_admission(EvaluateWorkspaceAdmission {
                canonical_path: "/code/workspace-a".to_owned(),
                provider_id: "codex".to_owned(),
                at: 3_000,
            })
            .unwrap_err();

        assert!(error.to_string().contains("is not bound"));
    }

    #[test]
    fn workspace_bindings_reject_duplicate_paths_and_scope_reuse() {
        let mut service = configured_service();
        add_workspace_allocation(&mut service);
        service
            .bind_workspace(BindWorkspace {
                canonical_path: "/code/workspace-a".to_owned(),
                scope_reference: "workspace-a".to_owned(),
                bound_at: 2_000,
            })
            .unwrap();

        let duplicate_path = service.bind_workspace(BindWorkspace {
            canonical_path: "/code/workspace-a".to_owned(),
            scope_reference: "workspace-a".to_owned(),
            bound_at: 3_000,
        });
        let duplicate_scope = service.bind_workspace(BindWorkspace {
            canonical_path: "/code/another".to_owned(),
            scope_reference: "workspace-a".to_owned(),
            bound_at: 3_000,
        });

        assert!(matches!(
            duplicate_path,
            Err(ApplicationError::InvalidRequest { .. })
        ));
        assert!(matches!(
            duplicate_scope,
            Err(ApplicationError::InvalidRequest { .. })
        ));
    }

    #[test]
    fn allocated_workspace_creation_saves_scope_allocation_and_binding_atomically() {
        let mut service = configured_service();

        service
            .create_allocated_workspace(CreateAllocatedWorkspace {
                id: "workspace-a".to_owned(),
                display_name: "Workspace A".to_owned(),
                canonical_path: "/code/workspace-a".to_owned(),
                window_id: "week-1".to_owned(),
                amount: 30,
                unit: "quota_points".to_owned(),
                bound_at: 2_000,
            })
            .unwrap();
        let context = service
            .workspace_context(GetWorkspaceContext {
                canonical_path: "/code/workspace-a/nested".to_owned(),
                at: 3_000,
            })
            .unwrap();

        assert_eq!(context.binding.unwrap().scope_id, "workspace-a");
        assert_eq!(context.allocations[0].limit, 30);

        let duplicate = service.create_allocated_workspace(CreateAllocatedWorkspace {
            id: "workspace-rolled-back".to_owned(),
            display_name: "Rolled Back".to_owned(),
            canonical_path: "/code/workspace-a".to_owned(),
            window_id: "week-1".to_owned(),
            amount: 0,
            unit: "quota_points".to_owned(),
            bound_at: 3_000,
        });
        assert!(duplicate.is_err());
        let state = service
            .local_state(GetLocalState {
                selected_window_id: None,
                at: 3_000,
            })
            .unwrap();
        assert!(!state
            .scopes
            .iter()
            .any(|scope| scope.id == "workspace-rolled-back"));
    }

    #[test]
    fn quota_source_onboarding_creates_the_complete_catalog_chain() {
        let mut service = QuotaService::new(Database::open_in_memory().unwrap());

        service
            .create_quota_source(detected_codex_source("1"))
            .unwrap();

        let state = service
            .local_state(GetLocalState {
                selected_window_id: None,
                at: 3_000,
            })
            .unwrap();

        assert_eq!(state.sources.len(), 1);
        assert_eq!(state.sources[0].unit, "percent");
        let dashboard = state.dashboard.unwrap();
        assert_eq!(dashboard.window.unattributed_usage, 24);
        assert_eq!(dashboard.window.provider_remaining, 76);
    }

    #[test]
    fn provider_managed_source_cannot_be_added_twice() {
        let mut service = QuotaService::new(Database::open_in_memory().unwrap());
        service
            .create_quota_source(detected_codex_source("first"))
            .unwrap();

        let error = service
            .create_quota_source(detected_codex_source("duplicate"))
            .unwrap_err();

        assert!(matches!(
            error,
            ApplicationError::Storage(StorageError::DuplicateSource { .. })
        ));
        let state = service
            .local_state(GetLocalState {
                selected_window_id: None,
                at: 3_000,
            })
            .unwrap();
        assert_eq!(state.sources.len(), 1);
    }

    #[test]
    fn archived_source_is_hidden_but_history_remains_and_source_can_be_added_again() {
        let mut service = QuotaService::new(Database::open_in_memory().unwrap());
        let source = detected_codex_source("first");
        let archived_pool_id = source.pool_id.clone();
        let archived_window_id = source.window_id.clone();
        service.create_quota_source(source).unwrap();

        service
            .archive_quota_source(ArchiveQuotaSource {
                pool_id: archived_pool_id,
                archived_at: 4_000,
            })
            .unwrap();

        let state = service
            .local_state(GetLocalState {
                selected_window_id: None,
                at: 4_000,
            })
            .unwrap();
        assert!(state.sources.is_empty());
        assert_eq!(
            service
                .dashboard(GetQuotaDashboard {
                    window_id: archived_window_id,
                    at: 4_000,
                })
                .unwrap()
                .window
                .provider_remaining,
            76
        );

        service
            .create_quota_source(detected_codex_source("replacement"))
            .unwrap();
        assert_eq!(
            service
                .local_state(GetLocalState {
                    selected_window_id: None,
                    at: 4_000,
                })
                .unwrap()
                .sources
                .len(),
            1
        );
    }

    #[test]
    fn provider_sync_replaces_the_absolute_snapshot_instead_of_adding_usage() {
        let mut service = configured_service();
        service
            .record_usage(RecordUsage {
                id: "old-provider-reading".to_owned(),
                window_id: "week-1".to_owned(),
                scope_id: None,
                amount: 68,
                unit: "quota_points".to_owned(),
                observed_at: 2_000,
                source: UsageSource::ProviderConfirmed,
                confidence: Confidence::Confirmed,
                reservation_id: None,
            })
            .unwrap();

        for (used, expected_remaining) in [(73, 27), (74, 26), (65, 35)] {
            service
                .sync_provider_quota(SyncProviderQuota {
                    current_window_id: "week-1".to_owned(),
                    adapter: "codex_app_server".to_owned(),
                    remote_limit_id: "codex".to_owned(),
                    remote_window_kind: "secondary".to_owned(),
                    starts_at: 1_000,
                    ends_at: 10_000,
                    capacity: 100,
                    used,
                    unit: "quota_points".to_owned(),
                    observed_at: 3_000,
                })
                .unwrap();

            let dashboard = service
                .dashboard(GetQuotaDashboard {
                    window_id: "week-1".to_owned(),
                    at: 3_000,
                })
                .unwrap();
            assert_eq!(dashboard.window.unattributed_usage, used);
            assert_eq!(dashboard.window.provider_remaining, expected_remaining);
        }
    }

    #[test]
    fn provider_reset_rolls_allocations_into_a_fresh_window() {
        let mut service = configured_service();
        let result = service
            .sync_provider_quota(SyncProviderQuota {
                current_window_id: "week-1".to_owned(),
                adapter: "codex_app_server".to_owned(),
                remote_limit_id: "codex".to_owned(),
                remote_window_kind: "secondary".to_owned(),
                starts_at: 10_000,
                ends_at: 20_000,
                capacity: 100,
                used: 4,
                unit: "quota_points".to_owned(),
                observed_at: 11_000,
            })
            .unwrap();

        assert!(result.rolled_over);
        let dashboard = service
            .dashboard(GetQuotaDashboard {
                window_id: result.window_id,
                at: 11_000,
            })
            .unwrap();
        assert_eq!(dashboard.window.provider_remaining, 96);
        assert_eq!(snapshot(&dashboard, "project-a").limit, 70);
        assert_eq!(snapshot(&dashboard, "feature-a").limit, 50);

        let state = service
            .local_state(GetLocalState {
                selected_window_id: None,
                at: 11_000,
            })
            .unwrap();
        assert_eq!(state.sources.len(), 1);
        assert_eq!(
            state.selected_window_id.as_deref(),
            Some("codex-weekly-window-20000")
        );
    }

    #[test]
    fn local_state_recovers_a_stale_window_selection_after_rollover() {
        let mut service = configured_service();
        let result = service
            .sync_provider_quota(SyncProviderQuota {
                current_window_id: "week-1".to_owned(),
                adapter: "codex_app_server".to_owned(),
                remote_limit_id: "codex".to_owned(),
                remote_window_kind: "secondary".to_owned(),
                starts_at: 10_000,
                ends_at: 20_000,
                capacity: 100,
                used: 4,
                unit: "quota_points".to_owned(),
                observed_at: 11_000,
            })
            .unwrap();

        let state = service
            .local_state(GetLocalState {
                selected_window_id: Some("week-1".to_owned()),
                at: 11_000,
            })
            .unwrap();

        assert_eq!(
            state.selected_window_id.as_deref(),
            Some(result.window_id.as_str())
        );
        assert_eq!(state.dashboard.unwrap().window.id, result.window_id);
    }

    #[test]
    fn allocated_scope_creation_rolls_back_when_capacity_is_exceeded() {
        let mut service = configured_service();

        let error = service
            .create_allocated_scope(CreateAllocatedScope {
                id: "project-b".to_owned(),
                parent_id: None,
                kind: ScopeKind::Project,
                display_name: "Project B".to_owned(),
                window_id: "week-1".to_owned(),
                amount: 40,
                unit: "quota_points".to_owned(),
            })
            .unwrap_err();
        let state = service
            .local_state(GetLocalState {
                selected_window_id: Some("week-1".to_owned()),
                at: 3_000,
            })
            .unwrap();

        assert!(matches!(error, ApplicationError::Validation(_)));
        assert!(!state.scopes.iter().any(|scope| scope.id == "project-b"));
    }
}
