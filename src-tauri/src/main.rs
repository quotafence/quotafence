// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments == ["hook", "codex"] {
        let output = agent_quota_manager_lib::providers::codex_hooks::run_installed_hook(
            std::io::stdin().lock(),
        )
        .unwrap_or_else(|error| {
            if std::env::var_os("AQM_HOOK_DEBUG").is_some() {
                eprintln!("aqm hook: {error}");
            }
            serde_json::json!({})
        });
        println!("{output}");
        return;
    }
    agent_quota_manager_lib::run()
}
