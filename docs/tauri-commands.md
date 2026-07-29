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
- `create_scope`
- `set_allocation`
- `reserve_quota`
- `release_reservation`
- `record_usage`
- `get_quota_dashboard`

Every command accepts one camelCase `request` object. For example:

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
cannot bypass identifier, amount, hierarchy, or lifecycle invariants.

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
- `not_found`
- `conflict`
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
filesystem command. ID creation remains a caller concern for now, allowing the
frontend to keep a stable identity when retrying a request.
