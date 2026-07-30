# Codex-First Roadmap

This roadmap turns AQM from a local quota dashboard into a budget guard in the
execution path of an AI coding agent. It favors small, testable vertical
progress over broad provider coverage or speculative domain rewrites.

## Product direction

AQM protects capacity for important repository and task work. A managed launch
resolves work to a budget, admits or rejects it under policy, supervises the
provider process, and reconciles usage afterward. Routing across providers is a
later extension of this control loop, not part of v0.1.

The initial user is a solo power user running multiple agents or repositories
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
- absolute snapshots that do not double-count repeated reads; and
- reset-window rollover that carries allocations without carrying old usage.
- canonical Git repository bindings and a read-only `aqm context` CLI.

It does **not** yet have managed-session records, provider process supervision,
automatic session attribution,
persisted per-scope policy, admission enforcement, or burn-rate forecasting.

## Gap to an end-to-end Codex slice

| Capability | Current state | Required end state |
| --- | --- | --- |
| Provider checkpoint | Implemented | Reused before and after managed work |
| Window rollover | Implemented | Covered during session reconciliation |
| Repository context | Implemented | Reused for admission and managed launch |
| Managed launch | Detection process only | AQM owns the Codex child lifecycle and exit result |
| Reservation | Domain/storage implemented; not in daily workflow | Admission reserves capacity before spawn |
| Attribution | Ledger primitives only; no user-entered estimates | Session result produces scoped observed usage |
| Reconciliation | Provider total affects dashboard | Pre/post session delta is reconciled without false precision |
| Policy | In-memory standard thresholds; read-only decision | Persisted effective policy drives CLI behavior |
| Enforcement | None | Warn, confirm, or refuse an AQM-managed launch |
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

### M1 — Repository identity and binding — complete

- Add a repository binding from a canonical Git worktree root to a repository
  scope; keep the binding separate from the scope itself.
- Resolve nested working directories to the nearest repository root.
- Add a read-only command such as `aqm context` that reports the resolved scope,
  allocation, window, and remaining capacity.
- Provide an explicit bind command; do not silently create budgets.

Verification: tests for nested directories, symlinks/case normalization where
supported, unmapped repositories, duplicate bindings, deleted paths, and no
source-file reads.

### M2 — Admission without process launch

- Add the CLI shell and a dry-run admission path.
- Refresh the relevant Codex checkpoint before evaluating admission.
- Evaluate allocation, active reservations, and effective policy.
- Return stable outcomes and exit codes for allow, warn, confirmation required,
  and stop.
- Require an explicit override in non-interactive confirmation cases.

Verification: deterministic application-service tests with a fake clock and
fake provider adapter; CLI contract tests for output and exit codes.

### M3 — One managed Codex session

- Implement `aqm run codex -- [args]`.
- Persist a minimal session record before spawning the process.
- Run Codex in the resolved repository, inherit the user's terminal, forward
  termination signals, and preserve the provider exit code.
- Spawn a resolved executable with an argument vector; never interpolate user
  input into a shell command.
- Reserve before spawn and release or reconcile on every known exit path.
- Recover stale `starting` or `running` sessions on the next invocation.

Verification: process tests using a fake executable for successful, failed,
interrupted, and crash-recovery paths. No real Codex call is required in CI.

### M4 — Automatic attribution and reconciliation

- Read a provider checkpoint immediately before and after the managed session.
- Link the reservation, session, repository scope, and resulting usage event.
- Attribute a provider delta only with `observed` confidence unless a stronger
  provider signal exists.
- Keep ambiguous external or concurrent consumption unattributed.
- Initially allow at most one attributable managed session per quota pool when
  the adapter only exposes an aggregate account total.

Verification: fake-adapter tests for zero delta, exact delta, window rollover,
external usage ambiguity, provider failure, and duplicate reconciliation.

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

### Repository mapping

Store an explicit binding from a canonical local repository root to a scope ID.
The path is local metadata, not the scope's permanent identity. Discover the Git
root from the current working directory, but do not read repository contents.
Remote identity or a privacy-preserving fingerprint can be added only when path
moves become a demonstrated problem.

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

Persist the v0.1 policy on the repository scope so it survives provider-window
rollover. Resolve the effective policy as scope override, then application
default. More granular task or provider overrides should wait for evidence.

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
| Repository identity | Explicit canonical local-root binding | Path moves create real user pain |
| Session concurrency | One attributable session per quota pool | Adapter exposes session-level usage |
| Attribution confidence | `observed` for aggregate pre/post delta | Provider exposes causal session usage |
| Policy ownership | Repository scope override, then app default | Task/provider-specific policy is required |
| Stop semantics | Refuse an AQM-managed launch | Timely live signal supports safe termination |
| Default reservation | Current scope spendable capacity | Reliable session-size estimates exist |
| Domain expansion | Keep existing quota model | A second resource behavior is implemented |

## Change discipline

- Prefer application-service tests before wiring UI or CLI.
- Hide provider processes behind a narrow, injectable adapter boundary.
- Add schema only for the next milestone's invariant.
- Keep current implementation and future intent visibly separate in docs.
- Do not add another provider until the Codex loop is usable end to end.
