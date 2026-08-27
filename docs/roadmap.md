# QuotaFence roadmap

This roadmap turns QuotaFence from a local quota dashboard into a budget guard in the
execution path of an AI coding agent. It favors small, testable vertical
progress over broad provider coverage or speculative domain rewrites.

## Product direction

QuotaFence protects capacity for important folder-based workspace work. A managed launch
resolves work to a budget, admits or rejects it under policy, supervises the
provider process, and reconciles usage afterward. Routing across providers is a
later extension of this control loop, not part of v0.1.

The initial user is a solo power user running multiple agents or workspaces
against constrained subscriptions. The project quota engine remains open and
local. Commercial work adds convenience, advanced insight, automation, and
optional coordination without making cloud access a requirement for the core.
See the [product and monetization direction](product-and-monetization.md).

## Immediate execution queue

This queue is the source of truth for what maintainers intend to ship next.
Items are ordered by user risk and vertical product value. A later item must not
delay a correctness or false-block fix above it.

### P0 — Make Desktop confirmation semantics honest — complete

Problem: the managed CLI can explicitly accept a confirmation-required
decision, but a Codex Desktop hook has no interactive confirmation channel.
Treating `RequireConfirmation` as `Blocked` therefore makes the confirmation
threshold behave like an undocumented early stop.

- Keep `Warn` advisory for Codex Desktop.
- Do not block a Desktop prompt at `Confirm` until QuotaFence provides a real approval
  interaction.
- Keep `Stop` as the actual blocking boundary.
- Explain capability-specific behavior in Settings and policy help text.
- Preserve managed CLI confirmation and its audited `--yes` override.

Done when: regression tests prove that Desktop allows work across the confirm
boundary, blocks at stop, and the managed CLI still requires explicit
confirmation. Existing customized policies must migrate without silent data
loss.

### P1 — Simplify policy to Warn → Stop — complete

- Remove confirmation controls and dialogs from the product workflow.
- Keep warning advisory and stop as the only blocking boundary.
- Treat persisted confirmation thresholds as legacy no-op data so existing
  beta databases remain readable without a destructive migration.
- Keep historical confirmation tables for migration compatibility, but do not
  create new approval requests or audits.

Done when: Desktop and managed CLI never pause at a confirmation threshold,
warning remains advisory, stop remains blocking, and existing databases open
without user action.

### P2 — Attribution confidence and recovery hardening — complete

- Make scoped usage, unattributed usage, contention, and provider corrections
  visibly distinct in the dashboard.
- Continue reconciling aggregate quota percentage points; do not invent token
  counts for a Codex subscription.
- Add diagnostics for missing lifecycle events, helper working directories,
  disabled hooks, and stale observations.
- Keep reset rollover and provider correction scenarios as release-blocking
  regression tests.

Done when: a beta user can tell why folder usage did or did not move, and the
recovery matrix passes without false blocks or usage leaking across windows.

The Overview now separates folder-tracked usage, unassigned provider usage,
and open observations. Allocation rows also show the amount safely attributed
to that folder without presenting aggregate percentage points as token counts.

### P3 — Installable macOS beta without paid notarization — skipped for now

- Publish universal ad-hoc-signed DMG artifacts and checksums.
- Document the explicit Gatekeeper first-open steps and the security tradeoff.
- Keep notarization optional until an Apple Developer account is configured.
- Add signed/notarized release and updater work only after credentials and an
  update trust model are available.

Done when: another macOS user can verify a checksum, install the beta using the
documented first-open flow, and understand why Gatekeeper warns.

### P4 — Codex beta exit criteria — deferred

- Validate the complete setup, sync, allocation, attribution, warning,
  stop, reset, disable, uninstall, and recovery journey with external testers.
- Resolve correctness and false-block reports before adding another provider.
- Decide whether the daily entry point remains Desktop hooks, the managed CLI,
  or a smaller combined workflow based on observed use.

Done when: the Codex vertical slice is reliable enough for daily use and its
precision/capability limits are visible at the point of action.

The automated and live smoke gates have passed, but the complete installed-app
matrix was explicitly deferred on 13 August 2026. This is not a Pass: the beta
must continue to describe protection as best-effort, and production-readiness
claims remain blocked until the manual rows have dated evidence. The executable
procedures remain in the
[Codex beta exit checklist](beta-exit-checklist.md) for a future release gate.

### After the Codex vertical slice

The project may now evaluate Claude Code or another provider without claiming
that the Codex beta gate passed. The first multi-provider work should define a
capability matrix and preserve unlike units rather than pretending subscription
percentages, credits, USD spend, and concurrency are interchangeable. It must
not weaken Codex correctness or reuse provider-specific assumptions as shared
domain rules.

The initial comparison and Claude Code beta slice are defined in the
[provider capability matrix](provider-capability-matrix.md). Discovery,
reversible status-line setup, 5-hour/7-day checkpoint ingestion, rollover,
lifecycle attribution, new-prompt Warn/Stop protection, and an explicit-window
managed CLI launch are implemented. Cross-provider routing remains
evidence-gated because the native windows are not interchangeable.

### P5 — Commercial beta foundation — in progress

- Centralize Free and future paid behavior behind a capability registry.
- Preserve the full local quota engine, unlimited projects, basic history,
  forecast, alerts, and export in the open core.
- Fall back to Free without deleting or locking data when a future grant is
  missing, invalid, offline, or expired.
- Complete signed distribution, external native testing, privacy/terms/support
  operations, and one real Pro workflow before accepting payment.
- Defer checkout, cloud sync, and Team until those gates pass.

The capability contract, Free resolver, frontend IPC types, and commercial
launch checklist are implemented. Payment, license persistence, automatic
updates, advanced analytics, cloud sync, and Team remain intentionally
unimplemented.

## Current implementation

The repository currently has:

- a provider-neutral amount, window, scope, allocation, reservation, usage, and
  policy domain;
- transactional SQLite storage and versioned migrations;
- a Tauri/React desktop workflow for source setup and manual allocations;
- drag-and-drop folder priority with target-versus-current protection funding;
- a Codex adapter using the official local App Server protocol;
- an opt-in Claude subscription refresh using an existing Claude login, plus a
  reversible CLI status-line observer, for provider-reported 5-hour and 7-day
  windows with memory-only credentials, automatic source creation, and rollover;
- automatic Codex checkpoint refresh on startup and explicit refresh;
- absolute snapshots that do not double-count repeated reads;
- reset-window rollover that carries allocations without carrying old usage;
- canonical folder workspace bindings and a read-only `qfence here` CLI;
- a provider-refreshing `qfence admit codex` dry run with stable policy outcomes;
- a persisted `qfence codex` managed-session lifecycle with atomic reservation,
  direct process supervision, pre/post checkpoint reconciliation, terminal
  cleanup, and Unix/Windows orphan recovery;
- a Claude beta lifecycle covering both provider windows, folder hooks, and
  `qfence claude --window <weekly|5h>` without inventing token totals;
- passive Codex Desktop attribution from local thread metadata at refresh, plus
  optional trusted lifecycle hooks for pre-prompt admission and
  higher-frequency observation;
- persisted Warn -> Stop workspace policy overrides shared by dashboard, CLI
  admission, and managed launch; and
- an evidence-gated managed burn rate and depletion signal for the active
  provider window.

It does **not** yet have in-flight usage enforcement, native Windows console
signal-forwarding validation, or exact token accounting.

## Gap to an end-to-end Codex slice

| Capability | Current state | Required end state |
| --- | --- | --- |
| Provider checkpoint | Implemented | Reused before and after managed work |
| Window rollover | Implemented | Covered during session reconciliation |
| Workspace context | Implemented | Reused for admission and managed launch |
| Managed launch | Implemented in `qfence codex` | Harden from real daily use |
| Reservation | Reserved before spawn, then consumed or released atomically | Add a smaller session cap only with evidence |
| Attribution | Managed deltas are observed and scoped only without visible contention | Richer provider signals when available |
| Reconciliation | Implemented for exact delta, zero, ambiguity, rollover, and failure | Retry/recovery improvements from real usage |
| Policy | Persisted workspace override, then application default | Add provider-specific policy only with evidence |
| Enforcement | Warn or refuse a new prompt/managed launch at Stop | Live termination only with a timely signal |
| Forecasting | Managed-session burn rate with evidence gate | Refine from real history and stronger provider signals |
| Routing | None | Deferred until one provider loop is reliable |

## Milestones

Each milestone should ship in one reviewable PR where practical.

### M0 — Reliable Codex checkpoints — complete

- Discover signed-in Codex subscription windows.
- Replace absolute provider snapshots on refresh.
- Roll reset windows forward while preserving allocations.

Verification: adapter parsing tests, migration tests, snapshot replacement
tests, rollover tests, and manual QA against the installed Codex client.

### M1 — Workspace identity and binding — complete

- Add a workspace binding from any canonical local folder to a workspace scope;
  keep the binding separate from the scope itself.
- Resolve nested working directories to the nearest ancestor binding.
- Add a read-only command such as `qfence here` that reports the resolved scope,
  allocation, window, and remaining capacity.
- Provide an explicit bind command; do not silently create budgets.

Verification: tests for plain non-Git folders, nested directories,
symlinks/case normalization where supported, unmapped workspaces, duplicate
bindings, deleted paths, and no source-file reads.

### M2 — Admission without process launch — complete

- Add the CLI shell and a dry-run admission path.
- Refresh the relevant Codex checkpoint before evaluating admission.
- Evaluate allocation, active reservations, and effective policy.
- Return stable outcomes and exit codes for allow, warn, confirmation required,
  and stop.
- Require an explicit override in non-interactive confirmation cases.

Verification: deterministic application-service tests with a fake clock and
fake provider adapter; CLI contract tests for output and exit codes.

Implemented as `qfence admit codex`. The application boundary receives an explicit
timestamp, the Codex checkpoint application accepts fabricated detection data
in tests, and the CLI reserves exit codes `0`, `10`, `20`, and `30` for policy
outcomes. `--yes` accepts confirmation but never overrides stop.

### M2.5 — Codex Desktop attribution and prompt gate — implemented, conservative

- Read minimal local Codex Desktop thread metadata without parsing rollout
  files, prompts, previews, or responses.
- Establish a baseline and correlate later folder activity with provider
  checkpoint movement during desktop startup or refresh.
- Attribute only when all pending activity resolves to one workspace.
- Keep multiple, unmapped, reset, or interrupted observation gaps
  unattributed.
- Optionally install user-level `UserPromptSubmit` and `Stop`
  hooks for higher-frequency turn boundaries without
  overwriting unrelated hook configuration.
- When at least one Codex allocation exists, reject a new prompt whose working
  folder has no allocation.
- Apply the mapped workspace's allocation boundary before an allowed turn.
- Map the lifecycle event's working folder to its nearest workspace allocation.
- Refresh before and after a turn and append the aggregate percentage delta at
  inferred confidence.
- Mark overlapping turns contended so account-wide consumption is never split
  by guesswork.
- Keep provider failures, zero deltas, rollover, and unmapped work out of the
  scoped ledger.
- Support status and uninstall for the integration.

Verification: read-only scanner schema/privacy tests, baseline and pending
cursor tests, mapped and ambiguous reconciliation tests, hook-schema parser
tests that ignore prompt/transcript fields, installer round-trip tests,
rollover tests, and an end-to-end fake-checkpoint test proving that a 4%
provider delta reduces the mapped folder allocation by 4%.

This milestone makes ordinary Codex app usage observable and optionally gates
new prompts, but does not make the process QuotaFence-managed. Passive scans require
no hook trust. Optional hooks remain user-controlled; explicit policy decisions
block, while integration failures fail open. The gate cannot terminate a turn
already in progress. Aggregate integer percentage checkpoints cannot expose
exact workspace token consumption.

### M3 — One managed Codex session — complete

- Implement `qfence codex [args]`.
- Persist a minimal session record before spawning the process.
- Run Codex in the resolved workspace, inherit the user's terminal, forward
  termination signals, and preserve the provider exit code.
- Spawn a resolved executable with an argument vector; never interpolate user
  input into a shell command.
- Reserve before spawn and release or reconcile on every known exit path.
- Recover stale `starting` or `running` sessions on the next invocation.

Verification: process tests using a fake executable for successful, failed,
interrupted, and crash-recovery paths. No real Codex call is required in CI.

Implemented as `qfence codex`. The wrapper owns the child process, inherits
the terminal, forwards Unix termination signals, preserves the provider exit
code, and atomically pairs a persisted starting session with its reservation.
Terminal transitions release that reservation, and the next invocation
recovers active records whose supervisor process no longer exists on Unix or
Windows. Native Windows console termination forwarding still requires a dated
manual smoke test.

### M4 — Automatic attribution and reconciliation — complete

- Read a provider checkpoint immediately before and after the managed session.
- Link the reservation, session, workspace scope, and resulting usage event.
- Attribute a provider delta only with `observed` confidence unless a stronger
  provider signal exists.
- Keep ambiguous external or concurrent consumption unattributed.
- Initially allow at most one attributable managed session per quota pool when
  the adapter only exposes an aggregate account total.

Verification: fake-adapter tests for zero delta, exact delta, window rollover,
external usage ambiguity, provider failure, and duplicate reconciliation.

Implemented in the managed `qfence codex` exit path. The baseline is persisted
before spawn. A final checkpoint is reconciled in the same transaction as the
terminal session state and reservation transition. Exact non-contended deltas
become immutable workspace usage at `observed` confidence; visible concurrent
work becomes unattributed usage; zero, rollover, and unavailable checkpoints do
not fabricate scoped consumption. Managed child hooks inherit a QuotaFence marker so
the same work is not counted again as an experimental desktop turn.

### M5 — Enforced policy in the daily workflow — complete

- Persist policy at the appropriate allocation or scope boundary.
- Surface advisory warnings in the CLI without hiding provider output.
- Define stop as refusal to launch an over-budget QuotaFence-managed session.
- Only add in-flight termination when the adapter provides a sufficiently
  timely usage signal and QuotaFence owns the process.
- Make `qfence codex` fast enough that bypassing it is less convenient than
  using it.

Verification: policy precedence tests, warning and stop boundary tests, and
proof that unmanaged Codex sessions are never described as hard-enforced.

Implemented with workspace-scope policy rows using integer basis points.
`qfence policy show/set/reset` and the desktop allocation form share the same
application service. `qfence admit codex` remains a side-effect-free assessment;
warning remains advisory and Stop refuses launch. Legacy confirmation data is
kept readable but is not part of the active policy workflow. Live termination
remains intentionally unsupported because the aggregate Codex checkpoint is
not a timely in-flight signal.

### M6 — Burn rate and depletion signal — complete

- Derive burn rate from reconciled managed-session history, not dashboard
  refresh frequency.
- Show the observation window and confidence.
- Forecast whether protected capacity is likely to survive until reset.
- Avoid a precise estimate when history is sparse or mostly unattributed.

Verification: fixed-clock tests for sparse history, no usage, steady usage,
bursty usage, and reset boundaries.

Implemented as a pure, fixed-clock application calculation over terminal
managed-session reconciliation in the current window. A precise signal requires
two trustworthy samples, one hour of observation, and at least 50% attribution
coverage. The dashboard shows the observation duration and confidence; sparse,
mostly unattributed, pre-window, and expired-window states omit the burn rate
and depletion timestamp. Codex aggregate checkpoints cap confidence at medium.

### M7 — Integration health and beta hardening — complete

- Combine provider checkpoint, Desktop metadata scan, workspace mappings, and
  prompt-gate status into one diagnostic view.
- Retain the latest sync failure in the UI instead of relying on a transient
  toast.
- Provide one explicit health check that refreshes the provider, scans Desktop
  metadata, reloads hook configuration, and reads recent protection decisions.
- Distinguish healthy passive tracking from verified prompt protection; an
  installed hook is not sufficient evidence that enforcement is active.
- Exercise real Codex Desktop recovery paths: reset rollover, missing `Stop`,
  helper working directories, app restart, disabled hooks, and concurrent or
  unmapped tasks.

Verification: production frontend build, existing adapter/application suites,
and a manual beta matrix against the installed Codex Desktop client. This
milestone is complete only when reset and recovery scenarios do not falsely
block a mapped workspace.

The diagnostic view and explicit health check are implemented. Provider sync
health is persisted per quota window, so the latest failure remains visible
after an app restart and is cleared by the next successful checkpoint. The
view also reports pending, overlapping, and stale Desktop turn observations so
missing `Stop` recovery is visible instead of silently degrading attribution.
The real Desktop recovery matrix is complete for the first macOS beta. It
covers reset rollover, missing `Stop`, helper working directories, app restart,
disabled hooks, concurrent work, provider corrections, and UI-generated source
identifiers. Newly discovered regressions remain release blockers rather than
being hidden by the milestone status.

The current automated and live results are tracked in the
[Codex Desktop beta recovery matrix](beta-recovery-matrix.md).

### Beta release readiness — current

- Publish honest source-build onboarding, privacy boundaries, and known limits.
- Keep JavaScript, Rust, and Tauri versions aligned for `v0.1.0-beta.1`.
- Provide structured issue forms for sync, attribution, and false-block bugs.
- Require the full frontend and Rust validation suite before tagging.

The repository now builds checksum-bearing macOS and Windows artifacts in
GitHub Actions. macOS tag builds create a draft prerelease; Windows remains an
unsigned workflow artifact until its native smoke matrix and signing strategy
are complete. Automatic updates, Windows signing, and production Apple
notarization remain later release-engineering work.

### Later — Capability-aware routing

Only after M1–M6 work end to end should QuotaFence evaluate another provider. Routing
will need workload priority/deadline plus comparable provider capabilities,
quota, credits, cost, and concurrency. It must not sum unlike units into one
fictional balance.

## Recommended architecture decisions

These are defaults for the next milestones, not permission for a large rewrite.

### Managed-session lifecycle

Use the CLI wrapper first and reuse the existing Rust application layer. A
minimal persisted lifecycle is:

```text
planned → admitted → starting → running → completed | failed | interrupted
```

Persist before side effects, make terminal transitions idempotent, and retain
provider exit status plus timestamps. Track reconciliation separately as
`pending`, `reconciled`, or `unavailable` so reconciliation does not erase the
process outcome. Do not add a daemon until the desktop and CLI genuinely need
shared long-running ownership.

### Workspace mapping

Store an explicit binding from a canonical local folder to a scope ID. The path
is local metadata, not the scope's permanent identity. Resolve a current
directory through the most specific ancestor binding, but do not read workspace
contents. Git and remote-host identity are optional metadata only and should be
added only when path moves become a demonstrated problem.

### Attribution

Treat an aggregate provider delta as an observation, not proof of causality.
When exactly one managed session is active for a pool, QuotaFence may associate the
delta with that session at `observed` confidence. If concurrent or external
usage makes the split ambiguous, retain the ambiguity as unattributed usage.
Never rewrite immutable usage history during reconciliation.

For the first aggregate-only Codex implementation, serialize attributable
managed sessions per quota pool. This is a temporary correctness constraint,
not the long-term concurrency product.

### Enforcement

Separate policy decision from adapter capability:

- warn is always advisory;
- stop initially means refusing to launch through QuotaFence;
- live termination requires both process ownership and a timely, trustworthy
  usage signal; and
- unmanaged sessions remain outside hard enforcement.

Persist the v0.1 policy on the workspace scope so it survives provider-window
rollover. Resolve the effective policy as workspace override, then application
default. Provider-specific overrides should wait for evidence.

### Reservation strategy

The initial session admission should reserve the scope's currently spendable
capacity, not pretend to predict exact session usage. Together with one active
attributable session per pool, this prevents competing launches from consuming
the same capacity. Reconciliation replaces the reservation with observed usage
or releases it when no usage is observed. A lower explicit session cap can be
added later without changing the admission contract.

### Budget dimensions

Do not encode every future concern as a quota unit:

- rate limits, credits, and spend are consumable capacities; represent USD in
  integer minor units rather than floating point;
- concurrency is instantaneous capacity and fits reservation/admission
  semantics better than cumulative usage; and
- priority and deadline describe a workload and influence policy or routing;
  they are not balances.

The existing free-form `QuotaUnit` and pool/window model are sufficient for the
Codex slice. Introduce a resource-kind abstraction only when implementing a
second behavior, not in anticipation of one.

## Decision summary for the next implementation

| Decision | v0.1 default | Revisit when |
| --- | --- | --- |
| Daily entry point | `qfence codex` CLI wrapper | Desktop launch proves materially simpler |
| Long-running owner | CLI child process; no daemon | Multiple clients need shared background ownership |
| Workspace identity | Explicit canonical folder binding | Path moves create real user pain |
| Session concurrency | One attributable session per quota pool | Adapter exposes session-level usage |
| Attribution confidence | `observed` for aggregate pre/post delta | Provider exposes causal session usage |
| Policy ownership | Workspace override, then app default | Provider-specific policy is required |
| Stop semantics | Refuse a QuotaFence-managed launch | Timely live signal supports safe termination |
| Default reservation | Current scope spendable capacity | Reliable session-size estimates exist |
| Domain expansion | Keep existing quota model | A second resource behavior is implemented |

## Change discipline

- Prefer application-service tests before wiring UI or CLI.
- Hide provider processes behind a narrow, injectable adapter boundary.
- Add schema only for the next milestone's invariant.
- Keep current implementation and future intent visibly separate in docs.
- Do not add another provider until the Codex loop is usable end to end.
