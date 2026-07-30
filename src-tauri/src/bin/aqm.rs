use std::{
    env, fs, io,
    path::PathBuf,
    process::{Child, Command, ExitCode, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicI32, Ordering},
        Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use agent_quota_manager_lib::{
    application::{
        AdmissionAssessment, BindWorkspace, EvaluateWorkspaceAdmission, FinishManagedSession,
        GetWorkspaceContext, GetWorkspacePolicy, ManagedSessionOutcome,
        ManagedSessionReconciliation, ManagedSessionReconciliationStatus,
        MarkManagedSessionRunning, PrepareManagedSession, QuotaService, ResetWorkspacePolicy,
        SetWorkspacePolicy, WorkspaceContext, WorkspacePolicySummary,
    },
    domain::EnforcementDecision,
    paths,
    providers::{
        codex::{self, CodexSyncResult, CodexSyncStatus},
        codex_hooks::{self, CodexHookEvent, CodexHookEventKind},
    },
    workspace::canonicalize_workspace_path,
};
use serde::Serialize;

const EXIT_ALLOW: u8 = 0;
const EXIT_ERROR: u8 = 1;
const EXIT_WARN: u8 = 10;
const EXIT_CONFIRM: u8 = 20;
const EXIT_STOP: u8 = 30;
const SESSION_RESERVATION_MILLIS: i64 = 24 * 60 * 60 * 1_000;
const MANAGED_SESSION_ENV: &str = "AQM_MANAGED_SESSION_ID";
static FORWARDED_SIGNAL: AtomicI32 = AtomicI32::new(0);
static SIGNAL_HANDLER_LOCK: Mutex<()> = Mutex::new(());

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
    RunCodex {
        options: CommonOptions,
        assume_yes: bool,
        agent_args: Vec<String>,
    },
    HookCodex(CommonOptions),
    Hooks {
        action: HooksAction,
    },
    Policy {
        action: PolicyAction,
    },
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HooksAction {
    Install,
    Status,
    Uninstall,
}

#[derive(Debug)]
enum PolicyAction {
    Show(CommonOptions),
    Set {
        options: CommonOptions,
        warn_at_basis_points: Option<u16>,
        confirm_at_basis_points: Option<u16>,
        stop_at_basis_points: Option<u16>,
    },
    Reset(CommonOptions),
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
            let (mut service, canonical_path) = open_context(&options)?;
            let context = service
                .workspace_context(GetWorkspaceContext {
                    canonical_path,
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
            let (mut service, canonical_path) = open_context(&options)?;
            service
                .bind_workspace(BindWorkspace {
                    canonical_path: canonical_path.clone(),
                    scope_reference,
                    bound_at: now_millis()?,
                })
                .map_err(|error| error.to_string())?;
            let context = service
                .workspace_context(GetWorkspaceContext {
                    canonical_path,
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
            let (mut service, canonical_path) = open_context(&options)?;
            let now = now_millis()?;
            let context = service
                .workspace_context(GetWorkspaceContext {
                    canonical_path: canonical_path.clone(),
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
                .evaluate_workspace_admission(EvaluateWorkspaceAdmission {
                    canonical_path,
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
        CliCommand::RunCodex {
            options,
            assume_yes,
            agent_args,
        } => run_managed_codex(options, assume_yes, agent_args),
        CliCommand::HookCodex(options) => {
            // A tracking hook must never break the Codex turn it observes.
            // Diagnostics are opt-in so normal Codex sessions remain quiet.
            if let Err(error) = run_codex_hook(&options) {
                if env::var_os("AQM_HOOK_DEBUG").is_some() {
                    eprintln!("aqm hook: {error}");
                }
            }
            println!("{{}}");
            Ok(EXIT_ALLOW)
        }
        CliCommand::Hooks { action } => {
            let config_path = codex_hooks::default_user_hooks_path()?;
            match action {
                HooksAction::Install => {
                    let executable = env::current_exe()
                        .map_err(|error| format!("cannot resolve aqm executable: {error}"))?;
                    let changed = codex_hooks::install_user_hooks(&config_path, &executable)?;
                    if changed {
                        println!(
                            "Installed Codex tracking hooks in {}",
                            config_path.display()
                        );
                    } else {
                        println!(
                            "Codex tracking hooks are already installed in {}",
                            config_path.display()
                        );
                    }
                    println!("Review and trust the new hooks with `/hooks` in Codex.");
                }
                HooksAction::Status => {
                    if codex_hooks::user_hooks_installed(&config_path)? {
                        println!(
                            "Codex tracking hooks are configured in {}",
                            config_path.display()
                        );
                    } else {
                        println!(
                            "Codex tracking hooks are not configured in {}",
                            config_path.display()
                        );
                    }
                }
                HooksAction::Uninstall => {
                    if codex_hooks::uninstall_user_hooks(&config_path)? {
                        println!(
                            "Removed Codex tracking hooks from {}",
                            config_path.display()
                        );
                    } else {
                        println!(
                            "No Codex tracking hooks were found in {}",
                            config_path.display()
                        );
                    }
                }
            }
            Ok(EXIT_ALLOW)
        }
        CliCommand::Policy { action } => run_policy_command(action),
    }
}

fn run_policy_command(action: PolicyAction) -> Result<u8, String> {
    match action {
        PolicyAction::Show(options) => {
            let (mut service, canonical_path) = open_context(&options)?;
            let summary = service
                .workspace_policy(GetWorkspacePolicy {
                    canonical_path,
                    at: now_millis()?,
                })
                .map_err(|error| error.to_string())?;
            print_policy(&summary, options.json)?;
        }
        PolicyAction::Set {
            options,
            warn_at_basis_points,
            confirm_at_basis_points,
            stop_at_basis_points,
        } => {
            let (mut service, canonical_path) = open_context(&options)?;
            let at = now_millis()?;
            let workspace = service
                .workspace_policy(GetWorkspacePolicy { canonical_path, at })
                .map_err(|error| error.to_string())?;
            service
                .set_workspace_policy(SetWorkspacePolicy {
                    scope_id: workspace.scope_id,
                    warn_at_basis_points,
                    confirm_at_basis_points,
                    stop_at_basis_points,
                    updated_at: at,
                })
                .map_err(|error| error.to_string())?;
            let updated = service
                .workspace_policy(GetWorkspacePolicy {
                    canonical_path: workspace.canonical_path,
                    at,
                })
                .map_err(|error| error.to_string())?;
            print_policy(&updated, options.json)?;
        }
        PolicyAction::Reset(options) => {
            let (mut service, canonical_path) = open_context(&options)?;
            let at = now_millis()?;
            let workspace = service
                .workspace_policy(GetWorkspacePolicy { canonical_path, at })
                .map_err(|error| error.to_string())?;
            service
                .reset_workspace_policy(ResetWorkspacePolicy {
                    scope_id: workspace.scope_id,
                })
                .map_err(|error| error.to_string())?;
            let reset = service
                .workspace_policy(GetWorkspacePolicy {
                    canonical_path: workspace.canonical_path,
                    at,
                })
                .map_err(|error| error.to_string())?;
            print_policy(&reset, options.json)?;
        }
    }

    Ok(EXIT_ALLOW)
}

fn run_managed_codex(
    options: CommonOptions,
    assume_yes: bool,
    agent_args: Vec<String>,
) -> Result<u8, String> {
    let executable = codex::resolve_executable().ok_or_else(|| {
        "Codex CLI was not found; install Codex or set AGENT_QUOTA_CODEX_BIN".to_owned()
    })?;
    let (mut service, canonical_path) = open_context(&options)?;
    let now = now_millis()?;
    recover_orphaned_sessions(&mut service, now)?;

    let context = service
        .workspace_context(GetWorkspaceContext {
            canonical_path: canonical_path.clone(),
            at: now,
        })
        .map_err(|error| error.to_string())?;
    let allocation = provider_allocation(&context, "codex")?;
    let checkpoint = codex::sync_detection(
        &mut service,
        allocation.window_id.clone(),
        now,
        codex::detect(),
    );
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

    let identity = managed_session_identity(now);
    let expires_at = now
        .checked_add(SESSION_RESERVATION_MILLIS)
        .ok_or_else(|| "managed session reservation expiry overflowed".to_owned())?;
    let launch = service
        .prepare_managed_session(PrepareManagedSession {
            id: identity.0.clone(),
            reservation_id: identity.1,
            canonical_path: canonical_path.clone(),
            provider_id: "codex".to_owned(),
            assume_yes,
            admitted_at: now,
            expires_at,
            supervisor_pid: std::process::id(),
        })
        .map_err(|error| error.to_string())?;

    println!(
        "AQM: launching Codex for {} with {} {} reserved",
        launch.assessment.scope_display_name, launch.reserved_amount, launch.assessment.unit
    );
    if launch.assessment.decision == EnforcementDecision::Warn {
        println!("AQM warning: this workspace is approaching its policy boundary.");
    }

    let mut child = match Command::new(&executable)
        .args(agent_args)
        .current_dir(&canonical_path)
        .env(MANAGED_SESSION_ENV, &launch.session_id)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            finish_session_after_error(
                &mut service,
                &launch.session_id,
                ManagedSessionOutcome::Failed,
                None,
            )?;
            return Err(format!(
                "cannot launch Codex from {}: {error}",
                executable.display()
            ));
        }
    };

    let started_at = match now_millis() {
        Ok(started_at) => started_at,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            finish_session_after_error(
                &mut service,
                &launch.session_id,
                ManagedSessionOutcome::Interrupted,
                None,
            )?;
            return Err(error);
        }
    };
    if let Err(error) = service.mark_managed_session_running(MarkManagedSessionRunning {
        id: launch.session_id.clone(),
        child_pid: child.id(),
        started_at,
    }) {
        let _ = child.kill();
        let _ = child.wait();
        finish_session_after_error(
            &mut service,
            &launch.session_id,
            ManagedSessionOutcome::Interrupted,
            None,
        )?;
        return Err(format!(
            "cannot mark managed Codex session as running: {error}"
        ));
    }

    let process = match wait_for_managed_child(&mut child) {
        Ok(process) => process,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            finish_session_after_error(
                &mut service,
                &launch.session_id,
                ManagedSessionOutcome::Interrupted,
                None,
            )?;
            return Err(error);
        }
    };
    let finished_at = now_millis()?;
    let current_window_id =
        refresh_managed_checkpoint(&mut service, &launch.assessment.window_id, finished_at);
    let reconciliation = service
        .finish_managed_session(FinishManagedSession {
            id: launch.session_id,
            outcome: process.outcome,
            finished_at,
            exit_code: process.provider_exit_code,
            current_window_id,
        })
        .map_err(|error| error.to_string())?;
    print_managed_reconciliation(&reconciliation, &launch.assessment.unit);
    Ok(process.shell_exit_code)
}

fn refresh_managed_checkpoint(
    service: &mut QuotaService,
    baseline_window_id: &str,
    observed_at: i64,
) -> Option<String> {
    let checkpoint = codex::sync_detection(
        service,
        baseline_window_id.to_owned(),
        observed_at,
        codex::detect(),
    );
    if checkpoint.status == CodexSyncStatus::Synced {
        return checkpoint.window_id;
    }
    eprintln!(
        "AQM: Codex exited, but its final quota checkpoint is unavailable: {}",
        checkpoint
            .message
            .as_deref()
            .unwrap_or("provider temporarily unavailable")
    );
    None
}

fn print_managed_reconciliation(reconciliation: &ManagedSessionReconciliation, unit: &str) {
    match reconciliation.status {
        ManagedSessionReconciliationStatus::Attributed => println!(
            "AQM: attributed {} {} to this workspace",
            reconciliation.amount.unwrap_or(0),
            unit
        ),
        ManagedSessionReconciliationStatus::NoUsage => {
            println!("AQM: provider checkpoint did not change during this session")
        }
        ManagedSessionReconciliationStatus::Ambiguous => println!(
            "AQM: kept {} {} unattributed because concurrent usage was observed",
            reconciliation.amount.unwrap_or(0),
            unit
        ),
        ManagedSessionReconciliationStatus::WindowRolledOver => {
            println!("AQM: quota window rolled over; no cross-window usage was attributed")
        }
        ManagedSessionReconciliationStatus::SnapshotUnavailable => {
            println!("AQM: session ended without a reconcilable provider checkpoint")
        }
    }
}

fn finish_session_after_error(
    service: &mut QuotaService,
    session_id: &str,
    outcome: ManagedSessionOutcome,
    exit_code: Option<i32>,
) -> Result<(), String> {
    service
        .finish_managed_session(FinishManagedSession {
            id: session_id.to_owned(),
            outcome,
            finished_at: now_millis()?,
            exit_code,
            current_window_id: None,
        })
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn recover_orphaned_sessions(service: &mut QuotaService, recovered_at: i64) -> Result<(), String> {
    for session in service
        .active_managed_sessions()
        .map_err(|error| error.to_string())?
    {
        if process_is_running(session.supervisor_pid) {
            continue;
        }
        service
            .finish_managed_session(FinishManagedSession {
                id: session.session_id,
                outcome: ManagedSessionOutcome::Interrupted,
                finished_at: recovered_at,
                exit_code: None,
                current_window_id: None,
            })
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

struct ManagedProcessResult {
    outcome: ManagedSessionOutcome,
    provider_exit_code: Option<i32>,
    shell_exit_code: u8,
}

fn wait_for_managed_child(child: &mut Child) -> Result<ManagedProcessResult, String> {
    let _signal_guard = SIGNAL_HANDLER_LOCK
        .lock()
        .map_err(|_| "managed process signal handler lock is poisoned".to_owned())?;
    install_signal_forwarding();
    loop {
        let status = match child.try_wait() {
            Ok(status) => status,
            Err(error) => {
                reset_signal_forwarding();
                return Err(format!("cannot read Codex process status: {error}"));
            }
        };
        if let Some(status) = status {
            reset_signal_forwarding();
            return Ok(process_result(status, None));
        }

        let signal = FORWARDED_SIGNAL.swap(0, Ordering::SeqCst);
        if signal != 0 {
            forward_signal(child, signal);
            let status = match child.wait() {
                Ok(status) => status,
                Err(error) => {
                    reset_signal_forwarding();
                    return Err(format!(
                        "cannot wait for interrupted Codex process: {error}"
                    ));
                }
            };
            reset_signal_forwarding();
            return Ok(process_result(status, Some(signal)));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn process_result(status: ExitStatus, forwarded_signal: Option<i32>) -> ManagedProcessResult {
    if let Some(signal) = forwarded_signal {
        return ManagedProcessResult {
            outcome: ManagedSessionOutcome::Interrupted,
            provider_exit_code: status.code(),
            shell_exit_code: u8::try_from(128_i32.saturating_add(signal)).unwrap_or(EXIT_ERROR),
        };
    }
    match status.code() {
        Some(0) => ManagedProcessResult {
            outcome: ManagedSessionOutcome::Completed,
            provider_exit_code: Some(0),
            shell_exit_code: EXIT_ALLOW,
        },
        Some(code) => ManagedProcessResult {
            outcome: ManagedSessionOutcome::Failed,
            provider_exit_code: Some(code),
            shell_exit_code: u8::try_from(code).unwrap_or(EXIT_ERROR),
        },
        None => ManagedProcessResult {
            outcome: ManagedSessionOutcome::Interrupted,
            provider_exit_code: None,
            shell_exit_code: 130,
        },
    }
}

extern "C" fn capture_signal(signal: i32) {
    FORWARDED_SIGNAL.store(signal, Ordering::SeqCst);
}

#[cfg(unix)]
fn install_signal_forwarding() {
    FORWARDED_SIGNAL.store(0, Ordering::SeqCst);
    unsafe {
        libc::signal(
            libc::SIGINT,
            capture_signal as *const () as libc::sighandler_t,
        );
        libc::signal(
            libc::SIGTERM,
            capture_signal as *const () as libc::sighandler_t,
        );
    }
}

#[cfg(not(unix))]
fn install_signal_forwarding() {
    FORWARDED_SIGNAL.store(0, Ordering::SeqCst);
}

#[cfg(unix)]
fn reset_signal_forwarding() {
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_DFL);
        libc::signal(libc::SIGTERM, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn reset_signal_forwarding() {}

#[cfg(unix)]
fn forward_signal(child: &Child, signal: i32) {
    unsafe {
        libc::kill(child.id() as i32, signal);
    }
}

#[cfg(not(unix))]
fn forward_signal(child: &mut Child, _signal: i32) {
    let _ = child.kill();
}

#[cfg(unix)]
fn process_is_running(pid: u32) -> bool {
    let result = unsafe { libc::kill(pid as i32, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn process_is_running(_pid: u32) -> bool {
    true
}

fn managed_session_identity(now: i64) -> (String, String) {
    let session_id = format!("managed-codex-{}-{now}", std::process::id());
    let reservation_id = format!("{session_id}-reservation");
    (session_id, reservation_id)
}

fn run_codex_hook(options: &CommonOptions) -> Result<(), String> {
    if env::var_os(MANAGED_SESSION_ENV).is_some() {
        return Ok(());
    }
    let event = CodexHookEvent::from_reader(io::stdin().lock())?;
    let mut service = open_service(options)?;
    codex_hooks::handle_event(&mut service, &event, now_millis()?, || {
        matches!(
            event.kind(),
            CodexHookEventKind::UserPromptSubmit | CodexHookEventKind::Stop
        )
        .then(codex::detect)
    })?;
    Ok(())
}

fn provider_allocation<'a>(
    context: &'a WorkspaceContext,
    provider_id: &str,
) -> Result<&'a agent_quota_manager_lib::application::WorkspaceAllocationContext, String> {
    let binding = context.binding.as_ref().ok_or_else(|| {
        format!(
            "workspace {} is not bound; run `aqm bind --scope <name-or-id>` first",
            context.canonical_path
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
    let canonical_path = canonicalize_workspace_path(start).map_err(|error| error.to_string())?;
    let service = open_service(options)?;
    Ok((service, canonical_path))
}

fn open_service(options: &CommonOptions) -> Result<QuotaService, String> {
    let database_path = database_path(options)?;
    if let Some(parent) = database_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create app data directory: {error}"))?;
    }
    QuotaService::open(database_path).map_err(|error| error.to_string())
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
    if command == "hook" {
        return parse_hook_command(&args);
    }
    if command == "hooks" {
        return parse_hooks_command(&args);
    }
    if command == "run" {
        return parse_run_command(&args);
    }
    if command == "policy" {
        return parse_policy_command(&args);
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

fn parse_run_command(args: &[String]) -> Result<CliCommand, String> {
    if args.get(1).map(String::as_str) != Some("codex") {
        return Err("usage: aqm run codex [--path <directory>] [--yes] -- [codex args]".to_owned());
    }
    let mut options = CommonOptions::default();
    let mut assume_yes = false;
    let mut index = 2;
    while index < args.len() {
        match args[index].as_str() {
            "--" => {
                return Ok(CliCommand::RunCodex {
                    options,
                    assume_yes,
                    agent_args: args[index + 1..].to_vec(),
                });
            }
            "--path" => {
                index += 1;
                options.path = Some(PathBuf::from(required_value(args, index, "--path")?));
            }
            "--database" => {
                index += 1;
                options.database = Some(PathBuf::from(required_value(args, index, "--database")?));
            }
            "--yes" => assume_yes = true,
            "-h" | "--help" => return Ok(CliCommand::Help),
            option => {
                return Err(format!(
                    "unknown aqm run option {option:?}; place Codex arguments after --"
                ))
            }
        }
        index += 1;
    }
    Ok(CliCommand::RunCodex {
        options,
        assume_yes,
        agent_args: Vec::new(),
    })
}

fn parse_hook_command(args: &[String]) -> Result<CliCommand, String> {
    if args.get(1).map(String::as_str) != Some("codex") {
        return Err("usage: aqm hook codex [--database <path>]".to_owned());
    }
    let mut options = CommonOptions::default();
    let mut index = 2;
    while index < args.len() {
        match args[index].as_str() {
            "--database" => {
                index += 1;
                options.database = Some(PathBuf::from(required_value(args, index, "--database")?));
            }
            "-h" | "--help" => return Ok(CliCommand::Help),
            option => return Err(format!("unknown option {option:?}")),
        }
        index += 1;
    }
    Ok(CliCommand::HookCodex(options))
}

fn parse_hooks_command(args: &[String]) -> Result<CliCommand, String> {
    let action = match args.get(1).map(String::as_str) {
        Some("install") => HooksAction::Install,
        Some("status") => HooksAction::Status,
        Some("uninstall") => HooksAction::Uninstall,
        _ => return Err("usage: aqm hooks <install|status|uninstall> codex".to_owned()),
    };
    if args.get(2).map(String::as_str) != Some("codex") || args.len() != 3 {
        return Err("usage: aqm hooks <install|status|uninstall> codex".to_owned());
    }
    Ok(CliCommand::Hooks { action })
}

fn parse_policy_command(args: &[String]) -> Result<CliCommand, String> {
    let action = args
        .get(1)
        .map(String::as_str)
        .ok_or_else(|| "usage: aqm policy <show|set|reset> [options]".to_owned())?;
    if !matches!(action, "show" | "set" | "reset") {
        return Err("usage: aqm policy <show|set|reset> [options]".to_owned());
    }

    let mut options = CommonOptions::default();
    let mut warn = None;
    let mut confirm = None;
    let mut stop = None;
    let mut index = 2;
    while index < args.len() {
        match args[index].as_str() {
            "--path" => {
                index += 1;
                options.path = Some(PathBuf::from(required_value(args, index, "--path")?));
            }
            "--database" => {
                index += 1;
                options.database = Some(PathBuf::from(required_value(args, index, "--database")?));
            }
            "--json" => options.json = true,
            "--warn" if action == "set" => {
                index += 1;
                warn = Some(parse_policy_threshold(required_value(
                    args, index, "--warn",
                )?)?);
            }
            "--confirm" if action == "set" => {
                index += 1;
                confirm = Some(parse_policy_threshold(required_value(
                    args,
                    index,
                    "--confirm",
                )?)?);
            }
            "--stop" if action == "set" => {
                index += 1;
                stop = Some(parse_policy_threshold(required_value(
                    args, index, "--stop",
                )?)?);
            }
            "-h" | "--help" => return Ok(CliCommand::Help),
            option => return Err(format!("unknown policy option {option:?}")),
        }
        index += 1;
    }

    let action = match action {
        "show" => PolicyAction::Show(options),
        "set" => PolicyAction::Set {
            options,
            warn_at_basis_points: warn
                .ok_or_else(|| "`aqm policy set` requires `--warn <percent|off>`".to_owned())?,
            confirm_at_basis_points: confirm
                .ok_or_else(|| "`aqm policy set` requires `--confirm <percent|off>`".to_owned())?,
            stop_at_basis_points: stop
                .ok_or_else(|| "`aqm policy set` requires `--stop <percent|off>`".to_owned())?,
        },
        "reset" => PolicyAction::Reset(options),
        _ => unreachable!("policy action was validated"),
    };
    Ok(CliCommand::Policy { action })
}

fn parse_policy_threshold(value: &str) -> Result<Option<u16>, String> {
    if value.eq_ignore_ascii_case("off") {
        return Ok(None);
    }

    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if whole.is_empty()
        || fraction.len() > 2
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(format!(
            "invalid policy threshold {value:?}; use 0.01..100 or off"
        ));
    }
    let whole = whole
        .parse::<u16>()
        .map_err(|_| format!("invalid policy threshold {value:?}; use 0.01..100 or off"))?;
    let fraction = match fraction.len() {
        0 => 0,
        1 => fraction.parse::<u16>().unwrap_or(0) * 10,
        2 => fraction.parse::<u16>().unwrap_or(0),
        _ => unreachable!("fraction length was validated"),
    };
    let basis_points = whole
        .checked_mul(100)
        .and_then(|whole| whole.checked_add(fraction))
        .filter(|value| (1..=10_000).contains(value))
        .ok_or_else(|| format!("invalid policy threshold {value:?}; use 0.01..100 or off"))?;
    Ok(Some(basis_points))
}

fn required_value<'a>(args: &'a [String], index: usize, option: &str) -> Result<&'a str, String> {
    args.get(index)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{option} requires a value"))
}

fn print_context(context: &WorkspaceContext, json: bool) -> Result<(), String> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(context)
                .map_err(|error| format!("cannot serialize context: {error}"))?
        );
        return Ok(());
    }

    println!("Workspace path: {}", context.canonical_path);
    match &context.binding {
        Some(binding) => {
            println!(
                "Workspace: {} ({})",
                binding.scope_display_name, binding.scope_id
            );
            println!("Bound folder: {}", binding.canonical_path);
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
            println!("Workspace: unmapped");
            if context.available_workspace_scopes.is_empty() {
                println!("No unbound workspace scopes are available.");
                println!("Create a workspace allocation in the desktop app first.");
            } else {
                println!("Available workspace scopes:");
                for scope in &context.available_workspace_scopes {
                    println!("  {} ({})", scope.display_name, scope.id);
                }
                println!("Bind with: aqm bind --scope <name-or-id>");
            }
        }
    }
    Ok(())
}

fn print_policy(summary: &WorkspacePolicySummary, json: bool) -> Result<(), String> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(summary)
                .map_err(|error| format!("cannot serialize policy: {error}"))?
        );
        return Ok(());
    }

    println!(
        "Workspace: {} ({})",
        summary.scope_display_name, summary.scope_id
    );
    println!("Path: {}", summary.canonical_path);
    println!(
        "Policy: {}",
        if summary.policy.customized {
            "workspace override"
        } else {
            "default"
        }
    );
    println!(
        "Warn: {}",
        format_policy_threshold(summary.policy.warn_at_basis_points)
    );
    println!(
        "Confirm: {}",
        format_policy_threshold(summary.policy.confirm_at_basis_points)
    );
    println!(
        "Stop: {}",
        format_policy_threshold(summary.policy.stop_at_basis_points)
    );
    Ok(())
}

fn format_policy_threshold(value: Option<u16>) -> String {
    match value {
        Some(value) if value % 100 == 0 => format!("{}%", value / 100),
        Some(value) => format!("{}.{:02}%", value / 100, value % 100),
        None => "off".to_owned(),
    }
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
        "Workspace: {} ({})",
        assessment.scope_display_name, assessment.canonical_path
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
  aqm run codex [--path <directory>] [--yes] -- [codex args]
  aqm policy show [--path <directory>] [--json]
  aqm policy set --warn <percent|off> --confirm <percent|off> --stop <percent|off>
  aqm policy reset [--path <directory>] [--json]
  aqm hook codex [--database <path>]
  aqm hooks <install|status|uninstall> codex

Options:
  --path <directory>   Resolve a workspace from this directory instead of cwd
  --database <path>    Override the local database (or set AQM_DATABASE_PATH)
  --yes                Explicitly accept a confirmation-required admission
  percent|off          Percentage of a workspace allocation consumed, or disabled
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

    #[test]
    fn managed_run_parses_aqm_options_and_preserves_codex_arguments() {
        let command = parse_args(vec![
            "run".to_owned(),
            "codex".to_owned(),
            "--path".to_owned(),
            "/code/project".to_owned(),
            "--yes".to_owned(),
            "--".to_owned(),
            "--model".to_owned(),
            "gpt-5".to_owned(),
            "fix the tests".to_owned(),
        ])
        .unwrap();
        let CliCommand::RunCodex {
            options,
            assume_yes,
            agent_args,
        } = command
        else {
            panic!("expected managed Codex run");
        };

        assert_eq!(options.path.as_deref(), Some(Path::new("/code/project")));
        assert!(assume_yes);
        assert_eq!(
            agent_args,
            ["--model", "gpt-5", "fix the tests"].map(str::to_owned)
        );
    }

    #[cfg(unix)]
    #[test]
    fn managed_process_preserves_success_and_failure_exit_codes() {
        let mut successful = Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn()
            .unwrap();
        let success = wait_for_managed_child(&mut successful).unwrap();
        assert_eq!(success.outcome, ManagedSessionOutcome::Completed);
        assert_eq!(success.provider_exit_code, Some(0));
        assert_eq!(success.shell_exit_code, 0);

        let mut failed = Command::new("/bin/sh")
            .args(["-c", "exit 7"])
            .spawn()
            .unwrap();
        let failure = wait_for_managed_child(&mut failed).unwrap();
        assert_eq!(failure.outcome, ManagedSessionOutcome::Failed);
        assert_eq!(failure.provider_exit_code, Some(7));
        assert_eq!(failure.shell_exit_code, 7);
    }

    #[cfg(unix)]
    #[test]
    fn managed_process_reports_signal_termination_as_interrupted() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "sleep 5"])
            .spawn()
            .unwrap();
        let signal = thread::spawn(|| {
            thread::sleep(Duration::from_millis(100));
            FORWARDED_SIGNAL.store(libc::SIGTERM, Ordering::SeqCst);
        });

        let result = wait_for_managed_child(&mut child).unwrap();
        signal.join().unwrap();

        assert_eq!(result.outcome, ManagedSessionOutcome::Interrupted);
        assert_eq!(result.provider_exit_code, None);
        assert_eq!(result.shell_exit_code, 143);
    }

    #[cfg(unix)]
    #[test]
    fn orphan_detection_distinguishes_running_and_finished_supervisors() {
        let mut supervisor = Command::new("/bin/sh")
            .args(["-c", "sleep 1"])
            .spawn()
            .unwrap();
        let supervisor_pid = supervisor.id();

        assert!(process_is_running(supervisor_pid));
        supervisor.wait().unwrap();
        assert!(!process_is_running(supervisor_pid));
    }

    #[test]
    fn hook_commands_parse_without_exposing_hook_payload_options() {
        let command = parse_args(vec![
            "hook".to_owned(),
            "codex".to_owned(),
            "--database".to_owned(),
            "/tmp/aqm.sqlite3".to_owned(),
        ])
        .unwrap();
        let CliCommand::HookCodex(options) = command else {
            panic!("expected Codex hook command");
        };
        assert_eq!(
            options.database.as_deref(),
            Some(Path::new("/tmp/aqm.sqlite3"))
        );

        assert!(matches!(
            parse_args(vec![
                "hooks".to_owned(),
                "install".to_owned(),
                "codex".to_owned()
            ])
            .unwrap(),
            CliCommand::Hooks {
                action: HooksAction::Install
            }
        ));
    }

    #[test]
    fn policy_set_parses_decimal_percentages_and_disabled_thresholds() {
        let command = parse_args(vec![
            "policy".to_owned(),
            "set".to_owned(),
            "--warn".to_owned(),
            "75.5".to_owned(),
            "--confirm".to_owned(),
            "off".to_owned(),
            "--stop".to_owned(),
            "100".to_owned(),
            "--path".to_owned(),
            "/code/project".to_owned(),
        ])
        .unwrap();
        let CliCommand::Policy {
            action:
                PolicyAction::Set {
                    options,
                    warn_at_basis_points,
                    confirm_at_basis_points,
                    stop_at_basis_points,
                },
        } = command
        else {
            panic!("expected policy set command");
        };

        assert_eq!(options.path.as_deref(), Some(Path::new("/code/project")));
        assert_eq!(warn_at_basis_points, Some(7_550));
        assert_eq!(confirm_at_basis_points, None);
        assert_eq!(stop_at_basis_points, Some(10_000));
    }

    #[test]
    fn policy_threshold_parser_rejects_out_of_range_or_over_precise_values() {
        for value in ["0", "100.01", "80.001", "-1", "wat"] {
            assert!(
                parse_policy_threshold(value).is_err(),
                "{value} should be invalid"
            );
        }
        assert_eq!(parse_policy_threshold("0.01").unwrap(), Some(1));
        assert_eq!(parse_policy_threshold("OFF").unwrap(), None);
    }
}
