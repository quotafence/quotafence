# Application Services

The `src-tauri/src/application` module is the use-case boundary between
Tauri/provider adapters and the domain/storage layers.

## Responsibilities

- Accept transport-friendly command DTOs.
- Construct validated domain objects.
- Coordinate storage repositories and transactions.
- Apply the effective enforcement policy.
- Return stable, serializable read models for the UI.
- Preserve typed domain and storage errors for the future IPC adapter.

The layer contains no Tauri commands, filesystem-path discovery, provider
processes, or UI state.

Application DTOs serialize field names as `camelCase` for React/TypeScript.
Provider and policy enum values remain explicit `snake_case` strings.

## Commands

The current command DTOs cover:

- provider, account, quota-pool, and quota-window creation;
- atomic quota-source onboarding;
- atomic folder selection, workspace allocation, and binding;
- explicit canonical-folder to workspace-scope binding;
- nearest-ancestor workspace context and active-allocation queries;
- provider-neutral workspace admission assessment;
- effective workspace-policy reads, validated overrides, and reset to defaults;
- managed-session admission, reservation, process-state transitions, and
  recovery reads;
- managed-session baseline capture plus atomic terminal reconciliation and
  reservation transition;
- allocation updates;
- quota reservation and release;
- usage recording with optional atomic reservation consumption;
- absolute provider-snapshot synchronization and reset rollover;
- optional Codex Desktop cursor reconciliation in the same snapshot
  transaction;
- provider-turn baseline creation, lookup, reconciliation, and cleanup;
- burn-rate and depletion forecasting from reconciled managed sessions;
- quota-dashboard queries; and
- complete local-state queries for application startup and refresh.

The CLI and Tauri boundary canonicalize folders outside this layer, then pass a
canonical path to these use cases. Application services never scan workspace
contents and do not silently create a scope or budget for an unmapped path.

`EvaluateWorkspaceAdmission` selects the active allocation for a bound
workspace and provider. Its effective decision is the more restrictive of the
workspace allocation balance and account-wide provider capacity. Refreshing a
provider checkpoint remains an adapter responsibility and happens before this
use case is evaluated.

Provider-turn observation is deliberately split around that adapter boundary.
The adapter refreshes a checkpoint, then
`BeginProviderTurnObservation` stores its absolute baseline. At turn end the
adapter refreshes again and `ReconcileProviderTurnObservation` atomically
removes the active observation and appends an inferred, provider-observed usage
event only when the stored turn is unambiguous. Missing checkpoints, rollover,
unmapped work, and concurrency produce no scoped event.

`SyncProviderQuota` may also receive a complete passive desktop metadata scan.
The service advances per-thread cursors and the provider snapshot atomically.
The first scan is baseline-only. A later provider delta is scoped only when all
pending thread activity resolves to one workspace; otherwise it stays visible
only in the provider total. Callers that did not perform a desktop scan pass no
observation value, which invalidates pending correlation instead of treating an
unknown scan as an empty one.

`PrepareManagedSession` re-evaluates admission, applies stop and explicit
confirmation boundaries, reserves the workspace's current spendable capacity,
and persists the starting session atomically. When `--yes` accepts a real
confirmation boundary, its audit record is committed in that same transaction.
The CLI reports the child PID
through `MarkManagedSessionRunning`; `FinishManagedSession` then commits the
terminal outcome and reservation release together. Process spawning and signal
handling remain outside the application layer.

Command IDs are supplied by the caller to give retries a stable identity. A
duplicate currently returns a storage conflict rather than silently succeeding;
full idempotent-command semantics remain a later application concern. ID
generation belongs at the adapter/command boundary rather than inside domain
entities.

## Dashboard

`QuotaDashboard` contains a window summary and one snapshot per allocation.
Each workspace allocation snapshot includes:

- workspace identity, canonical folder, and display name;
- persisted priority;
- target limit, attributed usage, active reservations, remaining, and spendable
  quota;
- the amount protected now after higher-priority targets are funded from
  current provider capacity;
- provider-native unit; and
- the effective allow/warn/confirm/stop decision.

The window summary distinguishes:

- capacity;
- quota allocated to root scopes;
- unallocated quota;
- unattributed provider usage; and
- provider-level remaining and spendable capacity.

The overview derives its live window composition from the same snapshot:

- used capacity is `capacity - provider_spendable`;
- planned/protected capacity is the sum of priority-funded `protected_now`;
- unassigned capacity is the remainder of `provider_spendable` after that
  funding.

These three values account for the full provider window. They are intentionally
different from the static sum of allocation targets, especially after usage has
already consumed part of the window.

`QuotaDashboard::forecast` uses only terminal managed sessions reconciled in
the selected window. The pure calculation receives an explicit current time,
reports its observation interval, sample count, attribution coverage, and
confidence, and withholds rate/depletion fields when the evidence gate is not
met. Provider refresh frequency is not an input.

Each current allocation is top-level and maps to one local folder. A provider
snapshot is reconciled with local observations using the greater total; active
reservations remain additional committed capacity because they have not yet
appeared in provider usage. Legacy nested rows remain readable for migration
compatibility but cannot be created through the desktop IPC boundary.

`SetAllocationPriorityOrder` validates that the submitted order contains every
root allocation in the selected window exactly once, then persists ranks for
the quota pool. Priority therefore survives window rollover. Dashboard funding
walks bound root allocations in that order; a target is never reinterpreted as
a percentage of current remaining quota.

## Policy

`QuotaService::new` uses the standard 80/90/100 percent policy.
`QuotaService::with_policy` allows a caller or test to inject another validated
application default. A persisted workspace-scope override takes precedence and
survives provider-window rollover. Dashboard, context, admission, and managed
launches all resolve that same effective policy.

`allow`, `warn`, `require_confirmation`, and `stop` remain assessment results
at this layer. A confirmation is enforced and audited only for an actual
AQM-managed launch; a stop refuses that launch. The trusted Desktop prompt gate
uses priority-funded `protected_now` plus the allocation-specific result and
rejects non-interactive confirmation or stop before the prompt starts. Usage
outside a trusted AQM gate can still reduce real provider capacity and therefore
current protection; AQM cannot recreate capacity already consumed. A dry run
has no audit side effect, and an already-running Codex turn is not terminated.
The dashboard may still compute the same priority-funded amounts while the
Desktop hook is unverified, but the UI labels them as planned capacity. Provider
usage consumes the remaining unassigned buffer before reducing funded
allocations from lowest to highest priority.

## Tauri boundary

The implemented Tauri boundary resolves the application-data database path,
holds `QuotaService` in synchronized managed state, maps typed application
errors to stable IPC errors, and exposes fixed use cases to the React frontend.
See [Tauri commands](tauri-commands.md).

The caller continues to supply stable command IDs. ID generation and retry
semantics will be designed with the frontend workflow rather than hidden inside
the application service.
