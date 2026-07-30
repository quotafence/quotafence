use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::{
    application::{
        AbandonProviderSessionObservations, AbandonProviderTurnObservation,
        BeginProviderTurnObservation, GetLocalState, GetProviderTurnObservation,
        GetWorkspaceContext, QuotaService, ReconcileProviderTurnObservation,
        TurnReconciliationResult, WorkspaceContext,
    },
    workspace::canonicalize_workspace_path,
};

use super::codex::{self, CodexDetection, CodexSyncStatus};

const CODEX_ADAPTER: &str = "codex_app_server";
const AQM_STATUS_PREFIX: &str = "AQM: ";
const TRACKED_EVENTS: [(&str, u64, &str); 3] = [
    ("UserPromptSubmit", 90, "capturing quota baseline"),
    ("Stop", 90, "reconciling workspace usage"),
    ("SessionEnd", 3, "cleaning session state"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexHookEventKind {
    UserPromptSubmit,
    Stop,
    SessionEnd,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CodexHookEvent {
    pub session_id: String,
    pub cwd: String,
    pub hook_event_name: String,
    #[serde(default)]
    pub turn_id: Option<String>,
}

impl CodexHookEvent {
    pub fn parse(input: &str) -> Result<Self, String> {
        serde_json::from_str(input).map_err(|error| format!("invalid Codex hook input: {error}"))
    }

    pub fn from_reader(reader: impl Read) -> Result<Self, String> {
        serde_json::from_reader(reader)
            .map_err(|error| format!("invalid Codex hook input: {error}"))
    }

    pub fn kind(&self) -> CodexHookEventKind {
        match self.hook_event_name.as_str() {
            "UserPromptSubmit" => CodexHookEventKind::UserPromptSubmit,
            "Stop" => CodexHookEventKind::Stop,
            "SessionEnd" => CodexHookEventKind::SessionEnd,
            _ => CodexHookEventKind::Other,
        }
    }

    fn required_turn_id(&self) -> Result<&str, String> {
        self.turn_id
            .as_deref()
            .filter(|turn_id| !turn_id.trim().is_empty())
            .ok_or_else(|| format!("{} hook input has no turn_id", self.hook_event_name))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexHookOutcome {
    ObservationStarted {
        window_id: String,
        scope_id: Option<String>,
        contended: bool,
    },
    Reconciled(TurnReconciliationResult),
    SessionCleaned {
        abandoned_turns: usize,
    },
    Skipped,
}

pub fn default_user_hooks_path() -> Result<PathBuf, String> {
    dirs::home_dir()
        .map(|home| home.join(".codex").join("hooks.json"))
        .ok_or_else(|| "cannot resolve the current user's home directory".to_owned())
}

pub fn install_user_hooks(config_path: &Path, executable: &Path) -> Result<bool, String> {
    let original = read_hook_config(config_path)?;
    let mut updated = original.clone();
    remove_aqm_hooks(&mut updated)?;
    add_aqm_hooks(&mut updated, executable)?;
    if updated == original {
        return Ok(false);
    }
    write_hook_config(config_path, &updated)?;
    Ok(true)
}

pub fn uninstall_user_hooks(config_path: &Path) -> Result<bool, String> {
    let original = read_hook_config(config_path)?;
    let mut updated = original.clone();
    remove_aqm_hooks(&mut updated)?;
    if updated == original {
        return Ok(false);
    }
    write_hook_config(config_path, &updated)?;
    Ok(true)
}

pub fn user_hooks_installed(config_path: &Path) -> Result<bool, String> {
    let config = read_hook_config(config_path)?;
    let Some(hooks) = config.get("hooks").and_then(Value::as_object) else {
        return Ok(false);
    };
    Ok(TRACKED_EVENTS.iter().all(|(event, _, _)| {
        hooks
            .get(*event)
            .and_then(Value::as_array)
            .is_some_and(|groups| groups.iter().any(group_contains_aqm_hook))
    }))
}

pub fn handle_event<F>(
    service: &mut QuotaService,
    event: &CodexHookEvent,
    observed_at: i64,
    detect: F,
) -> Result<CodexHookOutcome, String>
where
    F: FnOnce() -> Option<CodexDetection>,
{
    match event.kind() {
        CodexHookEventKind::UserPromptSubmit => begin_turn(service, event, observed_at, detect),
        CodexHookEventKind::Stop => finish_turn(service, event, observed_at, detect),
        CodexHookEventKind::SessionEnd => {
            let abandoned_turns = service
                .abandon_provider_session_observations(AbandonProviderSessionObservations {
                    session_id: event.session_id.clone(),
                })
                .map_err(|error| error.to_string())?;
            Ok(CodexHookOutcome::SessionCleaned { abandoned_turns })
        }
        CodexHookEventKind::Other => Ok(CodexHookOutcome::Skipped),
    }
}

fn begin_turn<F>(
    service: &mut QuotaService,
    event: &CodexHookEvent,
    observed_at: i64,
    detect: F,
) -> Result<CodexHookOutcome, String>
where
    F: FnOnce() -> Option<CodexDetection>,
{
    let turn_id = event.required_turn_id()?;
    let canonical_path =
        canonicalize_workspace_path(&event.cwd).map_err(|error| error.to_string())?;
    let context = service
        .workspace_context(GetWorkspaceContext {
            canonical_path: canonical_path.clone(),
            at: observed_at,
        })
        .map_err(|error| error.to_string())?;
    let (window_id, scope_id) = observation_target(service, &context, observed_at)?;
    let Some(window_id) = window_id else {
        return Ok(CodexHookOutcome::Skipped);
    };
    let Some(detection) = detect() else {
        return Ok(CodexHookOutcome::Skipped);
    };
    let checkpoint = codex::sync_detection(service, window_id, observed_at, detection);
    if checkpoint.status != CodexSyncStatus::Synced {
        return Ok(CodexHookOutcome::Skipped);
    }
    let Some(window_id) = checkpoint.window_id else {
        return Ok(CodexHookOutcome::Skipped);
    };

    let result = service
        .begin_provider_turn_observation(BeginProviderTurnObservation {
            session_id: event.session_id.clone(),
            turn_id: turn_id.to_owned(),
            adapter: CODEX_ADAPTER.to_owned(),
            canonical_path,
            scope_id: scope_id.clone(),
            window_id: window_id.clone(),
            started_at: observed_at,
        })
        .map_err(|error| error.to_string())?;

    Ok(CodexHookOutcome::ObservationStarted {
        window_id,
        scope_id,
        contended: result.contended,
    })
}

fn finish_turn<F>(
    service: &mut QuotaService,
    event: &CodexHookEvent,
    observed_at: i64,
    detect: F,
) -> Result<CodexHookOutcome, String>
where
    F: FnOnce() -> Option<CodexDetection>,
{
    let turn_id = event.required_turn_id()?;
    let observation = service
        .provider_turn_observation(GetProviderTurnObservation {
            session_id: event.session_id.clone(),
            turn_id: turn_id.to_owned(),
        })
        .map_err(|error| error.to_string())?;
    let Some(observation) = observation else {
        return Ok(CodexHookOutcome::Skipped);
    };

    let Some(detection) = detect() else {
        abandon_turn(service, event, turn_id)?;
        return Ok(CodexHookOutcome::Skipped);
    };
    let checkpoint = codex::sync_detection(service, observation.window_id, observed_at, detection);
    if checkpoint.status != CodexSyncStatus::Synced {
        abandon_turn(service, event, turn_id)?;
        return Ok(CodexHookOutcome::Skipped);
    }
    let Some(current_window_id) = checkpoint.window_id else {
        abandon_turn(service, event, turn_id)?;
        return Ok(CodexHookOutcome::Skipped);
    };

    let result = service
        .reconcile_provider_turn_observation(ReconcileProviderTurnObservation {
            session_id: event.session_id.clone(),
            turn_id: turn_id.to_owned(),
            current_window_id,
            observed_at,
        })
        .map_err(|error| error.to_string())?;
    Ok(CodexHookOutcome::Reconciled(result))
}

fn abandon_turn(
    service: &mut QuotaService,
    event: &CodexHookEvent,
    turn_id: &str,
) -> Result<(), String> {
    service
        .abandon_provider_turn_observation(AbandonProviderTurnObservation {
            session_id: event.session_id.clone(),
            turn_id: turn_id.to_owned(),
        })
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn observation_target(
    service: &mut QuotaService,
    context: &WorkspaceContext,
    observed_at: i64,
) -> Result<(Option<String>, Option<String>), String> {
    let mut mapped = context.allocations.iter().filter(|allocation| {
        allocation
            .provider_display_name
            .eq_ignore_ascii_case("codex")
            && allocation.unit == "percent"
    });
    if let Some(allocation) = mapped.next() {
        if mapped.next().is_some() {
            return Err("workspace has multiple active Codex percentage allocations".to_owned());
        }
        return Ok((
            Some(allocation.window_id.clone()),
            context
                .binding
                .as_ref()
                .map(|binding| binding.scope_id.clone()),
        ));
    }

    let state = service
        .local_state(GetLocalState {
            selected_window_id: None,
            at: observed_at,
        })
        .map_err(|error| error.to_string())?;
    let mut sources = state.sources.into_iter().filter(|source| {
        source.provider_managed
            && source.provider_display_name.eq_ignore_ascii_case("codex")
            && source.unit == "percent"
    });
    let source = sources.next();
    if sources.next().is_some() {
        return Err("multiple Codex percentage sources are not supported".to_owned());
    }

    Ok((source.map(|source| source.window_id), None))
}

fn read_hook_config(config_path: &Path) -> Result<Value, String> {
    if !config_path.exists() {
        return Ok(json!({}));
    }
    let contents = fs::read_to_string(config_path)
        .map_err(|error| format!("cannot read {}: {error}", config_path.display()))?;
    let value: Value = serde_json::from_str(&contents)
        .map_err(|error| format!("{} is not valid JSON: {error}", config_path.display()))?;
    if !value.is_object() {
        return Err(format!(
            "{} must contain a JSON object",
            config_path.display()
        ));
    }
    Ok(value)
}

fn write_hook_config(config_path: &Path, config: &Value) -> Result<(), String> {
    let parent = config_path.parent().ok_or_else(|| {
        format!(
            "hook configuration path {} has no parent directory",
            config_path.display()
        )
    })?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;

    if config_path.exists() {
        let backup = config_path.with_extension("json.aqm.bak");
        if !backup.exists() {
            fs::copy(config_path, &backup).map_err(|error| {
                format!(
                    "cannot back up {} to {}: {error}",
                    config_path.display(),
                    backup.display()
                )
            })?;
        }
    }

    let contents = serde_json::to_string_pretty(config)
        .map_err(|error| format!("cannot serialize hook configuration: {error}"))?;
    let temporary = config_path.with_extension("json.aqm.tmp");
    fs::write(&temporary, format!("{contents}\n"))
        .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, config_path).map_err(|error| {
        format!(
            "cannot replace {} with {}: {error}",
            config_path.display(),
            temporary.display()
        )
    })?;
    Ok(())
}

fn add_aqm_hooks(config: &mut Value, executable: &Path) -> Result<(), String> {
    let root = config
        .as_object_mut()
        .ok_or_else(|| "hook configuration root must be a JSON object".to_owned())?;
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| "hooks must be a JSON object".to_owned())?;
    let command = hook_command(executable);

    for (event, timeout, message) in TRACKED_EVENTS {
        let groups = hooks
            .entry(event)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| format!("hooks.{event} must be an array"))?;
        groups.push(json!({
            "hooks": [{
                "type": "command",
                "command": command,
                "timeout": timeout,
                "statusMessage": format!("{AQM_STATUS_PREFIX}{message}")
            }]
        }));
    }
    Ok(())
}

fn remove_aqm_hooks(config: &mut Value) -> Result<(), String> {
    let Some(root) = config.as_object_mut() else {
        return Err("hook configuration root must be a JSON object".to_owned());
    };
    let Some(hooks_value) = root.get_mut("hooks") else {
        return Ok(());
    };
    let hooks = hooks_value
        .as_object_mut()
        .ok_or_else(|| "hooks must be a JSON object".to_owned())?;

    for (event, _, _) in TRACKED_EVENTS {
        let Some(groups_value) = hooks.get_mut(event) else {
            continue;
        };
        let groups = groups_value
            .as_array_mut()
            .ok_or_else(|| format!("hooks.{event} must be an array"))?;
        for group in groups.iter_mut() {
            let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                continue;
            };
            handlers.retain(|handler| !is_aqm_handler(handler));
        }
        groups.retain(|group| {
            group
                .get("hooks")
                .and_then(Value::as_array)
                .is_none_or(|handlers| !handlers.is_empty())
        });
    }
    Ok(())
}

fn group_contains_aqm_hook(group: &Value) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|handlers| handlers.iter().any(is_aqm_handler))
}

fn is_aqm_handler(handler: &Value) -> bool {
    handler
        .get("statusMessage")
        .and_then(Value::as_str)
        .is_some_and(|message| message.starts_with(AQM_STATUS_PREFIX))
        && handler
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|command| command.ends_with(" hook codex"))
}

fn hook_command(executable: &Path) -> String {
    #[cfg(windows)]
    {
        format!("\"{}\" hook codex", executable.display())
    }
    #[cfg(not(windows))]
    {
        let path = executable.to_string_lossy().replace('\'', "'\\''");
        format!("'{path}' hook codex")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::{
            CreateAllocatedWorkspace, CreateQuotaSource, GetQuotaDashboard,
            ProviderQuotaSnapshotInput,
        },
        providers::codex::{DetectedQuotaWindow, DetectionStatus},
        storage::Database,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn hook_parser_ignores_prompt_and_assistant_content() {
        let event = CodexHookEvent::parse(
            r#"{
                "session_id": "session-1",
                "turn_id": "turn-1",
                "cwd": "/code/project",
                "hook_event_name": "UserPromptSubmit",
                "prompt": "sensitive prompt",
                "last_assistant_message": "sensitive response",
                "transcript_path": "/tmp/transcript.jsonl"
            }"#,
        )
        .unwrap();

        assert_eq!(event.session_id, "session-1");
        assert_eq!(event.turn_id.as_deref(), Some("turn-1"));
        assert_eq!(event.kind(), CodexHookEventKind::UserPromptSubmit);
    }

    #[test]
    fn only_turn_scoped_events_require_a_turn_id() {
        let end = CodexHookEvent::parse(
            r#"{
                "session_id": "session-1",
                "cwd": "/code/project",
                "hook_event_name": "SessionEnd"
            }"#,
        )
        .unwrap();
        assert_eq!(end.kind(), CodexHookEventKind::SessionEnd);

        let stop = CodexHookEvent {
            hook_event_name: "Stop".to_owned(),
            ..end
        };
        assert!(stop.required_turn_id().unwrap_err().contains("turn_id"));
    }

    #[test]
    fn hook_install_preserves_unrelated_handlers_and_is_idempotent() {
        let mut config = json!({
            "description": "existing",
            "hooks": {
                "Stop": [{
                    "hooks": [{
                        "type": "command",
                        "command": "existing-hook"
                    }]
                }]
            }
        });
        add_aqm_hooks(&mut config, Path::new("/Applications/AQM/aqm")).unwrap();
        let once = config.clone();
        remove_aqm_hooks(&mut config).unwrap();
        add_aqm_hooks(&mut config, Path::new("/Applications/AQM/aqm")).unwrap();

        assert_eq!(config, once);
        let stop = config["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2);
        assert_eq!(
            stop[0]["hooks"][0]["command"].as_str(),
            Some("existing-hook")
        );
    }

    #[test]
    fn uninstall_removes_only_aqm_handlers() {
        let mut config = json!({
            "hooks": {
                "Stop": [{
                    "hooks": [
                        {
                            "type": "command",
                            "command": "existing-hook"
                        },
                        {
                            "type": "command",
                            "command": "'/tmp/aqm' hook codex",
                            "statusMessage": "AQM: reconciling workspace usage"
                        }
                    ]
                }]
            }
        });
        remove_aqm_hooks(&mut config).unwrap();

        let handlers = config["hooks"]["Stop"][0]["hooks"].as_array().unwrap();
        assert_eq!(handlers.len(), 1);
        assert_eq!(handlers[0]["command"].as_str(), Some("existing-hook"));
    }

    #[test]
    fn installer_round_trip_is_idempotent_and_keeps_existing_config() {
        let directory = temporary_folder("installer");
        let config_path = directory.join("hooks.json");
        std::fs::write(
            &config_path,
            r#"{
                "description": "existing",
                "hooks": {
                    "Stop": [{
                        "hooks": [{
                            "type": "command",
                            "command": "existing-hook"
                        }]
                    }]
                }
            }"#,
        )
        .unwrap();

        assert!(install_user_hooks(&config_path, Path::new("/tmp/aqm")).unwrap());
        assert!(user_hooks_installed(&config_path).unwrap());
        assert!(!install_user_hooks(&config_path, Path::new("/tmp/aqm")).unwrap());
        assert!(uninstall_user_hooks(&config_path).unwrap());
        assert!(!user_hooks_installed(&config_path).unwrap());
        let restored: Value =
            serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(restored["description"].as_str(), Some("existing"));
        assert_eq!(
            restored["hooks"]["Stop"][0]["hooks"][0]["command"].as_str(),
            Some("existing-hook")
        );

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn hook_turn_attributes_checkpoint_delta_to_the_bound_folder() {
        let folder = temporary_folder("attribution");
        let canonical_path = canonicalize_workspace_path(&folder).unwrap();
        let mut service = QuotaService::new(Database::open_in_memory().unwrap());
        service
            .create_quota_source(CreateQuotaSource {
                provider_id: "codex".to_owned(),
                provider_display_name: "Codex".to_owned(),
                account_id: "codex-account".to_owned(),
                account_display_name: "Subscription".to_owned(),
                pool_id: "codex-weekly".to_owned(),
                pool_display_name: "Weekly allowance".to_owned(),
                window_id: "codex-window".to_owned(),
                starts_at: 1_000,
                ends_at: 10_000,
                capacity: 100,
                unit: "percent".to_owned(),
                provider_snapshot: Some(ProviderQuotaSnapshotInput {
                    adapter: CODEX_ADAPTER.to_owned(),
                    remote_limit_id: "codex".to_owned(),
                    remote_window_kind: "secondary".to_owned(),
                    used: 10,
                    observed_at: 2_000,
                    resets_at: 10_000,
                }),
            })
            .unwrap();
        service
            .create_allocated_workspace(CreateAllocatedWorkspace {
                id: "workspace-a".to_owned(),
                display_name: "Workspace A".to_owned(),
                canonical_path: canonical_path.clone(),
                window_id: "codex-window".to_owned(),
                amount: 20,
                unit: "percent".to_owned(),
                bound_at: 2_000,
            })
            .unwrap();

        let start = CodexHookEvent {
            session_id: "session-1".to_owned(),
            turn_id: Some("turn-1".to_owned()),
            cwd: canonical_path.clone(),
            hook_event_name: "UserPromptSubmit".to_owned(),
        };
        let outcome = handle_event(&mut service, &start, 3_000, || Some(detection(10))).unwrap();
        assert!(matches!(
            outcome,
            CodexHookOutcome::ObservationStarted {
                scope_id: Some(ref scope_id),
                contended: false,
                ..
            } if scope_id == "workspace-a"
        ));

        let stop = CodexHookEvent {
            hook_event_name: "Stop".to_owned(),
            ..start
        };
        let outcome = handle_event(&mut service, &stop, 4_000, || Some(detection(14))).unwrap();
        assert!(matches!(
            outcome,
            CodexHookOutcome::Reconciled(TurnReconciliationResult {
                status: crate::application::TurnReconciliationStatus::Attributed,
                amount: Some(4),
                ..
            })
        ));

        let dashboard = service
            .dashboard(GetQuotaDashboard {
                window_id: "codex-window".to_owned(),
                at: 4_000,
            })
            .unwrap();
        assert_eq!(dashboard.allocations[0].attributed_usage, 4);
        assert_eq!(dashboard.allocations[0].spendable, 16);
        assert_eq!(dashboard.window.unattributed_usage, 10);

        std::fs::remove_dir(folder).unwrap();
    }

    #[test]
    fn missing_stop_checkpoint_abandons_the_baseline() {
        let folder = temporary_folder("abandon");
        let canonical_path = canonicalize_workspace_path(&folder).unwrap();
        let mut service = QuotaService::new(Database::open_in_memory().unwrap());
        service
            .create_quota_source(CreateQuotaSource {
                provider_id: "codex".to_owned(),
                provider_display_name: "Codex".to_owned(),
                account_id: "codex-account".to_owned(),
                account_display_name: "Subscription".to_owned(),
                pool_id: "codex-weekly".to_owned(),
                pool_display_name: "Weekly allowance".to_owned(),
                window_id: "codex-window".to_owned(),
                starts_at: 1_000,
                ends_at: 10_000,
                capacity: 100,
                unit: "percent".to_owned(),
                provider_snapshot: Some(ProviderQuotaSnapshotInput {
                    adapter: CODEX_ADAPTER.to_owned(),
                    remote_limit_id: "codex".to_owned(),
                    remote_window_kind: "secondary".to_owned(),
                    used: 10,
                    observed_at: 2_000,
                    resets_at: 10_000,
                }),
            })
            .unwrap();

        let event = CodexHookEvent {
            session_id: "session-1".to_owned(),
            turn_id: Some("turn-1".to_owned()),
            cwd: canonical_path,
            hook_event_name: "UserPromptSubmit".to_owned(),
        };
        handle_event(&mut service, &event, 3_000, || Some(detection(10))).unwrap();
        let stop = CodexHookEvent {
            hook_event_name: "Stop".to_owned(),
            ..event
        };
        assert_eq!(
            handle_event(&mut service, &stop, 4_000, || None).unwrap(),
            CodexHookOutcome::Skipped
        );
        assert!(service
            .provider_turn_observation(GetProviderTurnObservation {
                session_id: "session-1".to_owned(),
                turn_id: "turn-1".to_owned(),
            })
            .unwrap()
            .is_none());

        std::fs::remove_dir(folder).unwrap();
    }

    #[test]
    fn hook_start_refreshes_an_expired_local_window_before_observing() {
        let folder = temporary_folder("rollover");
        let canonical_path = canonicalize_workspace_path(&folder).unwrap();
        let mut service = QuotaService::new(Database::open_in_memory().unwrap());
        service
            .create_quota_source(CreateQuotaSource {
                provider_id: "codex".to_owned(),
                provider_display_name: "Codex".to_owned(),
                account_id: "codex-account".to_owned(),
                account_display_name: "Subscription".to_owned(),
                pool_id: "codex-weekly".to_owned(),
                pool_display_name: "Weekly allowance".to_owned(),
                window_id: "codex-window".to_owned(),
                starts_at: 1_000,
                ends_at: 10_000,
                capacity: 100,
                unit: "percent".to_owned(),
                provider_snapshot: Some(ProviderQuotaSnapshotInput {
                    adapter: CODEX_ADAPTER.to_owned(),
                    remote_limit_id: "codex".to_owned(),
                    remote_window_kind: "secondary".to_owned(),
                    used: 90,
                    observed_at: 9_000,
                    resets_at: 10_000,
                }),
            })
            .unwrap();
        service
            .create_allocated_workspace(CreateAllocatedWorkspace {
                id: "workspace-a".to_owned(),
                display_name: "Workspace A".to_owned(),
                canonical_path: canonical_path.clone(),
                window_id: "codex-window".to_owned(),
                amount: 20,
                unit: "percent".to_owned(),
                bound_at: 2_000,
            })
            .unwrap();
        let event = CodexHookEvent {
            session_id: "session-1".to_owned(),
            turn_id: Some("turn-1".to_owned()),
            cwd: canonical_path,
            hook_event_name: "UserPromptSubmit".to_owned(),
        };
        let next_window = detection_window(3, 10_000, 19_000);

        let outcome = handle_event(&mut service, &event, 11_000, || Some(next_window)).unwrap();

        assert!(matches!(
            outcome,
            CodexHookOutcome::ObservationStarted {
                ref window_id,
                scope_id: Some(ref scope_id),
                ..
            } if window_id == "codex-weekly-window-19000" && scope_id == "workspace-a"
        ));
        std::fs::remove_dir(folder).unwrap();
    }

    fn detection(used: u64) -> CodexDetection {
        detection_window(used, 1_000, 10_000)
    }

    fn detection_window(used: u64, starts_at: i64, ends_at: i64) -> CodexDetection {
        CodexDetection {
            status: DetectionStatus::Detected,
            provider_id: "codex".to_owned(),
            provider_display_name: "Codex".to_owned(),
            plan_type: Some("plus".to_owned()),
            windows: vec![DetectedQuotaWindow {
                id: "codex-secondary".to_owned(),
                display_name: "Weekly allowance".to_owned(),
                kind: "secondary".to_owned(),
                starts_at,
                ends_at,
                capacity: 100,
                used,
                remaining: 100 - used,
                unit: "percent".to_owned(),
                duration_minutes: 1,
            }],
            message: None,
        }
    }

    fn temporary_folder(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let folder = std::env::temp_dir().join(format!(
            "aqm-codex-hook-{label}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&folder).unwrap();
        folder
    }
}
