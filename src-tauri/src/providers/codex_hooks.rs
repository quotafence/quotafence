use std::{
    env, fs,
    io::Read,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::{
    application::{
        AbandonProviderSessionObservations, AbandonProviderTurnObservation,
        BeginProviderTurnObservation, EvaluateWorkspaceAdmission, GetLocalState,
        GetProviderTurnObservation, GetWorkspaceContext, QuotaService,
        ReconcileProviderTurnObservation, RecordCodexProtectionEvent, TurnReconciliationResult,
        WorkspaceContext,
    },
    domain::EnforcementDecision,
    paths,
    storage::Database,
    workspace::canonicalize_workspace_path,
};

use super::codex::{self, CodexDetection, CodexSyncStatus};

const CODEX_ADAPTER: &str = "codex_app_server";
const AQM_STATUS_PREFIX: &str = "AQM: ";
const INSTALLED_EVENTS: [(&str, u64, &str); 2] = [
    ("UserPromptSubmit", 90, "checking workspace allocation"),
    ("Stop", 90, "reconciling workspace usage"),
];
const OWNED_EVENT_NAMES: [&str; 3] = ["UserPromptSubmit", "Stop", "SessionEnd"];

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
    Blocked {
        reason: String,
    },
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexProtectionStatus {
    pub installed: bool,
    pub has_aqm_hooks: bool,
    pub requires_review: bool,
    pub verification_required_after: Option<i64>,
    pub config_path: String,
    pub state: CodexProtectionState,
    pub issue: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexProtectionState {
    Disabled,
    Configured,
    Misconfigured,
}

pub fn hook_output(outcome: &CodexHookOutcome) -> Value {
    match outcome {
        CodexHookOutcome::Blocked { reason } => json!({
            "decision": "block",
            "reason": reason,
        }),
        _ => json!({}),
    }
}

pub fn protection_status() -> Result<CodexProtectionStatus, String> {
    let config_path = default_user_hooks_path()?;
    let executable = env::current_exe()
        .map_err(|error| format!("cannot resolve the Agent Quota Manager executable: {error}"))?;
    Ok(protection_status_for(&config_path, &executable))
}

pub fn install_protection(executable: &Path) -> Result<CodexProtectionStatus, String> {
    let config_path = default_user_hooks_path()?;
    install_user_hooks(&config_path, executable)?;
    Ok(protection_status_for(&config_path, executable))
}

pub fn uninstall_protection() -> Result<CodexProtectionStatus, String> {
    let config_path = default_user_hooks_path()?;
    uninstall_user_hooks(&config_path)?;
    let executable = env::current_exe()
        .map_err(|error| format!("cannot resolve the Agent Quota Manager executable: {error}"))?;
    Ok(protection_status_for(&config_path, &executable))
}

fn protection_status_for(config_path: &Path, executable: &Path) -> CodexProtectionStatus {
    let config_path_text = config_path.display().to_string();
    let verification_required_after = [config_path, executable]
        .into_iter()
        .filter_map(file_modified_at_millis)
        .max();
    let config = match read_hook_config(config_path) {
        Ok(config) => config,
        Err(issue) => {
            return CodexProtectionStatus {
                installed: false,
                has_aqm_hooks: false,
                requires_review: false,
                verification_required_after,
                config_path: config_path_text,
                state: CodexProtectionState::Misconfigured,
                issue: Some(issue),
            };
        }
    };
    let expected_command = hook_command(executable);
    let Some(hooks) = config.get("hooks").and_then(Value::as_object) else {
        return CodexProtectionStatus {
            installed: false,
            has_aqm_hooks: false,
            requires_review: false,
            verification_required_after,
            config_path: config_path_text,
            state: CodexProtectionState::Disabled,
            issue: None,
        };
    };
    let aqm_handlers = OWNED_EVENT_NAMES
        .iter()
        .flat_map(|event| {
            hooks
                .get(*event)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|group| group.get("hooks").and_then(Value::as_array))
                .flatten()
                .filter(|handler| is_aqm_handler(handler))
        })
        .collect::<Vec<_>>();
    if aqm_handlers.is_empty() {
        return CodexProtectionStatus {
            installed: false,
            has_aqm_hooks: false,
            requires_review: false,
            verification_required_after,
            config_path: config_path_text,
            state: CodexProtectionState::Disabled,
            issue: None,
        };
    }

    let every_event_is_current = INSTALLED_EVENTS.iter().all(|(event, _, _)| {
        hooks
            .get(*event)
            .and_then(Value::as_array)
            .is_some_and(|groups| {
                groups
                    .iter()
                    .filter_map(|group| group.get("hooks").and_then(Value::as_array))
                    .flatten()
                    .any(|handler| {
                        is_aqm_handler(handler)
                            && handler.get("command").and_then(Value::as_str)
                                == Some(expected_command.as_str())
                    })
            })
    });
    let has_legacy_session_end = hooks
        .get("SessionEnd")
        .and_then(Value::as_array)
        .is_some_and(|groups| groups.iter().any(group_contains_aqm_hook));
    if every_event_is_current && executable.is_file() && !has_legacy_session_end {
        CodexProtectionStatus {
            installed: true,
            has_aqm_hooks: true,
            requires_review: true,
            verification_required_after,
            config_path: config_path_text,
            state: CodexProtectionState::Configured,
            issue: None,
        }
    } else {
        CodexProtectionStatus {
            installed: false,
            has_aqm_hooks: true,
            requires_review: false,
            verification_required_after,
            config_path: config_path_text,
            state: CodexProtectionState::Misconfigured,
            issue: Some(if has_legacy_session_end {
                "AQM found a legacy SessionEnd entry that Codex Desktop may not expose for review. Repair protection to keep only the required prompt and reconciliation hooks."
                    .to_owned()
            } else {
                "AQM hook entries are incomplete or point to a different app executable. Enable protection again to repair them."
                    .to_owned()
            }),
        }
    }
}

fn file_modified_at_millis(path: &Path) -> Option<i64> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
}

pub fn run_installed_hook(reader: impl Read) -> Result<Value, String> {
    if env::var_os("AQM_MANAGED_SESSION_ID").is_some() {
        return Ok(json!({}));
    }
    let event = CodexHookEvent::from_reader(reader)?;
    let database_path = paths::default_database_path()
        .map_err(|error| format!("cannot resolve database: {error}"))?;
    let database = Database::open(database_path).map_err(|error| error.to_string())?;
    let mut service = QuotaService::new(database);
    let observed_at = current_time_millis();
    let outcome = handle_event(&mut service, &event, observed_at, || {
        matches!(
            event.kind(),
            CodexHookEventKind::UserPromptSubmit | CodexHookEventKind::Stop
        )
        .then(codex::detect)
    })?;
    record_prompt_decision(&mut service, &event, &outcome, observed_at);
    Ok(hook_output(&outcome))
}

fn record_prompt_decision(
    service: &mut QuotaService,
    event: &CodexHookEvent,
    outcome: &CodexHookOutcome,
    occurred_at: i64,
) {
    if event.kind() != CodexHookEventKind::UserPromptSubmit {
        return;
    }
    let (blocked, scope_id, reason) = match outcome {
        CodexHookOutcome::ObservationStarted { scope_id, .. } => (
            false,
            scope_id.clone(),
            "Prompt admitted within the workspace's protected quota.".to_owned(),
        ),
        CodexHookOutcome::Blocked { reason } => (true, None, reason.clone()),
        _ => return,
    };
    let canonical_path =
        canonicalize_workspace_path(&event.cwd).unwrap_or_else(|_| event.cwd.clone());
    let _ = service.record_codex_protection_event(RecordCodexProtectionEvent {
        session_id: event.session_id.clone(),
        turn_id: event.turn_id.clone().unwrap_or_default(),
        canonical_path,
        scope_id,
        blocked,
        reason,
        occurred_at,
    });
}

fn current_time_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
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
    Ok(INSTALLED_EVENTS.iter().all(|(event, _, _)| {
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
    if scope_id.is_none() && protection_enabled(service, &window_id, observed_at)? {
        return Ok(CodexHookOutcome::Blocked {
            reason: format!(
                "AQM blocked this prompt because {canonical_path} has no Codex allocation. Add this folder in Agent Quota Manager before using Codex here."
            ),
        });
    }
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
    if scope_id.is_some() {
        let assessment = service
            .evaluate_workspace_admission(EvaluateWorkspaceAdmission {
                canonical_path: canonical_path.clone(),
                provider_id: "codex".to_owned(),
                at: observed_at,
            })
            .map_err(|error| error.to_string())?;
        if assessment.provider_remaining <= 0 {
            return Ok(CodexHookOutcome::Blocked {
                reason: "AQM blocked this prompt because the Codex provider quota is exhausted."
                    .to_owned(),
            });
        }
        if assessment.protected_now == 0 {
            if assessment.allocation_remaining <= 0 {
                return Ok(CodexHookOutcome::Blocked {
                    reason: format!(
                        "AQM blocked this prompt because the Codex allocation for {} is exhausted.",
                        assessment.scope_display_name
                    ),
                });
            }
            return Ok(CodexHookOutcome::Blocked {
                reason: format!(
                    "AQM blocked this prompt because {} has no protected quota at its current priority. Reorder workspace priorities or wait for the next reset.",
                    assessment.scope_display_name
                ),
            });
        }
        match assessment.allocation_decision {
            EnforcementDecision::Stop => {
                return Ok(CodexHookOutcome::Blocked {
                    reason: format!(
                        "AQM blocked this prompt because the Codex allocation for {} is exhausted.",
                        assessment.scope_display_name
                    ),
                });
            }
            EnforcementDecision::RequireConfirmation => {
                return Ok(CodexHookOutcome::Blocked {
                    reason: format!(
                        "AQM blocked this prompt because {} reached its confirmation boundary. Adjust its policy or use an AQM-managed launch with explicit confirmation.",
                        assessment.scope_display_name
                    ),
                });
            }
            EnforcementDecision::Allow | EnforcementDecision::Warn => {}
        }
    }

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

fn protection_enabled(
    service: &mut QuotaService,
    window_id: &str,
    observed_at: i64,
) -> Result<bool, String> {
    let state = service
        .local_state(GetLocalState {
            selected_window_id: Some(window_id.to_owned()),
            at: observed_at,
        })
        .map_err(|error| error.to_string())?;
    Ok(state
        .dashboard
        .is_some_and(|dashboard| !dashboard.allocations.is_empty()))
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

    for (event, timeout, message) in INSTALLED_EVENTS {
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

    for event in OWNED_EVENT_NAMES {
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
            ProviderQuotaSnapshotInput, RecordUsage,
        },
        domain::{Confidence, UsageSource},
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
    fn protection_status_distinguishes_disabled_current_and_stale_hooks() {
        let directory = temporary_folder("protection-status");
        let config_path = directory.join("hooks.json");
        let executable = directory.join("Agent Quota Manager");
        std::fs::write(&executable, "test executable").unwrap();

        let disabled = protection_status_for(&config_path, &executable);
        assert_eq!(disabled.state, CodexProtectionState::Disabled);
        assert!(!disabled.installed);

        install_user_hooks(&config_path, &executable).unwrap();
        let configured = protection_status_for(&config_path, &executable);
        assert_eq!(configured.state, CodexProtectionState::Configured);
        assert!(configured.installed);
        assert!(configured.requires_review);
        assert!(configured.verification_required_after.is_some());

        let mut legacy_config = read_hook_config(&config_path).unwrap();
        legacy_config["hooks"]["SessionEnd"] = json!([{
            "hooks": [{
                "type": "command",
                "command": hook_command(&executable),
                "timeout": 3,
                "statusMessage": "AQM: cleaning session state"
            }]
        }]);
        std::fs::write(
            &config_path,
            serde_json::to_string_pretty(&legacy_config).unwrap(),
        )
        .unwrap();
        let legacy = protection_status_for(&config_path, &executable);
        assert_eq!(legacy.state, CodexProtectionState::Misconfigured);
        assert!(legacy.issue.unwrap().contains("legacy SessionEnd"));

        assert!(install_user_hooks(&config_path, &executable).unwrap());
        let repaired = read_hook_config(&config_path).unwrap();
        assert!(repaired["hooks"]["SessionEnd"]
            .as_array()
            .is_none_or(|groups| !groups.iter().any(group_contains_aqm_hook)));
        assert_eq!(
            protection_status_for(&config_path, &executable).state,
            CodexProtectionState::Configured
        );

        let replacement = directory.join("replacement");
        std::fs::write(&replacement, "replacement executable").unwrap();
        let stale = protection_status_for(&config_path, &replacement);
        assert_eq!(stale.state, CodexProtectionState::Misconfigured);
        assert!(!stale.installed);
        assert!(stale.issue.is_some());

        uninstall_user_hooks(&config_path).unwrap();
        assert_eq!(
            protection_status_for(&config_path, &executable).state,
            CodexProtectionState::Disabled
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
        record_prompt_decision(&mut service, &start, &outcome, 3_000);
        assert!(matches!(
            outcome,
            CodexHookOutcome::ObservationStarted {
                scope_id: Some(ref scope_id),
                contended: false,
                ..
            } if scope_id == "workspace-a"
        ));
        let decisions = service
            .codex_protection_events(crate::application::GetCodexProtectionEvents { limit: 5 })
            .unwrap();
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].outcome, "allowed");
        assert_eq!(decisions[0].workspace_name.as_deref(), Some("Workspace A"));

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
    fn unmapped_prompt_is_blocked_when_a_codex_workspace_is_allocated() {
        let allocated_folder = temporary_folder("allocated");
        let other_folder = temporary_folder("unmapped");
        let mut service = protected_service(&allocated_folder);
        let event = CodexHookEvent {
            session_id: "session-unmapped".to_owned(),
            turn_id: Some("turn-unmapped".to_owned()),
            cwd: canonicalize_workspace_path(&other_folder).unwrap(),
            hook_event_name: "UserPromptSubmit".to_owned(),
        };

        let outcome = handle_event(&mut service, &event, 3_000, || {
            panic!("unmapped protection should block before provider detection")
        })
        .unwrap();

        assert!(matches!(
            outcome,
            CodexHookOutcome::Blocked { ref reason }
                if reason.contains("has no Codex allocation")
        ));
        assert_eq!(
            hook_output(&outcome),
            json!({
                "decision": "block",
                "reason": format!(
                    "AQM blocked this prompt because {} has no Codex allocation. Add this folder in Agent Quota Manager before using Codex here.",
                    canonicalize_workspace_path(&other_folder).unwrap()
                )
            })
        );

        std::fs::remove_dir(allocated_folder).unwrap();
        std::fs::remove_dir(other_folder).unwrap();
    }

    #[test]
    fn exhausted_workspace_prompt_is_blocked_without_starting_an_observation() {
        let folder = temporary_folder("exhausted");
        let canonical_path = canonicalize_workspace_path(&folder).unwrap();
        let mut service = protected_service(&folder);
        service
            .record_usage(RecordUsage {
                id: "workspace-exhausted".to_owned(),
                window_id: "codex-window".to_owned(),
                scope_id: Some("workspace-a".to_owned()),
                amount: 20,
                unit: "percent".to_owned(),
                observed_at: 2_500,
                source: UsageSource::LocalMeasured,
                confidence: Confidence::Observed,
                reservation_id: None,
            })
            .unwrap();
        let event = CodexHookEvent {
            session_id: "session-exhausted".to_owned(),
            turn_id: Some("turn-exhausted".to_owned()),
            cwd: canonical_path,
            hook_event_name: "UserPromptSubmit".to_owned(),
        };

        let outcome = handle_event(&mut service, &event, 3_000, || Some(detection(10))).unwrap();

        assert!(matches!(
            outcome,
            CodexHookOutcome::Blocked { ref reason } if reason.contains("is exhausted")
        ));
        assert!(service
            .provider_turn_observation(GetProviderTurnObservation {
                session_id: "session-exhausted".to_owned(),
                turn_id: "turn-exhausted".to_owned(),
            })
            .unwrap()
            .is_none());

        std::fs::remove_dir(folder).unwrap();
    }

    #[test]
    fn allocated_workspace_can_use_its_reserve_after_global_usage_crosses_confirmation() {
        let folder = temporary_folder("reserved");
        let canonical_path = canonicalize_workspace_path(&folder).unwrap();
        let mut service = protected_service(&folder);
        let event = CodexHookEvent {
            session_id: "session-reserved".to_owned(),
            turn_id: Some("turn-reserved".to_owned()),
            cwd: canonical_path,
            hook_event_name: "UserPromptSubmit".to_owned(),
        };

        let outcome = handle_event(&mut service, &event, 3_000, || Some(detection(92))).unwrap();

        assert!(matches!(
            outcome,
            CodexHookOutcome::ObservationStarted {
                scope_id: Some(ref scope_id),
                ..
            } if scope_id == "workspace-a"
        ));

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

    fn protected_service(folder: &Path) -> QuotaService {
        let canonical_path = canonicalize_workspace_path(folder).unwrap();
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
                canonical_path,
                window_id: "codex-window".to_owned(),
                amount: 20,
                unit: "percent".to_owned(),
                bound_at: 2_000,
            })
            .unwrap();
        service
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
