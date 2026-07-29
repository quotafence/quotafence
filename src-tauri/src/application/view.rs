use serde::Serialize;

use crate::domain::{EnforcementDecision, ScopeKind};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaDashboard {
    pub window: WindowSummary,
    pub allocations: Vec<AllocationSnapshot>,
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
pub struct AllocationSnapshot {
    pub scope_id: String,
    pub parent_id: Option<String>,
    pub scope_kind: ScopeKind,
    pub display_name: String,
    pub unit: String,
    pub limit: u64,
    pub attributed_usage: u64,
    pub active_reservations: u64,
    pub remaining: i64,
    pub spendable: u64,
    pub decision: EnforcementDecision,
}
