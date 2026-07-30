# AQM CLI

The lightweight `aqm` binary shares the Rust application, provider adapter, and
SQLite storage layers with the desktop app. It implements repository identity,
binding, and Codex admission dry runs. It does not launch or enforce a
coding-agent session yet.

## Development usage

From the repository root:

```bash
npm run aqm -- context
```

The command resolves the nearest Git worktree root from the current directory.
Nested directories and symlinked paths normalize to the same canonical root.
It reads Git metadata through the fixed command
`git rev-parse --show-toplevel`; it does not inspect repository source files.

An unmapped repository reports the unbound repository scopes created in the
desktop app:

```text
Repository: /code/example
Scope: unmapped
Available repository scopes:
  Example (repository-...)
Bind with: aqm bind --scope <name-or-id>
```

Binding is always explicit:

```bash
npm run aqm -- bind --scope "Example"
```

The scope reference may be an exact scope ID or an unambiguous,
case-insensitive display name. A Git root and a repository scope can each have
only one active binding.

Use `--path <directory>` to resolve a directory other than the current working
directory, and `--json` for machine-readable output:

```bash
npm run aqm -- context --path /code/example --json
```

`--database <path>` and `AQM_DATABASE_PATH` exist for development and isolated
testing. Without an override, the CLI opens the same operating-system app-data
database as the desktop.

## Admission dry run

After binding the repository, refresh its Codex checkpoint and evaluate policy:

```bash
npm run aqm -- admit codex
```

Admission considers both:

- attributed usage and active reservations against the repository allocation;
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
| `1` | configuration, repository, or provider error | no |

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
capacity, or modify repository files.

## Current boundary

`aqm context` and `aqm admit codex` now cover:

- canonical Git root;
- bound repository scope;
- active provider pool and quota window;
- allocation limit, remaining capacity, and current policy decision;
- pre-admission Codex refresh and reset rollover; and
- effective policy assessment across repository and provider capacity.

The CLI still does not reserve capacity, launch an agent, attribute session
usage, or enforce a decision against a process. Those behaviors begin with the
managed-session milestone.
