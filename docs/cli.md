# AQM CLI

The lightweight `aqm` binary shares the Rust application, provider adapter, and
SQLite storage layers with the desktop app. It implements workspace identity,
binding, Codex admission dry runs, and an experimental Codex lifecycle-hook
entrypoint. It does not launch or enforce a coding-agent session yet.

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

## Experimental Codex desktop tracking

Install user-level lifecycle hooks with the development CLI:

```bash
npm run aqm -- hooks install codex
```

The installer merges AQM handlers into `~/.codex/hooks.json`, preserves
unrelated hooks, and creates `~/.codex/hooks.json.aqm.bak` before its first
change to an existing file. Re-running it is idempotent.

Codex does not run a new non-managed command hook until the exact definition is
reviewed and trusted. Open Codex CLI, run `/hooks`, review the three AQM hooks,
and trust them. If Codex app was already open, restart it, then start a new task
so the session loads the updated configuration.

The installed lifecycle is:

1. `UserPromptSubmit` invokes `aqm hook codex` in the task's working folder.
2. AQM resolves the folder binding and captures an absolute provider baseline.
3. `Stop` captures another checkpoint and records the delta against the folder
   only when the turn was mapped, uncontended, and remained in the same window.
4. `SessionEnd` removes unfinished observations.

These event names and stdin fields follow the official
[Codex hooks contract](https://learn.chatgpt.com/docs/hooks).

Check or remove the integration:

```bash
npm run aqm -- hooks status codex
npm run aqm -- hooks uninstall codex
```

`status` verifies the AQM definitions in the JSON file; Codex remains the source
of truth for whether their current hash has been trusted.

The hook entrypoint is fail-open: a parse, database, or provider failure never
blocks the Codex task. Set `AQM_HOOK_DEBUG=1` only while diagnosing integration
errors.

Current precision limits:

- Codex exposes the subscription checkpoint as an account-wide integer
  percentage, not a fixed token allowance.
- A small turn may consume tokens without moving that integer percentage.
- If two observed turns overlap, neither receives the shared provider delta; it
  remains unattributed.
- Usage outside AQM hooks between the two checkpoints is indistinguishable from
  the observed turn and is why the scoped event carries `inferred` confidence.
- These hooks observe Codex app usage but do not make the session AQM-managed
  and cannot hard-stop it.

Codex includes prompt and transcript fields in some lifecycle event payloads.
AQM's typed hook parser ignores those fields and stores only session ID, turn
ID, event type, canonical folder, optional scope, window baseline, timestamps,
and contention state.

During development, the hook command points to the current compiled
`target/debug/aqm` binary. Removing the build directory breaks that hook until
you run the install command again. Release packaging for a stable installed CLI
path remains future work.

## Current boundary

`aqm context`, `aqm admit codex`, and the experimental hook entrypoint now
cover:

- canonical current folder and nearest bound workspace;
- bound workspace scope;
- active provider pool and quota window;
- allocation limit, remaining capacity, and current policy decision;
- pre-admission Codex refresh and reset rollover;
- effective policy assessment across workspace and provider capacity;
- per-turn provider baselines and rollover-safe reconciliation; and
- inferred attribution for one uncontended mapped Codex desktop turn.

The CLI still does not reserve capacity, launch an agent, or enforce a decision
against a process. Hook attribution is observed rather than managed. Those
behaviors begin with the managed-session milestone.
