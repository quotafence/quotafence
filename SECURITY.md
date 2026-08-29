# Security policy

QuotaFence is an early beta. Security reports are welcome, especially for local
credential exposure, hook configuration changes, command execution, update or
release integrity, path handling, and accidental collection of prompts, source
code, or transcripts.

## Report privately

For the public beta, use GitHub's **Report a vulnerability** flow in the
repository Security tab to open a private security advisory. Maintainers must
enable and verify that channel before changing the repository to public. If the
button is absent, do not publish an issue for an unpatched vulnerability;
contact the maintainer through the
[GitHub profile](https://github.com/buisonanh) before sharing technical details.
Never include real provider credentials, prompts, transcripts, source files,
or a complete user database in a report.

Include the affected QuotaFence version/commit, operating system, impact,
minimal reproduction steps, and sanitized logs when available. Maintainers
will acknowledge the advisory, reproduce it, coordinate a fix and disclosure,
and credit the reporter when requested. Response-time guarantees are not yet
offered during the beta.

## Supported versions

Until the first stable release, only the latest beta and current `main` branch
receive security fixes. Users should verify published checksums and read the
signing status in each release's `BUILD-INFO.txt`; current preview artifacts may
not yet have production platform signatures.

## Security boundary

QuotaFence is local-first but integrates with locally installed agent tools and
their configuration. It intentionally avoids storing provider credentials in
its SQLite database and does not read workspace source files, prompts, or
transcripts for quota attribution. The documented boundary and known
limitations are in `docs/beta.md`, `docs/architecture.md`, and
`docs/installing.md`.

## Security-sensitive areas

Changes receive additional scrutiny when they affect:

- provider authentication or access to an installed agent;
- local provider metadata databases and data-minimizing queries;
- process launching, environment variables, or command arguments;
- user-level Codex or Claude hook installation and pre-prompt decisions;
- repository discovery and filesystem access;
- local quota, attribution, entitlement, or audit data;
- Tauri capabilities, commands, content security policy, or external links; and
- logs, diagnostics, exports, crash reports, signing, or automatic updates.

The project should reuse supported provider authentication surfaces where
possible and avoid copying provider credentials into its own storage. Local
provider databases must be opened read-only, queried with explicit columns, and
treated as versioned untrusted input. Features must not select or persist
prompt, response, transcript, preview, credential, or source-code content
unless a separately reviewed requirement makes that access necessary.

Hook configuration changes only after an explicit install or uninstall action,
and QuotaFence preserves unrelated handlers. Although an agent may supply a
complete lifecycle payload on standard input, typed parsers ignore content
fields and store only the minimum lifecycle, folder, allocation, checkpoint,
and time metadata. Integration failures follow each adapter's documented
fail-open/fail-closed policy; only explicit allocation or policy decisions may
block new work.
