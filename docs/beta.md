# v0.1 Beta Guide

Agent Quota Manager `v0.1.0-beta.1` is a source-distributed macOS beta for solo
Codex power users. Treat it as a safety aid with explicit health states, not as
an account-level guarantee.

## Before relying on protection

1. Start AQM and add the detected Codex source.
2. Sync successfully and confirm the provider window and reset time look right.
3. Add each local folder that should receive capacity and order allocations by
   priority.
4. In AQM Settings, install Codex Desktop protection.
5. In Codex **Settings → Hooks → User config**, review, trust, and enable the
   AQM entries under `UserPromptSubmit` and `Stop`.
6. Quit Codex with **Cmd+Q**, reopen it, and submit a test prompt from an
   allocated folder.
7. Run the AQM health check. Protection is ready only when the app reports it as
   verified; installed, unverified, or degraded states are not equivalent.

You can disable or uninstall protection from AQM Settings without modifying
unrelated Codex hook entries.

## Capability boundary

- A verified prompt gate can allow or block a **new** Codex Desktop prompt based
  on its mapped folder and remaining allocation.
- `aqm run codex` owns a managed CLI process and can refuse its launch at a
  policy boundary.
- AQM cannot stop a Codex turn that is already running.
- If hook integration fails, it fails open and reports degraded health rather
  than claiming protection.
- Passive tracking does not require hooks, but it is observation rather than
  enforcement.

## Accounting limits

Codex exposes an aggregate account percentage, not exact project token totals.
AQM attributes a provider percentage delta only when local activity points to
one mapped folder. Overlapping, unmapped, or ambiguous work remains unassigned.
Provider totals remain the source of truth and can correct local attribution.

Allocation percentages are shares of the full provider window. When current
quota is below all targets, higher-priority folders are funded first. Unmanaged
usage consumes unassigned capacity first, then erodes the lowest-priority
funding. A provider reset starts a new accounting window and makes carried
allocation targets spendable again.

## Privacy

AQM stores its configuration, allocations, observations, and decisions in a
local SQLite database. It communicates with the installed Codex App Server to
read the account quota checkpoint. Passive attribution reads Codex's local
state database in read-only mode and selects only thread identifier, working
folder, cumulative token counter, and update time.

AQM does not read or store Codex credentials, prompts, responses, transcript
contents, source files, or workspace file contents. Hook decisions retain
folder identity and policy outcomes, not prompt text. Review diagnostic output
before posting it because local paths may reveal usernames or project names.

## Known beta limitations

- Codex only; no Claude Code or provider routing yet.
- macOS is the live-tested platform for this beta.
- Source build only; no signed installer, automatic update, or checksum release
  flow yet.
- Percentage checkpoints are integer and aggregate, so small or concurrent
  changes may be delayed or remain unassigned.
- Cross-platform managed-process orphan recovery is incomplete.
- No cloud sync, team workspace, RBAC, billing, or remote enforcement.

## Reporting a problem

Use the repository's structured issue form for **sync**, **attribution**, or
**false blocking**. Include the AQM version, macOS and Codex versions, protection
health state, window/reset context, and reproducible steps. Redact usernames and
project names from paths. Never attach credentials, prompts, responses,
transcripts, source code, or the entire Codex/AQM database.

The exercised recovery scenarios are recorded in the
[Codex Desktop beta recovery matrix](beta-recovery-matrix.md).
