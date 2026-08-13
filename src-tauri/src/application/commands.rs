use serde::{Deserialize, Serialize};

use crate::domain::{Confidence, ScopeKind, UsageSource};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProvider {
    pub id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAccount {
    pub id: String,
    pub provider_id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateQuotaPool {
    pub id: String,
    pub account_id: String,
    pub display_name: String,
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateQuotaWindow {
    pub id: String,
    pub pool_id: String,
    pub starts_at: i64,
    pub ends_at: i64,
    pub capacity: u64,
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateQuotaSource {
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
    pub provider_snapshot: Option<ProviderQuotaSnapshotInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveQuotaSource {
    pub pool_id: String,
    pub archived_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindWorkspace {
    pub canonical_path: String,
    pub scope_reference: String,
    pub bound_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAllocatedWorkspace {
    pub id: String,
    pub display_name: String,
    pub canonical_path: String,
    pub window_id: String,
    pub amount: u64,
    pub unit: String,
    pub bound_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetWorkspaceContext {
    pub canonical_path: String,
    pub at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluateWorkspaceAdmission {
    pub canonical_path: String,
    pub provider_id: String,
    pub at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetWorkspacePolicy {
    pub canonical_path: String,
    pub at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetWorkspacePolicy {
    pub scope_id: String,
    pub warn_at_basis_points: Option<u16>,
    pub confirm_at_basis_points: Option<u16>,
    pub stop_at_basis_points: Option<u16>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetWorkspacePolicy {
    pub scope_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderQuotaSnapshotInput {
    pub adapter: String,
    pub remote_limit_id: String,
    pub remote_window_kind: String,
    pub used: u64,
    pub observed_at: i64,
    pub resets_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProviderQuota {
    pub current_window_id: String,
    pub adapter: String,
    pub remote_limit_id: String,
    pub remote_window_kind: String,
    pub starts_at: i64,
    pub ends_at: i64,
    pub capacity: u64,
    pub used: u64,
    pub unit: String,
    pub observed_at: i64,
    #[serde(default)]
    pub desktop_observations: Option<Vec<DesktopUsageObservation>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopUsageObservation {
    pub thread_id: String,
    pub canonical_path: String,
    pub total_tokens: u64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeginProviderTurnObservation {
    pub session_id: String,
    pub turn_id: String,
    pub adapter: String,
    pub canonical_path: String,
    pub scope_id: Option<String>,
    pub window_id: String,
    pub started_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetProviderTurnObservation {
    pub session_id: String,
    pub turn_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconcileProviderTurnObservation {
    pub session_id: String,
    pub turn_id: String,
    pub current_window_id: String,
    pub observed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AbandonProviderTurnObservation {
    pub session_id: String,
    pub turn_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AbandonProviderSessionObservations {
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordCodexProtectionEvent {
    pub session_id: String,
    pub turn_id: String,
    pub canonical_path: String,
    pub scope_id: Option<String>,
    pub blocked: bool,
    pub reason: String,
    pub occurred_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetCodexProtectionEvents {
    pub limit: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetPendingCodexConfirmations {
    pub at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveCodexConfirmation {
    pub id: String,
    pub approved: bool,
    pub resolved_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateScope {
    pub id: String,
    pub parent_id: Option<String>,
    pub kind: ScopeKind,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAllocatedScope {
    pub id: String,
    pub parent_id: Option<String>,
    pub kind: ScopeKind,
    pub display_name: String,
    pub window_id: String,
    pub amount: u64,
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetAllocation {
    pub scope_id: String,
    pub window_id: String,
    pub amount: u64,
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveWorkspaceAllocation {
    pub scope_id: String,
    pub window_id: String,
    pub removed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetAllocationPriorityOrder {
    pub window_id: String,
    pub ordered_scope_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReserveQuota {
    pub id: String,
    pub scope_id: String,
    pub window_id: String,
    pub amount: u64,
    pub unit: String,
    pub admitted_at: i64,
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseReservation {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareManagedSession {
    pub id: String,
    pub reservation_id: String,
    pub canonical_path: String,
    pub provider_id: String,
    pub assume_yes: bool,
    pub admitted_at: i64,
    pub expires_at: i64,
    pub supervisor_pid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkManagedSessionRunning {
    pub id: String,
    pub child_pid: u32,
    pub started_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedSessionOutcome {
    Completed,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishManagedSession {
    pub id: String,
    pub outcome: ManagedSessionOutcome,
    pub finished_at: i64,
    pub exit_code: Option<i32>,
    pub current_window_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordUsage {
    pub id: String,
    pub window_id: String,
    pub scope_id: Option<String>,
    pub amount: u64,
    pub unit: String,
    pub observed_at: i64,
    pub source: UsageSource,
    pub confidence: Confidence,
    pub reservation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetQuotaDashboard {
    pub window_id: String,
    pub at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetLocalState {
    pub selected_window_id: Option<String>,
    pub at: i64,
}
