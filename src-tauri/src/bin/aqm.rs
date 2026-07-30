use std::{
    env, fs,
    path::PathBuf,
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};

use agent_quota_manager_lib::{
    application::{BindRepository, GetRepositoryContext, QuotaService, RepositoryContext},
    paths,
    repository::resolve_git_root,
};

#[derive(Debug)]
enum CliCommand {
    Context(CommonOptions),
    Bind {
        options: CommonOptions,
        scope_reference: String,
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
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("aqm: {message}");
            ExitCode::from(1)
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    match parse_args(args)? {
        CliCommand::Help => {
            print_help();
            Ok(())
        }
        CliCommand::Context(options) => {
            let (mut service, canonical_root) = open_context(&options)?;
            let context = service
                .repository_context(GetRepositoryContext {
                    canonical_root,
                    at: now_millis()?,
                })
                .map_err(|error| error.to_string())?;
            print_context(&context, options.json)
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
            print_context(&context, options.json)
        }
    }
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
    if !matches!(command, "context" | "bind") {
        return Err(format!("unknown command {command:?}; run `aqm --help`"));
    }

    let mut options = CommonOptions::default();
    let mut scope_reference = None;
    let mut index = 1;
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
            "--json" => options.json = true,
            "-h" | "--help" => return Ok(CliCommand::Help),
            option => return Err(format!("unknown option {option:?}")),
        }
        index += 1;
    }

    let options = options;
    if command == "context" {
        Ok(CliCommand::Context(options))
    } else {
        Ok(CliCommand::Bind {
            options,
            scope_reference: scope_reference
                .ok_or_else(|| "`aqm bind` requires `--scope <name-or-id>`".to_owned())?,
        })
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

Options:
  --path <directory>   Resolve a repository from this directory instead of cwd
  --database <path>    Override the local database (or set AQM_DATABASE_PATH)
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
}
