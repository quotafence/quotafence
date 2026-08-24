use std::{
    env, fs,
    io::{self, IsTerminal},
    path::PathBuf,
    process::{Child, Command, ExitCode, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicI32, Ordering},
        Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use quotafence_lib::{
    application::{
        AdmissionAssessment, BindWorkspace, EvaluateWorkspaceAdmission, FinishManagedSession,
        GetLocalState, GetQuotaDashboard, GetWorkspaceContext, GetWorkspacePolicy, LocalState,
        ManagedSessionOutcome, ManagedSessionReconciliation, ManagedSessionReconciliationStatus,
        MarkManagedSessionRunning, PrepareManagedSession, QuotaService, QuotaSourceSummary,
        ResetWorkspacePolicy, SetWorkspacePolicy, WorkspaceContext, WorkspacePolicySummary,
    },
    domain::EnforcementDecision,
    paths,
    providers::{
        claude_code, claude_hooks,
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
const MANAGED_SESSION_ENV: &str = "QUOTAFENCE_MANAGED_SESSION_ID";
static FORWARDED_SIGNAL: AtomicI32 = AtomicI32::new(0);
static SIGNAL_HANDLER_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug)]
enum CliCommand {
    Status(CommonOptions),
    Sync(CommonOptions),
    Sources {
        options: CommonOptions,
        query: Option<String>,
    },
    Allocations(CommonOptions),
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
    RunAgent {
        agent: ManagedAgent,
        claude_window: ClaudeManagedWindow,
        options: CommonOptions,
        assume_yes: bool,
        agent_args: Vec<String>,
    },
    HookCodex(CommonOptions),
    HookClaude,
    Hooks {
        action: HooksAction,
        provider: HookProvider,
    },
    Policy {
        action: PolicyAction,
    },
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagedAgent {
    Codex,
    Claude,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaudeManagedWindow {
    Weekly,
    FiveHour,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HooksAction {
    Install,
    Status,
    Uninstall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HookProvider {
    Codex,
    Claude,
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
            eprintln!("quotafence: {message}");
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
        CliCommand::Status(options) => {
            let mut service = open_service(&options)?;
            let now = now_millis()?;
            let sync_warnings = refresh_status_sources(&mut service, now)?;
            let state = service
                .local_state(GetLocalState {
                    selected_window_id: None,
                    at: now,
                })
                .map_err(|error| error.to_string())?;
            print_status(&state, &sync_warnings, now, options.json)?;
            Ok(EXIT_ALLOW)
        }
        CliCommand::Sync(options) => {
            let mut service = open_service(&options)?;
            let now = now_millis()?;
            let sync_warnings = refresh_status_sources(&mut service, now)?;
            let state = service
                .local_state(GetLocalState {
                    selected_window_id: None,
                    at: now,
                })
                .map_err(|error| error.to_string())?;
            print_status(&state, &sync_warnings, now, options.json)?;
            Ok(EXIT_ALLOW)
        }
        CliCommand::Sources { options, query } => {
            let mut service = open_service(&options)?;
            let mut state = service
                .local_state(GetLocalState {
                    selected_window_id: None,
                    at: now_millis()?,
                })
                .map_err(|error| error.to_string())?;
            if let Some(query) = query {
                let needle = query.to_ascii_lowercase();
                state.sources.retain(|source| {
                    source.provider_id.to_ascii_lowercase().contains(&needle)
                        || source
                            .provider_display_name
                            .to_ascii_lowercase()
                            .contains(&needle)
                        || source
                            .pool_display_name
                            .to_ascii_lowercase()
                            .contains(&needle)
                        || source.window_id.to_ascii_lowercase() == needle
                });
                if state.sources.is_empty() {
                    return Err(format!("no quota source matches {query:?}"));
                }
            }
            print_sources(&state, now_millis()?, options.json)?;
            Ok(EXIT_ALLOW)
        }
        CliCommand::Allocations(options) => {
            let mut service = open_service(&options)?;
            let now = now_millis()?;
            let state = service
                .local_state(GetLocalState {
                    selected_window_id: None,
                    at: now,
                })
                .map_err(|error| error.to_string())?;
            print_allocations(&mut service, &state, now, options.json)?;
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
            let resolved_provider_id = allocation.provider_id.clone();
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
                    provider_id: resolved_provider_id,
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
        CliCommand::RunAgent {
            agent,
            claude_window,
            options,
            assume_yes,
            agent_args,
        } => run_managed_agent(agent, claude_window, options, assume_yes, agent_args),
        CliCommand::HookCodex(options) => {
            // Infrastructure failures remain fail-open. Explicit policy
            // outcomes may block UserPromptSubmit through the hook contract.
            let output = match run_codex_hook(&options) {
                Ok(output) => output,
                Err(error) => {
                    if env::var_os("QUOTAFENCE_HOOK_DEBUG").is_some() {
                        eprintln!("quotafence hook: {error}");
                    }
                    serde_json::json!({})
                }
            };
            println!("{output}");
            Ok(EXIT_ALLOW)
        }
        CliCommand::HookClaude => {
            let output = claude_hooks::run_installed_hook(io::stdin().lock())?;
            println!("{output}");
            Ok(EXIT_ALLOW)
        }
        CliCommand::Hooks { action, provider } => {
            if provider == HookProvider::Claude {
                let status = match action {
                    HooksAction::Install => {
                        let executable = env::current_exe().map_err(|error| {
                            format!("cannot resolve quotafence executable: {error}")
                        })?;
                        claude_hooks::install_protection(&executable)?
                    }
                    HooksAction::Status => claude_hooks::protection_status()?,
                    HooksAction::Uninstall => claude_hooks::uninstall_protection()?,
                };
                println!(
                    "Claude workspace protection: {} ({})",
                    if status.installed { "installed" } else { "off" },
                    status.config_path
                );
                if let Some(issue) = status.issue {
                    println!("Issue: {issue}");
                }
                return Ok(EXIT_ALLOW);
            }
            let config_path = codex_hooks::default_user_hooks_path()?;
            match action {
                HooksAction::Install => {
                    let executable = env::current_exe().map_err(|error| {
                        format!("cannot resolve quotafence executable: {error}")
                    })?;
                    let changed = codex_hooks::install_user_hooks(&config_path, &executable)?;
                    if changed {
                        println!(
                            "Installed Codex workspace protection hooks in {}",
                            config_path.display()
                        );
                    } else {
                        println!(
                            "Codex workspace protection hooks are already installed in {}",
                            config_path.display()
                        );
                    }
                    println!("Review and trust the new hooks with `/hooks` in Codex.");
                }
                HooksAction::Status => {
                    let status = codex_hooks::protection_status()?;
                    if status.installed {
                        println!(
                            "Codex workspace protection hooks are configured in {}",
                            config_path.display()
                        );
                    } else {
                        println!(
                            "Codex workspace protection hooks are not configured in {}",
                            config_path.display()
                        );
                    }
                    if let Some(issue) = status.issue {
                        println!("Issue: {issue}");
                    }
                }
                HooksAction::Uninstall => {
                    if codex_hooks::uninstall_user_hooks(&config_path)? {
                        println!(
                            "Removed Codex workspace protection hooks from {}",
                            config_path.display()
                        );
                    } else {
                        println!(
                            "No Codex workspace protection hooks were found in {}",
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

fn run_managed_agent(
    agent: ManagedAgent,
    claude_window: ClaudeManagedWindow,
    options: CommonOptions,
    assume_yes: bool,
    agent_args: Vec<String>,
) -> Result<u8, String> {
    let (agent_name, executable) = match agent {
        ManagedAgent::Codex => (
            "Codex",
            codex::resolve_executable().ok_or_else(|| {
                "Codex CLI was not found; install Codex or set AGENT_QUOTA_CODEX_BIN".to_owned()
            })?,
        ),
        ManagedAgent::Claude => (
            "Claude",
            claude_code::resolve_executable().ok_or_else(|| {
                "Claude Code CLI was not found; install Claude Code or set AGENT_QUOTA_CLAUDE_BIN"
                    .to_owned()
            })?,
        ),
    };
    let (mut service, canonical_path) = open_context(&options)?;
    let now = now_millis()?;
    recover_orphaned_sessions(&mut service, now)?;

    if agent == ManagedAgent::Claude {
        let observation = claude_code::fetch_subscription_usage()?;
        claude_code::ingest_observation(&mut service, &observation, now)?;
    }
    let context = service
        .workspace_context(GetWorkspaceContext {
            canonical_path: canonical_path.clone(),
            at: now,
        })
        .map_err(|error| error.to_string())?;
    let allocation = managed_allocation(&context, agent, claude_window)?;
    let resolved_provider_id = allocation.provider_id.clone();
    if agent == ManagedAgent::Codex {
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
    }

    let identity = managed_session_identity(agent_name, now);
    let expires_at = now
        .checked_add(SESSION_RESERVATION_MILLIS)
        .ok_or_else(|| "managed session reservation expiry overflowed".to_owned())?;
    let launch = service
        .prepare_managed_session(PrepareManagedSession {
            id: identity.0.clone(),
            reservation_id: identity.1,
            canonical_path: canonical_path.clone(),
            provider_id: resolved_provider_id,
            assume_yes,
            admitted_at: now,
            expires_at,
            supervisor_pid: std::process::id(),
        })
        .map_err(|error| error.to_string())?;

    println!(
        "QuotaFence: launching {agent_name} for {} with {} {} reserved ({})",
        launch.assessment.scope_display_name,
        launch.reserved_amount,
        launch.assessment.unit,
        allocation.pool_display_name
    );
    if launch.assessment.decision == EnforcementDecision::Warn {
        println!("QuotaFence warning: this workspace is approaching its policy boundary.");
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
                "cannot launch {agent_name} from {}: {error}",
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
            "cannot mark managed {agent_name} session as running: {error}"
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
    let current_window_id = refresh_managed_checkpoint(
        &mut service,
        agent,
        claude_window,
        &launch.assessment.window_id,
        finished_at,
    );
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
    agent: ManagedAgent,
    claude_window: ClaudeManagedWindow,
    baseline_window_id: &str,
    observed_at: i64,
) -> Option<String> {
    if agent == ManagedAgent::Claude {
        return match claude_code::fetch_subscription_usage().and_then(|observation| {
            claude_code::ingest_observation(service, &observation, observed_at)
        }) {
            Ok(sync) => sync
                .windows
                .into_iter()
                .find(|window| match claude_window {
                    ClaudeManagedWindow::Weekly => window.kind == "seven_day",
                    ClaudeManagedWindow::FiveHour => window.kind == "five_hour",
                })
                .map(|window| window.window_id),
            Err(error) => {
                eprintln!(
                    "QuotaFence: Claude exited, but its final quota checkpoint is unavailable: {error}"
                );
                None
            }
        };
    }
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
        "QuotaFence: Codex exited, but its final quota checkpoint is unavailable: {}",
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
            "QuotaFence: attributed {} {} to this workspace",
            reconciliation.amount.unwrap_or(0),
            unit
        ),
        ManagedSessionReconciliationStatus::NoUsage => {
            println!("QuotaFence: provider checkpoint did not change during this session")
        }
        ManagedSessionReconciliationStatus::Ambiguous => println!(
            "QuotaFence: kept {} {} unattributed because concurrent usage was observed",
            reconciliation.amount.unwrap_or(0),
            unit
        ),
        ManagedSessionReconciliationStatus::WindowRolledOver => {
            println!("QuotaFence: quota window rolled over; no cross-window usage was attributed")
        }
        ManagedSessionReconciliationStatus::SnapshotUnavailable => {
            println!("QuotaFence: session ended without a reconcilable provider checkpoint")
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
    wait_for_managed_child_after_signal_setup(child, || {})
}

fn wait_for_managed_child_after_signal_setup(
    child: &mut Child,
    after_signal_setup: impl FnOnce(),
) -> Result<ManagedProcessResult, String> {
    let _signal_guard = SIGNAL_HANDLER_LOCK
        .lock()
        .map_err(|_| "managed process signal handler lock is poisoned".to_owned())?;
    install_signal_forwarding();
    after_signal_setup();
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

#[cfg(windows)]
fn process_is_running(pid: u32) -> bool {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, STILL_ACTIVE},
        System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };

    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return false;
        }
        let mut exit_code = 0;
        let available = GetExitCodeProcess(process, &mut exit_code) != 0;
        CloseHandle(process);
        available && exit_code == STILL_ACTIVE as u32
    }
}

#[cfg(not(any(unix, windows)))]
fn process_is_running(_pid: u32) -> bool {
    true
}

fn managed_session_identity(agent_name: &str, now: i64) -> (String, String) {
    let session_id = format!(
        "managed-{}-{}-{now}",
        agent_name.to_ascii_lowercase(),
        std::process::id()
    );
    let reservation_id = format!("{session_id}-reservation");
    (session_id, reservation_id)
}

fn run_codex_hook(options: &CommonOptions) -> Result<serde_json::Value, String> {
    if env::var_os(MANAGED_SESSION_ENV).is_some() {
        return Ok(serde_json::json!({}));
    }
    let event = CodexHookEvent::from_reader(io::stdin().lock())?;
    let mut service = open_service(options)?;
    let outcome = codex_hooks::handle_event(&mut service, &event, now_millis()?, || {
        matches!(
            event.kind(),
            CodexHookEventKind::UserPromptSubmit | CodexHookEventKind::Stop
        )
        .then(codex::detect)
    })?;
    Ok(codex_hooks::hook_output(&outcome))
}

fn provider_allocation<'a>(
    context: &'a WorkspaceContext,
    provider_id: &str,
) -> Result<&'a quotafence_lib::application::WorkspaceAllocationContext, String> {
    let binding = context.binding.as_ref().ok_or_else(|| {
        format!(
            "workspace {} is not bound; run `quotafence bind --scope <name-or-id>` first",
            context.canonical_path
        )
    })?;
    let mut matching = context.allocations.iter().filter(|allocation| {
        provider_matches(
            &allocation.provider_id,
            &allocation.provider_display_name,
            provider_id,
        )
    });
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

fn managed_allocation(
    context: &WorkspaceContext,
    agent: ManagedAgent,
    claude_window: ClaudeManagedWindow,
) -> Result<&quotafence_lib::application::WorkspaceAllocationContext, String> {
    if agent == ManagedAgent::Codex {
        return provider_allocation(context, "codex");
    }
    let binding = context
        .binding
        .as_ref()
        .ok_or_else(|| format!("workspace {} is not bound", context.canonical_path))?;
    context
        .allocations
        .iter()
        .find(|allocation| {
            allocation
                .provider_display_name
                .eq_ignore_ascii_case("Claude Code")
                && match claude_window {
                    ClaudeManagedWindow::Weekly => allocation
                        .pool_display_name
                        .eq_ignore_ascii_case("Weekly allowance"),
                    ClaudeManagedWindow::FiveHour => allocation
                        .pool_display_name
                        .eq_ignore_ascii_case("5-hour allowance"),
                }
        })
        .ok_or_else(|| {
            format!(
                "scope {} has no Claude {} allocation",
                binding.scope_display_name,
                match claude_window {
                    ClaudeManagedWindow::Weekly => "weekly",
                    ClaudeManagedWindow::FiveHour => "5-hour",
                }
            )
        })
}

fn provider_matches(provider_id: &str, display_name: &str, reference: &str) -> bool {
    provider_id.eq_ignore_ascii_case(reference) || display_name.eq_ignore_ascii_case(reference)
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
    if let Some(path) = env::var_os("QUOTAFENCE_DATABASE_PATH") {
        return Ok(PathBuf::from(path));
    }
    paths::default_database_path().map_err(|error| error.to_string())
}

fn parse_args(mut args: Vec<String>) -> Result<CliCommand, String> {
    if args.is_empty() {
        return Ok(CliCommand::Status(CommonOptions::default()));
    }
    if let Some(command) = args.first_mut() {
        *command = match command.as_str() {
            "ls" | "list" | "source" => "sources".to_owned(),
            "alloc" => "allocations".to_owned(),
            "here" => "context".to_owned(),
            _ => command.clone(),
        };
    }
    let Some(command) = args.first().map(String::as_str) else {
        unreachable!("empty arguments return status");
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
        return parse_run_command(&args, false);
    }
    if matches!(command, "codex" | "claude") {
        let mut run_args = vec!["run".to_owned()];
        run_args.extend(args);
        return parse_run_command(&run_args, true);
    }
    if matches!(command, "status" | "sync" | "sources" | "allocations") {
        return parse_read_command(&args);
    }
    if command == "policy" {
        if args.len() == 1 {
            return Ok(CliCommand::Policy {
                action: PolicyAction::Show(CommonOptions::default()),
            });
        }
        return parse_policy_command(&args);
    }
    if !matches!(command, "context" | "bind" | "admit") {
        return Err(format!(
            "unknown command {command:?}; run `quotafence --help`"
        ));
    }

    let mut options = CommonOptions::default();
    let mut scope_reference = None;
    let mut assume_yes = false;
    let provider_id = if command == "admit" {
        Some(
            args.get(1)
                .filter(|value| !value.starts_with('-'))
                .cloned()
                .ok_or_else(|| {
                    "`quotafence admit` requires a provider, for example `codex`".to_owned()
                })?,
        )
    } else {
        None
    };
    let mut index = if command == "admit" { 2 } else { 1 };
    if command == "bind" {
        if let Some(reference) = args.get(1).filter(|value| !value.starts_with('-')) {
            scope_reference = Some(reference.clone());
            index = 2;
        }
    }
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
                .ok_or_else(|| "usage: qfence bind <workspace-name-or-id>".to_owned())?,
        }),
        "admit" => Ok(CliCommand::Admit {
            options,
            provider_id: provider_id.expect("admit provider was validated"),
            assume_yes,
        }),
        _ => unreachable!("command was validated"),
    }
}

fn parse_read_command(args: &[String]) -> Result<CliCommand, String> {
    let command = args[0].as_str();
    let mut options = CommonOptions::default();
    let mut query = None;
    let mut index = 1;

    if command == "sources" {
        match args.get(index).map(String::as_str) {
            Some("list") => index += 1,
            Some("show") => {
                index += 1;
                query = Some(
                    args.get(index)
                        .filter(|value| !value.starts_with('-'))
                        .cloned()
                        .ok_or_else(|| "`quotafence sources show` requires a source".to_owned())?,
                );
                index += 1;
            }
            Some(value) if !value.starts_with('-') => {
                return Err(format!(
                    "unknown sources action {value:?}; use list or show <source>"
                ));
            }
            _ => {}
        }
    } else if command == "allocations" && args.get(index).map(String::as_str) == Some("list") {
        index += 1;
    }

    while index < args.len() {
        match args[index].as_str() {
            "--database" => {
                index += 1;
                options.database = Some(PathBuf::from(required_value(args, index, "--database")?));
            }
            "--json" => options.json = true,
            "-h" | "--help" => return Ok(CliCommand::Help),
            option => return Err(format!("unknown {command} option {option:?}")),
        }
        index += 1;
    }

    match command {
        "status" => Ok(CliCommand::Status(options)),
        "sync" => Ok(CliCommand::Sync(options)),
        "sources" => Ok(CliCommand::Sources { options, query }),
        "allocations" => Ok(CliCommand::Allocations(options)),
        _ => unreachable!("read command was validated"),
    }
}

fn parse_run_command(args: &[String], implicit_agent_args: bool) -> Result<CliCommand, String> {
    let agent = match args.get(1).map(String::as_str) {
        Some("codex") => ManagedAgent::Codex,
        Some("claude") => ManagedAgent::Claude,
        _ => return Err("usage: quotafence run <codex|claude> [--path <directory>] [--window <weekly|5h>] [--yes] -- [agent args]".to_owned()),
    };
    let mut options = CommonOptions::default();
    let mut assume_yes = false;
    let mut claude_window = ClaudeManagedWindow::Weekly;
    let mut index = 2;
    while index < args.len() {
        match args[index].as_str() {
            "--" => {
                return Ok(CliCommand::RunAgent {
                    agent,
                    claude_window,
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
            "--window" if agent == ManagedAgent::Claude => {
                index += 1;
                claude_window = match required_value(args, index, "--window")? {
                    "weekly" | "week" => ClaudeManagedWindow::Weekly,
                    "5h" | "five-hour" => ClaudeManagedWindow::FiveHour,
                    value => {
                        return Err(format!("unknown Claude window {value:?}; use weekly or 5h"))
                    }
                };
            }
            "-h" | "--help" => return Ok(CliCommand::Help),
            option => {
                if implicit_agent_args {
                    return Ok(CliCommand::RunAgent {
                        agent,
                        claude_window,
                        options,
                        assume_yes,
                        agent_args: args[index..].to_vec(),
                    });
                }
                return Err(format!(
                    "unknown quotafence run option {option:?}; place agent arguments after --"
                ));
            }
        }
        index += 1;
    }
    Ok(CliCommand::RunAgent {
        agent,
        claude_window,
        options,
        assume_yes,
        agent_args: Vec::new(),
    })
}

fn parse_hook_command(args: &[String]) -> Result<CliCommand, String> {
    if args.get(1).map(String::as_str) == Some("claude") {
        if args.len() != 2 {
            return Err("usage: quotafence hook claude".to_owned());
        }
        return Ok(CliCommand::HookClaude);
    }
    if args.get(1).map(String::as_str) != Some("codex") {
        return Err("usage: quotafence hook <codex|claude> [--database <path>]".to_owned());
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
        _ => {
            return Err(
                "usage: quotafence hooks <install|status|uninstall> <codex|claude>".to_owned(),
            )
        }
    };
    let provider = match args.get(2).map(String::as_str) {
        Some("codex") => HookProvider::Codex,
        Some("claude") => HookProvider::Claude,
        _ => {
            return Err(
                "usage: quotafence hooks <install|status|uninstall> <codex|claude>".to_owned(),
            )
        }
    };
    if args.len() != 3 {
        return Err("usage: quotafence hooks <install|status|uninstall> <codex|claude>".to_owned());
    }
    Ok(CliCommand::Hooks { action, provider })
}

fn parse_policy_command(args: &[String]) -> Result<CliCommand, String> {
    let action = args
        .get(1)
        .map(String::as_str)
        .ok_or_else(|| "usage: quotafence policy <show|set|reset> [options]".to_owned())?;
    if !matches!(action, "show" | "set" | "reset") {
        return Err("usage: quotafence policy <show|set|reset> [options]".to_owned());
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
            warn_at_basis_points: warn.ok_or_else(|| {
                "`quotafence policy set` requires `--warn <percent|off>`".to_owned()
            })?,
            confirm_at_basis_points: confirm.ok_or_else(|| {
                "`quotafence policy set` requires `--confirm <percent|off>`".to_owned()
            })?,
            stop_at_basis_points: stop.ok_or_else(|| {
                "`quotafence policy set` requires `--stop <percent|off>`".to_owned()
            })?,
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
                println!("Bind with: quotafence bind --scope <name-or-id>");
            }
        }
    }
    Ok(())
}

fn refresh_status_sources(service: &mut QuotaService, at: i64) -> Result<Vec<String>, String> {
    let state = service
        .local_state(GetLocalState {
            selected_window_id: None,
            at,
        })
        .map_err(|error| error.to_string())?;
    let mut warnings = Vec::new();

    if state.sources.iter().any(|source| {
        source
            .provider_display_name
            .eq_ignore_ascii_case("Claude Code")
    }) {
        if let Err(error) = claude_code::fetch_subscription_usage()
            .and_then(|observation| claude_code::ingest_observation(service, &observation, at))
        {
            warnings.push(format!("Claude Code refresh failed: {error}"));
        }
    }

    if let Some(source) = state.sources.iter().find(|source| {
        source.provider_display_name.eq_ignore_ascii_case("Codex")
            && source.provider_managed
            && source.unit == "percent"
    }) {
        let checkpoint =
            codex::sync_detection(service, source.window_id.clone(), at, codex::detect());
        if checkpoint.status != CodexSyncStatus::Synced {
            warnings.push(format!(
                "Codex refresh failed: {}",
                checkpoint
                    .message
                    .as_deref()
                    .unwrap_or("provider temporarily unavailable")
            ));
        }
    }

    Ok(warnings)
}

fn print_status(
    state: &LocalState,
    sync_warnings: &[String],
    at: i64,
    json: bool,
) -> Result<(), String> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(state)
                .map_err(|error| format!("cannot serialize status: {error}"))?
        );
        return Ok(());
    }

    let provider_count = state
        .sources
        .iter()
        .map(|source| source.provider_display_name.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>()
        .len();
    let workspace_count = state
        .scopes
        .iter()
        .filter(|scope| scope.workspace_path.is_some())
        .count();
    println!("{}", paint("  QUOTAFENCE", "1;38;5;81"));
    println!(
        "  {}  {} providers  {}  {} windows  {}  {} workspaces",
        paint("●", "38;5;84"),
        provider_count,
        paint("◆", "38;5;81"),
        state.sources.len(),
        paint("⌂", "38;5;220"),
        workspace_count
    );
    println!();
    print_sources(state, at, false)?;
    for warning in sync_warnings {
        println!("  {} {}", paint("▲", "38;5;220"), warning);
    }
    Ok(())
}

struct CliAllowanceCell {
    label: String,
    percent: Option<u64>,
}

struct CliAllowanceRow {
    five_hour: CliAllowanceCell,
    weekly: CliAllowanceCell,
}

fn print_sources(state: &LocalState, at: i64, json: bool) -> Result<(), String> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&state.sources)
                .map_err(|error| format!("cannot serialize sources: {error}"))?
        );
        return Ok(());
    }
    if state.sources.is_empty() {
        println!("No quota sources configured. Add one in the QuotaFence app.");
        return Ok(());
    }

    let mut groups: Vec<(&str, Vec<&QuotaSourceSummary>)> = Vec::new();
    for source in &state.sources {
        if let Some((_, sources)) = groups.iter_mut().find(|(provider_name, _)| {
            provider_name.eq_ignore_ascii_case(&source.provider_display_name)
        }) {
            sources.push(source);
        } else {
            groups.push((&source.provider_display_name, vec![source]));
        }
    }
    let provider_width = groups
        .iter()
        .map(|(_, sources)| sources[0].provider_display_name.chars().count())
        .max()
        .unwrap_or(8)
        .max(8);
    let allowance_rows: Vec<CliAllowanceRow> = groups
        .iter()
        .map(|(_, sources)| {
            let five_hour = sources
                .iter()
                .copied()
                .find(|source| is_five_hour_window(source));
            let weekly = sources
                .iter()
                .copied()
                .find(|source| is_weekly_window(source))
                .or_else(|| {
                    sources
                        .iter()
                        .copied()
                        .find(|source| !is_five_hour_window(source))
                });
            CliAllowanceRow {
                five_hour: allowance_cell(five_hour),
                weekly: allowance_cell(weekly),
            }
        })
        .collect();
    let five_hour_width = allowance_rows
        .iter()
        .map(|row| row.five_hour.label.chars().count())
        .max()
        .unwrap_or(6)
        .max(6);
    let weekly_width = allowance_rows
        .iter()
        .map(|row| row.weekly.label.chars().count())
        .max()
        .unwrap_or(6)
        .max(6);
    let sync_width = 12;
    println!(
        "{}",
        paint(
            &table_rule(
                '╭',
                '┬',
                '╮',
                &[provider_width, five_hour_width, weekly_width, sync_width]
            ),
            "2"
        )
    );
    println!(
        "{} {} {} {} {} {} {} {} {}",
        paint("│", "2"),
        paint(&format!("{:<provider_width$}", "PROVIDER"), "2"),
        paint("│", "2"),
        paint(&format!("{:<five_hour_width$}", "5-HOUR"), "2"),
        paint("│", "2"),
        paint(&format!("{:<weekly_width$}", "WEEKLY"), "2"),
        paint("│", "2"),
        paint(&format!("{:<sync_width$}", "SYNCED"), "2"),
        paint("│", "2")
    );
    println!(
        "{}",
        paint(
            &table_rule(
                '├',
                '┼',
                '┤',
                &[provider_width, five_hour_width, weekly_width, sync_width]
            ),
            "2"
        )
    );
    for ((_, sources), allowances) in groups.iter().zip(allowance_rows) {
        let five_hour_plain_width = allowances.five_hour.label.chars().count();
        let weekly_plain_width = allowances.weekly.label.chars().count();
        let five_hour_display =
            color_quota(&allowances.five_hour.label, allowances.five_hour.percent);
        let weekly_display = color_quota(&allowances.weekly.label, allowances.weekly.percent);
        let (sync, synced_at) = grouped_sync_status(sources);
        let sync_label = if sync == "synced" || sync == "healthy" {
            paint(&format!("● {}", relative_age(synced_at, at)), "38;5;84")
        } else if sync == "never" {
            paint("○ never", "2")
        } else {
            paint(&format!("● {sync}"), "38;5;220")
        };
        println!(
            "{} {} {} {}{} {} {}{} {} {} {} {}",
            paint("│", "2"),
            paint(
                &format!("{:<provider_width$}", sources[0].provider_display_name),
                "1"
            ),
            paint("│", "2"),
            five_hour_display,
            " ".repeat(five_hour_width - five_hour_plain_width),
            paint("│", "2"),
            weekly_display,
            " ".repeat(weekly_width - weekly_plain_width),
            paint("│", "2"),
            sync_label,
            " ".repeat(sync_width.saturating_sub(visible_sync_width(&sync, synced_at, at))),
            paint("│", "2")
        );
    }
    println!(
        "{}",
        paint(
            &table_rule(
                '╰',
                '┴',
                '╯',
                &[provider_width, five_hour_width, weekly_width, sync_width]
            ),
            "2"
        )
    );
    Ok(())
}

fn allowance_cell(source: Option<&QuotaSourceSummary>) -> CliAllowanceCell {
    let Some(source) = source else {
        return CliAllowanceCell {
            label: "—".to_owned(),
            percent: None,
        };
    };
    let percent = source
        .provider_used
        .map(|used| source.capacity.saturating_sub(used) * 100 / source.capacity.max(1));
    let meter = percent
        .map(|value| format!("{} {value:>3}%", quota_bar(value)))
        .unwrap_or_else(|| format!("{}   —", quota_bar_empty()));
    let reset = chrono::DateTime::from_timestamp_millis(source.ends_at)
        .map(|date| date.format("%d %b %H:%M").to_string())
        .unwrap_or_else(|| source.ends_at.to_string());
    CliAllowanceCell {
        label: format!("{meter} ↻ {reset}"),
        percent,
    }
}

fn is_five_hour_window(source: &QuotaSourceSummary) -> bool {
    let name = source.pool_display_name.to_ascii_lowercase();
    name.contains("5-hour") || name.contains("5h")
}

fn is_weekly_window(source: &QuotaSourceSummary) -> bool {
    source
        .pool_display_name
        .to_ascii_lowercase()
        .contains("weekly")
}

fn grouped_sync_status(sources: &[&QuotaSourceSummary]) -> (String, Option<i64>) {
    let statuses: Vec<&str> = sources
        .iter()
        .map(|source| {
            source
                .sync_health
                .as_ref()
                .map(|health| health.status.as_str())
                .unwrap_or(if source.last_synced_at.is_some() {
                    "synced"
                } else {
                    "never"
                })
        })
        .collect();
    for status in statuses {
        if status != "synced" && status != "healthy" {
            return (status.to_owned(), oldest_sync_at(sources));
        }
    }
    ("synced".to_owned(), oldest_sync_at(sources))
}

fn oldest_sync_at(sources: &[&QuotaSourceSummary]) -> Option<i64> {
    sources
        .iter()
        .filter_map(|source| source.last_synced_at)
        .min()
}

fn relative_age(synced_at: Option<i64>, at: i64) -> String {
    let Some(synced_at) = synced_at else {
        return "never".to_owned();
    };
    let seconds = at.saturating_sub(synced_at).max(0) / 1_000;
    match seconds {
        0..=4 => "just now".to_owned(),
        5..=59 => format!("{seconds}s ago"),
        60..=3_599 => format!("{}m ago", seconds / 60),
        3_600..=86_399 => format!("{}h ago", seconds / 3_600),
        _ => format!("{}d ago", seconds / 86_400),
    }
}

fn visible_sync_width(status: &str, synced_at: Option<i64>, at: i64) -> usize {
    if status == "synced" || status == "healthy" {
        // The status circles render as two terminal cells in common macOS
        // monospace fonts, plus one separating space before the label.
        relative_age(synced_at, at).chars().count() + 3
    } else if status == "never" {
        8
    } else {
        status.chars().count() + 3
    }
}

fn table_rule(left: char, middle: char, right: char, widths: &[usize]) -> String {
    let sections = widths
        .iter()
        .map(|width| "─".repeat(width + 2))
        .collect::<Vec<_>>()
        .join(&middle.to_string());
    format!("{left}{sections}{right}")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CliAllocation {
    provider: String,
    window: String,
    scope_id: String,
    scope: String,
    unit: String,
    limit: u64,
    remaining: i64,
    decision: EnforcementDecision,
}

fn print_allocations(
    service: &mut QuotaService,
    state: &LocalState,
    at: i64,
    json: bool,
) -> Result<(), String> {
    let mut rows = Vec::new();
    for source in &state.sources {
        let dashboard = service
            .dashboard(GetQuotaDashboard {
                window_id: source.window_id.clone(),
                at,
            })
            .map_err(|error| error.to_string())?;
        rows.extend(
            dashboard
                .allocations
                .iter()
                .map(|allocation| CliAllocation {
                    provider: source.provider_display_name.clone(),
                    window: source.pool_display_name.clone(),
                    scope_id: allocation.scope_id.clone(),
                    scope: allocation.display_name.clone(),
                    unit: allocation.unit.clone(),
                    limit: allocation.limit,
                    remaining: allocation.remaining,
                    decision: allocation.decision,
                }),
        );
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&rows)
                .map_err(|error| format!("cannot serialize allocations: {error}"))?
        );
    } else if rows.is_empty() {
        println!("No allocations configured. Create one in the QuotaFence app.");
    } else {
        let provider_width = rows
            .iter()
            .map(|row| row.provider.chars().count())
            .max()
            .unwrap_or(8)
            .max(8);
        let window_width = rows
            .iter()
            .map(|row| row.window.chars().count())
            .max()
            .unwrap_or(6)
            .max(6);
        let scope_width = rows
            .iter()
            .map(|row| row.scope.chars().count())
            .max()
            .unwrap_or(5)
            .max(5);
        println!(
            "  {}  {}  {}  {}  {}",
            paint(&format!("{:<provider_width$}", "PROVIDER"), "2"),
            paint(&format!("{:<window_width$}", "WINDOW"), "2"),
            paint(&format!("{:<scope_width$}", "SCOPE"), "2"),
            paint(&format!("{:<18}", "REMAINING"), "2"),
            paint("DECISION", "2")
        );
        for row in rows {
            let remaining = format!("{}/{} {}", row.remaining, row.limit, row.unit);
            let (icon, color) = decision_style(row.decision);
            println!(
                "  {:<provider_width$}  {:<window_width$}  {:<scope_width$}  {:<18}  {}",
                row.provider,
                row.window,
                row.scope,
                remaining,
                paint(
                    &format!("{icon} {}", decision_label(row.decision, false)),
                    color
                )
            );
        }
    }
    Ok(())
}

fn color_enabled() -> bool {
    env::var_os("NO_COLOR").is_none()
        && (io::stdout().is_terminal() || env::var_os("CLICOLOR_FORCE").is_some())
        && env::var("TERM").map_or(true, |term| term != "dumb")
}

fn paint(value: &str, ansi: &str) -> String {
    if color_enabled() {
        format!("\x1b[{ansi}m{value}\x1b[0m")
    } else {
        value.to_owned()
    }
}

fn quota_bar(percent: u64) -> String {
    let filled = (percent.min(100) * 8).div_ceil(100) as usize;
    format!("{}{}", "━".repeat(filled), "─".repeat(8 - filled))
}

fn quota_bar_empty() -> String {
    "────────".to_owned()
}

fn color_quota(value: &str, percent: Option<u64>) -> String {
    match percent {
        Some(0..=15) => paint(value, "38;5;203"),
        Some(16..=35) => paint(value, "38;5;220"),
        Some(_) => paint(value, "38;5;84"),
        None => paint(value, "2"),
    }
}

fn decision_style(decision: EnforcementDecision) -> (&'static str, &'static str) {
    match decision {
        EnforcementDecision::Allow => ("●", "38;5;84"),
        EnforcementDecision::Warn => ("▲", "38;5;220"),
        EnforcementDecision::RequireConfirmation => ("◆", "38;5;214"),
        EnforcementDecision::Stop => ("■", "38;5;203"),
    }
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
        println!("QuotaFence would refuse a managed launch at this policy boundary.");
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
        "QuotaFence CLI

Everyday:
  qfence                              Show current quota status
  qfence sync                         Force-refresh all providers
  qfence ls                           List cached quota sources
  qfence codex [codex args]           Run Codex with quota protection
  qfence claude [claude args]         Run Claude with quota protection
  qfence claude --window 5h           Use Claude's 5-hour allowance

Workspace:
  qfence here                         Show the current workspace mapping
  qfence bind <workspace>             Bind this folder to an allocation
  qfence allocations                  List workspace allocations
  qfence policy                       Show the current workspace policy

More:
  qfence sources show <provider> [--json]
  qfence admit codex [--path <directory>] [--yes] [--json]
  qfence policy set --warn <percent|off> --confirm <percent|off> --stop <percent|off>
  qfence policy reset [--path <directory>] [--json]
  qfence hooks <install|status|uninstall> <codex|claude>
  qfence help

Compatibility:
  The previous `quotafence ...`, `context`, `sources`, and `run` forms still work.

Options:
  --path <directory>   Resolve a workspace from this directory instead of cwd
  --database <path>    Override the local database (or set QUOTAFENCE_DATABASE_PATH)
  --yes                Accept a confirmation-required admission
  percent|off          Percentage of a workspace allocation consumed, or disabled
  --json               Print machine-readable output"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn bind_requires_an_explicit_workspace() {
        assert!(parse_args(vec!["bind".to_owned()])
            .unwrap_err()
            .contains("bind <workspace-name-or-id>"));
    }

    #[test]
    fn context_parses_path_database_and_json() {
        let command = parse_args(vec![
            "context".to_owned(),
            "--path".to_owned(),
            "/code/project".to_owned(),
            "--database".to_owned(),
            "/tmp/quotafence.sqlite3".to_owned(),
            "--json".to_owned(),
        ])
        .unwrap();
        let CliCommand::Context(options) = command else {
            panic!("expected context command");
        };
        assert_eq!(options.path.as_deref(), Some(Path::new("/code/project")));
        assert_eq!(
            options.database.as_deref(),
            Some(Path::new("/tmp/quotafence.sqlite3"))
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
    fn codex_reference_matches_a_ui_generated_provider_id_by_display_name() {
        assert!(provider_matches(
            "source-generated-provider-id",
            "Codex",
            "codex"
        ));
        assert!(provider_matches("codex", "OpenAI Codex", "CODEX"));
        assert!(!provider_matches(
            "source-generated-provider-id",
            "Claude Code",
            "codex"
        ));
    }

    #[test]
    fn managed_run_parses_quotafence_options_and_preserves_codex_arguments() {
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
        let CliCommand::RunAgent {
            agent,
            options,
            assume_yes,
            agent_args,
            ..
        } = command
        else {
            panic!("expected managed Codex run");
        };

        assert_eq!(agent, ManagedAgent::Codex);
        assert_eq!(options.path.as_deref(), Some(Path::new("/code/project")));
        assert!(assume_yes);
        assert_eq!(
            agent_args,
            ["--model", "gpt-5", "fix the tests"].map(str::to_owned)
        );
    }

    #[test]
    fn short_agent_commands_parse_like_managed_runs() {
        let command = parse_args(vec![
            "claude".to_owned(),
            "--window".to_owned(),
            "5h".to_owned(),
            "--".to_owned(),
            "--model".to_owned(),
            "sonnet".to_owned(),
        ])
        .unwrap();
        let CliCommand::RunAgent {
            agent,
            claude_window,
            agent_args,
            ..
        } = command
        else {
            panic!("expected managed Claude run");
        };
        assert_eq!(agent, ManagedAgent::Claude);
        assert_eq!(claude_window, ClaudeManagedWindow::FiveHour);
        assert_eq!(agent_args, ["--model", "sonnet"]);
    }

    #[test]
    fn read_commands_support_shorthand_and_json() {
        assert!(matches!(
            parse_args(vec!["status".to_owned()]).unwrap(),
            CliCommand::Status(_)
        ));
        assert!(matches!(
            parse_args(vec!["sync".to_owned(), "--json".to_owned()]).unwrap(),
            CliCommand::Sync(CommonOptions { json: true, .. })
        ));
        assert!(matches!(
            parse_args(vec![
                "sources".to_owned(),
                "show".to_owned(),
                "claude".to_owned(),
                "--json".to_owned(),
            ])
            .unwrap(),
            CliCommand::Sources {
                query: Some(_),
                options: CommonOptions { json: true, .. }
            }
        ));
        assert!(matches!(
            parse_args(vec!["allocations".to_owned(), "list".to_owned()]).unwrap(),
            CliCommand::Allocations(_)
        ));
    }

    #[test]
    fn friendly_commands_cover_the_common_workflow() {
        assert!(matches!(
            parse_args(Vec::new()).unwrap(),
            CliCommand::Status(_)
        ));
        assert!(matches!(
            parse_args(vec!["ls".to_owned()]).unwrap(),
            CliCommand::Sources { .. }
        ));
        assert!(matches!(
            parse_args(vec!["here".to_owned()]).unwrap(),
            CliCommand::Context(_)
        ));
        assert!(matches!(
            parse_args(vec!["policy".to_owned()]).unwrap(),
            CliCommand::Policy {
                action: PolicyAction::Show(_)
            }
        ));

        let CliCommand::Bind {
            scope_reference, ..
        } = parse_args(vec!["bind".to_owned(), "My workspace".to_owned()]).unwrap()
        else {
            panic!("expected positional workspace binding");
        };
        assert_eq!(scope_reference, "My workspace");

        let CliCommand::RunAgent { agent_args, .. } = parse_args(vec![
            "codex".to_owned(),
            "--model".to_owned(),
            "gpt-5".to_owned(),
            "fix tests".to_owned(),
        ])
        .unwrap() else {
            panic!("expected friendly Codex launch");
        };
        assert_eq!(agent_args, ["--model", "gpt-5", "fix tests"]);
    }

    #[test]
    fn sync_age_uses_human_scale_units() {
        let now = 10 * 86_400_000;
        assert_eq!(relative_age(Some(now - 2_000), now), "just now");
        assert_eq!(relative_age(Some(now - 18_000), now), "18s ago");
        assert_eq!(relative_age(Some(now - 240_000), now), "4m ago");
        assert_eq!(relative_age(Some(now - 7_200_000), now), "2h ago");
        assert_eq!(relative_age(Some(now - 172_800_000), now), "2d ago");
        assert_eq!(relative_age(None, now), "never");
    }

    #[test]
    fn managed_claude_run_selects_a_native_window() {
        let command = parse_args(vec![
            "run".to_owned(),
            "claude".to_owned(),
            "--window".to_owned(),
            "5h".to_owned(),
            "--".to_owned(),
            "--model".to_owned(),
            "sonnet".to_owned(),
        ])
        .unwrap();
        let CliCommand::RunAgent {
            agent,
            claude_window,
            agent_args,
            ..
        } = command
        else {
            panic!("expected managed Claude run");
        };
        assert_eq!(agent, ManagedAgent::Claude);
        assert_eq!(claude_window, ClaudeManagedWindow::FiveHour);
        assert_eq!(agent_args, ["--model", "sonnet"].map(str::to_owned));
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
        let result = wait_for_managed_child_after_signal_setup(&mut child, || {
            FORWARDED_SIGNAL.store(libc::SIGTERM, Ordering::SeqCst);
        })
        .unwrap();

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
            "/tmp/quotafence.sqlite3".to_owned(),
        ])
        .unwrap();
        let CliCommand::HookCodex(options) = command else {
            panic!("expected Codex hook command");
        };
        assert_eq!(
            options.database.as_deref(),
            Some(Path::new("/tmp/quotafence.sqlite3"))
        );

        assert!(matches!(
            parse_args(vec![
                "hooks".to_owned(),
                "install".to_owned(),
                "codex".to_owned()
            ])
            .unwrap(),
            CliCommand::Hooks {
                action: HooksAction::Install,
                provider: HookProvider::Codex,
            }
        ));
        assert!(matches!(
            parse_args(vec!["hook".to_owned(), "claude".to_owned()]).unwrap(),
            CliCommand::HookClaude
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
