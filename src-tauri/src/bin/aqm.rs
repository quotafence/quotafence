use std::{
    env, fs,
    path::PathBuf,
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};

use agent_quota_manager_lib::{
    application::{
        AdmissionAssessment, BindRepository, EvaluateRepositoryAdmission, GetRepositoryContext,
        QuotaService, RepositoryContext,
    },
    domain::EnforcementDecision,
    paths,
    providers::codex::{self, CodexSyncResult, CodexSyncStatus},
    repository::resolve_git_root,
};
use serde::Serialize;

const EXIT_ALLOW: u8 = 0;
const EXIT_ERROR: u8 = 1;
const EXIT_WARN: u8 = 10;
const EXIT_CONFIRM: u8 = 20;
const EXIT_STOP: u8 = 30;

#[derive(Debug)]
enum CliCommand {
    Context(CommonOptions),
    Bind {
        options: CommonOptions,
        scope_reference: String,
    },
    Admit {
        options: CommonOptions,
        provider_id: String,
        assume_yes: bool,
    },
    Help,
}

#[derive(Debug, Default)]
struct CommonOptions {
    path: Option<PathBuf>,
    database: Option<PathBuf>,
    json: bool,
}

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(code) => ExitCode::from(code),
        Err(message) => {
            eprintln!("aqm: {message}");
            ExitCode::from(EXIT_ERROR)
        }
    }
}

fn run(args: Vec<String>) -> Result<u8, String> {
    match parse_args(args)? {
        CliCommand::Help => {
            print_help();
            Ok(EXIT_ALLOW)
        }
        CliCommand::Context(options) => {
            let (mut service, canonical_root) = open_context(&options)?;
            let context = service
                .repository_context(GetRepositoryContext {
                    canonical_root,
                    at: now_millis()?,
                })
                .map_err(|error| error.to_string())?;
            print_context(&context, options.json)?;
            Ok(EXIT_ALLOW)
        }
        CliCommand::Bind {
            options,
            scope_reference,
        } => {
            let (mut service, canonical_root) = open_context(&options)?;
            service
                .bind_repository(BindRepository {
                    canonical_root: canonical_root.clone(),
                    scope_reference,
                    bound_at: now_millis()?,
                })
                .map_err(|error| error.to_string())?;
            let context = service
                .repository_context(GetRepositoryContext {
                    canonical_root,
                    at: now_millis()?,
                })
                .map_err(|error| error.to_string())?;
            print_context(&context, options.json)?;
            Ok(EXIT_ALLOW)
        }
        CliCommand::Admit {
            options,
            provider_id,
            assume_yes,
        } => {
            let (mut service, canonical_root) = open_context(&options)?;
            let now = now_millis()?;
            let context = service
                .repository_context(GetRepositoryContext {
                    canonical_root: canonical_root.clone(),
                    at: now,
                })
                .map_err(|error| error.to_string())?;
            let allocation = provider_allocation(&context, &provider_id)?;
            let checkpoint = match provider_id.to_ascii_lowercase().as_str() {
                "codex" => codex::sync_detection(
                    &mut service,
                    allocation.window_id.clone(),
                    now,
                    codex::detect(),
                ),
                _ => {
                    return Err(format!(
                        "provider {provider_id:?} is not supported for admission"
                    ))
                }
            };
            match checkpoint.status {
                CodexSyncStatus::Synced => {}
                CodexSyncStatus::NotApplicable => {
                    return Err(format!(
                        "{} is not a provider-managed Codex percentage source",
                        allocation.pool_display_name
                    ));
                }
                CodexSyncStatus::Unavailable => {
                    return Err(format!(
                        "cannot refresh Codex checkpoint: {}",
                        checkpoint
                            .message
                            .as_deref()
                            .unwrap_or("provider temporarily unavailable")
                    ));
                }
            }
            let assessment = service
                .evaluate_repository_admission(EvaluateRepositoryAdmission {
                    canonical_root,
                    provider_id,
                    at: now,
                })
                .map_err(|error| error.to_string())?;
            let exit_code = admission_exit_code(assessment.decision, assume_yes);
            print_admission(
                &assessment,
                &checkpoint,
                assume_yes && assessment.decision == EnforcementDecision::RequireConfirmation,
                exit_code,
                options.json,
            )?;
            Ok(exit_code)
        }
    }
}

fn provider_allocation<'a>(
    context: &'a RepositoryContext,
    provider_id: &str,
) -> Result<&'a agent_quota_manager_lib::application::RepositoryAllocationContext, String> {
    let binding = context.binding.as_ref().ok_or_else(|| {
        format!(
            "repository {} is not bound; run `aqm bind --scope <name-or-id>` first",
            context.canonical_root
        )
    })?;
    let mut matching = context
        .allocations
        .iter()
        .filter(|allocation| allocation.provider_id.eq_ignore_ascii_case(provider_id));
    let allocation = matching.next().ok_or_else(|| {
        format!(
            "scope {} has no allocation for provider {provider_id}",
            binding.scope_display_name
        )
    })?;
    if matching.next().is_some() {
        return Err(format!(
            "scope {} has multiple allocations for provider {provider_id}; pool selection is not supported yet",
            binding.scope_display_name
        ));
    }
    Ok(allocation)
}

fn open_context(options: &CommonOptions) -> Result<(QuotaService, String), String> {
    let start = match options.path.as_deref() {
        Some(path) => path.to_path_buf(),
        None => {
            env::current_dir().map_err(|error| format!("cannot read current directory: {error}"))?
        }
    };
    let canonical_root = resolve_git_root(start).map_err(|error| error.to_string())?;
    let database_path = database_path(options)?;
    if let Some(parent) = database_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create app data directory: {error}"))?;
    }
    let service = QuotaService::open(database_path).map_err(|error| error.to_string())?;
    Ok((service, canonical_root))
}

fn database_path(options: &CommonOptions) -> Result<PathBuf, String> {
    if let Some(path) = options.database.as_ref() {
        return Ok(path.clone());
    }
    if let Some(path) = env::var_os("AQM_DATABASE_PATH") {
        return Ok(PathBuf::from(path));
    }
    paths::default_database_path().map_err(|error| error.to_string())
}

fn parse_args(args: Vec<String>) -> Result<CliCommand, String> {
    let Some(command) = args.first().map(String::as_str) else {
        return Ok(CliCommand::Help);
    };
    if matches!(command, "-h" | "--help" | "help") {
        return Ok(CliCommand::Help);
    }
    if !matches!(command, "context" | "bind" | "admit") {
        return Err(format!("unknown command {command:?}; run `aqm --help`"));
    }

    let mut options = CommonOptions::default();
    let mut scope_reference = None;
    let mut assume_yes = false;
    let provider_id = if command == "admit" {
        Some(
            args.get(1)
                .filter(|value| !value.starts_with('-'))
                .cloned()
                .ok_or_else(|| "`aqm admit` requires a provider, for example `codex`".to_owned())?,
        )
    } else {
        None
    };
    let mut index = if command == "admit" { 2 } else { 1 };
    while index < args.len() {
        match args[index].as_str() {
            "--path" => {
                index += 1;
                options.path = Some(PathBuf::from(required_value(&args, index, "--path")?));
            }
            "--database" => {
                index += 1;
                options.database = Some(PathBuf::from(required_value(&args, index, "--database")?));
            }
            "--scope" if command == "bind" => {
                index += 1;
                scope_reference = Some(required_value(&args, index, "--scope")?.to_owned());
            }
            "--yes" if command == "admit" => assume_yes = true,
            "--json" => options.json = true,
            "-h" | "--help" => return Ok(CliCommand::Help),
            option => return Err(format!("unknown option {option:?}")),
        }
        index += 1;
    }

    let options = options;
    match command {
        "context" => Ok(CliCommand::Context(options)),
        "bind" => Ok(CliCommand::Bind {
            options,
            scope_reference: scope_reference
                .ok_or_else(|| "`aqm bind` requires `--scope <name-or-id>`".to_owned())?,
        }),
        "admit" => Ok(CliCommand::Admit {
            options,
            provider_id: provider_id.expect("admit provider was validated"),
            assume_yes,
        }),
        _ => unreachable!("command was validated"),
    }
}

fn required_value<'a>(args: &'a [String], index: usize, option: &str) -> Result<&'a str, String> {
    args.get(index)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{option} requires a value"))
}

fn print_context(context: &RepositoryContext, json: bool) -> Result<(), String> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(context)
                .map_err(|error| format!("cannot serialize context: {error}"))?
        );
        return Ok(());
    }

    println!("Repository: {}", context.canonical_root);
    match &context.binding {
        Some(binding) => {
            println!(
                "Scope: {} ({})",
                binding.scope_display_name, binding.scope_id
            );
            if context.allocations.is_empty() {
                println!("Allocation: none in an active quota window");
            } else {
                println!("Allocations:");
                for allocation in &context.allocations {
                    println!(
                        "  {} · {}: {} {} limit, {} remaining, {:?}",
                        allocation.provider_display_name,
                        allocation.pool_display_name,
                        allocation.limit,
                        allocation.unit,
                        allocation.remaining,
                        allocation.decision
                    );
                }
            }
        }
        None => {
            println!("Scope: unmapped");
            if context.available_repository_scopes.is_empty() {
                println!("No unbound repository scopes are available.");
                println!("Create a repository allocation in the desktop app first.");
            } else {
                println!("Available repository scopes:");
                for scope in &context.available_repository_scopes {
                    println!("  {} ({})", scope.display_name, scope.id);
                }
                println!("Bind with: aqm bind --scope <name-or-id>");
            }
        }
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AdmissionOutput<'a> {
    assessment: &'a AdmissionAssessment,
    checkpoint: &'a CodexSyncResult,
    override_applied: bool,
    proceed: bool,
    exit_code: u8,
}

fn admission_exit_code(decision: EnforcementDecision, assume_yes: bool) -> u8 {
    match decision {
        EnforcementDecision::Allow => EXIT_ALLOW,
        EnforcementDecision::Warn => EXIT_WARN,
        EnforcementDecision::RequireConfirmation if assume_yes => EXIT_ALLOW,
        EnforcementDecision::RequireConfirmation => EXIT_CONFIRM,
        EnforcementDecision::Stop => EXIT_STOP,
    }
}

fn print_admission(
    assessment: &AdmissionAssessment,
    checkpoint: &CodexSyncResult,
    override_applied: bool,
    exit_code: u8,
    json: bool,
) -> Result<(), String> {
    let proceed = matches!(
        assessment.decision,
        EnforcementDecision::Allow | EnforcementDecision::Warn
    ) || override_applied;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&AdmissionOutput {
                assessment,
                checkpoint,
                override_applied,
                proceed,
                exit_code,
            })
            .map_err(|error| format!("cannot serialize admission: {error}"))?
        );
        return Ok(());
    }

    println!(
        "Admission: {}",
        decision_label(assessment.decision, override_applied)
    );
    println!(
        "Repository: {} ({})",
        assessment.scope_display_name, assessment.canonical_root
    );
    println!(
        "Allocation: {} {} remaining of {}",
        assessment.allocation_remaining, assessment.unit, assessment.allocation_limit
    );
    println!(
        "Provider: {} {} remaining of {}",
        assessment.provider_remaining, assessment.unit, assessment.provider_capacity
    );
    println!(
        "Signals: allocation={:?}, provider={:?}",
        assessment.allocation_decision, assessment.provider_decision
    );
    if assessment.decision == EnforcementDecision::RequireConfirmation && !override_applied {
        println!("Re-run with --yes to explicitly accept this admission boundary.");
    }
    if assessment.decision == EnforcementDecision::Stop {
        println!("AQM would refuse a managed launch at this policy boundary.");
    }
    println!("Exit code: {exit_code}");
    Ok(())
}

fn decision_label(decision: EnforcementDecision, override_applied: bool) -> &'static str {
    match (decision, override_applied) {
        (EnforcementDecision::Allow, _) => "allow",
        (EnforcementDecision::Warn, _) => "warn",
        (EnforcementDecision::RequireConfirmation, true) => "allow (explicit override)",
        (EnforcementDecision::RequireConfirmation, false) => "confirmation required",
        (EnforcementDecision::Stop, _) => "stop",
    }
}

fn now_millis() -> Result<i64, String> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock is before Unix epoch: {error}"))?
        .as_millis();
    i64::try_from(millis).map_err(|_| "current time cannot be represented".to_owned())
}

fn print_help() {
    println!(
        "Agent Quota Manager CLI

Usage:
  aqm context [--path <directory>] [--json]
  aqm bind --scope <name-or-id> [--path <directory>] [--json]
  aqm admit codex [--path <directory>] [--yes] [--json]

Options:
  --path <directory>   Resolve a repository from this directory instead of cwd
  --database <path>    Override the local database (or set AQM_DATABASE_PATH)
  --yes                Explicitly accept a confirmation-required admission
  --json               Print machine-readable output"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn bind_requires_an_explicit_scope() {
        assert!(parse_args(vec!["bind".to_owned()])
            .unwrap_err()
            .contains("--scope"));
    }

    #[test]
    fn context_parses_path_database_and_json() {
        let command = parse_args(vec![
            "context".to_owned(),
            "--path".to_owned(),
            "/code/project".to_owned(),
            "--database".to_owned(),
            "/tmp/aqm.sqlite3".to_owned(),
            "--json".to_owned(),
        ])
        .unwrap();
        let CliCommand::Context(options) = command else {
            panic!("expected context command");
        };
        assert_eq!(options.path.as_deref(), Some(Path::new("/code/project")));
        assert_eq!(
            options.database.as_deref(),
            Some(Path::new("/tmp/aqm.sqlite3"))
        );
        assert!(options.json);
    }

    #[test]
    fn admit_requires_provider_and_parses_explicit_override() {
        assert!(parse_args(vec!["admit".to_owned()])
            .unwrap_err()
            .contains("requires a provider"));

        let command = parse_args(vec![
            "admit".to_owned(),
            "codex".to_owned(),
            "--yes".to_owned(),
            "--json".to_owned(),
        ])
        .unwrap();
        let CliCommand::Admit {
            provider_id,
            assume_yes,
            options,
        } = command
        else {
            panic!("expected admit command");
        };
        assert_eq!(provider_id, "codex");
        assert!(assume_yes);
        assert!(options.json);
    }

    #[test]
    fn admission_exit_codes_form_a_stable_shell_contract() {
        assert_eq!(
            admission_exit_code(EnforcementDecision::Allow, false),
            EXIT_ALLOW
        );
        assert_eq!(
            admission_exit_code(EnforcementDecision::Warn, false),
            EXIT_WARN
        );
        assert_eq!(
            admission_exit_code(EnforcementDecision::RequireConfirmation, false),
            EXIT_CONFIRM
        );
        assert_eq!(
            admission_exit_code(EnforcementDecision::RequireConfirmation, true),
            EXIT_ALLOW
        );
        assert_eq!(
            admission_exit_code(EnforcementDecision::Stop, true),
            EXIT_STOP
        );
    }
}
