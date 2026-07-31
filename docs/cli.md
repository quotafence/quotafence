# AQM CLI

The lightweight `aqm` binary shares the Rust application, provider adapter, and
SQLite storage layers with the desktop app. It implements workspace identity,
binding, Codex admission dry runs, one managed Codex process, and an
experimental Codex lifecycle-hook entrypoint.

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

## Workspace policy

Inspect the effective policy for the current bound folder:

```bash
npm run aqm -- policy show
```

The standard default is warn at 80%, require confirmation at 90%, and stop at
100% of the workspace allocation consumed. Persist a folder override by
supplying all three thresholds:

```bash
npm run aqm -- policy set --warn 75 --confirm 90 --stop 100
```

Values accept up to two decimal places. Use `off` to disable a threshold while
still preserving the required ordering among enabled thresholds:

```bash
npm run aqm -- policy set --warn off --confirm 90 --stop 100
```

Return to application defaults with:

```bash
npm run aqm -- policy reset
```

These commands accept `--path`, `--database`, and `--json`. Policy is stored on
the workspace, not the current provider window, so it survives quota rollover.

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

## Managed Codex session

Run Codex through the allocation bound to the current folder:

```bash
npm run aqm -- run codex
```

The wrapper:

1. on Unix, recovers orphaned managed sessions whose supervisor process no
   longer exists;
2. resolves the nearest bound workspace and refreshes its Codex checkpoint;
3. evaluates the workspace and provider policy boundary;
4. reserves the workspace's current spendable capacity;
5. starts the resolved Codex executable directly in that folder with inherited
   stdin, stdout, and stderr;
6. forwards `SIGINT` and `SIGTERM` on Unix and preserves ordinary Codex exit
   codes;
7. refreshes the same provider checkpoint after Codex exits; and
8. atomically records an unambiguous observed delta, marks the session
   terminal, and consumes or releases its reservation.

Confirmation-required launches need an explicit override:

```bash
npm run aqm -- run codex --yes
```

`--yes` never overrides a stop decision. Pass Codex arguments after a separator
so they cannot be confused with AQM options:

```bash
npm run aqm -- run codex -- --model gpt-5
```

The child is spawned with an argument vector, never an interpolated shell
command. AQM resolves Codex from `AGENT_QUOTA_CODEX_BIN`, `PATH`, and supported
installation locations. It stores folder and process metadata, but does not
read prompts, source files, transcripts, or provider credentials.

An accepted confirmation is written to the local audit table atomically with
the managed session and reservation. `aqm admit codex --yes` is only a dry-run
preview and deliberately does not create that audit record.

The baseline is persisted before spawn. A same-window delta is attributed to
the workspace at `observed` confidence only when AQM has not seen concurrent
Codex work. Visible contention keeps the delta unattributed; rollover, a lower
counter, or an unavailable final checkpoint never creates scoped usage.
Provider percentage checkpoints are aggregate and integer-valued, so external
usage that AQM cannot observe remains a known source of uncertainty.

Hooks launched by this managed Codex child inherit an AQM session marker and
return without recording a second turn observation.

## Codex Desktop protection and attribution

Install user-level lifecycle hooks with the development CLI:

```bash
npm run aqm -- hooks install codex
```

The installer merges AQM handlers into `~/.codex/hooks.json`, preserves
unrelated hooks, and creates `~/.codex/hooks.json.aqm.bak` before its first
change to an existing file. Re-running it is idempotent.

Codex does not run a new non-managed command hook until the exact definition is
reviewed, trusted, and enabled.

In Codex Desktop:

1. Open **Settings → Hooks → User config**.
2. Review, trust, and switch on the AQM entries under `UserPromptSubmit`,
   `Stop`, and `SessionEnd`.
3. Restart Codex, then start a new task so the session loads the updated
   configuration.

In Codex CLI, run `/hooks` and review the same three entries before restarting
the session. Until all three hooks are trusted and switched on, Codex prompts
can still run without AQM protection.

The installed lifecycle is:

1. `UserPromptSubmit` invokes `aqm hook codex` in the task's working folder.
2. When at least one Codex allocation exists, AQM blocks a prompt from an
   unallocated folder. For an allocated folder it refreshes quota and applies
   that workspace's warn, confirmation, and stop policy.
3. Allowed prompts capture an absolute provider baseline.
4. `Stop` captures another checkpoint and records the delta against the folder
   only when the turn was mapped, uncontended, and remained in the same window.
5. `SessionEnd` removes unfinished observations.

These event names and stdin fields follow the official
[Codex hooks contract](https://learn.chatgpt.com/docs/hooks).

Check or remove the integration:

```bash
npm run aqm -- hooks status codex
npm run aqm -- hooks uninstall codex
```

`status` verifies the AQM definitions in the JSON file; Codex remains the source
of truth for whether their current hash has been trusted and the hook is
enabled.

The desktop exposes the same integration in Settings as an on/off control.
Turning it off removes only handlers marked as AQM-owned. A partial
configuration or one that points at an old application executable is shown as
needing repair; turning protection on again replaces those entries with the
current executable.

Explicit allocation and policy decisions may return the official
`{"decision":"block"}` response. Infrastructure failures remain fail-open so a
broken local integration cannot permanently lock Codex. Set
`AQM_HOOK_DEBUG=1` only while diagnosing integration errors.

Current precision limits:

- Codex exposes the subscription checkpoint as an account-wide integer
  percentage, not a fixed token allowance.
- A small turn may consume tokens without moving that integer percentage.
- If two observed turns overlap, neither receives the shared provider delta; it
  remains unattributed.
- Usage outside AQM hooks between the two checkpoints is indistinguishable from
  the observed turn and is why the scoped event carries `inferred` confidence.
- The hook can refuse a new prompt but cannot terminate a turn that already
  started, cover another machine, or protect usage before the hook is installed
  and trusted.

Codex includes prompt and transcript fields in some lifecycle event payloads.
AQM's typed hook parser ignores those fields and stores only session ID, turn
ID, event type, canonical folder, optional scope, window baseline, timestamps,
and contention state. For visibility, each admitted or blocked
`UserPromptSubmit` also stores its folder, optional workspace, outcome, reason,
and timestamp. It never stores prompt text.

Installation from the desktop points the hook at the installed Agent Quota
Manager executable, which has a non-GUI `hook codex` entrypoint. Development
CLI installation points at the current compiled `aqm` binary; removing that
build directory requires reinstalling the hook.

## Current boundary

`aqm context`, `aqm admit codex`, `aqm run codex`, and the experimental hook
entrypoint now cover:

- canonical current folder and nearest bound workspace;
- bound workspace scope;
- active provider pool and quota window;
- allocation limit, remaining capacity, and current policy decision;
- pre-admission Codex refresh and reset rollover;
- effective policy assessment across workspace and provider capacity;
- persisted workspace policy configuration through CLI and desktop;
- persisted managed-session lifecycle and one active reservation per provider
  pool;
- direct child-process launch, inherited terminal, signal forwarding, exit-code
  preservation, and orphan recovery;
- managed pre/post checkpoints with atomic usage and reservation
  reconciliation;
- per-turn provider baselines and rollover-safe reconciliation; and
- inferred attribution for one uncontended mapped Codex desktop turn.

Managed launches now refuse stop decisions and require `--yes` at a
confirmation boundary. They reconcile their own provider delta at `observed`
confidence, but do not terminate a running process because the provider does
not expose a sufficiently timely live quota signal. Unmanaged work remains
outside hard enforcement. Hook attribution remains experimental and `inferred`.
