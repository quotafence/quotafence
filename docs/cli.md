# AQM CLI

The lightweight `aqm` binary shares the Rust application and SQLite storage
layers with the desktop app. It currently implements repository identity and
binding only; it does not launch or enforce a coding-agent session yet.

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

## Current boundary

`aqm context` reports:

- canonical Git root;
- bound repository scope;
- active provider pool and quota window;
- allocation limit, remaining capacity, and current policy decision.

It does not refresh a provider, reserve capacity, launch an agent, or enforce a
decision. Those behaviors begin in the admission and managed-session
milestones.
