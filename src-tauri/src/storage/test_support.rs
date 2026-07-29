use crate::domain::{
    Account, AccountId, Allocation, Provider, ProviderId, QuotaAmount, QuotaPool, QuotaPoolId,
    QuotaUnit, QuotaWindow, Scope, ScopeId, ScopeKind, UnixMillis, WindowId,
};

use super::Database;

pub(crate) fn points(value: u64) -> QuotaAmount {
    QuotaAmount::new(value, QuotaUnit::new("quota_points").unwrap())
}

pub(crate) fn seeded_database() -> Database {
    let database = Database::open_in_memory().unwrap();
    let catalog = database.catalog();

    catalog
        .insert_provider(&Provider::new(ProviderId::new("codex").unwrap(), "Codex").unwrap())
        .unwrap();
    catalog
        .insert_account(
            &Account::new(
                AccountId::new("codex-default").unwrap(),
                ProviderId::new("codex").unwrap(),
                "Default account",
            )
            .unwrap(),
        )
        .unwrap();
    catalog
        .insert_quota_pool(
            &QuotaPool::new(
                QuotaPoolId::new("codex-weekly").unwrap(),
                AccountId::new("codex-default").unwrap(),
                "Weekly quota",
                QuotaUnit::new("quota_points").unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
    catalog
        .insert_quota_window(
            &QuotaWindow::new(
                WindowId::new("week-1").unwrap(),
                QuotaPoolId::new("codex-weekly").unwrap(),
                UnixMillis::new(1_000),
                UnixMillis::new(10_000),
                points(100),
            )
            .unwrap(),
        )
        .unwrap();
    catalog
        .insert_scope(
            &Scope::new(
                ScopeId::new("project-a").unwrap(),
                None,
                ScopeKind::Project,
                "Project A",
            )
            .unwrap(),
        )
        .unwrap();
    catalog
        .insert_scope(
            &Scope::new(
                ScopeId::new("project-b").unwrap(),
                None,
                ScopeKind::Project,
                "Project B",
            )
            .unwrap(),
        )
        .unwrap();
    catalog
        .insert_scope(
            &Scope::new(
                ScopeId::new("feature-a").unwrap(),
                Some(ScopeId::new("project-a").unwrap()),
                ScopeKind::Task,
                "Feature A",
            )
            .unwrap(),
        )
        .unwrap();

    database
}

pub(crate) fn allocation(scope_id: &str, value: u64) -> Allocation {
    Allocation::new(
        ScopeId::new(scope_id).unwrap(),
        WindowId::new("week-1").unwrap(),
        points(value),
    )
}
