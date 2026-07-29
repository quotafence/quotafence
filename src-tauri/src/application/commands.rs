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
pub struct CreateScope {
    pub id: String,
    pub parent_id: Option<String>,
    pub kind: ScopeKind,
    pub display_name: String,
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
