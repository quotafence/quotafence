# Security Policy

Agent Quota Manager is pre-release software. Only the latest code on `main` is
currently supported; there are no supported release versions yet.

## Reporting a vulnerability

Please do not open a public issue for a vulnerability.

Use GitHub's private vulnerability reporting for this repository when it is
available. If it is not available, contact the maintainer privately through the
[GitHub profile](https://github.com/buisonanh) before sharing technical details
publicly.

Include a concise description, affected revision, reproduction steps, expected
impact, and any suggested mitigation. Do not include real provider credentials,
session tokens, prompts, or private repository content.

## Security-sensitive areas

Changes receive additional scrutiny when they affect:

- provider authentication or access to an installed agent;
- local provider metadata databases and data-minimizing queries;
- process launching, environment variables, or command arguments;
- user-level Codex hook installation, trust, and pre-prompt decisions;
- repository discovery and filesystem access;
- local quota, attribution, or audit data;
- Tauri capabilities, commands, content security policy, or external links; and
- logs, diagnostics, exports, or crash reports.

The project should reuse supported provider authentication surfaces where
possible and avoid copying provider credentials into its own storage.
Local provider databases must be opened read-only, queried with explicit
columns, and treated as versioned untrusted input. Features must not select or
persist prompt, response, transcript, preview, credential, or source-code
content unless a separately reviewed requirement makes that access necessary.

Codex hook configuration is changed only after an explicit install or uninstall
action. AQM preserves unrelated hook definitions. Although Codex supplies the
full lifecycle payload on standard input, AQM's typed parser ignores prompt,
response, and transcript fields and stores only lifecycle identifiers, the
canonical working folder, allocation context, quota checkpoints, and
timestamps. Infrastructure failures fail open; only explicit allocation or
policy decisions may block a new prompt.
