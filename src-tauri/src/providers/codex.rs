use std::{
    env,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

use serde::Serialize;
use serde_json::{json, Value};

const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(30);
const RATE_LIMIT_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionStatus {
    Detected,
    NotInstalled,
    NotAuthenticated,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedQuotaWindow {
    pub id: String,
    pub display_name: String,
    pub kind: String,
    pub starts_at: i64,
    pub ends_at: i64,
    pub capacity: u64,
    pub used: u64,
    pub remaining: u64,
    pub unit: String,
    pub duration_minutes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexDetection {
    pub status: DetectionStatus,
    pub provider_id: String,
    pub provider_display_name: String,
    pub plan_type: Option<String>,
    pub windows: Vec<DetectedQuotaWindow>,
    pub message: Option<String>,
}

impl CodexDetection {
    fn detected(plan_type: Option<String>, windows: Vec<DetectedQuotaWindow>) -> Self {
        Self {
            status: DetectionStatus::Detected,
            provider_id: "codex".to_owned(),
            provider_display_name: "Codex".to_owned(),
            plan_type,
            windows,
            message: None,
        }
    }

    fn unavailable(status: DetectionStatus, message: impl Into<String>) -> Self {
        Self {
            status,
            provider_id: "codex".to_owned(),
            provider_display_name: "Codex".to_owned(),
            plan_type: None,
            windows: Vec::new(),
            message: Some(message.into()),
        }
    }
}

#[derive(Debug)]
enum AdapterError {
    NotInstalled,
    Io,
    Protocol,
    TimedOut,
    Server(String),
}

pub fn detect() -> CodexDetection {
    match query_rate_limits() {
        Ok(result) => result,
        Err(AdapterError::NotInstalled) => CodexDetection::unavailable(
            DetectionStatus::NotInstalled,
            "Codex CLI was not found on this device.",
        ),
        Err(AdapterError::Server(message)) if looks_like_auth_error(&message) => {
            CodexDetection::unavailable(
                DetectionStatus::NotAuthenticated,
                "Codex is installed, but no signed-in subscription could be read.",
            )
        }
        Err(AdapterError::TimedOut) => CodexDetection::unavailable(
            DetectionStatus::Unavailable,
            "Codex took too long to return subscription limits. You can retry or use manual setup.",
        ),
        Err(AdapterError::Io | AdapterError::Protocol | AdapterError::Server(_)) => {
            CodexDetection::unavailable(
                DetectionStatus::Unavailable,
                "Codex subscription limits are temporarily unavailable. You can retry or use manual setup.",
            )
        }
    }
}

pub fn detection_failed() -> CodexDetection {
    CodexDetection::unavailable(
        DetectionStatus::Unavailable,
        "Codex subscription detection stopped unexpectedly. You can retry or use manual setup.",
    )
}

fn query_rate_limits() -> Result<CodexDetection, AdapterError> {
    for candidate in executable_candidates() {
        match query_candidate(&candidate) {
            Ok(detection) => return Ok(detection),
            Err(AdapterError::NotInstalled) => continue,
            Err(error) => return Err(error),
        }
    }

    Err(AdapterError::NotInstalled)
}

fn query_candidate(executable: &PathBuf) -> Result<CodexDetection, AdapterError> {
    let mut command = Command::new(executable);
    command
        .args(["app-server", "--stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(AdapterError::NotInstalled);
        }
        Err(_) => return Err(AdapterError::Io),
    };

    let result = communicate(&mut child);
    let _ = child.kill();
    let _ = child.wait();
    result
}

fn communicate(child: &mut Child) -> Result<CodexDetection, AdapterError> {
    let mut stdin = child.stdin.take().ok_or(AdapterError::Io)?;
    let stdout = child.stdout.take().ok_or(AdapterError::Io)?;
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(line) => {
                    if sender.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    send_message(
        &mut stdin,
        &json!({
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": {
                    "name": "agent-quota-manager",
                    "title": "Agent Quota Manager",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "experimentalApi": false
                }
            }
        }),
    )?;
    response_result(&receiver, 1, INITIALIZE_TIMEOUT)?;

    send_message(&mut stdin, &json!({ "method": "initialized" }))?;
    send_message(
        &mut stdin,
        &json!({
            "id": 2,
            "method": "account/rateLimits/read"
        }),
    )?;

    let snapshot = response_result(&receiver, 2, RATE_LIMIT_TIMEOUT)?;
    parse_rate_limit_response(&snapshot)
}

fn send_message(stdin: &mut impl Write, message: &Value) -> Result<(), AdapterError> {
    serde_json::to_writer(&mut *stdin, message).map_err(|_| AdapterError::Protocol)?;
    stdin.write_all(b"\n").map_err(|_| AdapterError::Io)?;
    stdin.flush().map_err(|_| AdapterError::Io)
}

fn response_result(
    receiver: &Receiver<String>,
    expected_id: i64,
    timeout: Duration,
) -> Result<Value, AdapterError> {
    let deadline = Instant::now() + timeout;

    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or(AdapterError::TimedOut)?;
        let line = receiver
            .recv_timeout(remaining)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => AdapterError::TimedOut,
                mpsc::RecvTimeoutError::Disconnected => AdapterError::Protocol,
            })?;
        let message: Value = serde_json::from_str(&line).map_err(|_| AdapterError::Protocol)?;

        if message.get("id").and_then(Value::as_i64) != Some(expected_id) {
            continue;
        }

        if let Some(error) = message.get("error") {
            let detail = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Codex App Server returned an error");
            return Err(AdapterError::Server(detail.to_owned()));
        }

        return message.get("result").cloned().ok_or(AdapterError::Protocol);
    }
}

fn parse_rate_limit_response(result: &Value) -> Result<CodexDetection, AdapterError> {
    let snapshot = result
        .get("rateLimitsByLimitId")
        .and_then(|limits| limits.get("codex"))
        .or_else(|| result.get("rateLimits"))
        .ok_or(AdapterError::Protocol)?;
    let plan_type = snapshot
        .get("planType")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    let mut windows = Vec::new();
    for (kind, field) in [("primary", "primary"), ("secondary", "secondary")] {
        if let Some(window) = snapshot.get(field).filter(|value| !value.is_null()) {
            windows.push(parse_window(kind, window)?);
        }
    }

    if windows.is_empty() {
        return Err(AdapterError::Server(
            "No Codex subscription rate-limit windows were returned".to_owned(),
        ));
    }

    Ok(CodexDetection::detected(plan_type, windows))
}

fn parse_window(kind: &str, window: &Value) -> Result<DetectedQuotaWindow, AdapterError> {
    let used = window
        .get("usedPercent")
        .and_then(Value::as_i64)
        .filter(|value| (0..=100).contains(value))
        .ok_or(AdapterError::Protocol)? as u64;
    let duration_minutes = window
        .get("windowDurationMins")
        .and_then(Value::as_i64)
        .filter(|value| *value > 0)
        .ok_or(AdapterError::Protocol)? as u64;
    let reset_seconds = window
        .get("resetsAt")
        .and_then(Value::as_i64)
        .ok_or(AdapterError::Protocol)?;
    let ends_at = reset_seconds
        .checked_mul(1_000)
        .ok_or(AdapterError::Protocol)?;
    let duration_millis = i64::try_from(duration_minutes)
        .ok()
        .and_then(|minutes| minutes.checked_mul(60_000))
        .ok_or(AdapterError::Protocol)?;
    let starts_at = ends_at
        .checked_sub(duration_millis)
        .ok_or(AdapterError::Protocol)?;

    Ok(DetectedQuotaWindow {
        id: format!("codex-{kind}"),
        display_name: window_display_name(duration_minutes),
        kind: kind.to_owned(),
        starts_at,
        ends_at,
        capacity: 100,
        used,
        remaining: 100 - used,
        unit: "percent".to_owned(),
        duration_minutes,
    })
}

fn window_display_name(duration_minutes: u64) -> String {
    match duration_minutes {
        10_080 => "Weekly allowance".to_owned(),
        1_440 => "Daily allowance".to_owned(),
        minutes if minutes % 1_440 == 0 => {
            format!("{}-day allowance", minutes / 1_440)
        }
        minutes if minutes % 60 == 0 => {
            format!("{}-hour allowance", minutes / 60)
        }
        minutes => format!("{minutes}-minute allowance"),
    }
}

fn executable_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(override_path) = env::var_os("AGENT_QUOTA_CODEX_BIN") {
        candidates.push(PathBuf::from(override_path));
    }

    candidates.push(PathBuf::from("codex"));

    #[cfg(target_os = "macos")]
    {
        candidates.push(PathBuf::from(
            "/Applications/ChatGPT.app/Contents/Resources/codex",
        ));
        candidates.push(PathBuf::from(
            "/Applications/Codex.app/Contents/Resources/codex",
        ));
    }

    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        candidates.push(home.join(".local/bin/codex"));
        candidates.push(home.join(".npm-global/bin/codex"));
    }

    candidates.push(PathBuf::from("/opt/homebrew/bin/codex"));
    candidates.push(PathBuf::from("/usr/local/bin/codex"));
    candidates
}

fn looks_like_auth_error(message: &str) -> bool {
    let normalized = message.to_ascii_lowercase();
    ["auth", "login", "sign in", "unauthorized", "401"]
        .iter()
        .any(|fragment| normalized.contains(fragment))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_official_rate_limit_snapshot_without_account_identity() {
        let result = json!({
            "rateLimits": {
                "limitId": "codex",
                "primary": {
                    "usedPercent": 19,
                    "windowDurationMins": 300,
                    "resetsAt": 1_800_000_000
                },
                "secondary": {
                    "usedPercent": 5,
                    "windowDurationMins": 10_080,
                    "resetsAt": 1_800_500_000
                },
                "planType": "plus"
            }
        });

        let detection = parse_rate_limit_response(&result).unwrap();

        assert_eq!(detection.status, DetectionStatus::Detected);
        assert_eq!(detection.plan_type.as_deref(), Some("plus"));
        assert_eq!(detection.windows.len(), 2);
        assert_eq!(detection.windows[0].display_name, "5-hour allowance");
        assert_eq!(detection.windows[0].remaining, 81);
        assert_eq!(detection.windows[1].display_name, "Weekly allowance");
        assert_eq!(detection.windows[1].used, 5);
    }

    #[test]
    fn prefers_codex_bucket_from_multi_bucket_response() {
        let result = json!({
            "rateLimits": {
                "primary": {
                    "usedPercent": 99,
                    "windowDurationMins": 60,
                    "resetsAt": 1_800_000_000
                }
            },
            "rateLimitsByLimitId": {
                "codex": {
                    "primary": {
                        "usedPercent": 12,
                        "windowDurationMins": 10_080,
                        "resetsAt": 1_800_000_000
                    },
                    "planType": "pro"
                }
            }
        });

        let detection = parse_rate_limit_response(&result).unwrap();

        assert_eq!(detection.windows[0].used, 12);
        assert_eq!(detection.plan_type.as_deref(), Some("pro"));
    }

    #[test]
    fn rejects_incomplete_or_out_of_range_windows() {
        let missing_reset = json!({
            "rateLimits": {
                "primary": {
                    "usedPercent": 5,
                    "windowDurationMins": 300
                }
            }
        });
        let invalid_percent = json!({
            "rateLimits": {
                "primary": {
                    "usedPercent": 101,
                    "windowDurationMins": 300,
                    "resetsAt": 1_800_000_000
                }
            }
        });

        assert!(parse_rate_limit_response(&missing_reset).is_err());
        assert!(parse_rate_limit_response(&invalid_percent).is_err());
    }

    #[test]
    fn recognizes_authentication_failures_without_exposing_details() {
        assert!(looks_like_auth_error("Please run codex login"));
        assert!(looks_like_auth_error("HTTP 401 Unauthorized"));
        assert!(!looks_like_auth_error("temporary transport failure"));
    }
}
