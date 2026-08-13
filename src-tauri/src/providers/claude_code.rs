use std::{
    env, fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    application::{
        CreateQuotaSource, GetLocalState, ProviderQuotaSnapshotInput, QuotaService,
        SyncProviderQuota,
    },
    paths,
    storage::Database,
};

const AQM_STATUS_LINE_MARKER: &str = " observe claude-statusline";
const CLAUDE_ADAPTER: &str = "claude_statusline";
const CLAUDE_PROVIDER_ID: &str = "claude-code";
const FIVE_HOURS_MILLIS: i64 = 5 * 60 * 60 * 1_000;
const SEVEN_DAYS_MILLIS: i64 = 7 * 24 * 60 * 60 * 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeStatusLineState {
    Disabled,
    Configured,
    Conflict,
    Misconfigured,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeStatusLineStatus {
    pub installed: bool,
    pub config_path: String,
    pub state: ClaudeStatusLineState,
    pub issue: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeCodeProbeStatus {
    Detected,
    NotInstalled,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeCodeProbe {
    pub status: ClaudeCodeProbeStatus,
    pub executable: Option<PathBuf>,
    pub version: Option<String>,
    pub message: Option<String>,
}

pub fn probe() -> ClaudeCodeProbe {
    let Some(executable) = resolve_executable() else {
        return ClaudeCodeProbe {
            status: ClaudeCodeProbeStatus::NotInstalled,
            executable: None,
            version: None,
            message: Some("Claude Code was not found on this device.".to_owned()),
        };
    };

    match Command::new(&executable).arg("--version").output() {
        Ok(output) if output.status.success() => match parse_version_output(&output.stdout) {
            Some(version) => ClaudeCodeProbe {
                status: ClaudeCodeProbeStatus::Detected,
                executable: Some(executable),
                version: Some(version),
                message: None,
            },
            None => unavailable_probe(executable, "Claude Code returned an empty version."),
        },
        Ok(_) | Err(_) => {
            unavailable_probe(executable, "Claude Code could not report its version.")
        }
    }
}

fn unavailable_probe(executable: PathBuf, message: &str) -> ClaudeCodeProbe {
    ClaudeCodeProbe {
        status: ClaudeCodeProbeStatus::Unavailable,
        executable: Some(executable),
        version: None,
        message: Some(message.to_owned()),
    }
}

pub fn resolve_executable() -> Option<PathBuf> {
    executable_candidates()
        .into_iter()
        .find_map(resolve_executable_candidate)
}

fn executable_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(override_path) = env::var_os("AGENT_QUOTA_CLAUDE_BIN") {
        candidates.push(PathBuf::from(override_path));
    }
    candidates.push(PathBuf::from("claude"));
    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".local/bin/claude"));
        candidates.push(home.join(".npm-global/bin/claude"));
        candidates.push(home.join(".claude/local/claude"));
    }
    candidates.push(PathBuf::from("/opt/homebrew/bin/claude"));
    candidates.push(PathBuf::from("/usr/local/bin/claude"));
    candidates
}

fn resolve_executable_candidate(candidate: PathBuf) -> Option<PathBuf> {
    if candidate.components().count() > 1 {
        return candidate.is_file().then_some(candidate);
    }
    env::var_os("PATH")
        .into_iter()
        .flat_map(|path| env::split_paths(&path).collect::<Vec<_>>())
        .map(|directory| directory.join(&candidate))
        .find(|path| path.is_file())
}

fn parse_version_output(output: &[u8]) -> Option<String> {
    let version = std::str::from_utf8(output).ok()?.trim();
    (!version.is_empty() && version.len() <= 200).then(|| version.to_owned())
}

pub fn default_user_settings_path() -> Result<PathBuf, String> {
    dirs::home_dir()
        .map(|home| home.join(".claude").join("settings.json"))
        .ok_or_else(|| "cannot resolve the current user's home directory".to_owned())
}

pub fn integration_status() -> Result<ClaudeStatusLineStatus, String> {
    let config_path = default_user_settings_path()?;
    let executable = env::current_exe()
        .map_err(|error| format!("cannot resolve the Agent Quota Manager executable: {error}"))?;
    Ok(status_line_status(&config_path, &executable))
}

pub fn install_integration(executable: &Path) -> Result<ClaudeStatusLineStatus, String> {
    let config_path = default_user_settings_path()?;
    install_status_line(&config_path, executable)?;
    Ok(status_line_status(&config_path, executable))
}

pub fn uninstall_integration() -> Result<ClaudeStatusLineStatus, String> {
    let config_path = default_user_settings_path()?;
    uninstall_status_line(&config_path)?;
    let executable = env::current_exe()
        .map_err(|error| format!("cannot resolve the Agent Quota Manager executable: {error}"))?;
    Ok(status_line_status(&config_path, &executable))
}

pub fn status_line_status(config_path: &Path, executable: &Path) -> ClaudeStatusLineStatus {
    let config_path_text = config_path.display().to_string();
    let config = match read_settings(config_path) {
        Ok(config) => config,
        Err(issue) => {
            return ClaudeStatusLineStatus {
                installed: false,
                config_path: config_path_text,
                state: ClaudeStatusLineState::Misconfigured,
                issue: Some(issue),
            };
        }
    };
    let Some(status_line) = config.get("statusLine") else {
        return ClaudeStatusLineStatus {
            installed: false,
            config_path: config_path_text,
            state: ClaudeStatusLineState::Disabled,
            issue: None,
        };
    };
    if !is_aqm_status_line(status_line) {
        return ClaudeStatusLineStatus {
            installed: false,
            config_path: config_path_text,
            state: ClaudeStatusLineState::Conflict,
            issue: Some(
                "Claude Code already has a custom status line. AQM left it unchanged because Claude supports only one statusLine command."
                    .to_owned(),
            ),
        };
    }
    let expected = status_line_command(executable);
    let current = status_line.get("command").and_then(Value::as_str);
    let configured = current == Some(expected.as_str()) && executable.is_file();
    ClaudeStatusLineStatus {
        installed: configured,
        config_path: config_path_text,
        state: if configured {
            ClaudeStatusLineState::Configured
        } else {
            ClaudeStatusLineState::Misconfigured
        },
        issue: (!configured).then(|| {
            "AQM found an outdated Claude status-line entry. Repair the integration to point it to the current application executable."
                .to_owned()
        }),
    }
}

pub fn install_status_line(config_path: &Path, executable: &Path) -> Result<bool, String> {
    let original = read_settings(config_path)?;
    if original
        .get("statusLine")
        .is_some_and(|entry| !is_aqm_status_line(entry))
    {
        return Err(
            "Claude Code already has a custom status line. Remove it explicitly before installing AQM; the existing command was not changed."
                .to_owned(),
        );
    }
    let mut updated = original.clone();
    updated
        .as_object_mut()
        .ok_or_else(|| "Claude settings root must be a JSON object".to_owned())?
        .insert(
            "statusLine".to_owned(),
            json!({
                "type": "command",
                "command": status_line_command(executable),
            }),
        );
    if updated == original {
        return Ok(false);
    }
    write_settings(config_path, &updated)?;
    Ok(true)
}

pub fn uninstall_status_line(config_path: &Path) -> Result<bool, String> {
    let original = read_settings(config_path)?;
    if !original.get("statusLine").is_some_and(is_aqm_status_line) {
        return Ok(false);
    }
    let mut updated = original.clone();
    updated
        .as_object_mut()
        .ok_or_else(|| "Claude settings root must be a JSON object".to_owned())?
        .remove("statusLine");
    write_settings(config_path, &updated)?;
    Ok(true)
}

fn read_settings(config_path: &Path) -> Result<Value, String> {
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

fn write_settings(config_path: &Path, config: &Value) -> Result<(), String> {
    let parent = config_path.parent().ok_or_else(|| {
        format!(
            "Claude settings path {} has no parent",
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
        .map_err(|error| format!("cannot serialize Claude settings: {error}"))?;
    let temporary = config_path.with_extension("json.aqm.tmp");
    fs::write(&temporary, format!("{contents}\n"))
        .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, config_path).map_err(|error| {
        format!(
            "cannot replace {} with {}: {error}",
            config_path.display(),
            temporary.display()
        )
    })
}

fn is_aqm_status_line(value: &Value) -> bool {
    value.get("type").and_then(Value::as_str) == Some("command")
        && value
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|command| command.ends_with(AQM_STATUS_LINE_MARKER))
}

fn status_line_command(executable: &Path) -> String {
    #[cfg(windows)]
    {
        format!("\"{}\"{AQM_STATUS_LINE_MARKER}", executable.display())
    }
    #[cfg(not(windows))]
    {
        let path = executable.to_string_lossy().replace('\'', "'\\''");
        format!("'{path}'{AQM_STATUS_LINE_MARKER}")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeStatusLineObservation {
    pub session_id: String,
    pub current_directory: String,
    pub five_hour: Option<ClaudeRateLimitWindow>,
    pub seven_day: Option<ClaudeRateLimitWindow>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeRateLimitWindow {
    pub used_percentage: f64,
    pub resets_at_seconds: i64,
}

#[derive(Deserialize)]
struct StatusLineInput {
    session_id: String,
    workspace: StatusLineWorkspace,
    #[serde(default)]
    rate_limits: Option<StatusLineRateLimits>,
}

#[derive(Deserialize)]
struct StatusLineWorkspace {
    current_dir: String,
}

#[derive(Default, Deserialize)]
struct StatusLineRateLimits {
    five_hour: Option<StatusLineRateLimitWindow>,
    seven_day: Option<StatusLineRateLimitWindow>,
}

#[derive(Deserialize)]
struct StatusLineRateLimitWindow {
    used_percentage: f64,
    resets_at: i64,
}

impl ClaudeStatusLineObservation {
    pub fn parse(input: &str) -> Result<Self, String> {
        let input: StatusLineInput = serde_json::from_str(input)
            .map_err(|_| "invalid Claude status-line input".to_owned())?;
        Self::from_input(input)
    }

    pub fn from_reader(reader: impl Read) -> Result<Self, String> {
        let input: StatusLineInput = serde_json::from_reader(reader)
            .map_err(|_| "invalid Claude status-line input".to_owned())?;
        Self::from_input(input)
    }

    fn from_input(input: StatusLineInput) -> Result<Self, String> {
        let rate_limits = input.rate_limits.unwrap_or_default();
        Ok(Self {
            session_id: non_empty(input.session_id, "session_id")?,
            current_directory: non_empty(input.workspace.current_dir, "workspace.current_dir")?,
            five_hour: rate_limits
                .five_hour
                .map(ClaudeRateLimitWindow::try_from)
                .transpose()?,
            seven_day: rate_limits
                .seven_day
                .map(ClaudeRateLimitWindow::try_from)
                .transpose()?,
        })
    }
}

impl TryFrom<StatusLineRateLimitWindow> for ClaudeRateLimitWindow {
    type Error = String;

    fn try_from(value: StatusLineRateLimitWindow) -> Result<Self, Self::Error> {
        if !value.used_percentage.is_finite()
            || !(0.0..=100.0).contains(&value.used_percentage)
            || value.resets_at <= 0
        {
            return Err("invalid Claude subscription rate-limit window".to_owned());
        }
        Ok(Self {
            used_percentage: value.used_percentage,
            resets_at_seconds: value.resets_at,
        })
    }
}

pub fn run_status_line(reader: impl Read) -> Result<String, String> {
    let observation = ClaudeStatusLineObservation::from_reader(reader)?;
    let observed_at = current_time_millis();
    let database_path = paths::default_database_path()
        .map_err(|error| format!("cannot resolve the AQM database: {error}"))?;
    if let Some(parent) = database_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    let database = Database::open(database_path).map_err(|error| error.to_string())?;
    let mut service = QuotaService::new(database);
    ingest_observation(&mut service, &observation, observed_at)?;
    let summary = observation
        .seven_day
        .as_ref()
        .or(observation.five_hour.as_ref())
        .map(|window| 100.0 - window.used_percentage);
    Ok(summary.map_or_else(
        || "AQM · waiting for Claude subscription quota".to_owned(),
        |remaining| format!("AQM · Claude {remaining:.0}% left"),
    ))
}

pub fn ingest_observation(
    service: &mut QuotaService,
    observation: &ClaudeStatusLineObservation,
    observed_at: i64,
) -> Result<(), String> {
    if let Some(window) = observation.five_hour.as_ref() {
        ingest_window(
            service,
            "five_hour",
            "5-hour allowance",
            FIVE_HOURS_MILLIS,
            window,
            observed_at,
        )?;
    }
    if let Some(window) = observation.seven_day.as_ref() {
        ingest_window(
            service,
            "seven_day",
            "Weekly allowance",
            SEVEN_DAYS_MILLIS,
            window,
            observed_at,
        )?;
    }
    Ok(())
}

fn ingest_window(
    service: &mut QuotaService,
    kind: &str,
    display_name: &str,
    duration_millis: i64,
    window: &ClaudeRateLimitWindow,
    observed_at: i64,
) -> Result<(), String> {
    let ends_at = window
        .resets_at_seconds
        .checked_mul(1_000)
        .ok_or_else(|| "Claude reset timestamp is out of range".to_owned())?;
    if ends_at <= observed_at {
        return Err("Claude returned an expired subscription window".to_owned());
    }
    let starts_at = ends_at - duration_millis;
    let used = window.used_percentage.round() as u64;
    let state = service
        .local_state(GetLocalState {
            selected_window_id: None,
            at: observed_at,
        })
        .map_err(|error| error.to_string())?;
    let existing = state.sources.into_iter().find(|source| {
        source
            .provider_display_name
            .eq_ignore_ascii_case("Claude Code")
            && source.pool_display_name == display_name
            && source.unit == "percent"
    });
    if let Some(source) = existing {
        service
            .sync_provider_quota(SyncProviderQuota {
                current_window_id: source.window_id,
                adapter: CLAUDE_ADAPTER.to_owned(),
                remote_limit_id: CLAUDE_PROVIDER_ID.to_owned(),
                remote_window_kind: kind.to_owned(),
                starts_at,
                ends_at,
                capacity: 100,
                used,
                unit: "percent".to_owned(),
                observed_at,
                desktop_observations: None,
            })
            .map_err(|error| error.to_string())?;
    } else {
        let suffix = kind.replace('_', "-");
        service
            .create_quota_source(CreateQuotaSource {
                provider_id: format!("claude-code-{suffix}-provider"),
                provider_display_name: "Claude Code".to_owned(),
                account_id: format!("claude-code-{suffix}-account"),
                account_display_name: "Claude subscription".to_owned(),
                pool_id: format!("claude-code-{suffix}"),
                pool_display_name: display_name.to_owned(),
                window_id: format!("claude-code-{suffix}-window-{ends_at}"),
                starts_at,
                ends_at,
                capacity: 100,
                unit: "percent".to_owned(),
                provider_snapshot: Some(ProviderQuotaSnapshotInput {
                    adapter: CLAUDE_ADAPTER.to_owned(),
                    remote_limit_id: CLAUDE_PROVIDER_ID.to_owned(),
                    remote_window_kind: kind.to_owned(),
                    used,
                    observed_at,
                    resets_at: ends_at,
                }),
            })
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn current_time_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeHookEventKind {
    UserPromptSubmit,
    Stop,
    StopFailure,
    SessionEnd,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeHookObservation {
    pub event: ClaudeHookEventKind,
    pub session_id: String,
    pub current_directory: String,
}

#[derive(Deserialize)]
struct HookInput {
    session_id: String,
    cwd: String,
    hook_event_name: String,
}

impl ClaudeHookObservation {
    pub fn parse(input: &str) -> Result<Self, String> {
        let input: HookInput =
            serde_json::from_str(input).map_err(|_| "invalid Claude hook input".to_owned())?;
        Ok(Self {
            event: match input.hook_event_name.as_str() {
                "UserPromptSubmit" => ClaudeHookEventKind::UserPromptSubmit,
                "Stop" => ClaudeHookEventKind::Stop,
                "StopFailure" => ClaudeHookEventKind::StopFailure,
                "SessionEnd" => ClaudeHookEventKind::SessionEnd,
                _ => ClaudeHookEventKind::Other,
            },
            session_id: non_empty(input.session_id, "session_id")?,
            current_directory: non_empty(input.cwd, "cwd")?,
        })
    }
}

fn non_empty(value: String, field: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("Claude input has an empty {field}"));
    }
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{application::GetQuotaDashboard, storage::Database};

    fn temp_settings(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "aqm-claude-statusline-{name}-{}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).unwrap();
        directory.join("settings.json")
    }

    #[test]
    fn status_line_install_preserves_unrelated_claude_settings() {
        let path = temp_settings("preserve");
        fs::write(
            &path,
            r#"{"theme":"dark","permissions":{"allow":["Bash"]}}"#,
        )
        .unwrap();
        let executable = path.parent().unwrap().join("Agent Quota Manager");
        fs::write(&executable, "binary").unwrap();

        assert!(install_status_line(&path, &executable).unwrap());

        let settings = read_settings(&path).unwrap();
        assert_eq!(settings["theme"], "dark");
        assert_eq!(settings["permissions"]["allow"][0], "Bash");
        assert!(is_aqm_status_line(&settings["statusLine"]));
        assert_eq!(
            status_line_status(&path, &executable).state,
            ClaudeStatusLineState::Configured
        );
        assert!(path.with_extension("json.aqm.bak").exists());
    }

    #[test]
    fn install_refuses_to_replace_an_existing_custom_status_line() {
        let path = temp_settings("conflict");
        let original = r#"{"statusLine":{"type":"command","command":"my-status.sh"}}"#;
        fs::write(&path, original).unwrap();
        let executable = path.parent().unwrap().join("aqm");

        let error = install_status_line(&path, &executable).unwrap_err();

        assert!(error.contains("already has a custom status line"));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        assert_eq!(
            status_line_status(&path, &executable).state,
            ClaudeStatusLineState::Conflict
        );
    }

    #[test]
    fn uninstall_removes_only_an_aqm_owned_status_line() {
        let path = temp_settings("uninstall");
        fs::write(&path, r#"{"theme":"light"}"#).unwrap();
        let executable = path.parent().unwrap().join("aqm");
        install_status_line(&path, &executable).unwrap();

        assert!(uninstall_status_line(&path).unwrap());
        assert_eq!(read_settings(&path).unwrap(), json!({"theme": "light"}));
        assert!(!uninstall_status_line(&path).unwrap());
    }

    #[test]
    fn status_line_observation_creates_both_subscription_sources() {
        let mut service = QuotaService::new(Database::open_in_memory().unwrap());
        let observation = ClaudeStatusLineObservation {
            session_id: "session-1".to_owned(),
            current_directory: "/workspace".to_owned(),
            five_hour: Some(ClaudeRateLimitWindow {
                used_percentage: 12.4,
                resets_at_seconds: 120_000,
            }),
            seven_day: Some(ClaudeRateLimitWindow {
                used_percentage: 31.6,
                resets_at_seconds: 700_000,
            }),
        };

        ingest_observation(&mut service, &observation, 100_000_000).unwrap();

        let state = service
            .local_state(GetLocalState {
                selected_window_id: None,
                at: 100_000_000,
            })
            .unwrap();
        assert_eq!(state.sources.len(), 2);
        assert!(state
            .sources
            .iter()
            .all(|source| source.provider_display_name == "Claude Code"));
        let weekly = state
            .sources
            .iter()
            .find(|source| source.pool_display_name == "Weekly allowance")
            .unwrap();
        let dashboard = service
            .dashboard(GetQuotaDashboard {
                window_id: weekly.window_id.clone(),
                at: 100_000_000,
            })
            .unwrap();
        assert_eq!(dashboard.window.provider_remaining, 68);
    }

    #[test]
    fn status_line_observation_updates_and_rolls_over_existing_source() {
        let mut service = QuotaService::new(Database::open_in_memory().unwrap());
        let mut observation = ClaudeStatusLineObservation {
            session_id: "session-1".to_owned(),
            current_directory: "/workspace".to_owned(),
            five_hour: None,
            seven_day: Some(ClaudeRateLimitWindow {
                used_percentage: 20.0,
                resets_at_seconds: 700_000,
            }),
        };
        ingest_observation(&mut service, &observation, 100_000_000).unwrap();
        let first_window = service
            .local_state(GetLocalState {
                selected_window_id: None,
                at: 100_000_000,
            })
            .unwrap()
            .sources[0]
            .window_id
            .clone();

        observation.seven_day = Some(ClaudeRateLimitWindow {
            used_percentage: 7.0,
            resets_at_seconds: 1_304_800,
        });
        ingest_observation(&mut service, &observation, 700_001_000).unwrap();

        let state = service
            .local_state(GetLocalState {
                selected_window_id: None,
                at: 700_001_000,
            })
            .unwrap();
        assert_eq!(state.sources.len(), 1);
        assert_ne!(state.sources[0].window_id, first_window);
        assert_eq!(state.dashboard.unwrap().window.provider_remaining, 93);
    }

    #[test]
    fn version_output_must_be_short_non_empty_utf8() {
        assert_eq!(
            parse_version_output(b"2.1.0 (Claude Code)\n"),
            Some("2.1.0 (Claude Code)".to_owned())
        );
        assert_eq!(parse_version_output(b" \n"), None);
        assert_eq!(parse_version_output(&[0xff]), None);
        assert_eq!(parse_version_output(&vec![b'x'; 201]), None);
    }

    #[test]
    fn status_line_parser_keeps_only_workspace_and_subscription_windows() {
        let observation = ClaudeStatusLineObservation::parse(
            r#"{
              "session_id": "session-1",
              "workspace": {"current_dir": "/code/product"},
              "rate_limits": {
                "five_hour": {"used_percentage": 12.5, "resets_at": 2000},
                "seven_day": {"used_percentage": 31, "resets_at": 9000}
              },
              "context_window": {"used_percentage": 88},
              "cost": {"total_cost_usd": 42},
              "transcript": "private transcript"
            }"#,
        )
        .unwrap();

        assert_eq!(observation.session_id, "session-1");
        assert_eq!(observation.current_directory, "/code/product");
        assert_eq!(observation.five_hour.unwrap().used_percentage, 12.5);
        assert_eq!(observation.seven_day.unwrap().resets_at_seconds, 9000);
    }

    #[test]
    fn status_line_parser_allows_absent_subscription_limits() {
        let observation = ClaudeStatusLineObservation::parse(
            r#"{"session_id":"s","workspace":{"current_dir":"/code/product"}}"#,
        )
        .unwrap();
        assert_eq!(observation.five_hour, None);
        assert_eq!(observation.seven_day, None);
    }

    #[test]
    fn status_line_parser_rejects_invalid_provider_values() {
        let invalid = r#"{
          "session_id":"s",
          "workspace":{"current_dir":"/code/product"},
          "rate_limits":{"seven_day":{"used_percentage":101,"resets_at":9000}}
        }"#;
        assert!(ClaudeStatusLineObservation::parse(invalid).is_err());
    }

    #[test]
    fn hook_parser_ignores_content_fields() {
        let observation = ClaudeHookObservation::parse(
            r#"{
              "session_id":"session-1",
              "cwd":"/code/product",
              "hook_event_name":"UserPromptSubmit",
              "prompt":"private prompt",
              "transcript_path":"/private/transcript.jsonl",
              "tool_input":{"secret":"private"}
            }"#,
        )
        .unwrap();
        assert_eq!(observation.event, ClaudeHookEventKind::UserPromptSubmit);
        assert_eq!(observation.session_id, "session-1");
        assert_eq!(observation.current_directory, "/code/product");
    }
}
