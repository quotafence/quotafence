use std::{
    collections::BTreeMap,
    env, fs,
    io::{self, IsTerminal},
    path::PathBuf,
    process::{Child, Command, ExitCode, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicI32, Ordering},
        Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use quotafence_lib::{
    application::{
        AdmissionAssessment, BindWorkspace, CreateAllocatedWorkspace, EvaluateWorkspaceAdmission,
        FinishManagedSession, GetLocalState, GetQuotaDashboard, GetWorkspaceContext,
        GetWorkspacePolicy, LocalState, ManagedSessionOutcome, ManagedSessionReconciliation,
        ManagedSessionReconciliationStatus, MarkManagedSessionRunning, PrepareManagedSession,
        QuotaService, QuotaSourceSummary, RemoveWorkspaceAllocation, ResetWorkspacePolicy,
        SetAllocation, SetAllocationPriorityOrder, SetWorkspacePolicy, WorkspaceContext,
        WorkspacePolicySummary,
    },
    domain::EnforcementDecision,
    entitlements::EntitlementSnapshot,
    paths,
    providers::{
        claude_code, claude_hooks,
        codex::{self, CodexSyncResult, CodexSyncStatus},
        codex_hooks::{self, CodexHookEvent, CodexHookEventKind},
    },
    workspace::canonicalize_workspace_path,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Sparkline, Table, TableState, Tabs},
    Frame, Terminal,
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
    Features {
        json: bool,
    },
    Status(CommonOptions),
    Sync(CommonOptions),
    Sources {
        options: CommonOptions,
        query: Option<String>,
    },
    Allocations {
        action: AllocationAction,
    },
    History {
        options: CommonOptions,
        provider: Option<String>,
        days: u16,
    },
    Top {
        options: CommonOptions,
        interval_seconds: u64,
        once: bool,
    },
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
enum AllocationAction {
    List(CommonOptions),
    Add {
        options: CommonOptions,
        provider: String,
        percent: u64,
        name: Option<String>,
        donor: Option<String>,
    },
    Set {
        options: CommonOptions,
        project: String,
        percent: u64,
        donor: Option<String>,
        provider: Option<String>,
    },
    Remove {
        options: CommonOptions,
        project: String,
        provider: Option<String>,
    },
    Move {
        options: CommonOptions,
        project: String,
        direction: AllocationMove,
        provider: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AllocationMove {
    Up,
    Down,
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
        CliCommand::Features { json } => {
            print_features(&EntitlementSnapshot::free(), json)?;
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
        CliCommand::Allocations { action } => {
            run_allocation_command(action)?;
            Ok(EXIT_ALLOW)
        }
        CliCommand::History {
            options,
            provider,
            days,
        } => {
            let mut service = open_service(&options)?;
            let now = now_millis()?;
            print_history(&mut service, provider.as_deref(), days, now, options.json)?;
            Ok(EXIT_ALLOW)
        }
        CliCommand::Top {
            options,
            interval_seconds,
            once,
        } => {
            run_top(options, interval_seconds, once)?;
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
            options,
            assume_yes,
            agent_args,
        } => run_managed_agent(agent, options, assume_yes, agent_args),
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
    let allocation = managed_allocation(&context, agent)?;
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
    if let Some(window) = exhausted_native_window(&mut service, agent, now)? {
        return Err(format!(
            "cannot launch {agent_name}: the provider's {window} is exhausted"
        ));
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
                .find(|window| window.kind == "seven_day")
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
                && allocation
                    .pool_display_name
                    .eq_ignore_ascii_case("Weekly allowance")
        })
        .ok_or_else(|| {
            format!(
                "scope {} has no Claude weekly allocation",
                binding.scope_display_name
            )
        })
}

fn exhausted_native_window(
    service: &mut QuotaService,
    agent: ManagedAgent,
    at: i64,
) -> Result<Option<String>, String> {
    let provider_name = match agent {
        ManagedAgent::Codex => "Codex",
        ManagedAgent::Claude => "Claude Code",
    };
    let state = service
        .local_state(GetLocalState {
            selected_window_id: None,
            at,
        })
        .map_err(|error| error.to_string())?;
    Ok(state
        .sources
        .into_iter()
        .filter(|source| {
            source.is_active
                && source.provider_managed
                && source
                    .provider_display_name
                    .eq_ignore_ascii_case(provider_name)
                && source.unit == "percent"
        })
        .find(|source| {
            source
                .provider_used
                .is_some_and(|used| used >= source.capacity)
        })
        .map(|source| source.pool_display_name))
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
            "hist" => "history".to_owned(),
            "dashboard" => "top".to_owned(),
            "here" => "context".to_owned(),
            "feature" | "capabilities" => "features".to_owned(),
            _ => command.clone(),
        };
    }
    let Some(command) = args.first().map(String::as_str) else {
        unreachable!("empty arguments return status");
    };
    if matches!(command, "-h" | "--help" | "help") {
        return Ok(CliCommand::Help);
    }
    if command == "features" {
        return parse_features_command(&args);
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
    if command == "allocations" {
        return parse_allocations_command(&args);
    }
    if command == "history" {
        return parse_history_command(&args);
    }
    if command == "top" {
        return parse_top_command(&args);
    }
    if matches!(command, "status" | "sync" | "sources") {
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

fn parse_features_command(args: &[String]) -> Result<CliCommand, String> {
    let mut json = false;
    for option in &args[1..] {
        match option.as_str() {
            "--json" => json = true,
            "-h" | "--help" => return Ok(CliCommand::Help),
            option => return Err(format!("unknown features option {option:?}")),
        }
    }
    Ok(CliCommand::Features { json })
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
        _ => unreachable!("read command was validated"),
    }
}

fn parse_allocations_command(args: &[String]) -> Result<CliCommand, String> {
    let action = args.get(1).map(String::as_str).unwrap_or("list");
    let mut options = CommonOptions::default();
    let mut provider = None;
    let mut percent = None;
    let mut name = None;
    let mut donor = None;
    let mut positional = Vec::new();
    let mut index = if matches!(action, "list" | "add" | "set" | "remove" | "rm" | "move") {
        if args.len() > 1 {
            2
        } else {
            1
        }
    } else {
        return Err(format!(
            "unknown allocations action {action:?}; use list, add, set, remove, or move"
        ));
    };

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
            "--provider" => {
                index += 1;
                provider = Some(required_value(args, index, "--provider")?.to_owned());
            }
            "--percent" => {
                index += 1;
                percent = Some(parse_allocation_percent(required_value(
                    args,
                    index,
                    "--percent",
                )?)?);
            }
            "--name" => {
                index += 1;
                name = Some(required_value(args, index, "--name")?.to_owned());
            }
            "--from" => {
                index += 1;
                donor = Some(required_value(args, index, "--from")?.to_owned());
            }
            "--json" => options.json = true,
            "-h" | "--help" => return Ok(CliCommand::Help),
            value if !value.starts_with('-') => positional.push(value.to_owned()),
            option => return Err(format!("unknown allocations option {option:?}")),
        }
        index += 1;
    }

    let action = match action {
        "list" => AllocationAction::List(options),
        "add" => AllocationAction::Add {
            options,
            provider: provider.ok_or_else(|| {
                "usage: qfence allocations add --provider <codex|claude> --percent <1-100> [--path <folder>] [--name <name>] [--from <project>]".to_owned()
            })?,
            percent: percent.ok_or_else(|| "allocations add requires --percent".to_owned())?,
            name,
            donor,
        },
        "set" => AllocationAction::Set {
            options,
            project: one_positional(positional, "usage: qfence allocations set <project> --percent <1-100> [--from <project>]")?,
            percent: percent.ok_or_else(|| "allocations set requires --percent".to_owned())?,
            donor,
            provider,
        },
        "remove" | "rm" => AllocationAction::Remove {
            options,
            project: one_positional(positional, "usage: qfence allocations remove <project> [--provider <provider>]")?,
            provider,
        },
        "move" => {
            if positional.len() != 2 {
                return Err("usage: qfence allocations move <project> <up|down> [--provider <provider>]".to_owned());
            }
            let direction = match positional[1].as_str() {
                "up" => AllocationMove::Up,
                "down" => AllocationMove::Down,
                _ => return Err("allocation move direction must be up or down".to_owned()),
            };
            AllocationAction::Move {
                options,
                project: positional[0].clone(),
                direction,
                provider,
            }
        }
        _ => unreachable!("allocation action was validated"),
    };
    Ok(CliCommand::Allocations { action })
}

fn parse_history_command(args: &[String]) -> Result<CliCommand, String> {
    let mut options = CommonOptions::default();
    let mut provider = None;
    let mut days = 30u16;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--days" => {
                index += 1;
                days = required_value(args, index, "--days")?
                    .parse::<u16>()
                    .ok()
                    .filter(|value| (1..=184).contains(value))
                    .ok_or_else(|| "--days must be between 1 and 184".to_owned())?;
            }
            "--database" => {
                index += 1;
                options.database = Some(PathBuf::from(required_value(args, index, "--database")?));
            }
            "--json" => options.json = true,
            "-h" | "--help" => return Ok(CliCommand::Help),
            value if !value.starts_with('-') && provider.is_none() => {
                provider = Some(value.to_owned())
            }
            option => return Err(format!("unknown history option {option:?}")),
        }
        index += 1;
    }
    Ok(CliCommand::History {
        options,
        provider,
        days,
    })
}

fn parse_top_command(args: &[String]) -> Result<CliCommand, String> {
    let mut options = CommonOptions::default();
    let mut interval_seconds = 30u64;
    let mut once = false;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--interval" => {
                index += 1;
                interval_seconds = required_value(args, index, "--interval")?
                    .parse::<u64>()
                    .ok()
                    .filter(|value| (5..=3600).contains(value))
                    .ok_or_else(|| "--interval must be between 5 and 3600 seconds".to_owned())?;
            }
            "--database" => {
                index += 1;
                options.database = Some(PathBuf::from(required_value(args, index, "--database")?));
            }
            "--once" => once = true,
            "-h" | "--help" => return Ok(CliCommand::Help),
            option => return Err(format!("unknown top option {option:?}")),
        }
        index += 1;
    }
    Ok(CliCommand::Top {
        options,
        interval_seconds,
        once,
    })
}

fn parse_allocation_percent(value: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .ok()
        .filter(|value| (1..=100).contains(value))
        .ok_or_else(|| {
            format!("invalid allocation {value:?}; use a whole percentage from 1 to 100")
        })
}

fn one_positional(values: Vec<String>, usage: &str) -> Result<String, String> {
    if values.len() == 1 {
        Ok(values.into_iter().next().expect("one positional value"))
    } else {
        Err(usage.to_owned())
    }
}

fn parse_run_command(args: &[String], implicit_agent_args: bool) -> Result<CliCommand, String> {
    let agent =
        match args.get(1).map(String::as_str) {
            Some("codex") => ManagedAgent::Codex,
            Some("claude") => ManagedAgent::Claude,
            _ => return Err(
                "usage: quotafence run <codex|claude> [--path <directory>] [--yes] -- [agent args]"
                    .to_owned(),
            ),
        };
    let mut options = CommonOptions::default();
    let mut assume_yes = false;
    let mut index = 2;
    while index < args.len() {
        match args[index].as_str() {
            "--" => {
                return Ok(CliCommand::RunAgent {
                    agent,
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
                match required_value(args, index, "--window")? {
                    "weekly" | "week" => {}
                    "5h" | "five-hour" => {
                        return Err("Claude workspace allocations are weekly-only; the 5-hour allowance is checked automatically as a provider safety limit".to_owned())
                    }
                    value => return Err(format!("unknown Claude window {value:?}; use weekly")),
                }
            }
            "-h" | "--help" => return Ok(CliCommand::Help),
            option => {
                if implicit_agent_args {
                    return Ok(CliCommand::RunAgent {
                        agent,
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

fn run_allocation_command(action: AllocationAction) -> Result<(), String> {
    match action {
        AllocationAction::List(options) => print_current_allocations(&options),
        AllocationAction::Add {
            options,
            provider,
            percent,
            name,
            donor,
        } => {
            let now = now_millis()?;
            let (mut service, canonical_path) = open_context(&options)?;
            let state = local_state(&mut service, now)?;
            let source = weekly_source(&state, &provider)?.clone();
            let dashboard = service
                .dashboard(GetQuotaDashboard {
                    window_id: source.window_id.clone(),
                    at: now,
                })
                .map_err(|error| error.to_string())?;
            ensure_percent_window(&source)?;
            let donor_restore = fund_allocation_increase(
                &mut service,
                &dashboard,
                percent,
                donor.as_deref(),
                None,
            )?;
            let display_name = name.unwrap_or_else(|| workspace_name(&canonical_path));
            let result = service.create_allocated_workspace(CreateAllocatedWorkspace {
                id: format!("workspace-cli-{now}-{}", std::process::id()),
                display_name,
                canonical_path,
                window_id: source.window_id,
                amount: percent,
                unit: source.unit,
                bound_at: now,
            });
            if let Err(error) = result {
                restore_donor(&mut service, donor_restore);
                return Err(error.to_string());
            }
            print_current_allocations_with_service(&options, &mut service, now)
        }
        AllocationAction::Set {
            options,
            project,
            percent,
            donor,
            provider,
        } => {
            let now = now_millis()?;
            let mut service = open_service(&options)?;
            let state = local_state(&mut service, now)?;
            let (source, dashboard, target_index) =
                find_project_allocation(&mut service, &state, &project, provider.as_deref(), now)?;
            let target = &dashboard.allocations[target_index];
            let donor_restore = if percent > target.limit {
                fund_allocation_increase(
                    &mut service,
                    &dashboard,
                    percent - target.limit,
                    donor.as_deref(),
                    Some(&target.scope_id),
                )?
            } else {
                None
            };
            let result = service.set_allocation(SetAllocation {
                scope_id: target.scope_id.clone(),
                window_id: source.window_id,
                amount: percent,
                unit: source.unit,
            });
            if let Err(error) = result {
                restore_donor(&mut service, donor_restore);
                return Err(error.to_string());
            }
            print_current_allocations_with_service(&options, &mut service, now)
        }
        AllocationAction::Remove {
            options,
            project,
            provider,
        } => {
            let now = now_millis()?;
            let mut service = open_service(&options)?;
            let state = local_state(&mut service, now)?;
            let (source, dashboard, target_index) =
                find_project_allocation(&mut service, &state, &project, provider.as_deref(), now)?;
            service
                .remove_workspace_allocation(RemoveWorkspaceAllocation {
                    scope_id: dashboard.allocations[target_index].scope_id.clone(),
                    window_id: source.window_id,
                    removed_at: now,
                })
                .map_err(|error| error.to_string())?;
            print_current_allocations_with_service(&options, &mut service, now)
        }
        AllocationAction::Move {
            options,
            project,
            direction,
            provider,
        } => {
            let now = now_millis()?;
            let mut service = open_service(&options)?;
            let state = local_state(&mut service, now)?;
            let (source, dashboard, target_index) =
                find_project_allocation(&mut service, &state, &project, provider.as_deref(), now)?;
            let mut ordered = dashboard
                .allocations
                .iter()
                .filter(|allocation| allocation.parent_id.is_none())
                .map(|allocation| allocation.scope_id.clone())
                .collect::<Vec<_>>();
            let scope_id = &dashboard.allocations[target_index].scope_id;
            let current = ordered
                .iter()
                .position(|candidate| candidate == scope_id)
                .ok_or_else(|| "only top-level project allocations can be reordered".to_owned())?;
            let destination = match direction {
                AllocationMove::Up if current > 0 => current - 1,
                AllocationMove::Down if current + 1 < ordered.len() => current + 1,
                AllocationMove::Up => return Err("project is already first".to_owned()),
                AllocationMove::Down => return Err("project is already last".to_owned()),
            };
            ordered.swap(current, destination);
            service
                .set_allocation_priority_order(SetAllocationPriorityOrder {
                    window_id: source.window_id,
                    ordered_scope_ids: ordered,
                })
                .map_err(|error| error.to_string())?;
            print_current_allocations_with_service(&options, &mut service, now)
        }
    }
}

fn local_state(service: &mut QuotaService, at: i64) -> Result<LocalState, String> {
    service
        .local_state(GetLocalState {
            selected_window_id: None,
            at,
        })
        .map_err(|error| error.to_string())
}

fn print_current_allocations(options: &CommonOptions) -> Result<(), String> {
    let mut service = open_service(options)?;
    print_current_allocations_with_service(options, &mut service, now_millis()?)
}

fn print_current_allocations_with_service(
    options: &CommonOptions,
    service: &mut QuotaService,
    at: i64,
) -> Result<(), String> {
    let state = local_state(service, at)?;
    print_allocations(service, &state, at, options.json)
}

fn weekly_source<'a>(
    state: &'a LocalState,
    provider: &str,
) -> Result<&'a QuotaSourceSummary, String> {
    let matches = state
        .sources
        .iter()
        .filter(|source| {
            source.is_active
                && is_weekly_window(source)
                && provider_matches(&source.provider_id, &source.provider_display_name, provider)
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [source] => Ok(source),
        [] => Err(format!(
            "no active weekly quota source matches {provider:?}; run `qfence sync` first"
        )),
        _ => Err(format!(
            "multiple weekly quota sources match {provider:?}; use the provider ID"
        )),
    }
}

fn ensure_percent_window(source: &QuotaSourceSummary) -> Result<(), String> {
    if source.unit == "percent" && source.capacity == 100 {
        Ok(())
    } else {
        Err(format!(
            "{} allocations are not percentage-based",
            source.provider_display_name
        ))
    }
}

fn workspace_name(canonical_path: &str) -> String {
    PathBuf::from(canonical_path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Workspace")
        .to_owned()
}

fn allocation_matches(
    allocation: &quotafence_lib::application::AllocationSnapshot,
    reference: &str,
) -> bool {
    allocation.scope_id == reference || allocation.display_name.eq_ignore_ascii_case(reference)
}

fn find_project_allocation(
    service: &mut QuotaService,
    state: &LocalState,
    project: &str,
    provider: Option<&str>,
    at: i64,
) -> Result<
    (
        QuotaSourceSummary,
        quotafence_lib::application::QuotaDashboard,
        usize,
    ),
    String,
> {
    let mut matches = Vec::new();
    for source in state.sources.iter().filter(|source| {
        source.is_active
            && is_weekly_window(source)
            && provider.is_none_or(|reference| {
                provider_matches(
                    &source.provider_id,
                    &source.provider_display_name,
                    reference,
                )
            })
    }) {
        let dashboard = service
            .dashboard(GetQuotaDashboard {
                window_id: source.window_id.clone(),
                at,
            })
            .map_err(|error| error.to_string())?;
        for (index, allocation) in dashboard.allocations.iter().enumerate() {
            if allocation_matches(allocation, project) {
                matches.push((source.clone(), dashboard.clone(), index));
            }
        }
    }
    match matches.len() {
        0 => Err(format!("no weekly project allocation matches {project:?}")),
        1 => Ok(matches.remove(0)),
        _ => Err(format!(
            "project {project:?} exists for multiple providers; add --provider <provider>"
        )),
    }
}

fn fund_allocation_increase(
    service: &mut QuotaService,
    dashboard: &quotafence_lib::application::QuotaDashboard,
    increase: u64,
    donor_reference: Option<&str>,
    target_scope_id: Option<&str>,
) -> Result<Option<SetAllocation>, String> {
    let shortage = increase.saturating_sub(dashboard.window.unallocated);
    if shortage == 0 {
        return Ok(None);
    }
    let donor_reference = donor_reference.ok_or_else(|| {
        format!(
            "only {}% is free; choose a project with `--from <project>` to transfer the remaining {}%",
            dashboard.window.unallocated, shortage
        )
    })?;
    let mut donors = dashboard
        .allocations
        .iter()
        .filter(|allocation| {
            allocation_matches(allocation, donor_reference)
                && target_scope_id != Some(allocation.scope_id.as_str())
        })
        .collect::<Vec<_>>();
    if donors.len() != 1 {
        return Err(format!(
            "donor project {donor_reference:?} was not found or is ambiguous"
        ));
    }
    let donor = donors.remove(0);
    if donor.limit <= shortage {
        return Err(format!(
            "{} has {}%; it must keep at least 1% after transferring {}%",
            donor.display_name, donor.limit, shortage
        ));
    }
    let restore = SetAllocation {
        scope_id: donor.scope_id.clone(),
        window_id: dashboard.window.id.clone(),
        amount: donor.limit,
        unit: dashboard.window.unit.clone(),
    };
    service
        .set_allocation(SetAllocation {
            amount: donor.limit - shortage,
            ..restore.clone()
        })
        .map_err(|error| error.to_string())?;
    Ok(Some(restore))
}

fn restore_donor(service: &mut QuotaService, donor: Option<SetAllocation>) {
    if let Some(donor) = donor {
        let _ = service.set_allocation(donor);
    }
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CliAllocation {
    provider: String,
    window: String,
    scope_id: String,
    scope: String,
    path: Option<String>,
    priority: u64,
    unit: String,
    limit: u64,
    used: u64,
    remaining: i64,
    decision: EnforcementDecision,
}

fn print_allocations(
    service: &mut QuotaService,
    state: &LocalState,
    at: i64,
    json: bool,
) -> Result<(), String> {
    let rows = collect_allocations(service, state, at)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&rows)
                .map_err(|error| format!("cannot serialize allocations: {error}"))?
        );
    } else if rows.is_empty() {
        println!("No allocations configured. Create one with `qfence allocations add`.");
    } else {
        print_allocation_table(rows);
    }
    Ok(())
}

fn collect_allocations(
    service: &mut QuotaService,
    state: &LocalState,
    at: i64,
) -> Result<Vec<CliAllocation>, String> {
    let mut rows = Vec::new();
    for source in &state.sources {
        let dashboard = service
            .dashboard(GetQuotaDashboard {
                window_id: source.window_id.clone(),
                at,
            })
            .map_err(|error| error.to_string())?;
        rows.extend(dashboard.allocations.iter().map(|allocation| {
            CliAllocation {
                provider: source.provider_display_name.clone(),
                window: source.pool_display_name.clone(),
                scope_id: allocation.scope_id.clone(),
                scope: allocation.display_name.clone(),
                path: state
                    .scopes
                    .iter()
                    .find(|scope| scope.id == allocation.scope_id)
                    .and_then(|scope| scope.workspace_path.clone()),
                priority: allocation.priority,
                unit: allocation.unit.clone(),
                limit: allocation.limit,
                used: allocation.attributed_usage,
                remaining: allocation.remaining,
                decision: allocation.decision,
            }
        }));
    }
    Ok(rows)
}

fn print_allocation_table(rows: Vec<CliAllocation>) {
    let provider_width = rows
        .iter()
        .map(|row| row.provider.chars().count())
        .max()
        .unwrap_or(8)
        .max(8);
    let scope_width = rows
        .iter()
        .map(|row| row.scope.chars().count())
        .max()
        .unwrap_or(5)
        .clamp(7, 28);
    let widths = [3, provider_width, scope_width, 8, 8, 8, 12];
    println!("{}", paint(&table_rule('╭', '┬', '╮', &widths), "2"));
    println!(
            "{} {:<3} {} {:<provider_width$} {} {:<scope_width$} {} {:<8} {} {:<8} {} {:<8} {} {:<12} {}",
            paint("│", "2"), paint("#", "2"), paint("│", "2"), paint("PROVIDER", "2"),
            paint("│", "2"), paint("PROJECT", "2"), paint("│", "2"), paint("WEEKLY", "2"),
            paint("│", "2"), paint("USED", "2"), paint("│", "2"), paint("LEFT", "2"),
            paint("│", "2"), paint("STATUS", "2"), paint("│", "2")
        );
    println!("{}", paint(&table_rule('├', '┼', '┤', &widths), "2"));
    for row in rows {
        let project = truncate_text(&row.scope, scope_width);
        let (icon, color) = decision_style(row.decision);
        println!(
                "{} {:<3} {} {:<provider_width$} {} {:<scope_width$} {} {:<8} {} {:<8} {} {:<8} {} {:<12} {}",
                paint("│", "2"), row.priority + 1, paint("│", "2"), row.provider,
                paint("│", "2"), project, paint("│", "2"), format!("{}%", row.limit),
                paint("│", "2"), format!("{}%", row.used), paint("│", "2"),
                format!("{}%", row.remaining.max(0)), paint("│", "2"),
                paint(&format!("{icon} {}", decision_label(row.decision, false)), color),
                paint("│", "2")
            );
    }
    println!("{}", paint(&table_rule('╰', '┴', '╯', &widths), "2"));
}

fn truncate_text(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_owned();
    }
    value
        .chars()
        .take(width.saturating_sub(1))
        .collect::<String>()
        + "…"
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CliHistorySeries {
    provider: String,
    window: String,
    days: Vec<CliHistoryDay>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CliHistoryDay {
    date: String,
    used_percent: u64,
}

fn print_history(
    service: &mut QuotaService,
    provider: Option<&str>,
    days: u16,
    at: i64,
    json: bool,
) -> Result<(), String> {
    let series = collect_history(service, provider, days, at)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&series)
                .map_err(|error| format!("cannot serialize history: {error}"))?
        );
        return Ok(());
    }

    println!("{}", paint("  BASIC HISTORY · WEEKLY QUOTA", "1;38;5;81"));
    for item in &series {
        print_history_series(item);
    }
    println!(
        "\n  {} zero  {} higher daily use",
        paint("·", "2"),
        paint("▁▂▄▆█", "38;5;45")
    );
    Ok(())
}

fn collect_history(
    service: &mut QuotaService,
    provider: Option<&str>,
    days: u16,
    at: i64,
) -> Result<Vec<CliHistorySeries>, String> {
    let state = local_state(service, at)?;
    let sources = state
        .sources
        .iter()
        .filter(|source| {
            source.is_active
                && is_weekly_window(source)
                && provider.is_none_or(|reference| {
                    provider_matches(
                        &source.provider_id,
                        &source.provider_display_name,
                        reference,
                    )
                })
        })
        .collect::<Vec<_>>();
    if sources.is_empty() {
        return Err(match provider {
            Some(provider) => format!("no active weekly quota source matches {provider:?}"),
            None => "no active weekly quota sources; run `qfence sync` first".to_owned(),
        });
    }

    let today = chrono::DateTime::from_timestamp_millis(at)
        .ok_or_else(|| "current time cannot be formatted".to_owned())?
        .with_timezone(&chrono::Local)
        .date_naive();
    let first_day = today - chrono::Duration::days(i64::from(days.saturating_sub(1)));
    let mut series = Vec::new();
    for source in sources {
        let dashboard = service
            .dashboard(GetQuotaDashboard {
                window_id: source.window_id.clone(),
                at,
            })
            .map_err(|error| error.to_string())?;
        let mut usage = BTreeMap::<chrono::NaiveDate, u64>::new();
        for day_offset in 0..days {
            usage.insert(first_day + chrono::Duration::days(i64::from(day_offset)), 0);
        }
        let mut previous = Some(dashboard.window.capacity);
        for point in &dashboard.quota_history {
            let Some(observed) = chrono::DateTime::from_timestamp_millis(point.observed_at) else {
                continue;
            };
            let date = observed.with_timezone(&chrono::Local).date_naive();
            if let Some(previous_remaining) = previous {
                if date >= first_day && date <= today {
                    let consumed = previous_remaining.saturating_sub(point.remaining);
                    *usage.entry(date).or_default() += consumed;
                }
            }
            previous = Some(point.remaining);
        }
        series.push(CliHistorySeries {
            provider: source.provider_display_name.clone(),
            window: source.pool_display_name.clone(),
            days: usage
                .into_iter()
                .map(|(date, used_percent)| CliHistoryDay {
                    date: date.format("%Y-%m-%d").to_string(),
                    used_percent,
                })
                .collect(),
        });
    }

    Ok(series)
}

fn print_history_series(item: &CliHistorySeries) {
    let visible = item.days.len().saturating_sub(60);
    let visible_days = &item.days[visible..];
    let max = visible_days
        .iter()
        .map(|day| day.used_percent)
        .max()
        .unwrap_or(0);
    let chart = visible_days
        .iter()
        .map(|day| history_glyph(day.used_percent, max))
        .collect::<String>();
    let total = item.days.iter().map(|day| day.used_percent).sum::<u64>();
    println!(
        "\n  {}  {} · {}% used over {} days",
        paint("●", "38;5;84"),
        item.provider,
        total,
        item.days.len()
    );
    println!("  {}", paint(&chart, "38;5;45"));
    if let (Some(first), Some(last)) = (visible_days.first(), visible_days.last()) {
        let first_label = chrono::NaiveDate::parse_from_str(&first.date, "%Y-%m-%d")
            .map(|date| date.format("%d %b").to_string())
            .unwrap_or_else(|_| first.date.clone());
        let last_label = chrono::NaiveDate::parse_from_str(&last.date, "%Y-%m-%d")
            .map(|date| date.format("%d %b").to_string())
            .unwrap_or_else(|_| last.date.clone());
        let gap = chart
            .chars()
            .count()
            .saturating_sub(first_label.len() + last_label.len())
            .max(1);
        println!(
            "  {}{}{}",
            paint(&first_label, "2"),
            " ".repeat(gap),
            paint(&last_label, "2")
        );
    }
}

fn history_glyph(value: u64, max: u64) -> char {
    if value == 0 || max == 0 {
        return '·';
    }
    const LEVELS: [char; 5] = ['▁', '▂', '▄', '▆', '█'];
    let index = ((value.saturating_mul(LEVELS.len() as u64) - 1) / max)
        .min((LEVELS.len() - 1) as u64) as usize;
    LEVELS[index]
}

fn run_top(options: CommonOptions, interval_seconds: u64, once: bool) -> Result<(), String> {
    if options.json {
        return Err("`qfence top` is an interactive view and does not support --json".to_owned());
    }
    if once || !io::stdout().is_terminal() {
        return render_top_once(&options, interval_seconds);
    }
    run_top_tui(&options, interval_seconds)
}

#[derive(Debug)]
struct TopSnapshot {
    state: LocalState,
    allocations: Vec<CliAllocation>,
    history: Vec<CliHistorySeries>,
    warnings: Vec<String>,
    refreshed_at: i64,
}

#[derive(Debug)]
enum TopDialog {
    Add(AddAllocationForm),
    Edit(EditAllocationForm),
    Delete {
        scope_id: String,
        project: String,
        provider: String,
        error: Option<String>,
    },
}

#[derive(Debug)]
struct AddAllocationForm {
    field: usize,
    provider_index: usize,
    path: String,
    name: String,
    percent: String,
    percent_edited: bool,
    donor_index: usize,
    error: Option<String>,
}

#[derive(Debug)]
struct EditAllocationForm {
    field: usize,
    scope_id: String,
    project: String,
    provider: String,
    percent: String,
    percent_edited: bool,
    donor_index: usize,
    error: Option<String>,
}

enum DialogOutcome {
    Keep,
    Close,
    Changed,
}

fn load_top_snapshot(options: &CommonOptions) -> Result<TopSnapshot, String> {
    let mut service = open_service(options)?;
    let now = now_millis()?;
    let warnings = refresh_status_sources(&mut service, now)?;
    let state = local_state(&mut service, now)?;
    let allocations = collect_allocations(&mut service, &state, now)?;
    let history = collect_history(&mut service, None, 30, now).unwrap_or_default();
    Ok(TopSnapshot {
        state,
        allocations,
        history,
        warnings,
        refreshed_at: now,
    })
}

fn render_top_once(options: &CommonOptions, interval_seconds: u64) -> Result<(), String> {
    let snapshot = load_top_snapshot(options)?;
    println!(
        "{}  {}  {}",
        paint(" QUOTAFENCE TOP ", "1;30;48;5;45"),
        paint("live local control plane", "2"),
        paint("interactive mode: qfence top", "2")
    );
    println!();
    print_sources(&snapshot.state, snapshot.refreshed_at, false)?;
    for warning in &snapshot.warnings {
        println!("  {} {}", paint("▲", "38;5;220"), warning);
    }
    println!("\n{}", paint("  PROJECT ALLOCATIONS", "1;38;5;81"));
    if snapshot.allocations.is_empty() {
        println!("  No allocations configured.");
    } else {
        print_allocation_table(snapshot.allocations);
    }
    println!();
    for item in &snapshot.history {
        print_history_series(item);
    }
    println!(
        "\n  refreshed just now · live interval {}s",
        interval_seconds
    );
    Ok(())
}

fn run_top_tui(options: &CommonOptions, interval_seconds: u64) -> Result<(), String> {
    enable_raw_mode().map_err(|error| format!("cannot enable terminal UI: {error}"))?;
    let mut stdout = io::stdout();
    if let Err(error) = execute!(stdout, EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(format!("cannot enter terminal UI: {error}"));
    }
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = match Terminal::new(backend) {
        Ok(terminal) => terminal,
        Err(error) => {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
            return Err(format!("cannot create terminal UI: {error}"));
        }
    };

    let result = top_event_loop(&mut terminal, options, interval_seconds);
    let raw_result = disable_raw_mode();
    let screen_result = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let cursor_result = terminal.show_cursor();
    result?;
    raw_result.map_err(|error| format!("cannot restore terminal mode: {error}"))?;
    screen_result.map_err(|error| format!("cannot leave terminal UI: {error}"))?;
    cursor_result.map_err(|error| format!("cannot restore terminal cursor: {error}"))?;
    Ok(())
}

fn top_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    options: &CommonOptions,
    interval_seconds: u64,
) -> Result<(), String> {
    let mut snapshot = load_top_snapshot(options)?;
    let mut selected_tab = 0usize;
    let mut selected_row = 0usize;
    let mut last_refresh = Instant::now();
    let refresh_interval = Duration::from_secs(interval_seconds);
    let mut transient_error = None;
    let mut needs_draw = true;
    let mut last_draw = Instant::now();
    let mut dialog = None;

    loop {
        if needs_draw || last_draw.elapsed() >= Duration::from_secs(1) {
            terminal
                .draw(|frame| {
                    draw_top(
                        frame,
                        &snapshot,
                        selected_tab,
                        selected_row,
                        transient_error.as_deref(),
                        interval_seconds,
                        dialog.as_ref(),
                    )
                })
                .map_err(|error| format!("cannot draw terminal UI: {error}"))?;
            needs_draw = false;
            last_draw = Instant::now();
        }

        let wait = refresh_interval
            .saturating_sub(last_refresh.elapsed())
            .min(Duration::from_millis(250));
        if event::poll(wait).map_err(|error| format!("cannot read terminal input: {error}"))? {
            if let Event::Key(key) =
                event::read().map_err(|error| format!("cannot read terminal input: {error}"))?
            {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if let Some(active_dialog) = dialog.as_mut() {
                    match handle_top_dialog_key(active_dialog, key.code, options, &snapshot) {
                        DialogOutcome::Keep => {}
                        DialogOutcome::Close => dialog = None,
                        DialogOutcome::Changed => {
                            dialog = None;
                            match load_top_snapshot(options) {
                                Ok(next) => {
                                    snapshot = next;
                                    selected_row = selected_row
                                        .min(snapshot.allocations.len().saturating_sub(1));
                                    transient_error = None;
                                    last_refresh = Instant::now();
                                }
                                Err(error) => transient_error = Some(error),
                            }
                        }
                    }
                    needs_draw = true;
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('r') => match load_top_snapshot(options) {
                        Ok(next) => {
                            snapshot = next;
                            selected_row =
                                selected_row.min(snapshot.allocations.len().saturating_sub(1));
                            transient_error = None;
                            last_refresh = Instant::now();
                            needs_draw = true;
                        }
                        Err(error) => {
                            transient_error = Some(error);
                            needs_draw = true;
                        }
                    },
                    KeyCode::Char('a') => {
                        let current_path = env::current_dir()
                            .ok()
                            .and_then(|path| path.to_str().map(ToOwned::to_owned))
                            .unwrap_or_default();
                        dialog = Some(TopDialog::Add(AddAllocationForm {
                            field: 0,
                            provider_index: 0,
                            name: workspace_name(&current_path),
                            path: current_path,
                            percent: "10".to_owned(),
                            percent_edited: false,
                            donor_index: 0,
                            error: None,
                        }));
                        needs_draw = true;
                    }
                    KeyCode::Char('e') => {
                        if let Some(allocation) = snapshot.allocations.get(selected_row) {
                            dialog = Some(TopDialog::Edit(EditAllocationForm {
                                field: 0,
                                scope_id: allocation.scope_id.clone(),
                                project: allocation.scope.clone(),
                                provider: allocation.provider.clone(),
                                percent: allocation.limit.to_string(),
                                percent_edited: false,
                                donor_index: 0,
                                error: None,
                            }));
                            needs_draw = true;
                        }
                    }
                    KeyCode::Char('d') => {
                        if let Some(allocation) = snapshot.allocations.get(selected_row) {
                            dialog = Some(TopDialog::Delete {
                                scope_id: allocation.scope_id.clone(),
                                project: allocation.scope.clone(),
                                provider: allocation.provider.clone(),
                                error: None,
                            });
                            needs_draw = true;
                        }
                    }
                    KeyCode::Char('K') | KeyCode::Char('J') => {
                        if let Some(allocation) = snapshot.allocations.get(selected_row) {
                            let direction = if key.code == KeyCode::Char('K') {
                                "up"
                            } else {
                                "down"
                            };
                            let args = vec![
                                "allocations".to_owned(),
                                "move".to_owned(),
                                allocation.scope_id.clone(),
                                direction.to_owned(),
                                "--provider".to_owned(),
                                allocation.provider.clone(),
                                "--json".to_owned(),
                            ];
                            match execute_top_mutation(options, &args) {
                                Ok(()) => match load_top_snapshot(options) {
                                    Ok(next) => {
                                        snapshot = next;
                                        selected_row = if direction == "up" {
                                            selected_row.saturating_sub(1)
                                        } else {
                                            (selected_row + 1)
                                                .min(snapshot.allocations.len().saturating_sub(1))
                                        };
                                        transient_error = None;
                                        last_refresh = Instant::now();
                                    }
                                    Err(error) => transient_error = Some(error),
                                },
                                Err(error) => transient_error = Some(error),
                            }
                            needs_draw = true;
                        }
                    }
                    KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                        selected_tab = (selected_tab + 1) % 3;
                        needs_draw = true;
                    }
                    KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                        selected_tab = (selected_tab + 2) % 3;
                        needs_draw = true;
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        selected_row =
                            (selected_row + 1).min(snapshot.allocations.len().saturating_sub(1));
                        needs_draw = true;
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        selected_row = selected_row.saturating_sub(1);
                        needs_draw = true;
                    }
                    KeyCode::Home => {
                        selected_row = 0;
                        needs_draw = true;
                    }
                    KeyCode::End => {
                        selected_row = snapshot.allocations.len().saturating_sub(1);
                        needs_draw = true;
                    }
                    _ => {}
                }
            }
        }
        if last_refresh.elapsed() >= refresh_interval {
            match load_top_snapshot(options) {
                Ok(next) => {
                    snapshot = next;
                    selected_row = selected_row.min(snapshot.allocations.len().saturating_sub(1));
                    transient_error = None;
                    needs_draw = true;
                }
                Err(error) => {
                    transient_error = Some(error);
                    needs_draw = true;
                }
            }
            last_refresh = Instant::now();
        }
    }
    Ok(())
}

fn top_provider_choices(snapshot: &TopSnapshot) -> Vec<String> {
    let mut providers = Vec::new();
    for source in snapshot
        .state
        .sources
        .iter()
        .filter(|source| source.is_active && is_weekly_window(source))
    {
        if !providers
            .iter()
            .any(|provider: &String| provider.eq_ignore_ascii_case(&source.provider_display_name))
        {
            providers.push(source.provider_display_name.clone());
        }
    }
    providers
}

fn top_donor_choices<'a>(
    snapshot: &'a TopSnapshot,
    provider: &str,
    excluded_scope_id: Option<&str>,
) -> Vec<&'a CliAllocation> {
    snapshot
        .allocations
        .iter()
        .filter(|allocation| {
            allocation.provider.eq_ignore_ascii_case(provider)
                && excluded_scope_id != Some(allocation.scope_id.as_str())
        })
        .collect()
}

fn cycle_choice(index: &mut usize, length: usize, forward: bool) {
    if length == 0 {
        *index = 0;
    } else if forward {
        *index = (*index + 1) % length;
    } else {
        *index = (*index + length - 1) % length;
    }
}

fn edit_dialog_text(value: &mut String, key: KeyCode, numeric: bool) -> bool {
    match key {
        KeyCode::Backspace => {
            value.pop();
            true
        }
        KeyCode::Char(character) if !numeric || character.is_ascii_digit() => {
            value.push(character);
            true
        }
        _ => false,
    }
}

fn edit_percent_text(value: &mut String, edited: &mut bool, key: KeyCode) -> bool {
    match key {
        KeyCode::Backspace => {
            if !*edited {
                value.clear();
                *edited = true;
            } else {
                value.pop();
            }
            true
        }
        KeyCode::Char(character) if character.is_ascii_digit() => {
            if !*edited {
                value.clear();
                *edited = true;
            }
            if value.len() < 3 {
                value.push(character);
            }
            true
        }
        _ => false,
    }
}

fn adjust_percent(value: &mut String, edited: &mut bool, increase: bool) {
    let current = value.parse::<u64>().unwrap_or(1).clamp(1, 100);
    let next = if increase {
        (current + 1).min(100)
    } else {
        current.saturating_sub(1).max(1)
    };
    *value = next.to_string();
    // Arrow adjustment is complete on its own. A later typed digit should
    // still replace the displayed value rather than append to it.
    *edited = false;
}

fn handle_top_dialog_key(
    dialog: &mut TopDialog,
    key: KeyCode,
    options: &CommonOptions,
    snapshot: &TopSnapshot,
) -> DialogOutcome {
    if key == KeyCode::Esc {
        return DialogOutcome::Close;
    }
    match dialog {
        TopDialog::Delete {
            scope_id,
            provider,
            error,
            ..
        } => match key {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                let args = vec![
                    "allocations".to_owned(),
                    "remove".to_owned(),
                    scope_id.clone(),
                    "--provider".to_owned(),
                    provider.clone(),
                    "--json".to_owned(),
                ];
                match execute_top_mutation(options, &args) {
                    Ok(()) => DialogOutcome::Changed,
                    Err(message) => {
                        *error = Some(message);
                        DialogOutcome::Keep
                    }
                }
            }
            KeyCode::Char('n') | KeyCode::Char('N') => DialogOutcome::Close,
            _ => DialogOutcome::Keep,
        },
        TopDialog::Add(form) => {
            let providers = top_provider_choices(snapshot);
            let provider = providers
                .get(form.provider_index)
                .map(String::as_str)
                .unwrap_or("");
            let donors = top_donor_choices(snapshot, provider, None);
            match key {
                KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % 5,
                KeyCode::BackTab | KeyCode::Up => form.field = (form.field + 4) % 5,
                KeyCode::Left if form.field == 0 => {
                    cycle_choice(&mut form.provider_index, providers.len(), false);
                    form.donor_index = 0;
                }
                KeyCode::Right if form.field == 0 => {
                    cycle_choice(&mut form.provider_index, providers.len(), true);
                    form.donor_index = 0;
                }
                KeyCode::Left if form.field == 3 => {
                    adjust_percent(&mut form.percent, &mut form.percent_edited, false);
                }
                KeyCode::Right if form.field == 3 => {
                    adjust_percent(&mut form.percent, &mut form.percent_edited, true);
                }
                KeyCode::Left if form.field == 4 => {
                    cycle_choice(&mut form.donor_index, donors.len() + 1, false);
                }
                KeyCode::Right if form.field == 4 => {
                    cycle_choice(&mut form.donor_index, donors.len() + 1, true);
                }
                KeyCode::Enter if form.field < 4 => form.field += 1,
                KeyCode::Enter => {
                    let Some(provider) = providers.get(form.provider_index) else {
                        form.error = Some("No active weekly provider. Sync first.".to_owned());
                        return DialogOutcome::Keep;
                    };
                    let percent = match parse_allocation_percent(&form.percent) {
                        Ok(percent) => percent,
                        Err(error) => {
                            form.error = Some(error);
                            return DialogOutcome::Keep;
                        }
                    };
                    if form.path.trim().is_empty() {
                        form.error = Some("Folder path is required.".to_owned());
                        return DialogOutcome::Keep;
                    }
                    let mut args = vec![
                        "allocations".to_owned(),
                        "add".to_owned(),
                        "--provider".to_owned(),
                        provider.clone(),
                        "--percent".to_owned(),
                        percent.to_string(),
                        "--path".to_owned(),
                        form.path.trim().to_owned(),
                        "--json".to_owned(),
                    ];
                    if !form.name.trim().is_empty() {
                        args.extend(["--name".to_owned(), form.name.trim().to_owned()]);
                    }
                    if let Some(donor) = form
                        .donor_index
                        .checked_sub(1)
                        .and_then(|index| donors.get(index))
                    {
                        args.extend(["--from".to_owned(), donor.scope_id.clone()]);
                    }
                    return match execute_top_mutation(options, &args) {
                        Ok(()) => DialogOutcome::Changed,
                        Err(error) => {
                            form.error = Some(error);
                            DialogOutcome::Keep
                        }
                    };
                }
                _ => {
                    let changed = match form.field {
                        1 => edit_dialog_text(&mut form.path, key, false),
                        2 => edit_dialog_text(&mut form.name, key, false),
                        3 => edit_percent_text(&mut form.percent, &mut form.percent_edited, key),
                        _ => false,
                    };
                    if changed {
                        form.error = None;
                    }
                }
            }
            DialogOutcome::Keep
        }
        TopDialog::Edit(form) => {
            let donors = top_donor_choices(snapshot, &form.provider, Some(&form.scope_id));
            match key {
                KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % 2,
                KeyCode::BackTab | KeyCode::Up => form.field = (form.field + 1) % 2,
                KeyCode::Left if form.field == 0 => {
                    adjust_percent(&mut form.percent, &mut form.percent_edited, false);
                }
                KeyCode::Right if form.field == 0 => {
                    adjust_percent(&mut form.percent, &mut form.percent_edited, true);
                }
                KeyCode::Left if form.field == 1 => {
                    cycle_choice(&mut form.donor_index, donors.len() + 1, false);
                }
                KeyCode::Right if form.field == 1 => {
                    cycle_choice(&mut form.donor_index, donors.len() + 1, true);
                }
                KeyCode::Enter if form.field == 0 => form.field = 1,
                KeyCode::Enter => {
                    let percent = match parse_allocation_percent(&form.percent) {
                        Ok(percent) => percent,
                        Err(error) => {
                            form.error = Some(error);
                            return DialogOutcome::Keep;
                        }
                    };
                    let mut args = vec![
                        "allocations".to_owned(),
                        "set".to_owned(),
                        form.scope_id.clone(),
                        "--provider".to_owned(),
                        form.provider.clone(),
                        "--percent".to_owned(),
                        percent.to_string(),
                        "--json".to_owned(),
                    ];
                    if let Some(donor) = form
                        .donor_index
                        .checked_sub(1)
                        .and_then(|index| donors.get(index))
                    {
                        args.extend(["--from".to_owned(), donor.scope_id.clone()]);
                    }
                    return match execute_top_mutation(options, &args) {
                        Ok(()) => DialogOutcome::Changed,
                        Err(error) => {
                            form.error = Some(error);
                            DialogOutcome::Keep
                        }
                    };
                }
                _ if form.field == 0
                    && edit_percent_text(&mut form.percent, &mut form.percent_edited, key) =>
                {
                    form.error = None;
                }
                _ => {}
            }
            DialogOutcome::Keep
        }
    }
}

fn execute_top_mutation(options: &CommonOptions, args: &[String]) -> Result<(), String> {
    let executable = env::current_exe()
        .map_err(|error| format!("cannot find the qfence executable: {error}"))?;
    let mut command = Command::new(executable);
    command.args(args);
    if let Some(database) = options.database.as_ref() {
        command.arg("--database").arg(database);
    }
    let output = command
        .output()
        .map_err(|error| format!("cannot run allocation action: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(if message.is_empty() {
        format!("allocation action failed with {}", output.status)
    } else {
        message
    })
}

fn draw_top(
    frame: &mut Frame<'_>,
    snapshot: &TopSnapshot,
    selected_tab: usize,
    selected_row: usize,
    transient_error: Option<&str>,
    interval_seconds: u64,
    dialog: Option<&TopDialog>,
) {
    let area = frame.area();
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(area);
    let provider_count = snapshot
        .state
        .sources
        .iter()
        .map(|source| source.provider_display_name.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>()
        .len();
    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            " QUOTAFENCE ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "  {provider_count} providers  •  {} windows  •  {} projects",
            snapshot.state.sources.len(),
            snapshot.allocations.len()
        )),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" local-first agent quota control "),
    );
    frame.render_widget(title, rows[0]);

    let tabs = Tabs::new(["Overview", "Projects", "History"])
        .select(selected_tab)
        .divider("  ")
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(tabs, rows[1]);

    match selected_tab {
        0 => draw_top_overview(frame, rows[2], snapshot, selected_row),
        1 => draw_top_projects(frame, rows[2], snapshot, selected_row),
        _ => draw_top_history(frame, rows[2], snapshot),
    }

    let status = if let Some(error) = transient_error {
        vec![Line::from(vec![
            Span::styled(
                " refresh failed ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(error),
        ])]
    } else if let Some(warning) = snapshot.warnings.first() {
        vec![Line::from(vec![
            Span::styled(
                " warning ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(warning),
        ])]
    } else {
        vec![
            Line::from(vec![
                Span::styled(" a ", Style::default().fg(Color::Black).bg(Color::Cyan)),
                Span::raw("add  "),
                Span::styled(" e ", Style::default().fg(Color::Black).bg(Color::Cyan)),
                Span::raw("edit  "),
                Span::styled(" d ", Style::default().fg(Color::Black).bg(Color::Cyan)),
                Span::raw("delete  •  "),
                Span::styled(" j ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
                Span::raw("next project  "),
                Span::styled(" k ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
                Span::raw("previous project"),
            ]),
            Line::from(vec![
                Span::styled(" K ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
                Span::raw("priority up  "),
                Span::styled(" J ", Style::default().fg(Color::Black).bg(Color::DarkGray)),
                Span::raw("priority down  •  ←/→ tabs  •  r sync  •  q quit  •  synced "),
                Span::raw(relative_age(
                    Some(snapshot.refreshed_at),
                    now_millis().unwrap_or(snapshot.refreshed_at),
                )),
                Span::raw(format!("  •  auto {interval_seconds}s")),
            ]),
        ]
    };
    frame.render_widget(Paragraph::new(status), rows[3]);
    if let Some(dialog) = dialog {
        draw_top_dialog(frame, area, dialog, snapshot);
    }
}

fn dialog_line(label: &str, value: String, active: bool) -> Line<'static> {
    let marker = if active { "▶" } else { " " };
    let style = if active {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    Line::styled(format!("{marker} {label:<15} {value}"), style)
}

fn top_dialog_area(area: Rect, requested_height: u16) -> Rect {
    let width = area.width.saturating_sub(4).clamp(20, 92).min(area.width);
    let height = requested_height.min(area.height.saturating_sub(2)).max(5);
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn draw_top_dialog(frame: &mut Frame<'_>, area: Rect, dialog: &TopDialog, snapshot: &TopSnapshot) {
    let (title, height, lines) = match dialog {
        TopDialog::Add(form) => {
            let providers = top_provider_choices(snapshot);
            let provider = providers
                .get(form.provider_index)
                .map(String::as_str)
                .unwrap_or("No weekly provider");
            let donors = top_donor_choices(snapshot, provider, None);
            let donor = form
                .donor_index
                .checked_sub(1)
                .and_then(|index| donors.get(index))
                .map_or_else(
                    || "Use unallocated quota".to_owned(),
                    |allocation| format!("{} ({}%)", allocation.scope, allocation.limit),
                );
            let mut lines = vec![
                dialog_line("Provider", format!("‹ {provider} ›"), form.field == 0),
                dialog_line("Folder", form.path.clone(), form.field == 1),
                dialog_line("Name", form.name.clone(), form.field == 2),
                dialog_line(
                    "Weekly budget",
                    format!("{}%", form.percent),
                    form.field == 3,
                ),
                dialog_line("Take from", format!("‹ {donor} ›"), form.field == 4),
                Line::raw(""),
                Line::styled(
                    "↑/↓ or Tab: field  •  ←/→: choice or %  •  Enter: next/add  •  Esc: cancel",
                    Style::default().fg(Color::Gray),
                ),
            ];
            if let Some(error) = form.error.as_ref() {
                lines.push(Line::styled(error.clone(), Style::default().fg(Color::Red)));
            }
            (" Add allocation ", 10, lines)
        }
        TopDialog::Edit(form) => {
            let donors = top_donor_choices(snapshot, &form.provider, Some(&form.scope_id));
            let donor = form
                .donor_index
                .checked_sub(1)
                .and_then(|index| donors.get(index))
                .map_or_else(
                    || "Use unallocated quota".to_owned(),
                    |allocation| format!("{} ({}%)", allocation.scope, allocation.limit),
                );
            let mut lines = vec![
                Line::styled(
                    format!("{} · {}", form.project, form.provider),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Line::raw(""),
                dialog_line(
                    "Weekly budget",
                    format!("{}%", form.percent),
                    form.field == 0,
                ),
                dialog_line("Take from", format!("‹ {donor} ›"), form.field == 1),
                Line::raw(""),
                Line::styled(
                    "↑/↓ or Tab: field  •  ←/→: change  •  type: replace %  •  Enter: next/save",
                    Style::default().fg(Color::Gray),
                ),
            ];
            if let Some(error) = form.error.as_ref() {
                lines.push(Line::styled(error.clone(), Style::default().fg(Color::Red)));
            }
            (" Edit allocation ", 9, lines)
        }
        TopDialog::Delete {
            project,
            provider,
            error,
            ..
        } => {
            let mut lines = vec![
                Line::raw(format!("Remove {project} from {provider}?")),
                Line::raw("The folder binding is removed; usage history is preserved."),
                Line::raw(""),
                Line::styled(
                    "Y / Enter confirm  •  N / Esc cancel",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ];
            if let Some(error) = error.as_ref() {
                lines.push(Line::styled(error.clone(), Style::default().fg(Color::Red)));
            }
            (" Delete allocation ", 7, lines)
        }
    };
    let popup = top_dialog_area(area, height);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        popup,
    );
}

fn draw_top_overview(frame: &mut Frame<'_>, area: Rect, snapshot: &TopSnapshot, selected: usize) {
    let panels = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(area);
    draw_top_sources(frame, panels[0], snapshot);
    draw_top_allocation_table(frame, panels[1], snapshot, selected, " Projects ");
}

fn draw_top_sources(frame: &mut Frame<'_>, area: Rect, snapshot: &TopSnapshot) {
    let header = Row::new(["Provider", "5-hour", "Weekly", "Synced"]).style(
        Style::default()
            .fg(Color::Gray)
            .add_modifier(Modifier::BOLD),
    );
    let mut groups: Vec<(&str, Vec<&QuotaSourceSummary>)> = Vec::new();
    for source in &snapshot.state.sources {
        if let Some((_, sources)) = groups
            .iter_mut()
            .find(|(provider, _)| provider.eq_ignore_ascii_case(&source.provider_display_name))
        {
            sources.push(source);
        } else {
            groups.push((&source.provider_display_name, vec![source]));
        }
    }
    let rows = groups.into_iter().map(|(provider, sources)| {
        let five_hour = sources
            .iter()
            .copied()
            .find(|source| is_five_hour_window(source));
        let weekly = sources
            .iter()
            .copied()
            .find(|source| is_weekly_window(source));
        let (_, synced_at) = grouped_sync_status(&sources);
        Row::new([
            Cell::from(provider.to_owned()).style(Style::default().add_modifier(Modifier::BOLD)),
            top_allowance_cell(five_hour),
            top_allowance_cell(weekly),
            Cell::from(relative_age(synced_at, snapshot.refreshed_at)),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(18),
            Constraint::Percentage(32),
            Constraint::Percentage(32),
            Constraint::Percentage(18),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title(" Allowances "))
    .column_spacing(1);
    frame.render_widget(table, area);
}

fn top_allowance_cell(source: Option<&QuotaSourceSummary>) -> Cell<'static> {
    let Some(source) = source else {
        return Cell::from("—").style(Style::default().fg(Color::DarkGray));
    };
    let left = source
        .provider_used
        .map(|used| source.capacity.saturating_sub(used));
    let reset = chrono::DateTime::from_timestamp_millis(source.ends_at)
        .map(|date| {
            date.with_timezone(&chrono::Local)
                .format("%d %b %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "—".to_owned());
    let value = left.map_or_else(
        || format!("─────   —  {reset}"),
        |left| format!("{} {left:>3}% {reset}", top_quota_bar(left)),
    );
    let color = match left {
        Some(0..=15) => Color::Red,
        Some(16..=35) => Color::Yellow,
        Some(_) => Color::Green,
        None => Color::DarkGray,
    };
    Cell::from(value).style(Style::default().fg(color))
}

fn top_quota_bar(percent: u64) -> String {
    let filled = (percent.min(100) * 5).div_ceil(100) as usize;
    format!("{}{}", "━".repeat(filled), "─".repeat(5 - filled))
}

fn draw_top_projects(frame: &mut Frame<'_>, area: Rect, snapshot: &TopSnapshot, selected: usize) {
    let panels = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(7), Constraint::Length(6)])
        .split(area);
    draw_top_allocation_table(
        frame,
        panels[0],
        snapshot,
        selected,
        " Weekly project allocations ",
    );
    let detail = snapshot.allocations.get(selected).map_or_else(
        || "No allocation selected. Create one with qfence allocations add.".to_owned(),
        |allocation| {
            format!(
                "{}\n{}\nProvider: {}  •  priority {}  •  {}% of weekly quota",
                allocation.scope,
                allocation.path.as_deref().unwrap_or("No folder binding"),
                allocation.provider,
                allocation.priority + 1,
                allocation.limit
            )
        },
    );
    frame.render_widget(
        Paragraph::new(detail).block(Block::default().borders(Borders::ALL).title(" Selection ")),
        panels[1],
    );
}

fn draw_top_allocation_table(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &TopSnapshot,
    selected: usize,
    title: &str,
) {
    let header = Row::new([
        "#", "Project", "Provider", "Budget", "Used", "Left", "State",
    ])
    .style(
        Style::default()
            .fg(Color::Gray)
            .add_modifier(Modifier::BOLD),
    );
    let rows = snapshot.allocations.iter().map(|allocation| {
        let color = match allocation.decision {
            EnforcementDecision::Allow => Color::Green,
            EnforcementDecision::Warn | EnforcementDecision::RequireConfirmation => Color::Yellow,
            EnforcementDecision::Stop => Color::Red,
        };
        Row::new([
            Cell::from((allocation.priority + 1).to_string()),
            Cell::from(allocation.scope.clone()),
            Cell::from(allocation.provider.clone()),
            Cell::from(format!("{}%", allocation.limit)),
            Cell::from(format!("{}%", allocation.used)),
            Cell::from(format!("{}%", allocation.remaining.max(0))),
            Cell::from(decision_label(allocation.decision, false))
                .style(Style::default().fg(color)),
        ])
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(3),
            Constraint::Percentage(30),
            Constraint::Percentage(18),
            Constraint::Length(9),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(12),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("{title} a:add  e:edit  d:delete  K/J:priority ")),
    )
    .row_highlight_style(
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )
    .highlight_symbol("▶ ")
    .column_spacing(1);
    let mut state = TableState::default();
    if !snapshot.allocations.is_empty() {
        state.select(Some(selected.min(snapshot.allocations.len() - 1)));
    }
    frame.render_stateful_widget(table, area, &mut state);
}

fn draw_top_history(frame: &mut Frame<'_>, area: Rect, snapshot: &TopSnapshot) {
    if snapshot.history.is_empty() {
        frame.render_widget(
            Paragraph::new(
                "No weekly history yet. Sync a provider to establish the first checkpoint.",
            )
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Basic history "),
            ),
            area,
        );
        return;
    }
    let constraints = snapshot
        .history
        .iter()
        .map(|_| Constraint::Ratio(1, snapshot.history.len() as u32))
        .collect::<Vec<_>>();
    let panels = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);
    for (item, panel) in snapshot.history.iter().zip(panels.iter()) {
        let data = item
            .days
            .iter()
            .map(|day| day.used_percent)
            .collect::<Vec<_>>();
        let total = data.iter().sum::<u64>();
        let sparkline = Sparkline::default()
            .data(&data)
            .max(data.iter().copied().max().unwrap_or(1).max(1))
            .style(Style::default().fg(Color::Cyan))
            .block(Block::default().borders(Borders::ALL).title(format!(
                " {} · last {} days · {total}% used ",
                item.provider,
                item.days.len()
            )));
        frame.render_widget(sparkline, *panel);
    }
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
  qfence top                          Open the live terminal dashboard
  qfence ls                           List cached quota sources
  qfence history [provider]           Show basic daily usage history
  qfence codex [codex args]           Run Codex with quota protection
  qfence claude [claude args]         Run Claude with quota protection

Workspace:
  qfence here                         Show the current workspace mapping
  qfence bind <workspace>             Bind this folder to an allocation
  qfence allocations                  List workspace allocations
  qfence allocations add --provider codex --percent 20 [--path <folder>]
  qfence allocations set <project> --percent 30 [--from <project>]
  qfence allocations remove <project> [--provider <provider>]
  qfence allocations move <project> <up|down> [--provider <provider>]
  qfence policy                       Show the current workspace policy
  qfence features                     Show enabled product capabilities

More:
  qfence sources show <provider> [--json]
  qfence history [provider] [--days 1..184] [--json]
  qfence top [--interval 30] [--once]
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

fn print_features(snapshot: &EntitlementSnapshot, json: bool) -> Result<(), String> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(snapshot)
                .map_err(|error| format!("cannot serialize capabilities: {error}"))?
        );
        return Ok(());
    }

    let edition = match snapshot.source {
        quotafence_lib::entitlements::EntitlementSource::Free => "FREE · LOCAL CORE",
        quotafence_lib::entitlements::EntitlementSource::License => "LICENSED CAPABILITIES",
    };
    println!("{}  {edition}", paint("QUOTAFENCE", "38;5;45"));
    println!("\nEnabled capabilities\n");
    for capability in &snapshot.capabilities {
        println!("  {} {}", paint("●", "38;5;84"), capability.display_name());
    }
    println!(
        "\n{} capabilities · contract v{} · no license or network required",
        snapshot.capabilities.len(),
        snapshot.schema_version
    );
    Ok(())
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
            "--".to_owned(),
            "--model".to_owned(),
            "sonnet".to_owned(),
        ])
        .unwrap();
        let CliCommand::RunAgent {
            agent, agent_args, ..
        } = command
        else {
            panic!("expected managed Claude run");
        };
        assert_eq!(agent, ManagedAgent::Claude);
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
            CliCommand::Allocations {
                action: AllocationAction::List(_)
            }
        ));
    }

    #[test]
    fn allocation_mutations_have_short_predictable_forms() {
        let command = parse_args(vec![
            "alloc".to_owned(),
            "add".to_owned(),
            "--provider".to_owned(),
            "codex".to_owned(),
            "--percent".to_owned(),
            "20".to_owned(),
            "--from".to_owned(),
            "main".to_owned(),
            "--path".to_owned(),
            "/code/new".to_owned(),
        ])
        .unwrap();
        assert!(matches!(
            command,
            CliCommand::Allocations {
                action: AllocationAction::Add {
                    percent: 20,
                    donor: Some(_),
                    ..
                }
            }
        ));

        let command = parse_args(vec![
            "allocations".to_owned(),
            "move".to_owned(),
            "main".to_owned(),
            "up".to_owned(),
        ])
        .unwrap();
        assert!(matches!(
            command,
            CliCommand::Allocations {
                action: AllocationAction::Move {
                    direction: AllocationMove::Up,
                    ..
                }
            }
        ));
    }

    #[test]
    fn allocation_percent_rejects_zero_and_values_over_one_hundred() {
        assert_eq!(parse_allocation_percent("1").unwrap(), 1);
        assert_eq!(parse_allocation_percent("100").unwrap(), 100);
        assert!(parse_allocation_percent("0").is_err());
        assert!(parse_allocation_percent("101").is_err());
        assert!(parse_allocation_percent("1.5").is_err());
    }

    #[test]
    fn history_and_top_parse_safe_bounds() {
        assert!(matches!(
            parse_args(vec![
                "history".to_owned(),
                "codex".to_owned(),
                "--days".to_owned(),
                "90".to_owned(),
                "--json".to_owned(),
            ])
            .unwrap(),
            CliCommand::History {
                days: 90,
                provider: Some(_),
                options: CommonOptions { json: true, .. }
            }
        ));
        assert!(parse_args(vec![
            "history".to_owned(),
            "--days".to_owned(),
            "185".to_owned()
        ])
        .is_err());
        assert!(matches!(
            parse_args(vec![
                "top".to_owned(),
                "--interval".to_owned(),
                "5".to_owned(),
                "--once".to_owned(),
            ])
            .unwrap(),
            CliCommand::Top {
                interval_seconds: 5,
                once: true,
                ..
            }
        ));
    }

    #[test]
    fn history_glyph_scales_usage_and_keeps_zero_quiet() {
        assert_eq!(history_glyph(0, 10), '·');
        assert_eq!(history_glyph(1, 10), '▁');
        assert_eq!(history_glyph(10, 10), '█');
    }

    #[test]
    fn percentage_editor_replaces_the_prefilled_value() {
        let mut value = "70".to_owned();
        let mut edited = false;
        assert!(edit_percent_text(
            &mut value,
            &mut edited,
            KeyCode::Char('2')
        ));
        assert!(edit_percent_text(
            &mut value,
            &mut edited,
            KeyCode::Char('5')
        ));
        assert_eq!(value, "25");

        adjust_percent(&mut value, &mut edited, true);
        assert_eq!(value, "26");
        assert!(!edited);
        assert!(edit_percent_text(
            &mut value,
            &mut edited,
            KeyCode::Char('9')
        ));
        assert_eq!(value, "9");
    }

    #[test]
    fn allocation_transfer_reuses_free_capacity_then_takes_only_the_shortage() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let database = env::temp_dir().join(format!(
            "quotafence-cli-allocation-{}-{stamp}.sqlite3",
            std::process::id()
        ));
        let mut service = QuotaService::open(&database).unwrap();
        service
            .create_quota_source(quotafence_lib::application::CreateQuotaSource {
                provider_id: "codex".to_owned(),
                provider_display_name: "Codex".to_owned(),
                account_id: "account".to_owned(),
                account_display_name: "Account".to_owned(),
                pool_id: "weekly".to_owned(),
                pool_display_name: "Weekly allowance".to_owned(),
                window_id: "weekly-window".to_owned(),
                starts_at: 0,
                ends_at: i64::MAX,
                capacity: 100,
                unit: "percent".to_owned(),
                provider_snapshot: None,
            })
            .unwrap();
        for (id, name, amount) in [("main", "Main", 80), ("side", "Side", 10)] {
            service
                .create_allocated_workspace(CreateAllocatedWorkspace {
                    id: id.to_owned(),
                    display_name: name.to_owned(),
                    canonical_path: format!("/tmp/{id}"),
                    window_id: "weekly-window".to_owned(),
                    amount,
                    unit: "percent".to_owned(),
                    bound_at: 1,
                })
                .unwrap();
        }
        let dashboard = service
            .dashboard(GetQuotaDashboard {
                window_id: "weekly-window".to_owned(),
                at: 2,
            })
            .unwrap();
        assert_eq!(dashboard.window.unallocated, 10);
        let restore =
            fund_allocation_increase(&mut service, &dashboard, 20, Some("main"), Some("side"))
                .unwrap();
        assert_eq!(restore.as_ref().map(|value| value.amount), Some(80));
        service
            .set_allocation(SetAllocation {
                scope_id: "side".to_owned(),
                window_id: "weekly-window".to_owned(),
                amount: 30,
                unit: "percent".to_owned(),
            })
            .unwrap();
        let dashboard = service
            .dashboard(GetQuotaDashboard {
                window_id: "weekly-window".to_owned(),
                at: 3,
            })
            .unwrap();
        let limits = dashboard
            .allocations
            .iter()
            .map(|allocation| (allocation.scope_id.as_str(), allocation.limit))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(limits.get("main"), Some(&70));
        assert_eq!(limits.get("side"), Some(&30));
        drop(service);
        let _ = fs::remove_file(database);
    }

    #[test]
    fn feature_command_is_simple_and_supports_machine_readable_output() {
        assert!(matches!(
            parse_args(vec!["features".to_owned()]).unwrap(),
            CliCommand::Features { json: false }
        ));
        assert!(matches!(
            parse_args(vec!["capabilities".to_owned(), "--json".to_owned()]).unwrap(),
            CliCommand::Features { json: true }
        ));
        assert!(parse_args(vec!["features".to_owned(), "--database".to_owned()]).is_err());
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
    fn managed_claude_run_is_weekly_only() {
        let error = parse_args(vec![
            "run".to_owned(),
            "claude".to_owned(),
            "--window".to_owned(),
            "5h".to_owned(),
        ])
        .unwrap_err();
        assert!(error.contains("weekly-only"));

        let command = parse_args(vec![
            "run".to_owned(),
            "claude".to_owned(),
            "--window".to_owned(),
            "weekly".to_owned(),
            "--".to_owned(),
            "--model".to_owned(),
            "sonnet".to_owned(),
        ])
        .unwrap();
        let CliCommand::RunAgent {
            agent, agent_args, ..
        } = command
        else {
            panic!("expected managed Claude run");
        };
        assert_eq!(agent, ManagedAgent::Claude);
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
