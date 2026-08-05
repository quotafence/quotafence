mod allocations;
mod catalog;
mod codec;
mod database;
mod desktop_usage;
mod error;
mod ledger;
mod managed_sessions;
mod migrations;
mod policies;
mod protection_events;
mod provider_snapshots;
mod turn_observations;
mod workspace_bindings;

#[cfg(test)]
mod test_support;

pub use allocations::AllocationRepository;
pub use catalog::CatalogRepository;
pub use database::Database;
pub use desktop_usage::{
    DesktopReconciliation, DesktopReconciliationStatus, DesktopThreadObservation,
};
pub use error::{StorageError, StorageResult};
pub use ledger::LedgerRepository;
pub use managed_sessions::{
    ManagedSession, ManagedSessionReconciliationOutcome, ManagedSessionReconciliationResult,
    ManagedSessionRepository, ManagedSessionStatus, NewManagedSession, ReconciliationStatus,
};
pub use policies::{PolicyOverrideAudit, WorkspacePolicy, WorkspacePolicyRepository};
pub use protection_events::{
    CodexHookReceipt, CodexHookReceiptStatus, CodexProtectionEvent, CodexProtectionEventOutcome,
    CodexProtectionEventRepository, NewCodexProtectionEvent,
};
pub use provider_snapshots::{ProviderQuotaHistoryPoint, ProviderQuotaSnapshot};
pub use turn_observations::{
    BeginObservationResult, BeginObservationStatus, ProviderTurnObservation,
    ReconcileObservationResult, TurnObservationRepository,
};
pub use workspace_bindings::{WorkspaceBinding, WorkspaceBindingRepository};
