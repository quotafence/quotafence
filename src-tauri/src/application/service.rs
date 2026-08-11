use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use crate::{
    domain::{
        Account, AccountId, Allocation, BasisPoints, EnforcementPolicy, Provider, ProviderId,
        QuotaAmount, QuotaBalance, QuotaPool, QuotaPoolId, QuotaUnit, QuotaWindow, Reservation,
        ReservationId, Scope, ScopeId, UnixMillis, UsageAttribution, UsageEvent, UsageEventId,
        WindowId,
    },
    storage::{
        BeginObservationStatus, CodexProtectionEventOutcome, Database, DesktopReconciliationStatus,
        DesktopThreadObservation, ManagedSessionReconciliationOutcome,
        ManagedSessionReconciliationResult, ManagedSessionStatus, NewCodexProtectionEvent,
        NewManagedSession, ProviderQuotaSnapshot, ProviderTurnObservation,
        ReconcileObservationResult, StorageError, WorkspaceBinding, WorkspacePolicy,
    },
    workspace::{contains_path, path_depth},
};

use super::forecast::{calculate_depletion_forecast, ForecastInput, ManagedUsageSample};
use super::{
    error::to_view_integer, AbandonProviderSessionObservations, AbandonProviderTurnObservation,
    ActiveManagedSession, AdmissionAssessment, AllocationSnapshot, ApplicationError,
    ApplicationResult, ArchiveQuotaSource, BeginProviderTurnObservation, BindWorkspace,
    CodexProtectionEventSummary, CreateAccount, CreateAllocatedScope, CreateAllocatedWorkspace,
    CreateProvider, CreateQuotaPool, CreateQuotaSource, CreateQuotaWindow, CreateScope,
    DesktopUsageReconciliation, DesktopUsageReconciliationStatus, EvaluateWorkspaceAdmission,
    FinishManagedSession, GetCodexProtectionEvents, GetLocalState, GetProviderTurnObservation,
    GetQuotaDashboard, GetWorkspaceContext, GetWorkspacePolicy, LocalState, ManagedSessionLaunch,
    ManagedSessionOutcome, ManagedSessionReconciliation, ManagedSessionReconciliationStatus,
    MarkManagedSessionRunning, PolicySummary, PrepareManagedSession,
    ProviderTurnObservationSummary, QuotaDashboard, QuotaHistoryPoint, QuotaSourceSummary,
    ReconcileProviderTurnObservation, RecordCodexProtectionEvent, RecordUsage, ReleaseReservation,
    RemoveWorkspaceAllocation, ReserveQuota, ResetWorkspacePolicy, ScopeSummary, SetAllocation,
    SetAllocationPriorityOrder, SetWorkspacePolicy, SyncProviderQuota, SyncProviderQuotaResult,
    TurnObservationStartResult, TurnObservationStartStatus, TurnReconciliationResult,
    TurnReconciliationStatus, WindowSummary, WorkspaceAllocationContext, WorkspaceBindingSummary,
    WorkspaceContext, WorkspacePolicySummary,
};

const TURN_OBSERVATION_STALE_AFTER_MILLIS: i64 = 12 * 60 * 60 * 1_000;

pub struct QuotaService {
    database: Database,
    default_policy: EnforcementPolicy,
}

impl QuotaService {
    pub fn open(path: impl AsRef<Path>) -> ApplicationResult<Self> {
        Ok(Self::new(Database::open(path)?))
    }

    pub fn new(database: Database) -> Self {
        Self::with_policy(database, EnforcementPolicy::standard())
    }

    pub fn with_policy(database: Database, policy: EnforcementPolicy) -> Self {
        Self {
            database,
            default_policy: policy,
        }
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

        let desktop_observations = command.desktop_observations.map(|observations| {
            observations
                .into_iter()
                .map(|observation| DesktopThreadObservation {
                    thread_id: observation.thread_id,
                    canonical_path: observation.canonical_path,
                    total_tokens: observation.total_tokens,
                    updated_at: observation.updated_at,
                })
                .collect::<Vec<_>>()
        });
        let desktop_reconciliation = self.database.sync_provider_quota(
            &current_window_id,
            &target_window,
            &snapshot,
            desktop_observations.as_deref(),
        )?;

        Ok(SyncProviderQuotaResult {
            window_id: target_window_id.to_string(),
            rolled_over,
            desktop_reconciliation: desktop_reconciliation.map(|desktop_reconciliation| {
                DesktopUsageReconciliation {
                    status: match desktop_reconciliation.status {
                        DesktopReconciliationStatus::BaselineEstablished => {
                            DesktopUsageReconciliationStatus::BaselineEstablished
                        }
                        DesktopReconciliationStatus::NoActivity => {
                            DesktopUsageReconciliationStatus::NoActivity
                        }
                        DesktopReconciliationStatus::PendingProviderDelta => {
                            DesktopUsageReconciliationStatus::PendingProviderDelta
                        }
                        DesktopReconciliationStatus::Attributed => {
                            DesktopUsageReconciliationStatus::Attributed
                        }
                        DesktopReconciliationStatus::Ambiguous => {
                            DesktopUsageReconciliationStatus::Ambiguous
                        }
                        DesktopReconciliationStatus::WindowRolledOver => {
                            DesktopUsageReconciliationStatus::WindowRolledOver
                        }
                    },
                    observed_threads: desktop_reconciliation.observed_threads,
                    pending_tokens: desktop_reconciliation.pending_tokens,
                    attributed_amount: desktop_reconciliation.attributed_amount,
                    scope_id: desktop_reconciliation
                        .scope_id
                        .map(|scope| scope.to_string()),
                }
            }),
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
                turn_id: observation.turn_id().to_owned(),
                canonical_path: observation.canonical_path().to_owned(),
                window_id: observation.window_id().to_string(),
                scope_id: observation.scope_id().map(ToString::to_string),
                contended: observation.contended(),
            }))
    }

    pub fn previous_provider_turn_observation(
        &mut self,
        session_id: &str,
        current_turn_id: &str,
    ) -> ApplicationResult<Option<ProviderTurnObservationSummary>> {
        let session_id = required_request_text(session_id.to_owned(), "session ID")?;
        let current_turn_id = required_request_text(current_turn_id.to_owned(), "turn ID")?;
        Ok(self
            .database
            .turn_observations()
            .latest_for_session_except(&session_id, &current_turn_id)?
            .map(|observation| ProviderTurnObservationSummary {
                turn_id: observation.turn_id().to_owned(),
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

    pub fn record_codex_protection_event(
        &mut self,
        command: RecordCodexProtectionEvent,
    ) -> ApplicationResult<()> {
        let event = NewCodexProtectionEvent {
            session_id: required_request_text(command.session_id, "session ID")?,
            turn_id: required_request_text(command.turn_id, "turn ID")?,
            canonical_path: required_request_text(command.canonical_path, "workspace path")?,
            scope_id: command
                .scope_id
                .map(ScopeId::new)
                .transpose()?
                .map(|scope_id| scope_id.to_string()),
            outcome: if command.blocked {
                CodexProtectionEventOutcome::Blocked
            } else {
                CodexProtectionEventOutcome::Allowed
            },
            reason: required_request_text(command.reason, "protection reason")?,
            occurred_at: command.occurred_at,
        };
        self.database.codex_protection_events().record(&event)?;
        Ok(())
    }

    pub fn codex_protection_events(
        &self,
        command: GetCodexProtectionEvents,
    ) -> ApplicationResult<Vec<CodexProtectionEventSummary>> {
        let limit = command.limit.clamp(1, 100);
        Ok(self
            .database
            .codex_protection_events()
            .list_recent(limit)?
            .into_iter()
            .map(|event| CodexProtectionEventSummary {
                canonical_path: event.canonical_path,
                scope_id: event.scope_id,
                workspace_name: event.workspace_name,
                outcome: match event.outcome {
                    CodexProtectionEventOutcome::Allowed => "allowed",
                    CodexProtectionEventOutcome::Blocked => "blocked",
                }
                .to_owned(),
                reason: event.reason,
                occurred_at: event.occurred_at,
            })
            .collect())
    }

    pub fn codex_session_workspace(&self, session_id: &str) -> ApplicationResult<Option<String>> {
        let session_id = required_request_text(session_id.to_owned(), "session ID")?;
        Ok(self
            .database
            .codex_protection_events()
            .last_bound_workspace_for_session(&session_id)?)
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
        let (workspace_policy, _) = self.effective_policy(binding.scope_id())?;
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
                let provider_decision =
                    self.provider_decision(&dashboard.window, &workspace_policy)?;
                let allocation_decision = if allocation.protected_now == 0 {
                    crate::domain::EnforcementDecision::Stop
                } else {
                    allocation.decision
                };
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
                    protected_now: allocation.protected_now,
                    provider_capacity: dashboard.window.capacity,
                    provider_remaining: dashboard.window.provider_remaining,
                    provider_spendable: dashboard.window.provider_spendable,
                    allocation_decision,
                    provider_decision,
                    decision: allocation_decision.max(provider_decision),
                    policy: allocation.policy,
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
            protected_now: allocation.protected_now,
            provider_capacity: allocation.provider_capacity,
            provider_remaining: allocation.provider_remaining,
            allocation_decision: allocation.allocation_decision,
            provider_decision: allocation.provider_decision,
            decision: allocation.decision,
            policy: allocation.policy,
        })
    }

    pub fn prepare_managed_session(
        &mut self,
        command: PrepareManagedSession,
    ) -> ApplicationResult<ManagedSessionLaunch> {
        let session_id = required_request_text(command.id, "managed session ID")?;
        let reservation_id = required_request_text(command.reservation_id, "reservation ID")?;
        let provider_id = required_request_text(command.provider_id, "provider ID")?;
        if command.supervisor_pid == 0 {
            return Err(ApplicationError::InvalidRequest {
                message: "managed session supervisor PID must be greater than zero".to_owned(),
            });
        }

        let assessment = self.evaluate_workspace_admission(EvaluateWorkspaceAdmission {
            canonical_path: command.canonical_path,
            provider_id: provider_id.clone(),
            at: command.admitted_at,
        })?;
        match assessment.decision {
            crate::domain::EnforcementDecision::Stop => {
                return Err(ApplicationError::InvalidRequest {
                    message: format!(
                        "AQM refused to launch {} because the workspace or provider budget is exhausted",
                        assessment.provider_display_name
                    ),
                });
            }
            crate::domain::EnforcementDecision::RequireConfirmation if !command.assume_yes => {
                return Err(ApplicationError::InvalidRequest {
                    message: "managed launch requires confirmation; re-run with --yes".to_owned(),
                });
            }
            _ => {}
        }

        let reserved_amount = u64::try_from(assessment.allocation_remaining).map_err(|_| {
            ApplicationError::InvalidRequest {
                message: format!(
                    "workspace {} has no spendable quota to reserve",
                    assessment.scope_display_name
                ),
            }
        })?;
        if reserved_amount == 0 {
            return Err(ApplicationError::InvalidRequest {
                message: format!(
                    "workspace {} has no spendable quota to reserve",
                    assessment.scope_display_name
                ),
            });
        }

        let admitted_at = UnixMillis::new(command.admitted_at);
        let baseline = self
            .database
            .provider_quota_snapshot(&WindowId::new(assessment.window_id.clone())?)?
            .ok_or_else(|| ApplicationError::InvalidRequest {
                message: format!(
                    "quota window {} has no provider checkpoint for managed admission",
                    assessment.window_id
                ),
            })?;
        let reservation = Reservation::new(
            ReservationId::new(reservation_id.clone())?,
            ScopeId::new(assessment.scope_id.clone())?,
            WindowId::new(assessment.window_id.clone())?,
            QuotaAmount::new(reserved_amount, QuotaUnit::new(assessment.unit.clone())?),
            admitted_at,
            UnixMillis::new(command.expires_at),
        )?;
        let session = NewManagedSession {
            id: session_id.clone(),
            adapter: baseline.adapter().to_owned(),
            pool_id: assessment.pool_id.clone(),
            window_id: assessment.window_id.clone(),
            scope_id: assessment.scope_id.clone(),
            reservation_id: reservation_id.clone(),
            canonical_path: assessment.canonical_path.clone(),
            supervisor_pid: command.supervisor_pid,
            created_at: command.admitted_at,
            baseline_used: baseline.used(),
            baseline_observed_at: baseline.observed_at().value(),
        };
        self.database.start_managed_session(
            &reservation,
            &session,
            admitted_at,
            assessment.decision == crate::domain::EnforcementDecision::RequireConfirmation
                && command.assume_yes,
        )?;

        Ok(ManagedSessionLaunch {
            session_id,
            reservation_id,
            reserved_amount,
            assessment,
        })
    }

    pub fn mark_managed_session_running(
        &mut self,
        command: MarkManagedSessionRunning,
    ) -> ApplicationResult<()> {
        if command.child_pid == 0 {
            return Err(ApplicationError::InvalidRequest {
                message: "managed session child PID must be greater than zero".to_owned(),
            });
        }
        self.database.mark_managed_session_running(
            &required_request_text(command.id, "managed session ID")?,
            command.child_pid,
            command.started_at,
        )?;
        Ok(())
    }

    pub fn workspace_policy(
        &mut self,
        command: GetWorkspacePolicy,
    ) -> ApplicationResult<WorkspacePolicySummary> {
        let context = self.workspace_context(GetWorkspaceContext {
            canonical_path: command.canonical_path,
            at: command.at,
        })?;
        let binding = context
            .binding
            .ok_or_else(|| ApplicationError::InvalidRequest {
                message: format!("workspace {} is not bound", context.canonical_path),
            })?;
        let (_, policy) = self.effective_policy(&ScopeId::new(binding.scope_id.clone())?)?;
        Ok(WorkspacePolicySummary {
            canonical_path: binding.canonical_path,
            scope_id: binding.scope_id,
            scope_display_name: binding.scope_display_name,
            policy,
        })
    }

    pub fn set_workspace_policy(
        &mut self,
        command: SetWorkspacePolicy,
    ) -> ApplicationResult<PolicySummary> {
        let scope_id = ScopeId::new(command.scope_id)?;
        self.ensure_bound_workspace_scope(&scope_id)?;
        let policy = EnforcementPolicy::new(
            command
                .warn_at_basis_points
                .map(BasisPoints::new)
                .transpose()?,
            command
                .confirm_at_basis_points
                .map(BasisPoints::new)
                .transpose()?,
            command
                .stop_at_basis_points
                .map(BasisPoints::new)
                .transpose()?,
        )?;
        self.database
            .workspace_policies()
            .set(&WorkspacePolicy::new(
                scope_id.clone(),
                policy,
                UnixMillis::new(command.updated_at),
            ))?;
        let (_, summary) = self.effective_policy(&scope_id)?;
        Ok(summary)
    }

    pub fn reset_workspace_policy(
        &mut self,
        command: ResetWorkspacePolicy,
    ) -> ApplicationResult<PolicySummary> {
        let scope_id = ScopeId::new(command.scope_id)?;
        self.ensure_bound_workspace_scope(&scope_id)?;
        self.database.workspace_policies().reset(&scope_id)?;
        let (_, summary) = self.effective_policy(&scope_id)?;
        Ok(summary)
    }

    pub fn finish_managed_session(
        &mut self,
        command: FinishManagedSession,
    ) -> ApplicationResult<ManagedSessionReconciliation> {
        let status = match command.outcome {
            ManagedSessionOutcome::Completed => ManagedSessionStatus::Completed,
            ManagedSessionOutcome::Failed => ManagedSessionStatus::Failed,
            ManagedSessionOutcome::Interrupted => ManagedSessionStatus::Interrupted,
        };
        let current_window_id = command.current_window_id.map(WindowId::new).transpose()?;
        let result = self.database.finish_managed_session(
            &required_request_text(command.id, "managed session ID")?,
            status,
            command.finished_at,
            command.exit_code,
            current_window_id.as_ref(),
        )?;
        Ok(match result {
            ManagedSessionReconciliationResult::Attributed { amount, scope_id } => {
                ManagedSessionReconciliation {
                    status: ManagedSessionReconciliationStatus::Attributed,
                    amount: Some(amount),
                    scope_id: Some(scope_id.to_string()),
                }
            }
            ManagedSessionReconciliationResult::NoUsage => ManagedSessionReconciliation {
                status: ManagedSessionReconciliationStatus::NoUsage,
                amount: Some(0),
                scope_id: None,
            },
            ManagedSessionReconciliationResult::Ambiguous { amount } => {
                ManagedSessionReconciliation {
                    status: ManagedSessionReconciliationStatus::Ambiguous,
                    amount: Some(amount),
                    scope_id: None,
                }
            }
            ManagedSessionReconciliationResult::WindowRolledOver => ManagedSessionReconciliation {
                status: ManagedSessionReconciliationStatus::WindowRolledOver,
                amount: None,
                scope_id: None,
            },
            ManagedSessionReconciliationResult::SnapshotUnavailable => {
                ManagedSessionReconciliation {
                    status: ManagedSessionReconciliationStatus::SnapshotUnavailable,
                    amount: None,
                    scope_id: None,
                }
            }
        })
    }

    pub fn active_managed_sessions(&self) -> ApplicationResult<Vec<ActiveManagedSession>> {
        self.database
            .managed_sessions()
            .list_active()?
            .into_iter()
            .map(|session| {
                Ok(ActiveManagedSession {
                    session_id: session.id,
                    reservation_id: session.reservation_id,
                    pool_id: session.pool_id,
                    canonical_path: session.canonical_path,
                    supervisor_pid: session.supervisor_pid,
                })
            })
            .collect()
    }

    fn ensure_bound_workspace_scope(&self, scope_id: &ScopeId) -> ApplicationResult<()> {
        if self
            .database
            .workspace_bindings()
            .list()?
            .iter()
            .any(|binding| binding.scope_id() == scope_id)
        {
            return Ok(());
        }
        Err(ApplicationError::InvalidRequest {
            message: format!("scope {scope_id} is not bound to a local workspace"),
        })
    }

    fn effective_policy(
        &self,
        scope_id: &ScopeId,
    ) -> ApplicationResult<(EnforcementPolicy, PolicySummary)> {
        if let Some(workspace_policy) = self.database.workspace_policies().get(scope_id)? {
            let policy = workspace_policy.policy().clone();
            let summary =
                policy_summary(&policy, true, Some(workspace_policy.updated_at().value()));
            return Ok((policy, summary));
        }
        Ok((
            self.default_policy.clone(),
            policy_summary(&self.default_policy, false, None),
        ))
    }

    fn provider_decision(
        &self,
        window: &WindowSummary,
        policy: &EnforcementPolicy,
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
        Ok(policy.evaluate(&balance))
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

    pub fn remove_workspace_allocation(
        &mut self,
        command: RemoveWorkspaceAllocation,
    ) -> ApplicationResult<()> {
        self.database
            .remove_workspace_allocation(
                &ScopeId::new(command.scope_id)?,
                &WindowId::new(command.window_id)?,
                UnixMillis::new(command.removed_at),
            )
            .map_err(map_input_storage_error)?;
        Ok(())
    }

    pub fn set_allocation_priority_order(
        &mut self,
        command: SetAllocationPriorityOrder,
    ) -> ApplicationResult<()> {
        let window_id = WindowId::new(command.window_id)?;
        let ordered_scope_ids = command
            .ordered_scope_ids
            .into_iter()
            .map(ScopeId::new)
            .collect::<Result<Vec<_>, _>>()?;
        self.database
            .allocations()
            .set_priority_order(&window_id, &ordered_scope_ids)
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
        let bound_scope_ids = self
            .database
            .workspace_bindings()
            .list()?
            .into_iter()
            .map(|binding| binding.scope_id().to_string())
            .collect::<HashSet<_>>();
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

        for (priority, allocation) in allocations.into_iter().enumerate() {
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
            let (policy, policy_summary) = self.effective_policy(allocation.scope_id())?;

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
                priority: u64::try_from(priority).map_err(|_| {
                    ApplicationError::NumericOutOfRange {
                        field: "allocation priority",
                        value: i128::try_from(priority).unwrap_or(i128::MAX),
                    }
                })?,
                unit: allocation.limit().unit().to_string(),
                limit: allocation.limit().value(),
                attributed_usage: balance.attributed_usage().value(),
                active_reservations: balance.active_reservations().value(),
                remaining: to_view_integer(balance.remaining(), "allocation remaining")?,
                spendable: balance.spendable().value(),
                protected_now: 0,
                decision: policy.evaluate(&balance),
                policy: policy_summary,
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
        let mut protection_remaining = provider_spendable;
        for snapshot in snapshots.iter_mut().filter(|snapshot| {
            snapshot.parent_id.is_none() && bound_scope_ids.contains(&snapshot.scope_id)
        }) {
            snapshot.protected_now = snapshot.spendable.min(protection_remaining);
            protection_remaining = protection_remaining.saturating_sub(snapshot.protected_now);
        }
        let protected_by_scope = snapshots
            .iter()
            .filter(|snapshot| {
                snapshot.parent_id.is_none() && bound_scope_ids.contains(&snapshot.scope_id)
            })
            .map(|snapshot| (snapshot.scope_id.clone(), snapshot.protected_now))
            .collect::<HashMap<_, _>>();
        for snapshot in snapshots
            .iter_mut()
            .filter(|snapshot| snapshot.parent_id.is_some())
        {
            snapshot.protected_now = snapshot
                .parent_id
                .as_ref()
                .and_then(|parent_id| protected_by_scope.get(parent_id))
                .copied()
                .unwrap_or(0)
                .min(snapshot.spendable);
        }
        let provider_quota_remaining = window
            .capacity()
            .value()
            .saturating_sub(effective_observed_usage);
        let forecast_samples = self
            .database
            .managed_sessions()
            .list_reconciled_for_window(&window_id)?
            .into_iter()
            .filter_map(|session| {
                let outcome = session.reconciliation_outcome?;
                Some(ManagedUsageSample {
                    created_at: session.created_at,
                    reconciled_at: session.reconciled_at?,
                    amount: session.reconciled_amount.unwrap_or(0),
                    attributed: outcome == ManagedSessionReconciliationOutcome::Attributed,
                    trustworthy: matches!(
                        outcome,
                        ManagedSessionReconciliationOutcome::Attributed
                            | ManagedSessionReconciliationOutcome::NoUsage
                    ),
                })
            })
            .collect();
        let forecast = calculate_depletion_forecast(ForecastInput {
            window_starts_at: window.starts_at().value(),
            window_ends_at: window.ends_at().value(),
            at: command.at,
            provider_remaining: provider_quota_remaining,
            provider_observed_usage: effective_observed_usage,
            samples: forecast_samples,
        });

        let quota_history = self
            .database
            .provider_quota_history(&window_id)?
            .into_iter()
            .map(|point| QuotaHistoryPoint {
                observed_at: point.observed_at.value(),
                remaining: window.capacity().value().saturating_sub(point.used),
            })
            .collect();

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
            quota_history,
            forecast,
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

fn policy_summary(
    policy: &EnforcementPolicy,
    customized: bool,
    updated_at: Option<i64>,
) -> PolicySummary {
    PolicySummary {
        warn_at_basis_points: policy.warn_at().map(BasisPoints::value),
        confirm_at_basis_points: policy.confirm_at().map(BasisPoints::value),
        stop_at_basis_points: policy.stop_at().map(BasisPoints::value),
        customized,
        updated_at,
    }
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
            CreateQuotaWindow, CreateScope, DepletionForecastStatus, DesktopUsageObservation,
            EvaluateWorkspaceAdmission, FinishManagedSession, GetLocalState, GetQuotaDashboard,
            GetWorkspaceContext, GetWorkspacePolicy, ManagedSessionOutcome,
            MarkManagedSessionRunning, PrepareManagedSession, ProviderQuotaSnapshotInput,
            RecordUsage, ReserveQuota, ResetWorkspacePolicy, SetAllocation,
            SetAllocationPriorityOrder, SetWorkspacePolicy, SyncProviderQuota,
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

    #[test]
    fn priority_funds_bound_workspaces_from_current_provider_remaining() {
        let mut service = QuotaService::new(Database::open_in_memory().unwrap());
        service
            .create_quota_source(CreateQuotaSource {
                provider_id: "codex".to_owned(),
                provider_display_name: "Codex".to_owned(),
                account_id: "subscription".to_owned(),
                account_display_name: "Subscription".to_owned(),
                pool_id: "codex-weekly".to_owned(),
                pool_display_name: "Weekly".to_owned(),
                window_id: "week-1".to_owned(),
                starts_at: 1_000,
                ends_at: 10_000,
                capacity: 100,
                unit: "percent".to_owned(),
                provider_snapshot: Some(ProviderQuotaSnapshotInput {
                    adapter: "codex_app_server".to_owned(),
                    remote_limit_id: "codex".to_owned(),
                    remote_window_kind: "secondary".to_owned(),
                    used: 88,
                    observed_at: 2_000,
                    resets_at: 10_000,
                }),
            })
            .unwrap();
        for (id, path) in [("workspace-a", "/code/a"), ("workspace-b", "/code/b")] {
            service
                .create_allocated_workspace(CreateAllocatedWorkspace {
                    id: id.to_owned(),
                    display_name: id.to_owned(),
                    canonical_path: path.to_owned(),
                    window_id: "week-1".to_owned(),
                    amount: 20,
                    unit: "percent".to_owned(),
                    bound_at: 2_000,
                })
                .unwrap();
        }
        service
            .set_allocation_priority_order(SetAllocationPriorityOrder {
                window_id: "week-1".to_owned(),
                ordered_scope_ids: vec!["workspace-b".to_owned(), "workspace-a".to_owned()],
            })
            .unwrap();

        let dashboard = service
            .dashboard(GetQuotaDashboard {
                window_id: "week-1".to_owned(),
                at: 3_000,
            })
            .unwrap();

        assert_eq!(dashboard.allocations[0].scope_id, "workspace-b");
        assert_eq!(dashboard.allocations[0].protected_now, 12);
        assert_eq!(dashboard.allocations[0].limit, 20);
        assert_eq!(dashboard.allocations[1].scope_id, "workspace-a");
        assert_eq!(dashboard.allocations[1].protected_now, 0);
        assert_eq!(
            service
                .evaluate_workspace_admission(EvaluateWorkspaceAdmission {
                    canonical_path: "/code/a".to_owned(),
                    provider_id: "codex".to_owned(),
                    at: 3_000,
                })
                .unwrap()
                .allocation_decision,
            EnforcementDecision::Stop
        );
        assert_eq!(
            service
                .evaluate_workspace_admission(EvaluateWorkspaceAdmission {
                    canonical_path: "/code/b".to_owned(),
                    provider_id: "codex".to_owned(),
                    at: 3_000,
                })
                .unwrap()
                .protected_now,
            12
        );
    }

    #[test]
    fn unassigned_capacity_is_spent_before_low_priority_workspace_funding() {
        let mut service = QuotaService::new(Database::open_in_memory().unwrap());
        service
            .create_quota_source(CreateQuotaSource {
                provider_id: "codex".to_owned(),
                provider_display_name: "Codex".to_owned(),
                account_id: "subscription".to_owned(),
                account_display_name: "Subscription".to_owned(),
                pool_id: "codex-weekly".to_owned(),
                pool_display_name: "Weekly".to_owned(),
                window_id: "week-1".to_owned(),
                starts_at: 1_000,
                ends_at: 10_000,
                capacity: 100,
                unit: "percent".to_owned(),
                provider_snapshot: Some(ProviderQuotaSnapshotInput {
                    adapter: "codex_app_server".to_owned(),
                    remote_limit_id: "codex".to_owned(),
                    remote_window_kind: "secondary".to_owned(),
                    used: 50,
                    observed_at: 2_000,
                    resets_at: 10_000,
                }),
            })
            .unwrap();
        for (id, path) in [("workspace-a", "/code/a"), ("workspace-b", "/code/b")] {
            service
                .create_allocated_workspace(CreateAllocatedWorkspace {
                    id: id.to_owned(),
                    display_name: id.to_owned(),
                    canonical_path: path.to_owned(),
                    window_id: "week-1".to_owned(),
                    amount: 20,
                    unit: "percent".to_owned(),
                    bound_at: 2_000,
                })
                .unwrap();
        }
        service
            .set_allocation_priority_order(SetAllocationPriorityOrder {
                window_id: "week-1".to_owned(),
                ordered_scope_ids: vec!["workspace-a".to_owned(), "workspace-b".to_owned()],
            })
            .unwrap();

        let before_buffer_is_spent = service
            .dashboard(GetQuotaDashboard {
                window_id: "week-1".to_owned(),
                at: 2_500,
            })
            .unwrap();
        assert_eq!(before_buffer_is_spent.window.unallocated, 60);
        assert_eq!(before_buffer_is_spent.allocations[0].protected_now, 20);
        assert_eq!(before_buffer_is_spent.allocations[1].protected_now, 20);

        service
            .sync_provider_quota(SyncProviderQuota {
                current_window_id: "week-1".to_owned(),
                adapter: "codex_app_server".to_owned(),
                remote_limit_id: "codex".to_owned(),
                remote_window_kind: "secondary".to_owned(),
                starts_at: 1_000,
                ends_at: 10_000,
                capacity: 100,
                used: 65,
                unit: "percent".to_owned(),
                observed_at: 3_000,
                desktop_observations: None,
            })
            .unwrap();

        let after_buffer_is_spent = service
            .dashboard(GetQuotaDashboard {
                window_id: "week-1".to_owned(),
                at: 3_500,
            })
            .unwrap();
        assert_eq!(after_buffer_is_spent.allocations[0].protected_now, 20);
        assert_eq!(after_buffer_is_spent.allocations[1].protected_now, 15);
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

    fn managed_workspace_service() -> QuotaService {
        let mut service = configured_service();
        service
            .create_allocated_workspace(CreateAllocatedWorkspace {
                id: "workspace-a".to_owned(),
                display_name: "Workspace A".to_owned(),
                canonical_path: "/code/workspace-a".to_owned(),
                window_id: "week-1".to_owned(),
                amount: 30,
                unit: "quota_points".to_owned(),
                bound_at: 1_500,
            })
            .unwrap();
        sync_managed_snapshot(&mut service, 10, 1_900);
        service
    }

    fn sync_managed_snapshot(service: &mut QuotaService, used: u64, observed_at: i64) {
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
                observed_at,
                desktop_observations: None,
            })
            .unwrap();
    }

    fn sync_desktop_snapshot(
        service: &mut QuotaService,
        used: u64,
        observed_at: i64,
        observations: Vec<DesktopUsageObservation>,
    ) -> SyncProviderQuotaResult {
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
                observed_at,
                desktop_observations: Some(observations),
            })
            .unwrap()
    }

    fn prepare_session(
        service: &mut QuotaService,
        id: &str,
        reservation_id: &str,
    ) -> ManagedSessionLaunch {
        service
            .prepare_managed_session(PrepareManagedSession {
                id: id.to_owned(),
                reservation_id: reservation_id.to_owned(),
                canonical_path: "/code/workspace-a".to_owned(),
                provider_id: "codex".to_owned(),
                assume_yes: false,
                admitted_at: 2_000,
                expires_at: 8_000,
                supervisor_pid: 42,
            })
            .unwrap()
    }

    #[test]
    fn managed_session_reconciles_usage_and_consumes_its_reservation() {
        let mut service = managed_workspace_service();
        let launch = prepare_session(&mut service, "session-1", "session-1-reservation");

        assert_eq!(launch.reserved_amount, 30);
        assert_eq!(service.active_managed_sessions().unwrap().len(), 1);
        let reserved = service
            .dashboard(GetQuotaDashboard {
                window_id: "week-1".to_owned(),
                at: 3_000,
            })
            .unwrap();
        assert_eq!(snapshot(&reserved, "workspace-a").active_reservations, 30);

        service
            .mark_managed_session_running(MarkManagedSessionRunning {
                id: "session-1".to_owned(),
                child_pid: 84,
                started_at: 2_100,
            })
            .unwrap();
        sync_managed_snapshot(&mut service, 14, 3_900);
        let reconciliation = service
            .finish_managed_session(FinishManagedSession {
                id: "session-1".to_owned(),
                outcome: ManagedSessionOutcome::Completed,
                finished_at: 4_000,
                exit_code: Some(0),
                current_window_id: Some("week-1".to_owned()),
            })
            .unwrap();

        assert_eq!(
            reconciliation.status,
            ManagedSessionReconciliationStatus::Attributed
        );
        assert_eq!(reconciliation.amount, Some(4));
        assert_eq!(reconciliation.scope_id.as_deref(), Some("workspace-a"));
        assert!(service.active_managed_sessions().unwrap().is_empty());
        let reconciled = service
            .dashboard(GetQuotaDashboard {
                window_id: "week-1".to_owned(),
                at: 4_000,
            })
            .unwrap();
        assert_eq!(snapshot(&reconciled, "workspace-a").active_reservations, 0);
        assert_eq!(snapshot(&reconciled, "workspace-a").attributed_usage, 4);
        assert_eq!(
            reconciled.forecast.status,
            DepletionForecastStatus::InsufficientData
        );
        assert_eq!(reconciled.forecast.sample_count, 1);
        assert_eq!(reconciled.forecast.managed_usage, 4);

        let retried = service
            .finish_managed_session(FinishManagedSession {
                id: "session-1".to_owned(),
                outcome: ManagedSessionOutcome::Completed,
                finished_at: 4_100,
                exit_code: Some(0),
                current_window_id: Some("week-1".to_owned()),
            })
            .unwrap();
        assert_eq!(retried, reconciliation);
        let after_retry = service
            .dashboard(GetQuotaDashboard {
                window_id: "week-1".to_owned(),
                at: 4_100,
            })
            .unwrap();
        assert_eq!(snapshot(&after_retry, "workspace-a").attributed_usage, 4);
    }

    #[test]
    fn active_managed_reservation_prevents_a_competing_launch() {
        let mut service = managed_workspace_service();
        prepare_session(&mut service, "session-1", "session-1-reservation");

        let error = service
            .prepare_managed_session(PrepareManagedSession {
                id: "session-2".to_owned(),
                reservation_id: "session-2-reservation".to_owned(),
                canonical_path: "/code/workspace-a".to_owned(),
                provider_id: "codex".to_owned(),
                assume_yes: false,
                admitted_at: 2_100,
                expires_at: 8_000,
                supervisor_pid: 43,
            })
            .unwrap_err();

        let message = error.to_string();
        assert!(
            message.contains("refused") || message.contains("spendable"),
            "{message}"
        );
    }

    #[test]
    fn interrupted_starting_session_releases_its_reservation() {
        let mut service = managed_workspace_service();
        prepare_session(&mut service, "session-1", "session-1-reservation");

        let reconciliation = service
            .finish_managed_session(FinishManagedSession {
                id: "session-1".to_owned(),
                outcome: ManagedSessionOutcome::Interrupted,
                finished_at: 3_000,
                exit_code: None,
                current_window_id: None,
            })
            .unwrap();

        assert_eq!(
            reconciliation.status,
            ManagedSessionReconciliationStatus::SnapshotUnavailable
        );
        assert!(service.active_managed_sessions().unwrap().is_empty());
        let dashboard = service
            .dashboard(GetQuotaDashboard {
                window_id: "week-1".to_owned(),
                at: 3_000,
            })
            .unwrap();
        assert_eq!(snapshot(&dashboard, "workspace-a").active_reservations, 0);
    }

    #[test]
    fn managed_session_with_no_provider_delta_records_no_usage() {
        let mut service = managed_workspace_service();
        prepare_session(&mut service, "session-1", "session-1-reservation");

        let reconciliation = service
            .finish_managed_session(FinishManagedSession {
                id: "session-1".to_owned(),
                outcome: ManagedSessionOutcome::Completed,
                finished_at: 3_000,
                exit_code: Some(0),
                current_window_id: Some("week-1".to_owned()),
            })
            .unwrap();

        assert_eq!(
            reconciliation.status,
            ManagedSessionReconciliationStatus::NoUsage
        );
        assert_eq!(reconciliation.amount, Some(0));
        let dashboard = service
            .dashboard(GetQuotaDashboard {
                window_id: "week-1".to_owned(),
                at: 3_000,
            })
            .unwrap();
        assert_eq!(snapshot(&dashboard, "workspace-a").attributed_usage, 0);
        assert_eq!(snapshot(&dashboard, "workspace-a").active_reservations, 0);
    }

    #[test]
    fn managed_session_never_attributes_across_a_window_rollover() {
        let mut service = managed_workspace_service();
        prepare_session(&mut service, "session-1", "session-1-reservation");

        let reconciliation = service
            .finish_managed_session(FinishManagedSession {
                id: "session-1".to_owned(),
                outcome: ManagedSessionOutcome::Completed,
                finished_at: 10_100,
                exit_code: Some(0),
                current_window_id: Some("week-2".to_owned()),
            })
            .unwrap();

        assert_eq!(
            reconciliation.status,
            ManagedSessionReconciliationStatus::WindowRolledOver
        );
        assert!(service.active_managed_sessions().unwrap().is_empty());
    }

    #[test]
    fn concurrent_provider_turn_keeps_managed_delta_unattributed() {
        let mut service = managed_workspace_service();
        prepare_session(&mut service, "session-1", "session-1-reservation");
        service
            .begin_provider_turn_observation(BeginProviderTurnObservation {
                session_id: "external-session".to_owned(),
                turn_id: "external-turn".to_owned(),
                adapter: "codex_app_server".to_owned(),
                canonical_path: "/code/external".to_owned(),
                scope_id: None,
                window_id: "week-1".to_owned(),
                started_at: 1_950,
            })
            .unwrap();
        sync_managed_snapshot(&mut service, 15, 2_900);

        let reconciliation = service
            .finish_managed_session(FinishManagedSession {
                id: "session-1".to_owned(),
                outcome: ManagedSessionOutcome::Completed,
                finished_at: 3_000,
                exit_code: Some(0),
                current_window_id: Some("week-1".to_owned()),
            })
            .unwrap();

        assert_eq!(
            reconciliation.status,
            ManagedSessionReconciliationStatus::Ambiguous
        );
        assert_eq!(reconciliation.amount, Some(5));
        let dashboard = service
            .dashboard(GetQuotaDashboard {
                window_id: "week-1".to_owned(),
                at: 3_000,
            })
            .unwrap();
        assert_eq!(snapshot(&dashboard, "workspace-a").attributed_usage, 0);
        assert_eq!(snapshot(&dashboard, "workspace-a").active_reservations, 0);
        assert_eq!(dashboard.window.unattributed_usage, 15);
    }

    #[test]
    fn existing_provider_turn_marks_a_new_managed_session_contended() {
        let mut service = managed_workspace_service();
        service
            .begin_provider_turn_observation(BeginProviderTurnObservation {
                session_id: "external-session".to_owned(),
                turn_id: "external-turn".to_owned(),
                adapter: "codex_app_server".to_owned(),
                canonical_path: "/code/external".to_owned(),
                scope_id: None,
                window_id: "week-1".to_owned(),
                started_at: 1_950,
            })
            .unwrap();

        prepare_session(&mut service, "session-1", "session-1-reservation");

        let session = service
            .database
            .managed_sessions()
            .get("session-1")
            .unwrap()
            .unwrap();
        assert!(session.contended);
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
        assert_eq!(json["forecast"]["status"], "insufficient_data");
        assert_eq!(json["forecast"]["sampleCount"], 0);
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
    fn workspace_policy_is_shared_by_dashboard_context_and_admission_then_resets() {
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
                amount: 18,
                unit: "quota_points".to_owned(),
                observed_at: 2_500,
                source: UsageSource::LocalMeasured,
                confidence: Confidence::Observed,
                reservation_id: None,
            })
            .unwrap();

        let custom = service
            .set_workspace_policy(SetWorkspacePolicy {
                scope_id: "workspace-a".to_owned(),
                warn_at_basis_points: Some(5_000),
                confirm_at_basis_points: Some(8_000),
                stop_at_basis_points: Some(10_000),
                updated_at: 2_600,
            })
            .unwrap();
        assert!(custom.customized);
        assert_eq!(custom.warn_at_basis_points, Some(5_000));

        let dashboard = service
            .dashboard(GetQuotaDashboard {
                window_id: "week-1".to_owned(),
                at: 3_000,
            })
            .unwrap();
        assert_eq!(
            snapshot(&dashboard, "workspace-a").decision,
            EnforcementDecision::Warn
        );
        assert!(snapshot(&dashboard, "workspace-a").policy.customized);

        let context = service
            .workspace_context(GetWorkspaceContext {
                canonical_path: "/code/workspace-a/src".to_owned(),
                at: 3_000,
            })
            .unwrap();
        assert_eq!(context.allocations[0].decision, EnforcementDecision::Warn);
        assert_eq!(
            context.allocations[0].policy.warn_at_basis_points,
            Some(5_000)
        );

        let admission = service
            .evaluate_workspace_admission(EvaluateWorkspaceAdmission {
                canonical_path: "/code/workspace-a".to_owned(),
                provider_id: "codex".to_owned(),
                at: 3_000,
            })
            .unwrap();
        assert_eq!(admission.decision, EnforcementDecision::Warn);
        assert!(admission.policy.customized);

        let policy = service
            .workspace_policy(GetWorkspacePolicy {
                canonical_path: "/code/workspace-a/nested".to_owned(),
                at: 3_000,
            })
            .unwrap();
        assert_eq!(policy.scope_id, "workspace-a");
        assert_eq!(policy.policy.updated_at, Some(2_600));

        let reset = service
            .reset_workspace_policy(ResetWorkspacePolicy {
                scope_id: "workspace-a".to_owned(),
            })
            .unwrap();
        assert!(!reset.customized);
        assert_eq!(reset.warn_at_basis_points, Some(8_000));
        let dashboard = service
            .dashboard(GetQuotaDashboard {
                window_id: "week-1".to_owned(),
                at: 3_000,
            })
            .unwrap();
        assert_eq!(
            snapshot(&dashboard, "workspace-a").decision,
            EnforcementDecision::Allow
        );
    }

    #[test]
    fn invalid_workspace_policy_is_rejected_without_replacing_the_previous_policy() {
        let mut service = managed_workspace_service();
        service
            .set_workspace_policy(SetWorkspacePolicy {
                scope_id: "workspace-a".to_owned(),
                warn_at_basis_points: Some(5_000),
                confirm_at_basis_points: Some(8_000),
                stop_at_basis_points: Some(10_000),
                updated_at: 2_100,
            })
            .unwrap();

        let invalid = service.set_workspace_policy(SetWorkspacePolicy {
            scope_id: "workspace-a".to_owned(),
            warn_at_basis_points: Some(9_000),
            confirm_at_basis_points: Some(8_000),
            stop_at_basis_points: Some(10_000),
            updated_at: 2_200,
        });
        assert!(matches!(invalid, Err(ApplicationError::Validation(_))));

        let persisted = service
            .workspace_policy(GetWorkspacePolicy {
                canonical_path: "/code/workspace-a".to_owned(),
                at: 2_300,
            })
            .unwrap();
        assert_eq!(persisted.policy.warn_at_basis_points, Some(5_000));
        assert_eq!(persisted.policy.updated_at, Some(2_100));
    }

    #[test]
    fn managed_confirmation_override_is_required_and_audited_atomically() {
        let mut service = managed_workspace_service();
        service
            .set_workspace_policy(SetWorkspacePolicy {
                scope_id: "workspace-a".to_owned(),
                warn_at_basis_points: None,
                confirm_at_basis_points: Some(100),
                stop_at_basis_points: None,
                updated_at: 1_950,
            })
            .unwrap();

        let refused = service.prepare_managed_session(PrepareManagedSession {
            id: "session-refused".to_owned(),
            reservation_id: "reservation-refused".to_owned(),
            canonical_path: "/code/workspace-a".to_owned(),
            provider_id: "codex".to_owned(),
            assume_yes: false,
            admitted_at: 2_000,
            expires_at: 8_000,
            supervisor_pid: 42,
        });
        assert!(refused.unwrap_err().to_string().contains("--yes"));
        assert!(service
            .database
            .workspace_policies()
            .list_override_audits(&ScopeId::new("workspace-a").unwrap())
            .unwrap()
            .is_empty());

        let launch = service
            .prepare_managed_session(PrepareManagedSession {
                id: "session-accepted".to_owned(),
                reservation_id: "reservation-accepted".to_owned(),
                canonical_path: "/code/workspace-a".to_owned(),
                provider_id: "codex".to_owned(),
                assume_yes: true,
                admitted_at: 2_100,
                expires_at: 8_000,
                supervisor_pid: 42,
            })
            .unwrap();
        assert_eq!(
            launch.assessment.decision,
            EnforcementDecision::RequireConfirmation
        );

        let audits = service
            .database
            .workspace_policies()
            .list_override_audits(&ScopeId::new("workspace-a").unwrap())
            .unwrap();
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].session_id, "session-accepted");
        assert_eq!(audits[0].window_id.as_str(), "week-1");
        assert_eq!(audits[0].accepted_at.value(), 2_100);
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
    fn removing_workspace_allocation_releases_binding_and_preserves_scope_history() {
        let mut service = managed_workspace_service();

        service
            .remove_workspace_allocation(RemoveWorkspaceAllocation {
                scope_id: "workspace-a".to_owned(),
                window_id: "week-1".to_owned(),
                removed_at: 3_000,
            })
            .unwrap();

        let state = service
            .local_state(GetLocalState {
                selected_window_id: Some("week-1".to_owned()),
                at: 3_000,
            })
            .unwrap();
        assert!(!state
            .dashboard
            .unwrap()
            .allocations
            .iter()
            .any(|allocation| allocation.scope_id == "workspace-a"));
        assert!(state.scopes.iter().any(|scope| scope.id == "workspace-a"));

        let context = service
            .workspace_context(GetWorkspaceContext {
                canonical_path: "/code/workspace-a".to_owned(),
                at: 3_000,
            })
            .unwrap();
        assert!(context.binding.is_none());

        service
            .create_allocated_workspace(CreateAllocatedWorkspace {
                id: "workspace-a-readded".to_owned(),
                display_name: "Workspace A".to_owned(),
                canonical_path: "/code/workspace-a".to_owned(),
                window_id: "week-1".to_owned(),
                amount: 20,
                unit: "quota_points".to_owned(),
                bound_at: 3_100,
            })
            .unwrap();
    }

    #[test]
    fn removing_workspace_allocation_rejects_an_active_reservation() {
        let mut service = managed_workspace_service();
        service
            .reserve_quota(ReserveQuota {
                id: "active-reservation".to_owned(),
                scope_id: "workspace-a".to_owned(),
                window_id: "week-1".to_owned(),
                amount: 1,
                unit: "quota_points".to_owned(),
                admitted_at: 2_000,
                expires_at: 4_000,
            })
            .unwrap();

        let result = service.remove_workspace_allocation(RemoveWorkspaceAllocation {
            scope_id: "workspace-a".to_owned(),
            window_id: "week-1".to_owned(),
            removed_at: 3_000,
        });

        assert!(matches!(
            result,
            Err(ApplicationError::Storage(StorageError::InvalidState { .. }))
        ));
    }

    #[test]
    fn dashboard_exposes_provider_quota_history_in_observation_order() {
        let mut service = managed_workspace_service();
        sync_managed_snapshot(&mut service, 14, 2_500);
        sync_managed_snapshot(&mut service, 18, 3_000);

        let dashboard = service
            .dashboard(GetQuotaDashboard {
                window_id: "week-1".to_owned(),
                at: 3_000,
            })
            .unwrap();

        assert_eq!(
            dashboard
                .quota_history
                .iter()
                .map(|point| (point.observed_at, point.remaining))
                .collect::<Vec<_>>(),
            vec![(1_900, 90), (2_500, 86), (3_000, 82)]
        );
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
                    desktop_observations: None,
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
    fn desktop_thread_activity_is_attributed_after_the_provider_checkpoint_advances() {
        let mut service = managed_workspace_service();
        let baseline = DesktopUsageObservation {
            thread_id: "thread-1".to_owned(),
            canonical_path: "/code/workspace-a".to_owned(),
            total_tokens: 100,
            updated_at: 2_000,
        };

        let first = sync_desktop_snapshot(&mut service, 10, 2_100, vec![baseline.clone()]);
        assert_eq!(
            first.desktop_reconciliation.unwrap().status,
            DesktopUsageReconciliationStatus::BaselineEstablished
        );

        let mut active = baseline;
        active.total_tokens = 175;
        active.updated_at = 2_500;
        let pending = sync_desktop_snapshot(&mut service, 10, 2_600, vec![active.clone()]);
        assert_eq!(
            pending.desktop_reconciliation.as_ref().unwrap().status,
            DesktopUsageReconciliationStatus::PendingProviderDelta
        );
        assert_eq!(pending.desktop_reconciliation.unwrap().pending_tokens, 75);

        let attributed = sync_desktop_snapshot(&mut service, 12, 3_000, vec![active]);
        assert_eq!(
            attributed.desktop_reconciliation.as_ref().unwrap().status,
            DesktopUsageReconciliationStatus::Attributed
        );
        assert_eq!(
            attributed
                .desktop_reconciliation
                .as_ref()
                .unwrap()
                .attributed_amount,
            2
        );
        assert_eq!(
            attributed
                .desktop_reconciliation
                .as_ref()
                .unwrap()
                .scope_id
                .as_deref(),
            Some("workspace-a")
        );

        let dashboard = service
            .dashboard(GetQuotaDashboard {
                window_id: "week-1".to_owned(),
                at: 3_000,
            })
            .unwrap();
        assert_eq!(snapshot(&dashboard, "workspace-a").attributed_usage, 2);
        assert_eq!(dashboard.window.unattributed_usage, 10);
    }

    #[test]
    fn provider_refresh_without_a_desktop_scan_invalidates_pending_attribution() {
        let mut service = managed_workspace_service();
        let baseline = DesktopUsageObservation {
            thread_id: "thread-1".to_owned(),
            canonical_path: "/code/workspace-a".to_owned(),
            total_tokens: 100,
            updated_at: 2_000,
        };
        sync_desktop_snapshot(&mut service, 10, 2_100, vec![baseline.clone()]);

        let mut active = baseline;
        active.total_tokens = 175;
        active.updated_at = 2_500;
        sync_desktop_snapshot(&mut service, 10, 2_600, vec![active.clone()]);

        sync_managed_snapshot(&mut service, 12, 3_000);
        let before_desktop_scan = service
            .dashboard(GetQuotaDashboard {
                window_id: "week-1".to_owned(),
                at: 3_000,
            })
            .unwrap();
        assert_eq!(
            snapshot(&before_desktop_scan, "workspace-a").attributed_usage,
            0
        );

        let attributed = sync_desktop_snapshot(&mut service, 12, 3_100, vec![active]);
        assert_eq!(
            attributed.desktop_reconciliation.unwrap().status,
            DesktopUsageReconciliationStatus::NoActivity
        );
        let dashboard = service
            .dashboard(GetQuotaDashboard {
                window_id: "week-1".to_owned(),
                at: 3_100,
            })
            .unwrap();
        assert_eq!(snapshot(&dashboard, "workspace-a").attributed_usage, 0);
    }

    #[test]
    fn desktop_activity_in_multiple_folders_remains_unattributed() {
        let mut service = managed_workspace_service();
        let mapped = DesktopUsageObservation {
            thread_id: "thread-1".to_owned(),
            canonical_path: "/code/workspace-a".to_owned(),
            total_tokens: 100,
            updated_at: 2_000,
        };
        let unmapped = DesktopUsageObservation {
            thread_id: "thread-2".to_owned(),
            canonical_path: "/code/other".to_owned(),
            total_tokens: 100,
            updated_at: 2_000,
        };
        sync_desktop_snapshot(
            &mut service,
            10,
            2_100,
            vec![mapped.clone(), unmapped.clone()],
        );

        let mut mapped_active = mapped;
        mapped_active.total_tokens = 150;
        mapped_active.updated_at = 2_500;
        let mut unmapped_active = unmapped;
        unmapped_active.total_tokens = 150;
        unmapped_active.updated_at = 2_500;
        let result = sync_desktop_snapshot(
            &mut service,
            12,
            3_000,
            vec![mapped_active, unmapped_active],
        );

        assert_eq!(
            result.desktop_reconciliation.unwrap().status,
            DesktopUsageReconciliationStatus::Ambiguous
        );
        let dashboard = service
            .dashboard(GetQuotaDashboard {
                window_id: "week-1".to_owned(),
                at: 3_000,
            })
            .unwrap();
        assert_eq!(snapshot(&dashboard, "workspace-a").attributed_usage, 0);
        assert_eq!(dashboard.window.unattributed_usage, 12);
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
                desktop_observations: None,
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
                desktop_observations: None,
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
