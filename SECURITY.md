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
- process launching, environment variables, or command arguments;
- repository discovery and filesystem access;
- local quota, attribution, or audit data;
- Tauri capabilities, commands, content security policy, or external links; and
- logs, diagnostics, exports, or crash reports.

The project should reuse supported provider authentication surfaces where
possible and avoid copying provider credentials into its own storage.
