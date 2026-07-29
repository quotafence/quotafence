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
- scope creation;
- allocation updates;
- quota reservation and release;
- usage recording with optional atomic reservation consumption; and
- quota-dashboard queries.

Command IDs are supplied by the caller to give retries a stable identity. A
duplicate currently returns a storage conflict rather than silently succeeding;
full idempotent-command semantics remain a later application concern. ID
generation belongs at the adapter/command boundary rather than inside domain
entities.

## Dashboard

`QuotaDashboard` contains a window summary and one snapshot per allocation.
Each allocation snapshot includes:

- scope identity, kind, hierarchy, and display name;
- limit, attributed usage, active reservations, remaining, and spendable quota;
- provider-native unit; and
- the effective allow/warn/confirm/stop decision.

The window summary distinguishes:

- capacity;
- quota allocated to root scopes;
- unallocated quota;
- unattributed provider usage; and
- provider-level remaining and spendable capacity.

Child usage and reservations debit both their own allocation and every ancestor
allocation. The provider summary only sums root scopes, avoiding double-counting
child activity.

## Policy

`QuotaService::new` uses the standard 80/90/100 percent policy.
`QuotaService::with_policy` allows a caller or test to inject another validated
policy. Persisting per-scope policy is a later schema/application change.

## Next boundary

The Tauri command layer will:

1. resolve the application-data database path;
2. hold `QuotaService` in synchronized managed state;
3. generate command IDs where appropriate;
4. map typed application errors to stable IPC error codes; and
5. expose only explicitly approved commands to the React frontend.
