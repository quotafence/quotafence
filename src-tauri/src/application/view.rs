use serde::Serialize;

use crate::domain::{EnforcementDecision, ScopeKind};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalState {
    pub sources: Vec<QuotaSourceSummary>,
    pub scopes: Vec<ScopeSummary>,
    pub selected_window_id: Option<String>,
    pub dashboard: Option<QuotaDashboard>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaSourceSummary {
    pub provider_id: String,
    pub provider_display_name: String,
    pub account_id: String,
    pub account_display_name: String,
    pub pool_id: String,
    pub pool_display_name: String,
    pub window_id: String,
    pub starts_at: i64,
    pub ends_at: i64,
    pub capacity: u64,
    pub unit: String,
    pub is_active: bool,
    pub provider_managed: bool,
    pub last_synced_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeSummary {
    pub id: String,
    pub parent_id: Option<String>,
    pub kind: ScopeKind,
    pub display_name: String,
    pub workspace_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceBindingSummary {
    pub canonical_path: String,
    pub scope_id: String,
    pub scope_display_name: String,
    pub bound_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceAllocationContext {
    pub provider_id: String,
    pub provider_display_name: String,
    pub pool_id: String,
    pub pool_display_name: String,
    pub window_id: String,
    pub window_is_active: bool,
    pub unit: String,
    pub limit: u64,
    pub remaining: i64,
    pub spendable: u64,
    pub protected_now: u64,
    pub provider_capacity: u64,
    pub provider_remaining: i64,
    pub provider_spendable: u64,
    pub allocation_decision: EnforcementDecision,
    pub provider_decision: EnforcementDecision,
    pub decision: EnforcementDecision,
    pub policy: PolicySummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceContext {
    pub canonical_path: String,
    pub binding: Option<WorkspaceBindingSummary>,
    pub allocations: Vec<WorkspaceAllocationContext>,
    pub available_workspace_scopes: Vec<ScopeSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdmissionAssessment {
    pub canonical_path: String,
    pub scope_id: String,
    pub scope_display_name: String,
    pub provider_id: String,
    pub provider_display_name: String,
    pub pool_id: String,
    pub pool_display_name: String,
    pub window_id: String,
    pub unit: String,
    pub allocation_limit: u64,
    pub allocation_remaining: i64,
    pub protected_now: u64,
    pub provider_capacity: u64,
    pub provider_remaining: i64,
    pub allocation_decision: EnforcementDecision,
    pub provider_decision: EnforcementDecision,
    pub decision: EnforcementDecision,
    pub policy: PolicySummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicySummary {
    pub warn_at_basis_points: Option<u16>,
    pub confirm_at_basis_points: Option<u16>,
    pub stop_at_basis_points: Option<u16>,
    pub customized: bool,
    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePolicySummary {
    pub canonical_path: String,
    pub scope_id: String,
    pub scope_display_name: String,
    pub policy: PolicySummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedSessionLaunch {
    pub session_id: String,
    pub reservation_id: String,
    pub reserved_amount: u64,
    pub assessment: AdmissionAssessment,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveManagedSession {
    pub session_id: String,
    pub reservation_id: String,
    pub pool_id: String,
    pub canonical_path: String,
    pub supervisor_pid: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedSessionReconciliationStatus {
    Attributed,
    NoUsage,
    Ambiguous,
    WindowRolledOver,
    SnapshotUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedSessionReconciliation {
    pub status: ManagedSessionReconciliationStatus,
    pub amount: Option<u64>,
    pub scope_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaDashboard {
    pub window: WindowSummary,
    pub allocations: Vec<AllocationSnapshot>,
    pub forecast: DepletionForecast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DepletionForecastStatus {
    InsufficientData,
    NoManagedBurn,
    SurvivesToReset,
    DepletesBeforeReset,
    WindowEnded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ForecastConfidence {
    Low,
    Medium,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepletionForecast {
    pub status: DepletionForecastStatus,
    pub confidence: ForecastConfidence,
    pub sample_count: u32,
    pub observation_start: Option<i64>,
    pub observation_end: i64,
    pub managed_usage: u64,
    pub attributed_managed_usage: u64,
    pub coverage_basis_points: u16,
    pub burn_rate_per_day_milliunits: Option<u64>,
    pub projected_depletion_at: Option<i64>,
    pub projected_remaining_at_reset: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowSummary {
    pub id: String,
    pub pool_id: String,
    pub starts_at: i64,
    pub ends_at: i64,
    pub unit: String,
    pub capacity: u64,
    pub allocated_to_root_scopes: u64,
    pub unallocated: u64,
    pub unattributed_usage: u64,
    pub provider_remaining: i64,
    pub provider_spendable: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProviderQuotaResult {
    pub window_id: String,
    pub rolled_over: bool,
    pub desktop_reconciliation: Option<DesktopUsageReconciliation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopUsageReconciliationStatus {
    BaselineEstablished,
    NoActivity,
    PendingProviderDelta,
    Attributed,
    Ambiguous,
    WindowRolledOver,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopUsageReconciliation {
    pub status: DesktopUsageReconciliationStatus,
    pub observed_threads: u32,
    pub pending_tokens: u64,
    pub attributed_amount: u64,
    pub scope_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnObservationStartStatus {
    Started,
    AlreadyStarted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnObservationStartResult {
    pub status: TurnObservationStartStatus,
    pub contended: bool,
    pub window_id: String,
    pub scope_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTurnObservationSummary {
    pub canonical_path: String,
    pub window_id: String,
    pub scope_id: Option<String>,
    pub contended: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnReconciliationStatus {
    Attributed,
    NoUsage,
    Ambiguous,
    Unmapped,
    WindowRolledOver,
    SnapshotUnavailable,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnReconciliationResult {
    pub status: TurnReconciliationStatus,
    pub amount: Option<u64>,
    pub scope_id: Option<String>,
    pub window_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexProtectionEventSummary {
    pub canonical_path: String,
    pub scope_id: Option<String>,
    pub workspace_name: Option<String>,
    pub outcome: String,
    pub reason: String,
    pub occurred_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AllocationSnapshot {
    pub scope_id: String,
    pub parent_id: Option<String>,
    pub scope_kind: ScopeKind,
    pub display_name: String,
    pub priority: u64,
    pub unit: String,
    pub limit: u64,
    pub attributed_usage: u64,
    pub active_reservations: u64,
    pub remaining: i64,
    pub spendable: u64,
    pub protected_now: u64,
    pub decision: EnforcementDecision,
    pub policy: PolicySummary,
}
