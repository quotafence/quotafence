mod error;
mod state;

use std::{
    error::Error,
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use tauri::{Manager, Runtime, State};

use crate::providers::codex::{self, CodexDetection, CodexSyncResult, CodexSyncStatus};
use crate::providers::codex_hooks::{self, CodexProtectionStatus};
use crate::{
    application::{
        ArchiveQuotaSource, CodexProtectionEventSummary, CreateAccount, CreateAllocatedWorkspace,
        CreateProvider, CreateQuotaPool, CreateQuotaSource, CreateQuotaWindow,
        GetCodexProtectionEvents, GetLocalState, GetQuotaDashboard, LocalState, PolicySummary,
        QuotaDashboard, ReleaseReservation, RemoveWorkspaceAllocation, ReserveQuota,
        ResetWorkspacePolicy, SetAllocation, SetAllocationPriorityOrder, SetWorkspacePolicy,
    },
    paths::DATABASE_FILENAME,
    workspace::canonicalize_workspace_path,
};

pub use error::{IpcError, IpcResult};
use state::AppState;

pub(crate) fn initialize<R: Runtime>(app: &mut tauri::App<R>) -> Result<(), Box<dyn Error>> {
    let app_data_dir = app.path().app_data_dir()?;
    fs::create_dir_all(&app_data_dir)?;

    let state = AppState::open(app_data_dir.join(DATABASE_FILENAME))?;
    if !app.manage(state) {
        return Err(std::io::Error::other("quota service state is already managed").into());
    }

    Ok(())
}

#[tauri::command]
pub(crate) fn create_provider(
    state: State<'_, AppState>,
    request: CreateProvider,
) -> IpcResult<()> {
    state.execute(|service| service.create_provider(request))
}

#[tauri::command]
pub(crate) fn create_account(state: State<'_, AppState>, request: CreateAccount) -> IpcResult<()> {
    state.execute(|service| service.create_account(request))
}

#[tauri::command]
pub(crate) fn create_quota_pool(
    state: State<'_, AppState>,
    request: CreateQuotaPool,
) -> IpcResult<()> {
    state.execute(|service| service.create_quota_pool(request))
}

#[tauri::command]
pub(crate) fn create_quota_window(
    state: State<'_, AppState>,
    request: CreateQuotaWindow,
) -> IpcResult<()> {
    state.execute(|service| service.create_quota_window(request))
}

#[tauri::command]
pub(crate) fn create_quota_source(
    state: State<'_, AppState>,
    request: CreateQuotaSource,
) -> IpcResult<()> {
    state.execute(|service| service.create_quota_source(request))
}

#[tauri::command]
pub(crate) fn archive_quota_source(
    state: State<'_, AppState>,
    request: ArchiveQuotaSource,
) -> IpcResult<()> {
    state.execute(|service| service.archive_quota_source(request))
}

#[tauri::command]
pub(crate) fn create_allocated_workspace(
    state: State<'_, AppState>,
    mut request: CreateAllocatedWorkspace,
) -> IpcResult<()> {
    request.canonical_path = canonicalize_workspace_path(&request.canonical_path)
        .map_err(|error| IpcError::invalid_workspace(error.to_string()))?;
    state.execute(|service| service.create_allocated_workspace(request))
}

#[tauri::command]
pub(crate) fn set_allocation(state: State<'_, AppState>, request: SetAllocation) -> IpcResult<()> {
    state.execute(|service| service.set_allocation(request))
}

#[tauri::command]
pub(crate) fn remove_workspace_allocation(
    state: State<'_, AppState>,
    request: RemoveWorkspaceAllocation,
) -> IpcResult<()> {
    state.execute(|service| service.remove_workspace_allocation(request))
}

#[tauri::command]
pub(crate) fn set_allocation_priority_order(
    state: State<'_, AppState>,
    request: SetAllocationPriorityOrder,
) -> IpcResult<()> {
    state.execute(|service| service.set_allocation_priority_order(request))
}

#[tauri::command]
pub(crate) fn set_workspace_policy(
    state: State<'_, AppState>,
    request: SetWorkspacePolicy,
) -> IpcResult<PolicySummary> {
    state.execute(|service| service.set_workspace_policy(request))
}

#[tauri::command]
pub(crate) fn reset_workspace_policy(
    state: State<'_, AppState>,
    request: ResetWorkspacePolicy,
) -> IpcResult<PolicySummary> {
    state.execute(|service| service.reset_workspace_policy(request))
}

#[tauri::command]
pub(crate) fn reserve_quota(state: State<'_, AppState>, request: ReserveQuota) -> IpcResult<()> {
    state.execute(|service| service.reserve_quota(request))
}

#[tauri::command]
pub(crate) fn release_reservation(
    state: State<'_, AppState>,
    request: ReleaseReservation,
) -> IpcResult<()> {
    state.execute(|service| service.release_reservation(request))
}

#[tauri::command]
pub(crate) fn get_quota_dashboard(
    state: State<'_, AppState>,
    request: GetQuotaDashboard,
) -> IpcResult<QuotaDashboard> {
    state.execute(|service| service.dashboard(request))
}

#[tauri::command]
pub(crate) fn get_local_state(
    state: State<'_, AppState>,
    request: GetLocalState,
) -> IpcResult<LocalState> {
    state.execute(|service| service.local_state(request))
}

#[tauri::command]
pub(crate) async fn detect_codex_quota() -> CodexDetection {
    tauri::async_runtime::spawn_blocking(codex::detect)
        .await
        .unwrap_or_else(|_| codex::detection_failed())
}

#[tauri::command]
pub(crate) async fn sync_codex_quota(
    state: State<'_, AppState>,
    window_id: String,
) -> IpcResult<CodexSyncResult> {
    let now = current_time_millis();
    let (detection, desktop_scan) = tauri::async_runtime::spawn_blocking(|| {
        (codex::detect(), crate::providers::codex_desktop::scan())
    })
    .await
    .unwrap_or_else(|_| {
        (
            codex::detection_failed(),
            crate::providers::codex_desktop::CodexDesktopScan {
                observations: Vec::new(),
                message: Some("Codex Desktop metadata scan stopped unexpectedly.".to_owned()),
            },
        )
    });
    state.execute(|service| {
        let result = codex::sync_detection_with_desktop(
            service,
            window_id.clone(),
            now,
            detection,
            desktop_scan,
        );
        let status = match result.status {
            CodexSyncStatus::Synced => "synced",
            CodexSyncStatus::NotApplicable => "not_applicable",
            CodexSyncStatus::Unavailable => "unavailable",
        };
        let health_window_id = result.window_id.as_deref().unwrap_or(&window_id);
        service.record_provider_sync_health(
            health_window_id,
            status,
            result.message.as_deref(),
            now,
        )?;
        Ok(result)
    })
}

#[tauri::command]
pub(crate) fn get_codex_protection_status() -> IpcResult<CodexProtectionStatus> {
    codex_hooks::protection_status().map_err(IpcError::integration_error)
}

#[tauri::command]
pub(crate) fn install_codex_protection() -> IpcResult<CodexProtectionStatus> {
    let executable = std::env::current_exe().map_err(|error| {
        IpcError::integration_error(format!(
            "Could not resolve the Agent Quota Manager executable: {error}"
        ))
    })?;
    codex_hooks::install_protection(&executable).map_err(IpcError::integration_error)
}

#[tauri::command]
pub(crate) fn uninstall_codex_protection() -> IpcResult<CodexProtectionStatus> {
    codex_hooks::uninstall_protection().map_err(IpcError::integration_error)
}

#[tauri::command]
pub(crate) fn get_codex_protection_events(
    state: State<'_, AppState>,
    request: GetCodexProtectionEvents,
) -> IpcResult<Vec<CodexProtectionEventSummary>> {
    state.execute(|service| service.codex_protection_events(request))
}

fn current_time_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{application::QuotaService, storage::Database};

    fn in_memory_state() -> AppState {
        AppState::new(QuotaService::new(Database::open_in_memory().unwrap()))
    }

    #[test]
    fn state_executes_application_commands() {
        let state = in_memory_state();

        state
            .execute(|service| {
                service.create_provider(CreateProvider {
                    id: "codex".to_owned(),
                    display_name: "Codex".to_owned(),
                })
            })
            .unwrap();

        let duplicate = state
            .execute(|service| {
                service.create_provider(CreateProvider {
                    id: "codex".to_owned(),
                    display_name: "Codex".to_owned(),
                })
            })
            .unwrap_err();

        assert_eq!(duplicate.code, "conflict");
    }

    #[test]
    fn state_maps_validation_failures_for_the_frontend() {
        let state = in_memory_state();

        let error = state
            .execute(|service| {
                service.create_provider(CreateProvider {
                    id: " ".to_owned(),
                    display_name: "Codex".to_owned(),
                })
            })
            .unwrap_err();

        assert_eq!(error.code, "validation_error");
        assert_eq!(error.message, "provider ID cannot be empty");
    }
}
