# AQM CLI

The lightweight `aqm` binary shares the Rust application, provider adapter, and
SQLite storage layers with the desktop app. It implements workspace identity,
binding, and Codex admission dry runs. It does not launch or enforce a
coding-agent session yet.

## Development usage

From any local folder:

```bash
npm run aqm -- context
```

The command canonicalizes the current directory without invoking Git. If that
folder is nested under one or more bindings, the most specific ancestor
workspace wins. Symlinked paths normalize to the same canonical folder.
Workspace contents are not inspected.

An unmapped folder reports the unbound workspace scopes created in the
desktop app:

```text
Workspace path: /code/example
Workspace: unmapped
Available workspace scopes:
  Example (workspace-...)
Bind with: aqm bind --scope <name-or-id>
```

Binding is always explicit:

```bash
npm run aqm -- bind --scope "Example"
```

The scope reference may be an exact scope ID or an unambiguous,
case-insensitive display name. A canonical folder and a workspace scope can
each have only one active binding.

Use `--path <directory>` to resolve a directory other than the current working
directory, and `--json` for machine-readable output:

```bash
npm run aqm -- context --path /code/example --json
```

`--database <path>` and `AQM_DATABASE_PATH` exist for development and isolated
testing. Without an override, the CLI opens the same operating-system app-data
database as the desktop.

## Admission dry run

After binding the workspace, refresh its Codex checkpoint and evaluate policy:

```bash
npm run aqm -- admit codex
```

Admission considers both:

- attributed usage and active reservations against the workspace allocation;
- aggregate provider usage and active reservations against the subscription
  window.

The more restrictive signal wins. Standard policy thresholds are 80% for
`warn`, 90% for `require_confirmation`, and 100% for `stop`.

The command is deliberately non-interactive and returns stable shell exit
codes:

| Exit code | Outcome | Would a managed launch proceed? |
| ---: | --- | --- |
| `0` | allow | yes |
| `10` | warn | yes, with a warning |
| `20` | confirmation required | no, unless explicitly accepted |
| `30` | stop | no |
| `1` | configuration, workspace, or provider error | no |

Use `--yes` to explicitly accept only a confirmation-required outcome:

```bash
npm run aqm -- admit codex --yes
```

The assessment still reports `require_confirmation`, records that the override
was applied in JSON output, and exits `0`. `--yes` never overrides `stop`.

Use `--json` for a stable object containing the assessment, checkpoint result,
override state, proceed flag, and exit code:

```bash
npm run aqm -- admit codex --json
```

The command starts the official local Codex App Server only long enough to read
the subscription checkpoint. It does not launch a coding session, reserve
capacity, or read or modify workspace files.

## Current boundary

`aqm context` and `aqm admit codex` now cover:

- canonical current folder and nearest bound workspace;
- bound workspace scope;
- active provider pool and quota window;
- allocation limit, remaining capacity, and current policy decision;
- pre-admission Codex refresh and reset rollover; and
- effective policy assessment across workspace and provider capacity.

The CLI still does not reserve capacity, launch an agent, attribute session
usage, or enforce a decision against a process. Those behaviors begin with the
managed-session milestone.
