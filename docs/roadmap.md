# Codex-First Roadmap

This roadmap turns AQM from a local quota dashboard into a budget guard in the
execution path of an AI coding agent. It favors small, testable vertical
progress over broad provider coverage or speculative domain rewrites.

## Product direction

AQM protects capacity for important folder-based workspace work. A managed launch
resolves work to a budget, admits or rejects it under policy, supervises the
provider process, and reconciles usage afterward. Routing across providers is a
later extension of this control loop, not part of v0.1.

The initial user is a solo power user running multiple agents or workspaces
against one constrained Codex subscription. Team governance may become a paid
product later, but cloud workspaces, RBAC, billing, and organization policy are
not current requirements.

## Current implementation

The repository currently has:

- a provider-neutral amount, window, scope, allocation, reservation, usage, and
  policy domain;
- transactional SQLite storage and versioned migrations;
- a Tauri/React desktop workflow for source setup and manual allocations;
- a Codex adapter using the official local App Server protocol;
- automatic Codex checkpoint refresh on startup and explicit refresh;
- absolute snapshots that do not double-count repeated reads;
- reset-window rollover that carries allocations without carrying old usage;
- canonical folder workspace bindings and a read-only `aqm context` CLI;
- a provider-refreshing `aqm admit codex` dry run with stable policy outcomes;
- a persisted `aqm run codex` managed-session lifecycle with atomic reservation,
  direct process supervision, pre/post checkpoint reconciliation, terminal
  cleanup, and Unix orphan recovery; and
- experimental Codex desktop turn attribution through official lifecycle
  hooks, with contention and rollover kept unattributed.

It does **not** yet have persisted per-scope policy, in-flight usage
enforcement, cross-platform orphan recovery, exact token accounting, or
burn-rate forecasting.

## Gap to an end-to-end Codex slice

| Capability | Current state | Required end state |
| --- | --- | --- |
| Provider checkpoint | Implemented | Reused before and after managed work |
| Window rollover | Implemented | Covered during session reconciliation |
| Workspace context | Implemented | Reused for admission and managed launch |
| Managed launch | Implemented in `aqm run codex` | Reused by reconciliation and persisted policy |
| Reservation | Reserved before spawn, then consumed or released atomically | Reused by persisted policy |
| Attribution | Managed deltas are observed and scoped only without visible contention | Richer provider signals when available |
| Reconciliation | Implemented for exact delta, zero, ambiguity, rollover, and failure | Retry/recovery improvements from real usage |
| Policy | In-memory standard thresholds drive dry-run admission | Persisted effective policy drives managed launch |
| Enforcement | Dry-run result and shell exit code only | Warn, confirm, or refuse an AQM-managed launch |
| Forecasting | None | Depletion estimate based on trustworthy history |
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
- Add a read-only command such as `aqm context` that reports the resolved scope,
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

Implemented as `aqm admit codex`. The application boundary receives an explicit
timestamp, the Codex checkpoint application accepts fabricated detection data
in tests, and the CLI reserves exit codes `0`, `10`, `20`, and `30` for policy
outcomes. `--yes` accepts confirmation but never overrides stop.

### M2.5 — Observed Codex desktop attribution — experimental

- Install user-level `UserPromptSubmit`, `Stop`, and `SessionEnd` hooks without
  overwriting unrelated hook configuration.
- Map the lifecycle event's working folder to its nearest workspace allocation.
- Refresh before and after a turn and append the aggregate percentage delta at
  inferred confidence.
- Mark overlapping turns contended so account-wide consumption is never split
  by guesswork.
- Keep provider failures, zero deltas, rollover, and unmapped work out of the
  scoped ledger.
- Support status and uninstall for the integration.

Verification: hook-schema parser tests that ignore prompt/transcript fields,
installer round-trip tests, storage contention/idempotency tests, rollover
tests, and an end-to-end fake-checkpoint test proving that a 4% provider delta
reduces the mapped folder allocation by 4%.

This milestone makes ordinary Codex app usage observable but does not make it
AQM-managed. Hook trust is user-controlled, hooks are fail-open, and aggregate
integer percentage checkpoints cannot expose exact token consumption.

### M3 — One managed Codex session — complete

- Implement `aqm run codex -- [args]`.
- Persist a minimal session record before spawning the process.
- Run Codex in the resolved workspace, inherit the user's terminal, forward
  termination signals, and preserve the provider exit code.
- Spawn a resolved executable with an argument vector; never interpolate user
  input into a shell command.
- Reserve before spawn and release or reconcile on every known exit path.
- Recover stale `starting` or `running` sessions on the next invocation.

Verification: process tests using a fake executable for successful, failed,
interrupted, and crash-recovery paths. No real Codex call is required in CI.

Implemented as `aqm run codex`. The wrapper owns the child process, inherits
the terminal, forwards Unix termination signals, preserves the provider exit
code, and atomically pairs a persisted starting session with its reservation.
Terminal transitions release that reservation, and the next invocation
recovers active records whose supervisor process no longer exists on Unix.

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

Implemented in the managed `aqm run codex` exit path. The baseline is persisted
before spawn. A final checkpoint is reconciled in the same transaction as the
terminal session state and reservation transition. Exact non-contended deltas
become immutable workspace usage at `observed` confidence; visible concurrent
work becomes unattributed usage; zero, rollover, and unavailable checkpoints do
not fabricate scoped consumption. Managed child hooks inherit an AQM marker so
the same work is not counted again as an experimental desktop turn.

### M5 — Enforced policy in the daily workflow

- Persist policy at the appropriate allocation or scope boundary.
- Surface warn and confirmation outcomes in the CLI without hiding provider
  output.
- Define stop as refusal to launch an over-budget AQM-managed session.
- Only add in-flight termination when the adapter provides a sufficiently
  timely usage signal and AQM owns the process.
- Make `aqm run codex` fast enough that bypassing it is less convenient than
  using it.

Verification: policy precedence tests, interactive/non-interactive confirmation
tests, override audit records, and proof that unmanaged Codex sessions are
never described as hard-enforced.

### M6 — Burn rate and depletion signal

- Derive burn rate from reconciled managed-session history, not dashboard
  refresh frequency.
- Show the observation window and confidence.
- Forecast whether protected capacity is likely to survive until reset.
- Avoid a precise estimate when history is sparse or mostly unattributed.

Verification: fixed-clock tests for sparse history, no usage, steady usage,
bursty usage, and reset boundaries.

### Later — Capability-aware routing

Only after M1–M5 work end to end should AQM evaluate another provider. Routing
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
When exactly one managed session is active for a pool, AQM may associate the
delta with that session at `observed` confidence. If concurrent or external
usage makes the split ambiguous, retain the ambiguity as unattributed usage.
Never rewrite immutable usage history during reconciliation.

For the first aggregate-only Codex implementation, serialize attributable
managed sessions per quota pool. This is a temporary correctness constraint,
not the long-term concurrency product.

### Enforcement

Separate policy decision from adapter capability:

- warn is always advisory;
- confirmation is an AQM admission gate;
- stop initially means refusing to launch through AQM;
- live termination requires both process ownership and a timely, trustworthy
  usage signal; and
- unmanaged sessions remain outside hard enforcement.

An override should be explicit and auditable.

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
| Daily entry point | `aqm run codex` CLI wrapper | Desktop launch proves materially simpler |
| Long-running owner | CLI child process; no daemon | Multiple clients need shared background ownership |
| Workspace identity | Explicit canonical folder binding | Path moves create real user pain |
| Session concurrency | One attributable session per quota pool | Adapter exposes session-level usage |
| Attribution confidence | `observed` for aggregate pre/post delta | Provider exposes causal session usage |
| Policy ownership | Workspace override, then app default | Provider-specific policy is required |
| Stop semantics | Refuse an AQM-managed launch | Timely live signal supports safe termination |
| Default reservation | Current scope spendable capacity | Reliable session-size estimates exist |
| Domain expansion | Keep existing quota model | A second resource behavior is implemented |

## Change discipline

- Prefer application-service tests before wiring UI or CLI.
- Hide provider processes behind a narrow, injectable adapter boundary.
- Add schema only for the next milestone's invariant.
- Keep current implementation and future intent visibly separate in docs.
- Do not add another provider until the Codex loop is usable end to end.
