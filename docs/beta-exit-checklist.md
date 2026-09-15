# Codex Beta Exit Checklist

This checklist is the release gate for the Codex-first vertical slice. Automated
checks prevent known regressions; manual checks verify the installed Codex
Desktop integration that CI cannot reproduce. Do not mark a row complete from
code inspection alone.

## Automated gate

Run from the repository root:

```bash
npm ci
npm run check
```

The gate must pass the production frontend build, release-version alignment,
Rust formatting, locked dependency build, clippy with warnings denied, all
library tests, all CLI tests, a high-severity npm advisory gate, and
documentation whitespace checks.

The lightweight pull-request gate runs on Linux so ordinary changes do not
consume macOS and Windows runner minutes. Before a platform release, run the
manual `macOS beta release`, `Windows beta release`, and `Linux beta release`
workflows. They must build the platform installer and CLI, install and run both
CLI command names, uninstall them, verify every SHA-256 entry, enforce portable
manifest line endings, and upload the exact preview artifacts. A green Linux
compile without the Linux packaging and native smoke checks is not sufficient
for a release.

## Manual installed-app gate

Record the macOS, Codex, and QuotaFence versions plus the date in the recovery matrix.
Use disposable allocations where a stop test could interrupt real work.

| Journey | Procedure | Pass condition |
| --- | --- | --- |
| First setup | Add detected Codex source, allocate a local folder, install hooks, trust both entries, restart Codex | QuotaFence reports verified protection after a test prompt |
| Sync | Compare QuotaFence remaining percentage and reset time with Codex Usage | Values match the same provider window after Sync |
| Folder attribution | Run one isolated turn in an allocated folder, wait for `Stop`, then Sync | Folder tracked usage increases by the observed provider percentage delta, or the UI explains that integer rounding produced no delta |
| Ambiguous attribution | Overlap turns in two allocated folders | Provider movement remains unassigned; neither folder receives guessed usage |
| Warning | Set Warn below current allocation consumption and submit a prompt | Prompt proceeds and the policy is visibly advisory |
| Stop | Set Stop at or below current allocation consumption and submit a new prompt | New prompt is blocked with the correct folder and allocation; no running turn is claimed to be terminated |
| Provider reset | Keep an allocation through a real or fixture-driven rollover | New window carries targets, clears old usage, and admits newly funded work |
| Missing `Stop` | Interrupt/quit Codex after prompt admission, then submit another prompt | Previous observation is reconciled or abandoned conservatively without a false block |
| Helper CWD | Resume an existing task that reports a Codex helper directory | Session remains mapped to its original allocated folder |
| App restart | Quit and reopen QuotaFence with an open or stale observation | State rehydrates and diagnostics explain pending/stale recovery |
| Disable | Disable protection in QuotaFence and restart Codex if requested | Dashboard clearly reports protection off and prompts are not described as enforced |
| Uninstall | Uninstall QuotaFence hooks | Only QuotaFence hook entries are removed; unrelated hooks remain |
| Remove allocation | Remove an inactive folder allocation | Binding disappears, historical usage remains, and other priorities stay valid |
| Provider correction | Sync after Codex reports a lower used value than local attribution | QuotaFence follows provider total and does not keep the folder falsely exhausted |

## Release decision

The Codex beta can exit P4 only when:

- the automated gate passes from merged `main`;
- every manual row above has a dated Pass result in
  `beta-recovery-matrix.md`;
- no open false-block or cross-window attribution regression remains;
- limitations are visible in the app or beta guide at the point they matter;
- the chosen distribution artifact and Gatekeeper instructions have been
  tested by someone other than the build author.
- Windows support remains Preview until the Windows native smoke checklist has
  dated evidence; CI success alone does not promote it to live-tested support.
- Linux support remains Preview until the Linux native smoke checklist has dated
  evidence for both the AppImage and Debian package.

Failures are release blockers. Record the issue link and leave the row failed
or pending; do not weaken the expected behavior to make the matrix green.
