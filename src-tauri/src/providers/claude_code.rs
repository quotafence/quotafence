use std::{env, path::PathBuf, process::Command};

use serde::{Deserialize, Serialize};

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
