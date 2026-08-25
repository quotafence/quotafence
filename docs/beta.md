# v0.1 Beta Guide

QuotaFence `v0.1.0-beta.1` is an ad-hoc-signed macOS beta for solo
Codex and Claude Code power users. A Windows x64 source-built preview is also
available. Treat it as a safety aid
with explicit health states, not as an account-level guarantee.

Before installing an artifact, follow the
[install, upgrade, and removal guide](installing.md) to verify its checksum and
review the unsigned/ad-hoc signing warning.

## Before relying on protection

1. Start QuotaFence and add the detected Codex source.
2. Sync successfully and confirm the provider window and reset time look right.
3. Add each local folder that should receive capacity and order allocations by
   priority.
4. In QuotaFence Settings, install Codex Desktop protection.
5. In Codex **Settings → Hooks → User config**, review, trust, and enable the
   QuotaFence entries under `UserPromptSubmit` and `Stop`.
6. Quit Codex with **Cmd+Q**, reopen it, and submit a test prompt from an
   allocated folder.
7. Run the QuotaFence health check. Protection is ready only when the app reports it as
   verified; installed, unverified, or degraded states are not equivalent.

You can disable or uninstall protection from QuotaFence Settings without modifying
unrelated Codex hook entries.

For Claude, connect the observer, refresh the shared subscription windows, then
enable **Workspace protection** in the Claude Settings tab. Restart Claude Code
or Claude Desktop and send a test prompt from an allocated folder. QuotaFence preserves
unrelated Claude hooks and reports protection as unverified until it receives a
lifecycle event. For an owned CLI session use `qfence claude`; add
`--window 5h` when the 5-hour allocation should be the managed boundary.

## Capability boundary

- A verified prompt gate can allow or block a **new** Codex Desktop prompt based
  on its mapped folder and remaining allocation.
- `qfence codex` owns a managed CLI process and can refuse its launch at a
  policy boundary.
- QuotaFence cannot stop a Codex turn that is already running.
- If hook integration fails, it fails open and reports degraded health rather
  than claiming protection.
- Passive tracking does not require hooks, but it is observation rather than
  enforcement.

## Accounting limits

Codex exposes an aggregate account percentage, not exact project token totals.
QuotaFence attributes a provider percentage delta only when local activity points to
one mapped folder. Overlapping, unmapped, or ambiguous work remains unassigned.
Provider totals remain the source of truth and can correct local attribution.

Allocation percentages are shares of the full provider window. When current
quota is below all targets, higher-priority folders are funded first. Unmanaged
usage consumes unassigned capacity first, then erodes the lowest-priority
funding. A provider reset starts a new accounting window and makes carried
allocation targets spendable again.

## Privacy

QuotaFence stores its configuration, allocations, observations, and decisions in a
local SQLite database. It communicates with the installed Codex App Server to
read the account quota checkpoint. Passive attribution reads Codex's local
state database in read-only mode and selects only thread identifier, working
folder, cumulative token counter, and update time.

QuotaFence does not read or store Codex credentials, prompts, responses, transcript
contents, source files, or workspace file contents. Hook decisions retain
folder identity and policy outcomes, not prompt text. Review diagnostic output
before posting it because local paths may reveal usernames or project names.

## Known beta limitations

- Claude support is beta; cross-provider routing is not implemented because
  unlike native quota windows are not safely comparable.
- macOS is the live-tested platform for this beta. Windows x64 is compiled and
  tested in CI, but still needs native manual smoke testing before it reaches
  the same support level.
- The beta DMG is not notarized, so macOS displays a Gatekeeper warning and the
  user must explicitly allow the first launch. There is no automatic update or
  production signing flow yet.
- Percentage checkpoints are integer and aggregate, so small or concurrent
  changes may be delayed or remain unassigned.
- Windows can recover managed sessions whose supervisor process has exited;
  Windows console signal forwarding still needs native end-to-end validation.
- No cloud sync, team workspace, RBAC, billing, or remote enforcement.

## Reporting a problem

Use the repository's structured issue form for **sync**, **attribution**, or
**false blocking**. Include the QuotaFence version, macOS and Codex versions, protection
health state, window/reset context, and reproducible steps. Redact usernames and
project names from paths. Never attach credentials, prompts, responses,
transcripts, source code, or the entire Codex/QuotaFence database.

The release gate is defined in the
[Codex beta exit checklist](beta-exit-checklist.md), with exercised recovery
evidence recorded in the
[Codex Desktop beta recovery matrix](beta-recovery-matrix.md).
