use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// Product capabilities are the stable boundary between the open core and
/// optional commercial features. Callers must ask for a capability rather
/// than branching on a plan name.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    BasicUsage,
    ProjectQuotas,
    LocalEnforcement,
    BasicHistory,
    BasicForecasting,
    BasicAlerts,
    BasicExport,
    ManualConfiguration,
    AdvancedAnalytics,
    AdvancedForecasting,
    SmartAlerts,
    ScheduledReports,
    AdvancedExport,
    AdvancedRules,
    AutomaticRouting,
    AutomaticUpdates,
    MultiDeviceSync,
    BackupRestore,
    PrioritySupport,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntitlementSource {
    Free,
    License,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementSnapshot {
    pub source: EntitlementSource,
    pub capabilities: BTreeSet<Capability>,
}

impl EntitlementSnapshot {
    pub fn free() -> Self {
        Self {
            source: EntitlementSource::Free,
            capabilities: free_capabilities(),
        }
    }

    pub fn supports(&self, capability: Capability) -> bool {
        self.capabilities.contains(&capability)
    }
}

/// A verified grant is deliberately provider-neutral. Signature verification,
/// payment providers, and persistence belong in adapters outside the quota
/// domain; the resolver only combines capabilities and handles expiry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedEntitlementGrant {
    pub capabilities: BTreeSet<Capability>,
    pub expires_at: Option<i64>,
}

pub fn resolve_entitlements(
    grant: Option<&VerifiedEntitlementGrant>,
    now: i64,
) -> EntitlementSnapshot {
    let Some(grant) = grant.filter(|grant| {
        grant
            .expires_at
            .map(|expires_at| expires_at > now)
            .unwrap_or(true)
    }) else {
        return EntitlementSnapshot::free();
    };

    let mut capabilities = free_capabilities();
    capabilities.extend(grant.capabilities.iter().copied());
    EntitlementSnapshot {
        source: EntitlementSource::License,
        capabilities,
    }
}

fn free_capabilities() -> BTreeSet<Capability> {
    [
        Capability::BasicUsage,
        Capability::ProjectQuotas,
        Capability::LocalEnforcement,
        Capability::BasicHistory,
        Capability::BasicForecasting,
        Capability::BasicAlerts,
        Capability::BasicExport,
        Capability::ManualConfiguration,
    ]
    .into_iter()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_snapshot_contains_the_complete_open_core() {
        let snapshot = EntitlementSnapshot::free();

        for capability in [
            Capability::BasicUsage,
            Capability::ProjectQuotas,
            Capability::LocalEnforcement,
            Capability::BasicHistory,
            Capability::BasicForecasting,
            Capability::BasicAlerts,
            Capability::BasicExport,
            Capability::ManualConfiguration,
        ] {
            assert!(snapshot.supports(capability));
        }
        assert!(!snapshot.supports(Capability::AdvancedAnalytics));
        assert!(!snapshot.supports(Capability::MultiDeviceSync));
    }

    #[test]
    fn valid_grant_adds_capabilities_without_removing_free_features() {
        let grant = VerifiedEntitlementGrant {
            capabilities: [Capability::AdvancedAnalytics].into_iter().collect(),
            expires_at: Some(2_000),
        };

        let snapshot = resolve_entitlements(Some(&grant), 1_000);

        assert_eq!(snapshot.source, EntitlementSource::License);
        assert!(snapshot.supports(Capability::BasicUsage));
        assert!(snapshot.supports(Capability::AdvancedAnalytics));
    }

    #[test]
    fn expired_grant_falls_back_to_free_without_locking_core_data_access() {
        let grant = VerifiedEntitlementGrant {
            capabilities: [Capability::AdvancedExport].into_iter().collect(),
            expires_at: Some(1_000),
        };

        let snapshot = resolve_entitlements(Some(&grant), 1_000);

        assert_eq!(snapshot, EntitlementSnapshot::free());
        assert!(snapshot.supports(Capability::BasicExport));
        assert!(!snapshot.supports(Capability::AdvancedExport));
    }

    #[test]
    fn snapshot_serialization_is_a_stable_frontend_contract() {
        let value = serde_json::to_value(EntitlementSnapshot::free()).unwrap();

        assert_eq!(value["source"], "free");
        assert!(value["capabilities"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("project_quotas")));
        assert!(value.get("plan").is_none());
    }
}
