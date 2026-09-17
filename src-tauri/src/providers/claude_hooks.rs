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
        AbandonProviderSessionObservations, BeginProviderTurnObservation,
        EvaluateWorkspaceAdmission, GetLocalState, GetWorkspaceContext, QuotaService,
        QuotaSourceSummary, ReconcileProviderTurnObservation,
    },
    domain::EnforcementDecision,
    paths,
    storage::Database,
    workspace::canonicalize_workspace_path,
};

use super::claude_code::{self, ClaudeHookEventKind, ClaudeHookObservation, CLAUDE_ADAPTER};

const OWNED_MARKER: &str = " hook claude";
const HEARTBEAT_FILE: &str = "claude-hook-heartbeat.json";
const EVENTS: [&str; 4] = ["UserPromptSubmit", "Stop", "StopFailure", "SessionEnd"];
const CLAUDE_HOOK_CACHE_TTL_MILLIS: i64 = 60_000;
const CLAUDE_HOOK_FALLBACK_TTL_MILLIS: i64 = 5 * 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeProtectionState {
    Disabled,
    Configured,
    Misconfigured,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeProtectionStatus {
    pub installed: bool,
    pub config_path: String,
    pub state: ClaudeProtectionState,
    pub issue: Option<String>,
    pub last_hook_observed_at: Option<i64>,
    pub last_decision: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Heartbeat {
    last_hook_observed_at: Option<i64>,
    last_decision: Option<String>,
}

#[derive(Debug)]
enum HookOutcome {
    Allowed { warning: Option<String> },
    Blocked { reason: String },
    Reconciled,
    Skipped,
}

pub fn protection_status() -> Result<ClaudeProtectionStatus, String> {
    let config_path = claude_code::default_user_settings_path()?;
    let executable =
        env::current_exe().map_err(|error| format!("cannot resolve QuotaFence: {error}"))?;
    Ok(status_for(&config_path, &executable))
}

pub fn install_protection(executable: &Path) -> Result<ClaudeProtectionStatus, String> {
    let config_path = claude_code::default_user_settings_path()?;
    let original = read_settings(&config_path)?;
    let mut updated = original.clone();
    remove_owned_hooks(&mut updated)?;
    add_owned_hooks(&mut updated, executable)?;
    if updated != original {
        write_settings(&config_path, &updated)?;
    }
    Ok(status_for(&config_path, executable))
}

pub fn uninstall_protection() -> Result<ClaudeProtectionStatus, String> {
    let config_path = claude_code::default_user_settings_path()?;
    let original = read_settings(&config_path)?;
    let mut updated = original.clone();
    remove_owned_hooks(&mut updated)?;
    if updated != original {
        write_settings(&config_path, &updated)?;
    }
    let executable =
        env::current_exe().map_err(|error| format!("cannot resolve QuotaFence: {error}"))?;
    Ok(status_for(&config_path, &executable))
}

pub fn run_installed_hook(reader: impl Read) -> Result<Value, String> {
    if env::var_os("QUOTAFENCE_MANAGED_SESSION_ID").is_some() {
        return Ok(json!({}));
    }
    let mut input = String::new();
    let mut reader = reader;
    reader
        .read_to_string(&mut input)
        .map_err(|error| format!("cannot read Claude hook input: {error}"))?;
    let event = ClaudeHookObservation::parse(&input)?;
    let observed_at = now_millis();
    let outcome = run_event(&event, observed_at).unwrap_or_else(|error| {
        if event.event == ClaudeHookEventKind::UserPromptSubmit {
            HookOutcome::Blocked {
                reason: format!(
                    "QuotaFence could not verify this Claude workspace safely: {error}. Open QuotaFence, refresh Claude, and retry."
                ),
            }
        } else {
            HookOutcome::Skipped
        }
    });
    let decision = match &outcome {
        HookOutcome::Blocked { .. } => "blocked",
        HookOutcome::Allowed { warning: Some(_) } => "warned",
        HookOutcome::Allowed { warning: None } | HookOutcome::Reconciled => "allowed",
        HookOutcome::Skipped => "skipped",
    };
    let _ = write_heartbeat(observed_at, decision);
    Ok(match outcome {
        HookOutcome::Blocked { reason } => json!({"decision": "block", "reason": reason}),
        HookOutcome::Allowed {
            warning: Some(warning),
        } => json!({"systemMessage": warning}),
        _ => json!({}),
    })
}

fn run_event(event: &ClaudeHookObservation, observed_at: i64) -> Result<HookOutcome, String> {
    let database_path = paths::default_database_path()
        .map_err(|error| format!("cannot resolve QuotaFence database: {error}"))?;
    if let Some(parent) = database_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    let database = Database::open(database_path).map_err(|error| error.to_string())?;
    let mut service = QuotaService::new(database);
    match event.event {
        ClaudeHookEventKind::UserPromptSubmit => begin_prompt(&mut service, event, observed_at),
        ClaudeHookEventKind::Stop | ClaudeHookEventKind::StopFailure => {
            finish_prompt(&mut service, event, observed_at)
        }
        ClaudeHookEventKind::SessionEnd => {
            service
                .abandon_provider_session_observations(AbandonProviderSessionObservations {
                    session_id: event.session_id.clone(),
                })
                .map_err(|error| error.to_string())?;
            Ok(HookOutcome::Reconciled)
        }
        ClaudeHookEventKind::Other => Ok(HookOutcome::Skipped),
    }
}

fn begin_prompt(
    service: &mut QuotaService,
    event: &ClaudeHookObservation,
    observed_at: i64,
) -> Result<HookOutcome, String> {
    // Reconcile anything left by a missing Stop before taking the next baseline.
    if !service
        .provider_turn_observations_for_session(&event.session_id)
        .map_err(|error| error.to_string())?
        .is_empty()
        && finish_prompt(service, event, observed_at).is_err()
    {
        // A provider refresh can be rate limited or temporarily unavailable. Do not let an
        // unreconciled turn permanently prevent future prompts; provider quota protection is
        // still evaluated below from the most recent safe checkpoint.
        service
            .abandon_provider_session_observations(AbandonProviderSessionObservations {
                session_id: event.session_id.clone(),
            })
            .map_err(|error| error.to_string())?;
    }
    let synced = sync_claude_for_hook(service, observed_at)?;
    let canonical_path =
        canonicalize_workspace_path(&event.current_directory).map_err(|error| error.to_string())?;
    let context = service
        .workspace_context(GetWorkspaceContext {
            canonical_path: canonical_path.clone(),
            at: observed_at,
        })
        .map_err(|error| error.to_string())?;
    let allocations = context
        .allocations
        .iter()
        .filter(|allocation| {
            allocation
                .provider_display_name
                .eq_ignore_ascii_case("Claude Code")
                && allocation.unit == "percent"
        })
        .collect::<Vec<_>>();
    if allocations.is_empty() {
        let state = service
            .local_state(GetLocalState {
                selected_window_id: None,
                at: observed_at,
            })
            .map_err(|error| error.to_string())?;
        let claude_window_ids = state
            .sources
            .iter()
            .filter(|source| {
                source
                    .provider_display_name
                    .eq_ignore_ascii_case("Claude Code")
            })
            .map(|source| source.window_id.clone())
            .collect::<Vec<_>>();
        let mut protection_planned = false;
        for window_id in claude_window_ids {
            let window_state = service
                .local_state(GetLocalState {
                    selected_window_id: Some(window_id),
                    at: observed_at,
                })
                .map_err(|error| error.to_string())?;
            if window_state
                .dashboard
                .is_some_and(|dashboard| !dashboard.allocations.is_empty())
            {
                protection_planned = true;
                break;
            }
        }
        if protection_planned {
            return Ok(HookOutcome::Blocked {
                reason: format!(
                    "QuotaFence blocked this Claude prompt because {canonical_path} has no Claude allocation. Add the folder in QuotaFence or open an allocated workspace."
                ),
            });
        }
        return Ok(HookOutcome::Skipped);
    }
    if let Some(window) = exhausted_claude_window(service, observed_at)? {
        return Ok(HookOutcome::Blocked {
            reason: format!(
                "QuotaFence blocked this Claude prompt because the provider's {window} is exhausted."
            ),
        });
    }
    let scope_id = context
        .binding
        .as_ref()
        .map(|binding| binding.scope_id.clone())
        .ok_or_else(|| "allocated Claude workspace has no local binding".to_owned())?;
    let mut warning = None;
    for allocation in &allocations {
        let assessment = service
            .evaluate_workspace_admission(EvaluateWorkspaceAdmission {
                canonical_path: canonical_path.clone(),
                provider_id: allocation.provider_id.clone(),
                at: observed_at,
            })
            .map_err(|error| error.to_string())?;
        if assessment.provider_remaining <= 0
            || assessment.protected_now == 0
            || matches!(assessment.decision, EnforcementDecision::Stop)
        {
            return Ok(HookOutcome::Blocked {
                reason: format!(
                    "QuotaFence stopped this Claude prompt because {} has no protected quota left in {}.",
                    context
                        .binding
                        .as_ref()
                        .map(|binding| binding.scope_display_name.as_str())
                        .unwrap_or(canonical_path.as_str()),
                    allocation.pool_display_name
                ),
            });
        }
        if matches!(assessment.decision, EnforcementDecision::Warn) {
            warning = Some(format!(
                "QuotaFence warning: {} is approaching its {} allocation limit.",
                context
                    .binding
                    .as_ref()
                    .map(|binding| binding.scope_display_name.as_str())
                    .unwrap_or(canonical_path.as_str()),
                allocation.pool_display_name
            ));
        }
    }
    for allocation in allocations {
        let kind = synced
            .windows
            .iter()
            .find(|window| window.window_id == allocation.window_id)
            .map(|window| window.kind.as_str())
            .or_else(|| {
                synced
                    .windows
                    .iter()
                    .find(|window| window.display_name == allocation.pool_display_name)
                    .map(|window| window.kind.as_str())
            })
            .unwrap_or("window");
        let current_window_id = synced
            .windows
            .iter()
            .find(|window| window.kind == kind)
            .map(|window| window.window_id.clone())
            .unwrap_or_else(|| allocation.window_id.clone());
        service
            .begin_provider_turn_observation(BeginProviderTurnObservation {
                session_id: event.session_id.clone(),
                turn_id: format!("claude:{observed_at}:{kind}"),
                adapter: CLAUDE_ADAPTER.to_owned(),
                canonical_path: canonical_path.clone(),
                scope_id: Some(scope_id.clone()),
                window_id: current_window_id,
                started_at: observed_at,
            })
            .map_err(|error| error.to_string())?;
    }
    Ok(HookOutcome::Allowed { warning })
}

fn sync_claude_for_hook(
    service: &mut QuotaService,
    observed_at: i64,
) -> Result<claude_code::ClaudeSyncResult, String> {
    if let Some(cached) = cached_claude_sync(service, observed_at, CLAUDE_HOOK_CACHE_TTL_MILLIS)? {
        return Ok(cached);
    }

    match claude_code::fetch_subscription_usage()
        .and_then(|observation| claude_code::ingest_observation(service, &observation, observed_at))
    {
        Ok(synced) => Ok(synced),
        Err(refresh_error) => {
            cached_claude_sync(service, observed_at, CLAUDE_HOOK_FALLBACK_TTL_MILLIS)?
                .ok_or(refresh_error)
        }
    }
}

fn cached_claude_sync(
    service: &mut QuotaService,
    observed_at: i64,
    max_age_millis: i64,
) -> Result<Option<claude_code::ClaudeSyncResult>, String> {
    let state = service
        .local_state(GetLocalState {
            selected_window_id: None,
            at: observed_at,
        })
        .map_err(|error| error.to_string())?;
    let windows = state
        .sources
        .iter()
        .filter_map(|source| cached_claude_window(source, observed_at, max_age_millis))
        .collect::<Vec<_>>();
    Ok((!windows.is_empty()).then_some(claude_code::ClaudeSyncResult { windows }))
}

fn cached_claude_window(
    source: &QuotaSourceSummary,
    observed_at: i64,
    max_age_millis: i64,
) -> Option<claude_code::ClaudeSyncedWindow> {
    if !source.is_active
        || source.unit != "percent"
        || !source
            .provider_display_name
            .eq_ignore_ascii_case("Claude Code")
    {
        return None;
    }
    let synced_at = source.last_synced_at?;
    if observed_at.saturating_sub(synced_at) > max_age_millis {
        return None;
    }
    let normalized = source.pool_display_name.to_ascii_lowercase();
    let kind = if normalized.contains("5-hour") || normalized.contains("5 hour") {
        "five_hour"
    } else if normalized.contains("weekly") || normalized.contains("7-day") {
        "seven_day"
    } else {
        return None;
    };
    Some(claude_code::ClaudeSyncedWindow {
        kind: kind.to_owned(),
        display_name: source.pool_display_name.clone(),
        window_id: source.window_id.clone(),
        rolled_over: false,
    })
}

fn exhausted_claude_window(
    service: &mut QuotaService,
    observed_at: i64,
) -> Result<Option<String>, String> {
    let state = service
        .local_state(GetLocalState {
            selected_window_id: None,
            at: observed_at,
        })
        .map_err(|error| error.to_string())?;
    Ok(state
        .sources
        .into_iter()
        .find(is_exhausted_claude_window)
        .map(|source| source.pool_display_name))
}

fn is_exhausted_claude_window(source: &QuotaSourceSummary) -> bool {
    source.is_active
        && source.provider_managed
        && source
            .provider_display_name
            .eq_ignore_ascii_case("Claude Code")
        && source.unit == "percent"
        && source
            .provider_used
            .is_some_and(|used| used >= source.capacity)
}

fn finish_prompt(
    service: &mut QuotaService,
    event: &ClaudeHookObservation,
    observed_at: i64,
) -> Result<HookOutcome, String> {
    let pending = service
        .provider_turn_observations_for_session(&event.session_id)
        .map_err(|error| error.to_string())?;
    if pending.is_empty() {
        return Ok(HookOutcome::Skipped);
    }
    let observation = claude_code::fetch_subscription_usage()?;
    let synced = claude_code::ingest_observation(service, &observation, observed_at)?;
    for turn in pending {
        let kind = if turn.turn_id.ends_with(":five_hour") {
            "five_hour"
        } else if turn.turn_id.ends_with(":seven_day") {
            "seven_day"
        } else {
            continue;
        };
        let Some(current) = synced.windows.iter().find(|window| window.kind == kind) else {
            continue;
        };
        service
            .reconcile_provider_turn_observation(ReconcileProviderTurnObservation {
                session_id: event.session_id.clone(),
                turn_id: turn.turn_id,
                current_window_id: current.window_id.clone(),
                observed_at,
            })
            .map_err(|error| error.to_string())?;
    }
    Ok(HookOutcome::Reconciled)
}

fn status_for(config_path: &Path, executable: &Path) -> ClaudeProtectionStatus {
    let heartbeat = read_heartbeat();
    let result = read_settings(config_path).and_then(|settings| {
        let hooks = settings
            .get("hooks")
            .and_then(Value::as_object)
            .ok_or_else(|| "disabled".to_owned())?;
        let command = hook_command(executable);
        if EVENTS.iter().all(|event| {
            hooks
                .get(*event)
                .and_then(Value::as_array)
                .is_some_and(|groups| {
                    groups
                        .iter()
                        .any(|group| group_has_command(group, &command))
                })
        }) {
            Ok(())
        } else if hooks.values().any(contains_owned) {
            Err(
                "QuotaFence Claude hook entries are incomplete or point to an older app build."
                    .to_owned(),
            )
        } else {
            Err("disabled".to_owned())
        }
    });
    match result {
        Ok(()) => ClaudeProtectionStatus {
            installed: true,
            config_path: config_path.display().to_string(),
            state: ClaudeProtectionState::Configured,
            issue: None,
            last_hook_observed_at: heartbeat.last_hook_observed_at,
            last_decision: heartbeat.last_decision,
        },
        Err(issue) if issue == "disabled" => ClaudeProtectionStatus {
            installed: false,
            config_path: config_path.display().to_string(),
            state: ClaudeProtectionState::Disabled,
            issue: None,
            last_hook_observed_at: heartbeat.last_hook_observed_at,
            last_decision: heartbeat.last_decision,
        },
        Err(issue) => ClaudeProtectionStatus {
            installed: false,
            config_path: config_path.display().to_string(),
            state: ClaudeProtectionState::Misconfigured,
            issue: Some(issue),
            last_hook_observed_at: heartbeat.last_hook_observed_at,
            last_decision: heartbeat.last_decision,
        },
    }
}

fn hook_command(executable: &Path) -> String {
    format!("{} hook claude", shell_quote(executable))
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

fn add_owned_hooks(settings: &mut Value, executable: &Path) -> Result<(), String> {
    let root = settings
        .as_object_mut()
        .ok_or_else(|| "Claude settings must be a JSON object".to_owned())?;
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| "Claude hooks must be a JSON object".to_owned())?;
    for event in EVENTS {
        let groups = hooks
            .entry(event)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| format!("Claude {event} hooks must be an array"))?;
        groups.push(json!({
            "matcher": "",
            "hooks": [{
                "type": "command",
                "command": hook_command(executable),
                "timeout": 30
            }]
        }));
    }
    Ok(())
}

fn remove_owned_hooks(settings: &mut Value) -> Result<(), String> {
    let Some(root) = settings.as_object_mut() else {
        return Err("Claude settings must be a JSON object".to_owned());
    };
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return Ok(());
    };
    for event in EVENTS {
        if let Some(groups) = hooks.get_mut(event).and_then(Value::as_array_mut) {
            groups.retain(|group| !contains_owned(group));
        }
        if hooks
            .get(event)
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
        {
            hooks.remove(event);
        }
    }
    if hooks.is_empty() {
        root.remove("hooks");
    }
    Ok(())
}

fn group_has_command(group: &Value, expected: &str) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|handlers| {
            handlers
                .iter()
                .any(|handler| handler.get("command").and_then(Value::as_str) == Some(expected))
        })
}

fn contains_owned(value: &Value) -> bool {
    match value {
        Value::String(value) => value.contains(OWNED_MARKER),
        Value::Array(values) => values.iter().any(contains_owned),
        Value::Object(values) => values.values().any(contains_owned),
        _ => false,
    }
}

fn read_settings(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let content = fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let value: Value = serde_json::from_str(&content)
        .map_err(|error| format!("{} is invalid JSON: {error}", path.display()))?;
    value
        .is_object()
        .then_some(value)
        .ok_or_else(|| "Claude settings must be a JSON object".to_owned())
}

fn write_settings(path: &Path, settings: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    if path.exists() {
        let backup = path.with_extension("json.quotafence.bak");
        if !backup.exists() {
            fs::copy(path, &backup)
                .map_err(|error| format!("cannot back up {}: {error}", path.display()))?;
        }
    }
    let content = serde_json::to_string_pretty(settings)
        .map_err(|error| format!("cannot encode Claude settings: {error}"))?;
    fs::write(path, format!("{content}\n"))
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn heartbeat_path() -> Option<PathBuf> {
    paths::default_database_path()
        .ok()?
        .parent()
        .map(|parent| parent.join(HEARTBEAT_FILE))
}

fn read_heartbeat() -> Heartbeat {
    heartbeat_path()
        .and_then(|path| fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn write_heartbeat(observed_at: i64, decision: &str) -> Result<(), String> {
    let path = heartbeat_path().ok_or_else(|| "cannot resolve heartbeat path".to_owned())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let heartbeat = Heartbeat {
        last_hook_observed_at: Some(observed_at),
        last_decision: Some(decision.to_owned()),
    };
    fs::write(
        path,
        serde_json::to_vec(&heartbeat).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(provider: &str, pool: &str, used: u64) -> QuotaSourceSummary {
        QuotaSourceSummary {
            provider_id: provider.to_ascii_lowercase(),
            provider_display_name: provider.to_owned(),
            account_id: "account".to_owned(),
            account_display_name: "Subscription".to_owned(),
            pool_id: pool.to_ascii_lowercase(),
            pool_display_name: pool.to_owned(),
            window_id: format!("{pool}-window"),
            starts_at: 0,
            ends_at: 10_000,
            capacity: 100,
            provider_used: Some(used),
            unit: "percent".to_owned(),
            is_active: true,
            provider_managed: true,
            last_synced_at: Some(1_000),
            sync_health: None,
            turn_health: None,
        }
    }

    #[test]
    fn installing_hooks_preserves_unrelated_entries() {
        let mut settings = json!({
            "theme": "dark",
            "hooks": {"UserPromptSubmit": [{"hooks": [{"type":"command","command":"other"}]}]}
        });
        add_owned_hooks(
            &mut settings,
            Path::new("/Applications/QuotaFence.app/Contents/MacOS/quotafence"),
        )
        .unwrap();
        assert_eq!(settings["theme"], "dark");
        assert_eq!(
            settings["hooks"]["UserPromptSubmit"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        remove_owned_hooks(&mut settings).unwrap();
        assert_eq!(
            settings["hooks"]["UserPromptSubmit"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(settings["hooks"].get("Stop").is_none());
    }

    #[test]
    fn every_native_claude_window_remains_a_provider_safety_limit() {
        assert!(is_exhausted_claude_window(&source(
            "Claude Code",
            "5-hour allowance",
            100
        )));
        assert!(is_exhausted_claude_window(&source(
            "Claude Code",
            "Weekly allowance",
            100
        )));
        assert!(!is_exhausted_claude_window(&source(
            "Claude Code",
            "Weekly allowance",
            99
        )));
        assert!(!is_exhausted_claude_window(&source(
            "Codex",
            "Weekly allowance",
            100
        )));
    }

    #[test]
    fn recent_active_claude_source_can_back_a_hook_decision() {
        let mut source = source("Claude Code", "Weekly allowance", 12);
        source.last_synced_at = Some(9_000);
        let cached = cached_claude_window(&source, 10_000, 5_000).unwrap();
        assert_eq!(cached.kind, "seven_day");
        assert_eq!(cached.window_id, "Weekly allowance-window");
    }

    #[test]
    fn stale_or_inactive_sources_cannot_back_a_hook_decision() {
        let mut stale = source("Claude Code", "5-hour allowance", 12);
        stale.last_synced_at = Some(1_000);
        assert!(cached_claude_window(&stale, 10_000, 5_000).is_none());

        let mut inactive = source("Claude Code", "Weekly allowance", 12);
        inactive.last_synced_at = Some(9_000);
        inactive.is_active = false;
        assert!(cached_claude_window(&inactive, 10_000, 5_000).is_none());
    }
}
