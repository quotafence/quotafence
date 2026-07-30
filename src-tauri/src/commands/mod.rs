mod error;
mod state;

use std::{
    error::Error,
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use tauri::{Manager, Runtime, State};

use crate::application::{
    CreateAccount, CreateAllocatedScope, CreateProvider, CreateQuotaPool, CreateQuotaSource,
    CreateQuotaWindow, CreateScope, GetLocalState, GetQuotaDashboard, LocalState, QuotaDashboard,
    RecordUsage, ReleaseReservation, ReserveQuota, SetAllocation, SyncProviderQuota,
};
use crate::providers::codex::{self, CodexDetection, DetectedQuotaWindow, DetectionStatus};

pub use error::{IpcError, IpcResult};
use state::AppState;

const DATABASE_FILENAME: &str = "agent-quota-manager.sqlite3";

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
pub(crate) fn create_scope(state: State<'_, AppState>, request: CreateScope) -> IpcResult<()> {
    state.execute(|service| service.create_scope(request))
}

#[tauri::command]
pub(crate) fn create_allocated_scope(
    state: State<'_, AppState>,
    request: CreateAllocatedScope,
) -> IpcResult<()> {
    state.execute(|service| service.create_allocated_scope(request))
}

#[tauri::command]
pub(crate) fn set_allocation(state: State<'_, AppState>, request: SetAllocation) -> IpcResult<()> {
    state.execute(|service| service.set_allocation(request))
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
pub(crate) fn record_usage(state: State<'_, AppState>, request: RecordUsage) -> IpcResult<()> {
    state.execute(|service| service.record_usage(request))
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CodexSyncStatus {
    Synced,
    NotApplicable,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexSyncResult {
    status: CodexSyncStatus,
    window_id: Option<String>,
    rolled_over: bool,
    synced_at: Option<i64>,
    message: Option<String>,
}

#[tauri::command]
pub(crate) async fn sync_codex_quota(
    state: State<'_, AppState>,
    window_id: String,
) -> IpcResult<CodexSyncResult> {
    let now = current_time_millis();
    let source = match state.execute(|service| {
        service.local_state(GetLocalState {
            selected_window_id: Some(window_id.clone()),
            at: now,
        })
    }) {
        Ok(local_state) => local_state
            .sources
            .into_iter()
            .find(|source| source.window_id == window_id),
        Err(error) => {
            return Ok(CodexSyncResult {
                status: CodexSyncStatus::Unavailable,
                window_id: Some(window_id),
                rolled_over: false,
                synced_at: None,
                message: Some(error.message),
            });
        }
    };
    let Some(source) = source else {
        return Ok(CodexSyncResult {
            status: CodexSyncStatus::Unavailable,
            window_id: Some(window_id),
            rolled_over: false,
            synced_at: None,
            message: Some("The selected quota source no longer exists.".to_owned()),
        });
    };

    if !source.provider_display_name.eq_ignore_ascii_case("codex") || source.unit != "percent" {
        return Ok(CodexSyncResult {
            status: CodexSyncStatus::NotApplicable,
            window_id: Some(source.window_id),
            rolled_over: false,
            synced_at: None,
            message: None,
        });
    }

    let detection = tauri::async_runtime::spawn_blocking(codex::detect)
        .await
        .unwrap_or_else(|_| codex::detection_failed());
    if detection.status != DetectionStatus::Detected {
        return Ok(CodexSyncResult {
            status: CodexSyncStatus::Unavailable,
            window_id: Some(source.window_id),
            rolled_over: false,
            synced_at: None,
            message: detection.message,
        });
    }

    let Some(remote_window) = matching_window(&detection.windows, source.starts_at, source.ends_at)
    else {
        return Ok(CodexSyncResult {
            status: CodexSyncStatus::Unavailable,
            window_id: Some(source.window_id),
            rolled_over: false,
            synced_at: None,
            message: Some(
                "Codex returned quota windows, but none matched this local source.".to_owned(),
            ),
        });
    };
    let sync = state.execute(|service| {
        service.sync_provider_quota(SyncProviderQuota {
            current_window_id: source.window_id.clone(),
            adapter: "codex_app_server".to_owned(),
            remote_limit_id: detection.provider_id,
            remote_window_kind: remote_window.kind.clone(),
            starts_at: remote_window.starts_at,
            ends_at: remote_window.ends_at,
            capacity: remote_window.capacity,
            used: remote_window.used,
            unit: remote_window.unit.clone(),
            observed_at: now,
        })
    });

    Ok(match sync {
        Ok(result) => CodexSyncResult {
            status: CodexSyncStatus::Synced,
            window_id: Some(result.window_id),
            rolled_over: result.rolled_over,
            synced_at: Some(now),
            message: None,
        },
        Err(error) => CodexSyncResult {
            status: CodexSyncStatus::Unavailable,
            window_id: Some(source.window_id),
            rolled_over: false,
            synced_at: None,
            message: Some(error.message),
        },
    })
}

fn matching_window(
    windows: &[DetectedQuotaWindow],
    local_starts_at: i64,
    local_ends_at: i64,
) -> Option<&DetectedQuotaWindow> {
    let local_duration = local_ends_at.checked_sub(local_starts_at)?;
    windows
        .iter()
        .filter_map(|window| {
            let remote_duration = window.ends_at.checked_sub(window.starts_at)?;
            let difference = remote_duration.abs_diff(local_duration);
            (difference <= 5 * 60_000).then_some((difference, window))
        })
        .min_by_key(|(difference, _)| *difference)
        .map(|(_, window)| window)
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
