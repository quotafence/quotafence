# Public repository and beta release audit

Last reviewed: 29 August 2026 on `codex/release-readiness`.

This is a release-readiness record, not a guarantee that the repository or a
binary is free of vulnerabilities. Repeat the checks immediately before
changing repository visibility and before every public release.

## Completed repository checks

- [x] The tracked tree contains no `.env`, private key, signing certificate,
  provider credential, SQLite database, or log file.
- [x] The reachable Git history contains no filenames matching those sensitive
  categories.
- [x] A conservative token-pattern scan of the tracked tree and reachable
  history found no private-key block, GitHub token, Anthropic key, AWS access
  key, or Google API key.
- [x] The largest reachable blobs are the documented Codex provider artwork;
  no database, build artifact, or diagnostic archive is present.
- [x] GitHub recognizes the repository license as Apache-2.0.
- [x] Workflow permissions are explicit and there is no `pull_request_target`
  workflow that executes contributor code with repository secrets.
- [x] Local `.env*`, logs, frontend output, Cargo output, Tauri schemas, editor
  files, and local coverage are ignored.
- [x] The unused Tauri opener plugin and its default permission were removed.
- [x] The production webview has a local-only Content Security Policy instead
  of a disabled CSP.

The scan tools `gitleaks` and `trufflehog` were not installed on the audit
machine. The pattern and history checks above are useful defense in depth, not
a replacement for GitHub secret scanning or a dedicated scanner.

## Must be completed before visibility changes

- [ ] In **Settings → Security and analysis**, enable and verify private
  vulnerability reporting and secret scanning where GitHub makes them
  available. The private-vulnerability-reporting API returned `404` while the
  repository was private, so the channel has not been independently verified.
- [ ] Review organization members, outside collaborators, deploy keys, GitHub
  Apps, Actions secrets, environments, webhooks, and branch rules.
- [ ] Run a dedicated secret scanner against the full reachable history.
- [ ] Confirm the provider artwork notice and upstream brand requirements are
  acceptable for public distribution.
- [ ] Decide whether Discussions should remain disabled and name the public
  support/triage owner.
- [ ] Publish operator-specific privacy, terms, refund/cancellation, and
  support policies before accepting payment.

## Must be completed before a beta release

- [ ] Run all three manual platform release workflows for the exact release
  commit.
- [ ] Verify checksums and install/uninstall the generated artifacts on clean
  macOS, Windows, and Linux accounts.
- [ ] Complete the installed-app, Windows, and Linux native smoke matrices with
  dated evidence.
- [ ] Inspect `BUILD-INFO.txt` and label ad-hoc/unsigned artifacts clearly.
- [ ] Scan the final app, CLI, installer, and extracted bundle for credentials,
  developer-machine paths, database files, logs, and signing material.
- [ ] Produce dependency-license notices for the exact release dependency
  graph and review provider artwork separately from the Apache-2.0 code.

## Production signing remains pending

Developer ID signing/notarization and Windows code signing are distribution
safety requirements. They are not Free/Pro capability gates. Until those are
configured, artifacts may be published only as explicitly labelled previews
for testers who understand Gatekeeper and SmartScreen warnings.
