use std::{
    env, fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};

#[cfg(target_os = "macos")]
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use chrono::DateTime;
#[cfg(target_os = "macos")]
use pbkdf2::pbkdf2_hmac;
#[cfg(target_os = "macos")]
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[cfg(target_os = "macos")]
use sha1::Sha1;
#[cfg(target_os = "macos")]
use sha2::{Digest, Sha256};

use crate::{
    application::{
        CreateQuotaSource, GetLocalState, ProviderQuotaSnapshotInput, QuotaService,
        SyncProviderQuota,
    },
    paths,
    storage::Database,
};

const QUOTAFENCE_STATUS_LINE_MARKER: &str = " observe claude-statusline";
pub const CLAUDE_ADAPTER: &str = "claude_statusline";
const CLAUDE_PROVIDER_ID: &str = "claude-code";
const CLAUDE_HEARTBEAT_FILENAME: &str = "claude-statusline-heartbeat.json";
const FIVE_HOURS_MILLIS: i64 = 5 * 60 * 60 * 1_000;
const SEVEN_DAYS_MILLIS: i64 = 7 * 24 * 60 * 60 * 1_000;
const CLAUDE_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const CLAUDE_KEYCHAIN_SERVICE: &str = "Claude Code-credentials";
#[cfg(target_os = "macos")]
const CLAUDE_SAFE_STORAGE_SERVICE: &str = "Claude Safe Storage";
#[cfg(target_os = "macos")]
const CLAUDE_SAFE_STORAGE_ACCOUNT: &str = "Claude Key";
#[cfg(target_os = "macos")]
const CLAUDE_DESKTOP_CONFIG: &str = "Library/Application Support/Claude/config.json";

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
    pub last_observed_at: Option<i64>,
    pub last_quota_observed_at: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeStatusLineHeartbeat {
    last_observed_at: Option<i64>,
    last_quota_observed_at: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeCredentialsFile {
    claude_ai_oauth: ClaudeOAuthCredential,
    #[serde(flatten)]
    extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeOAuthCredential {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<f64>,
    #[serde(flatten)]
    extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Deserialize)]
struct ClaudeTokenRefreshResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct ClaudeUsageResponse {
    five_hour: Option<ClaudeUsageWindow>,
    seven_day: Option<ClaudeUsageWindow>,
}

#[derive(Debug, Deserialize)]
struct ClaudeUsageWindow {
    utilization: f64,
    resets_at: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeSyncedWindow {
    pub kind: String,
    pub display_name: String,
    pub window_id: String,
    pub rolled_over: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeSyncResult {
    pub windows: Vec<ClaudeSyncedWindow>,
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
        .flat_map(|directory| executable_paths_in(&directory, &candidate))
        .find(|path| path.is_file())
}

fn executable_paths_in(directory: &Path, candidate: &Path) -> Vec<PathBuf> {
    let path = directory.join(candidate);
    #[cfg(windows)]
    {
        if candidate.extension().is_none() {
            return vec![path.with_extension("exe"), path];
        }
    }
    vec![path]
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
        .map_err(|error| format!("cannot resolve the QuotaFence executable: {error}"))?;
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
        .map_err(|error| format!("cannot resolve the QuotaFence executable: {error}"))?;
    Ok(status_line_status(&config_path, &executable))
}

pub fn status_line_status(config_path: &Path, executable: &Path) -> ClaudeStatusLineStatus {
    let config_path_text = config_path.display().to_string();
    let heartbeat = read_status_line_heartbeat();
    let config = match read_settings(config_path) {
        Ok(config) => config,
        Err(issue) => {
            return ClaudeStatusLineStatus {
                installed: false,
                config_path: config_path_text,
                state: ClaudeStatusLineState::Misconfigured,
                issue: Some(issue),
                last_observed_at: heartbeat.last_observed_at,
                last_quota_observed_at: heartbeat.last_quota_observed_at,
            };
        }
    };
    let Some(status_line) = config.get("statusLine") else {
        return ClaudeStatusLineStatus {
            installed: false,
            config_path: config_path_text,
            state: ClaudeStatusLineState::Disabled,
            issue: None,
            last_observed_at: heartbeat.last_observed_at,
            last_quota_observed_at: heartbeat.last_quota_observed_at,
        };
    };
    if !is_quotafence_status_line(status_line) {
        return ClaudeStatusLineStatus {
            installed: false,
            config_path: config_path_text,
            state: ClaudeStatusLineState::Conflict,
            issue: Some(
                "Claude Code already has a custom status line. QuotaFence left it unchanged because Claude supports only one statusLine command."
                    .to_owned(),
            ),
            last_observed_at: heartbeat.last_observed_at,
            last_quota_observed_at: heartbeat.last_quota_observed_at,
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
            "QuotaFence found an outdated Claude status-line entry. Repair the integration to point it to the current application executable."
                .to_owned()
        }),
        last_observed_at: heartbeat.last_observed_at,
        last_quota_observed_at: heartbeat.last_quota_observed_at,
    }
}

fn status_line_heartbeat_path() -> Option<PathBuf> {
    paths::default_database_path().ok().and_then(|path| {
        path.parent()
            .map(|parent| parent.join(CLAUDE_HEARTBEAT_FILENAME))
    })
}

fn read_status_line_heartbeat() -> ClaudeStatusLineHeartbeat {
    status_line_heartbeat_path()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|contents| serde_json::from_str(&contents).ok())
        .unwrap_or_default()
}

fn record_status_line_heartbeat(observed_at: i64, has_quota: bool) -> Result<(), String> {
    let Some(path) = status_line_heartbeat_path() else {
        return Err("cannot resolve the Claude observer heartbeat path".to_owned());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    let mut heartbeat = read_status_line_heartbeat();
    heartbeat.last_observed_at = Some(observed_at);
    if has_quota {
        heartbeat.last_quota_observed_at = Some(observed_at);
    }
    let contents = serde_json::to_string(&heartbeat)
        .map_err(|error| format!("cannot serialize Claude observer heartbeat: {error}"))?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, contents)
        .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, &path)
        .map_err(|error| format!("cannot replace {}: {error}", path.display()))
}

pub fn install_status_line(config_path: &Path, executable: &Path) -> Result<bool, String> {
    let original = read_settings(config_path)?;
    if original
        .get("statusLine")
        .is_some_and(|entry| !is_quotafence_status_line(entry))
    {
        return Err(
            "Claude Code already has a custom status line. Remove it explicitly before installing QuotaFence; the existing command was not changed."
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
    if !original
        .get("statusLine")
        .is_some_and(is_quotafence_status_line)
    {
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
        let backup = config_path.with_extension("json.quotafence.bak");
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
    let temporary = config_path.with_extension("json.quotafence.tmp");
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

fn is_quotafence_status_line(value: &Value) -> bool {
    value.get("type").and_then(Value::as_str) == Some("command")
        && value
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(|command| command.ends_with(QUOTAFENCE_STATUS_LINE_MARKER))
}

fn status_line_command(executable: &Path) -> String {
    #[cfg(windows)]
    {
        format!(
            "\"{}\"{QUOTAFENCE_STATUS_LINE_MARKER}",
            executable.display()
        )
    }
    #[cfg(not(windows))]
    {
        let path = executable.to_string_lossy().replace('\'', "'\\''");
        format!("'{path}'{QUOTAFENCE_STATUS_LINE_MARKER}")
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

#[cfg(target_os = "macos")]
fn read_claude_code_credentials() -> Result<(String, ClaudeCredentialsFile), String> {
    let account = env::var("USER").map_err(|_| "cannot resolve the macOS account name")?;
    let output = Command::new("/usr/bin/security")
        .args([
            "find-generic-password",
            "-w",
            "-s",
            CLAUDE_KEYCHAIN_SERVICE,
            "-a",
            account.as_str(),
        ])
        .output()
        .map_err(|_| "could not ask macOS Keychain for the Claude Code login")?;
    if !output.status.success() {
        return Err(
            "Claude login access was denied or unavailable. Allow Keychain access and try again."
                .to_owned(),
        );
    }
    let credentials: ClaudeCredentialsFile = serde_json::from_slice(&output.stdout)
        .map_err(|_| "Claude Code returned an unsupported credential format")?;
    let token = credentials.claude_ai_oauth.access_token.trim();
    if token.is_empty() {
        return Err("Claude Code login does not contain an access token".to_owned());
    }
    Ok((account, credentials))
}

#[cfg(not(target_os = "macos"))]
fn read_claude_code_credentials() -> Result<(String, ClaudeCredentialsFile), String> {
    let path = claude_credentials_path()?;
    let credentials: ClaudeCredentialsFile =
        serde_json::from_slice(&fs::read(&path).map_err(|_| {
            format!(
                "Claude Code login was not found at {}. Run `claude`, sign in, and try again.",
                path.display()
            )
        })?)
        .map_err(|_| "Claude Code returned an unsupported credential format".to_owned())?;
    if credentials.claude_ai_oauth.access_token.trim().is_empty() {
        return Err("Claude Code login does not contain an access token".to_owned());
    }
    Ok((path.to_string_lossy().into_owned(), credentials))
}

#[cfg(not(target_os = "macos"))]
fn claude_credentials_path() -> Result<PathBuf, String> {
    env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".claude")))
        .map(|directory| directory.join(".credentials.json"))
        .ok_or_else(|| "cannot resolve the Claude configuration directory".to_owned())
}

#[cfg(target_os = "macos")]
fn write_claude_code_credentials(
    account: &str,
    credentials: &ClaudeCredentialsFile,
) -> Result<(), String> {
    let encoded = serde_json::to_string(credentials)
        .map_err(|_| "could not preserve the refreshed Claude login")?;
    let output = Command::new("/usr/bin/security")
        .args([
            "add-generic-password",
            "-U",
            "-s",
            CLAUDE_KEYCHAIN_SERVICE,
            "-a",
            account,
            "-w",
            encoded.as_str(),
        ])
        .output()
        .map_err(|_| "could not update the refreshed Claude login in macOS Keychain")?;
    if !output.status.success() {
        return Err("macOS Keychain could not save the refreshed Claude login".to_owned());
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn write_claude_code_credentials(
    account: &str,
    credentials: &ClaudeCredentialsFile,
) -> Result<(), String> {
    let path = PathBuf::from(account);
    let encoded = serde_json::to_string(credentials)
        .map_err(|_| "could not preserve the refreshed Claude login")?;
    fs::write(&path, format!("{encoded}\n")).map_err(|_| {
        format!(
            "could not update the refreshed Claude login at {}",
            path.display()
        )
    })
}

fn refresh_claude_access_token(
    client: &reqwest::blocking::Client,
    account: &str,
    credentials: &mut ClaudeCredentialsFile,
) -> Result<(), String> {
    const REFRESH_URL: &str = "https://platform.claude.com/v1/oauth/token";
    const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
    const SCOPES: &str =
        "user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";

    let refresh_token = credentials
        .claude_ai_oauth
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| "Claude login expired and does not contain a refresh token".to_owned())?;
    let response = client
        .post(REFRESH_URL)
        .json(&json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "client_id": CLIENT_ID,
            "scope": SCOPES,
        }))
        .send()
        .map_err(|_| "could not refresh the Claude login")?;
    if !response.status().is_success() {
        return Err(
            if response.status() == reqwest::StatusCode::BAD_REQUEST
                || response.status() == reqwest::StatusCode::UNAUTHORIZED
            {
                "Claude session has expired. Sign in to Claude Code again, then refresh.".to_owned()
            } else {
                format!(
                    "Claude login refresh failed with status {}",
                    response.status().as_u16()
                )
            },
        );
    }
    let refreshed: ClaudeTokenRefreshResponse = response
        .json()
        .map_err(|_| "Claude returned an unsupported login refresh response")?;
    if refreshed.access_token.trim().is_empty() {
        return Err("Claude returned an empty refreshed access token".to_owned());
    }
    credentials.claude_ai_oauth.access_token = refreshed.access_token;
    if let Some(refresh_token) = refreshed.refresh_token {
        credentials.claude_ai_oauth.refresh_token = Some(refresh_token);
    }
    if let Some(expires_in) = refreshed.expires_in {
        credentials.claude_ai_oauth.expires_at =
            Some(current_time_millis() as f64 + expires_in * 1_000.0);
    }
    write_claude_code_credentials(account, credentials)
}

#[cfg(target_os = "macos")]
fn read_keychain_password(service: &str, account: &str) -> Result<Vec<u8>, String> {
    let mut output = Command::new("/usr/bin/security")
        .args(["find-generic-password", "-w", "-s", service, "-a", account])
        .output()
        .map_err(|_| "could not ask macOS Keychain for Claude Desktop access")?;
    if !output.status.success() || output.stdout.is_empty() {
        return Err("Claude Desktop Keychain access was denied or unavailable".to_owned());
    }
    while output
        .stdout
        .last()
        .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        output.stdout.pop();
    }
    Ok(output.stdout)
}

#[cfg(target_os = "macos")]
fn decrypt_desktop_value(encrypted: &[u8], key: &[u8; 16]) -> Result<Vec<u8>, String> {
    if encrypted.len() <= 3 || &encrypted[..3] != b"v10" {
        return Err("Claude Desktop returned unsupported encrypted data".to_owned());
    }
    let mut payload = encrypted[3..].to_vec();
    let decrypted = cbc::Decryptor::<aes::Aes128>::new(key.into(), (&[0x20_u8; 16]).into())
        .decrypt_padded_mut::<Pkcs7>(&mut payload)
        .map_err(|_| "could not decrypt Claude Desktop login data")?;
    Ok(decrypted.to_vec())
}

#[cfg(target_os = "macos")]
fn desktop_safe_storage_key() -> Result<[u8; 16], String> {
    let password =
        read_keychain_password(CLAUDE_SAFE_STORAGE_SERVICE, CLAUDE_SAFE_STORAGE_ACCOUNT)?;
    let mut key = [0_u8; 16];
    pbkdf2_hmac::<Sha1>(&password, b"saltysalt", 1003, &mut key);
    Ok(key)
}

#[cfg(target_os = "macos")]
fn read_desktop_active_organization(key: &[u8; 16]) -> Result<String, String> {
    let home = dirs::home_dir().ok_or_else(|| "cannot resolve the home directory".to_owned())?;
    for relative in [
        "Library/Application Support/Claude/Cookies",
        "Library/Application Support/Claude/Network/Cookies",
    ] {
        let path = home.join(relative);
        if !path.exists() {
            continue;
        }
        let connection = match Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) {
            Ok(connection) => connection,
            Err(_) => continue,
        };
        for host in [".claude.ai", "claude.ai"] {
            let row: Option<(String, Vec<u8>)> = connection
                .query_row(
                    "SELECT value, encrypted_value FROM cookies WHERE name = 'lastActiveOrg' AND host_key = ?1 ORDER BY last_update_utc DESC LIMIT 1",
                    [host],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|_| "could not inspect Claude Desktop organization metadata")?;
            let Some((plain, encrypted)) = row else {
                continue;
            };
            let value = if !plain.is_empty() {
                plain.into_bytes()
            } else {
                let decrypted = decrypt_desktop_value(&encrypted, key)?;
                let host_hash = Sha256::digest(host.as_bytes());
                if !decrypted.starts_with(host_hash.as_slice()) {
                    continue;
                }
                decrypted[host_hash.len()..].to_vec()
            };
            let organization = String::from_utf8(value)
                .map_err(|_| "Claude Desktop returned invalid organization metadata")?;
            if organization.parse::<uuid::Uuid>().is_ok() {
                return Ok(organization.to_lowercase());
            }
        }
    }
    Err("Claude Desktop active organization was not found".to_owned())
}

#[cfg(target_os = "macos")]
fn read_desktop_access_token() -> Result<String, String> {
    use base64::Engine;

    let key = desktop_safe_storage_key()?;
    let organization = read_desktop_active_organization(&key)?;
    let config_path = dirs::home_dir()
        .ok_or_else(|| "cannot resolve the home directory".to_owned())?
        .join(CLAUDE_DESKTOP_CONFIG);
    let root: Value = serde_json::from_slice(
        &fs::read(config_path).map_err(|_| "Claude Desktop login cache was not found")?,
    )
    .map_err(|_| "Claude Desktop returned an unsupported login cache")?;
    let now_with_margin = current_time_millis() as f64 + 2.0 * 60_000.0;
    let mut candidates: Vec<(i32, usize, f64, String)> = Vec::new();
    for cache_name in ["oauth:tokenCacheV2", "oauth:tokenCache"] {
        let Some(encoded) = root.get(cache_name).and_then(Value::as_str) else {
            continue;
        };
        let encrypted = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| "Claude Desktop returned an invalid login cache")?;
        let cache: Value = serde_json::from_slice(&decrypt_desktop_value(&encrypted, &key)?)
            .map_err(|_| "Claude Desktop returned an unsupported login cache")?;
        let Some(entries) = cache.as_object() else {
            continue;
        };
        for (cache_key, entry) in entries {
            let marker = ":https://api.anthropic.com:";
            let Some((prefix, scopes_text)) = cache_key.split_once(marker) else {
                continue;
            };
            let Some((client_id, entry_org)) = prefix.split_once(':') else {
                continue;
            };
            if !entry_org.eq_ignore_ascii_case(&organization) {
                continue;
            }
            let scopes: Vec<&str> = scopes_text.split_whitespace().collect();
            if !scopes.contains(&"user:profile") {
                continue;
            }
            let Some(token) = entry.get("token").and_then(Value::as_str) else {
                continue;
            };
            let Some(expires_at) = entry.get("expiresAt").and_then(Value::as_f64) else {
                continue;
            };
            if token.trim().is_empty() || expires_at <= now_with_margin {
                continue;
            }
            let full_scope = scopes.contains(&"user:inference") as i32;
            let production_client = (client_id == "9d1c250a-e61b-44d9-88ed-5944d1962f5e") as i32;
            candidates.push((
                production_client * 2 + full_scope,
                scopes.len(),
                expires_at,
                token.to_owned(),
            ));
        }
    }
    candidates
        .into_iter()
        .max_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then(left.1.cmp(&right.1))
                .then(left.2.total_cmp(&right.2))
        })
        .map(|candidate| candidate.3)
        .ok_or_else(|| {
            "Claude Desktop has no current usage-capable login. Reopen Claude Desktop and try again."
                .to_owned()
        })
}

#[cfg(not(target_os = "macos"))]
fn read_desktop_access_token() -> Result<String, String> {
    Err(
        "Claude Code login could not be refreshed. Run `claude`, sign in again, and retry."
            .to_owned(),
    )
}

fn map_usage_window(window: ClaudeUsageWindow) -> Result<ClaudeRateLimitWindow, String> {
    if !window.utilization.is_finite() || !(0.0..=100.0).contains(&window.utilization) {
        return Err("Claude returned an invalid subscription utilization".to_owned());
    }
    let resets_at_seconds = DateTime::parse_from_rfc3339(&window.resets_at)
        .map_err(|_| "Claude returned an invalid subscription reset time")?
        .timestamp();
    if resets_at_seconds <= 0 {
        return Err("Claude returned an invalid subscription reset time".to_owned());
    }
    Ok(ClaudeRateLimitWindow {
        used_percentage: window.utilization,
        resets_at_seconds,
    })
}

pub fn fetch_subscription_usage() -> Result<ClaudeStatusLineObservation, String> {
    let (account, mut credentials) = read_claude_code_credentials()?;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|_| "could not initialize Claude usage refresh")?;
    let expires_soon = credentials
        .claude_ai_oauth
        .expires_at
        .is_some_and(|expires_at| expires_at <= current_time_millis() as f64 + 5.0 * 60_000.0);
    let mut using_desktop = false;
    if expires_soon && refresh_claude_access_token(&client, &account, &mut credentials).is_err() {
        credentials.claude_ai_oauth.access_token = read_desktop_access_token()?;
        using_desktop = true;
    }
    let mut response = client
        .get(CLAUDE_USAGE_URL)
        .bearer_auth(credentials.claude_ai_oauth.access_token.trim())
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("User-Agent", "claude-code/2.1.69")
        .send()
        .map_err(|_| "could not reach Claude subscription usage")?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED
        || response.status() == reqwest::StatusCode::FORBIDDEN
    {
        if using_desktop {
            return Err(
                "Claude Desktop login cannot read subscription usage. Reopen Claude Desktop and try again."
                    .to_owned(),
            );
        }
        if refresh_claude_access_token(&client, &account, &mut credentials).is_err() {
            credentials.claude_ai_oauth.access_token = read_desktop_access_token()?;
        }
        response = client
            .get(CLAUDE_USAGE_URL)
            .bearer_auth(credentials.claude_ai_oauth.access_token.trim())
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("anthropic-beta", "oauth-2025-04-20")
            .header("User-Agent", "claude-code/2.1.69")
            .send()
            .map_err(|_| "could not reach Claude subscription usage after login refresh")?;
    }
    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err("Claude usage refresh is rate limited. Wait before trying again.".to_owned());
    }
    if !response.status().is_success() {
        return Err(format!(
            "Claude usage refresh failed with status {}",
            response.status().as_u16()
        ));
    }
    let usage: ClaudeUsageResponse = response
        .json()
        .map_err(|_| "Claude returned an unsupported usage response")?;
    let observation = ClaudeStatusLineObservation {
        session_id: "oauth-usage-refresh".to_owned(),
        current_directory: "provider-account".to_owned(),
        five_hour: usage.five_hour.map(map_usage_window).transpose()?,
        seven_day: usage.seven_day.map(map_usage_window).transpose()?,
    };
    record_status_line_heartbeat(
        current_time_millis(),
        observation.five_hour.is_some() || observation.seven_day.is_some(),
    )?;
    Ok(observation)
}

pub fn run_status_line(reader: impl Read) -> Result<String, String> {
    let observation = ClaudeStatusLineObservation::from_reader(reader)?;
    let observed_at = current_time_millis();
    record_status_line_heartbeat(
        observed_at,
        observation.five_hour.is_some() || observation.seven_day.is_some(),
    )?;
    let database_path = paths::default_database_path()
        .map_err(|error| format!("cannot resolve the QuotaFence database: {error}"))?;
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
        || "QuotaFence · waiting for Claude subscription quota".to_owned(),
        |remaining| format!("QuotaFence · Claude {remaining:.0}% left"),
    ))
}

pub fn ingest_observation(
    service: &mut QuotaService,
    observation: &ClaudeStatusLineObservation,
    observed_at: i64,
) -> Result<ClaudeSyncResult, String> {
    let mut windows = Vec::new();
    if let Some(window) = observation.five_hour.as_ref() {
        windows.push(ingest_window(
            service,
            "five_hour",
            "5-hour allowance",
            FIVE_HOURS_MILLIS,
            window,
            observed_at,
        )?);
    }
    if let Some(window) = observation.seven_day.as_ref() {
        windows.push(ingest_window(
            service,
            "seven_day",
            "Weekly allowance",
            SEVEN_DAYS_MILLIS,
            window,
            observed_at,
        )?);
    }
    Ok(ClaudeSyncResult { windows })
}

fn ingest_window(
    service: &mut QuotaService,
    kind: &str,
    display_name: &str,
    duration_millis: i64,
    window: &ClaudeRateLimitWindow,
    observed_at: i64,
) -> Result<ClaudeSyncedWindow, String> {
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
    let (window_id, rolled_over) = if let Some(source) = existing {
        let result = service
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
        (result.window_id, result.rolled_over)
    } else {
        let suffix = kind.replace('_', "-");
        let window_id = format!("claude-code-{suffix}-window-{ends_at}");
        service
            .create_quota_source(CreateQuotaSource {
                provider_id: format!("claude-code-{suffix}-provider"),
                provider_display_name: "Claude Code".to_owned(),
                account_id: format!("claude-code-{suffix}-account"),
                account_display_name: "Claude subscription".to_owned(),
                pool_id: format!("claude-code-{suffix}"),
                pool_display_name: display_name.to_owned(),
                window_id: window_id.clone(),
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
        (window_id, false)
    };
    Ok(ClaudeSyncedWindow {
        kind: kind.to_owned(),
        display_name: display_name.to_owned(),
        window_id,
        rolled_over,
    })
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
            "quotafence-claude-statusline-{name}-{}",
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
        let executable = path.parent().unwrap().join("QuotaFence");
        fs::write(&executable, "binary").unwrap();

        assert!(install_status_line(&path, &executable).unwrap());

        let settings = read_settings(&path).unwrap();
        assert_eq!(settings["theme"], "dark");
        assert_eq!(settings["permissions"]["allow"][0], "Bash");
        assert!(is_quotafence_status_line(&settings["statusLine"]));
        assert_eq!(
            status_line_status(&path, &executable).state,
            ClaudeStatusLineState::Configured
        );
        assert!(path.with_extension("json.quotafence.bak").exists());
    }

    #[test]
    fn install_refuses_to_replace_an_existing_custom_status_line() {
        let path = temp_settings("conflict");
        let original = r#"{"statusLine":{"type":"command","command":"my-status.sh"}}"#;
        fs::write(&path, original).unwrap();
        let executable = path.parent().unwrap().join("quotafence");

        let error = install_status_line(&path, &executable).unwrap_err();

        assert!(error.contains("already has a custom status line"));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        assert_eq!(
            status_line_status(&path, &executable).state,
            ClaudeStatusLineState::Conflict
        );
    }

    #[test]
    fn uninstall_removes_only_an_quotafence_owned_status_line() {
        let path = temp_settings("uninstall");
        fs::write(&path, r#"{"theme":"light"}"#).unwrap();
        let executable = path.parent().unwrap().join("quotafence");
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
