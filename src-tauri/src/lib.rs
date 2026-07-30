pub mod application;
mod commands;
pub mod domain;
pub mod paths;
pub mod providers;
pub mod storage;
pub mod workspace;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(commands::initialize)
        .invoke_handler(tauri::generate_handler![
            commands::create_provider,
            commands::create_account,
            commands::create_quota_pool,
            commands::create_quota_window,
            commands::create_quota_source,
            commands::archive_quota_source,
            commands::create_allocated_workspace,
            commands::set_allocation,
            commands::set_workspace_policy,
            commands::reset_workspace_policy,
            commands::reserve_quota,
            commands::release_reservation,
            commands::get_quota_dashboard,
            commands::get_local_state,
            commands::detect_codex_quota,
            commands::sync_codex_quota,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
