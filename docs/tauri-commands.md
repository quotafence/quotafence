# Tauri Command Boundary

The `src-tauri/src/commands` module is the narrow IPC adapter between the React
webview and application services.

## Startup and state

At startup, Tauri:

1. resolves the operating system's application-data directory;
2. creates that directory when it does not exist;
3. opens `agent-quota-manager.sqlite3` inside it;
4. applies pending storage migrations; and
5. manages one `QuotaService` behind a mutex.

The SQLite connection never crosses the IPC boundary. Serialized access also
matches rusqlite's single-connection model and makes poisoned state a
recoverable IPC error instead of a panic.

## Commands

The approved product commands are:

- `create_provider`
- `create_account`
- `create_quota_pool`
- `create_quota_window`
- `create_quota_source`
- `archive_quota_source`
- `create_allocated_workspace`
- `set_allocation`
- `set_allocation_priority_order`
- `set_workspace_policy`
- `reset_workspace_policy`
- `reserve_quota`
- `release_reservation`
- `get_quota_dashboard`
- `get_local_state`
- `detect_codex_quota`
- `sync_codex_quota`
- `get_codex_protection_status`
- `install_codex_protection`
- `uninstall_codex_protection`
- `get_codex_protection_events`

Mutation and ledger query commands accept one camelCase `request` object. For
example:

```ts
import { invoke } from "@tauri-apps/api/core";

await invoke("create_provider", {
  request: {
    id: "codex",
    displayName: "Codex",
  },
});
```

Application DTOs remain the source of truth for request fields. Domain
constructors validate every request after deserialization, so the webview
cannot bypass identifier, amount, capacity, or lifecycle invariants. New
allocations enter through `create_allocated_workspace`, which atomically binds
one selected folder. Policy mutations accept integer basis points, run domain
range and ordering validation, and can target only an explicitly bound
workspace scope.

`set_allocation_priority_order` accepts one selected window and an ordered list
containing every root allocation exactly once. The order is persisted for the
quota pool, so it applies after provider-window rollover. It changes funding
priority only; it does not mutate allocation targets or historical usage.

## Errors

Rejected commands return a serializable object:

```ts
type IpcError = {
  code: string;
  message: string;
};
```

Stable codes currently include:

- `validation_error`
- `invalid_workspace`
- `integration_error`
- `not_found`
- `conflict`
- `duplicate_source`
- `insufficient_capacity`
- `inconsistent_data`
- `invalid_stored_data`
- `numeric_out_of_range`
- `incompatible_database`
- `storage_unavailable`
- `service_unavailable`

Expected domain failures retain actionable details. Unexpected SQLite failures
are deliberately sanitized so SQL statements, filesystem paths, and internal
database details are not exposed to the webview.

## Trust boundary

Commands expose fixed use cases only. There is no arbitrary SQL, shell, or
filesystem command. The desktop has only the Tauri dialog plugin's
`allow-open` permission, used to choose one folder. The selected path is passed
to `create_allocated_workspace`, canonicalized and verified as a directory in
Rust, then saved atomically with its scope and allocation. Folder contents are
not read.

`detect_codex_quota` may start the fixed
`codex app-server --stdio` process, perform its documented handshake, and read
subscription rate-limit metadata. It does not accept a command string from the
webview and returns sanitized detection states instead of raw process errors.
`sync_codex_quota` applies the selected provider window as an absolute local
snapshot and rolls the local window forward when its reset boundary changes.
In the same blocking task it opens Codex's newest local state database
read-only, selects only minimal thread usage metadata, and reconciles an
unambiguous provider delta to the nearest bound workspace. Its response reports
whether tracking established a baseline, has pending activity, attributed a
delta, remained ambiguous, rolled over, or was unavailable.
`archive_quota_source` hides a quota pool from active selection while retaining
its windows, allocations, snapshots, and append-only usage history.

`get_quota_dashboard` also returns a calculated depletion forecast. The signal
is derived locally from reconciled managed sessions, carries its evidence and
confidence, and omits a precise rate or timestamp when the minimum evidence
gate is not met.

The Codex protection commands manage only AQM's entries in the user-level
Codex hooks file. Installation is an explicit user action and points the hook
at the current desktop executable's non-GUI `hook codex` entrypoint. Codex
remains the source of truth for review and trust. Status distinguishes
`disabled`, `configured`, and `misconfigured`; `configured` means every AQM
handler points at the current executable, not that Codex has trusted its current
hash or enabled the hook. The frontend must direct Desktop users to
**Settings → Hooks → User config** to trust and enable all three AQM entries,
then tell them to restart Codex. `verificationRequiredAfter` is the latest
modification time of the hook file or current executable; only a protection
event at or after that timestamp may mark the current setup active. CLI users
can review the same entries with `/hooks`.
Uninstall removes only AQM-owned handlers.

`get_codex_protection_events` returns at most 20 recent prompt admission
decisions from the local ledger. Entries contain the canonical folder, optional
workspace identity, allow/block outcome, reason, and timestamp; they do not
contain prompt or source-code content.

ID creation remains a caller concern for now, allowing the frontend to keep a
stable identity when retrying a request.
