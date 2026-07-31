# Current Local MVP

This document describes the implemented baseline, not the full product
positioning. The desktop application currently configures local budgets and
observes Codex quota. The CLI can own one Codex child process and reserve its
workspace capacity. Desktop refresh can passively infer folder usage from
Codex's local thread metadata plus the provider's aggregate quota checkpoint.
An optional trusted Codex hook can refuse new prompts from unallocated folders
and apply the mapped workspace boundary before a turn starts.

## First-run workflow

Onboarding creates one quota source as a single transaction:

```text
provider → subscription account → quota pool → quota window
                                               └→ provider quota snapshot
```

The app first probes the installed Codex App Server. When a signed-in
subscription exposes rate limits, the user chooses one detected window and the
app imports its duration, reset time, percentage capacity, and current usage.
The provider total is stored as a replaceable absolute snapshot, so the
dashboard reflects provider remaining capacity without adding repeated reads
together.

Manual provider, allowance, unit, and reset configuration remains available as
an explicit fallback. Selecting Claude Code manually does not claim that a
Claude adapter is installed.

## Dashboard workflow

For the selected quota window, the UI can:

- lead with remaining quota, days until reset, and a daily safe-spend guide;
- show provider usage as one compact progress line for the active window;
- summarize allocated versus unassigned capacity with a donut chart;
- list workspace allocations with their used and currently protected shares;
- choose any local folder and create one workspace allocation for it;
- update existing allocation limits;
- reorder workspaces by drag and drop so scarce current capacity funds the
  highest-priority folders first;
- distinguish the full-window target from the amount protected in the current
  window;
- refresh the selected Codex source on startup or with `Cmd+R`/`Ctrl+R`;
- establish a Codex Desktop activity baseline at startup and reconcile later
  refreshes to one unambiguous mapped folder;
- carry allocations into the next provider reset window;
- prevent duplicate active bindings to the same detected provider limit;
- archive a quota source without deleting its ledger history;
- show folder-level reservations and policy decisions;
- configure per-folder warn, confirmation, and stop thresholds;
- show a depletion warning only after at least five managed sessions; and
- install and report the configuration status of Codex Desktop workspace
  protection from Settings, turn it on or off without changing unrelated hooks,
  and show recent allow/block decisions;
- switch between locally configured quota sources from a fixed sidebar whose
  source list scrolls independently;
- surface incomplete Codex setup as a compact, actionable header check; and
- follow the system appearance or persist an explicit light/dark theme.

Creating a workspace, its first allocation, and its folder binding is atomic.
An invalid or over-capacity allocation leaves none of those records behind.
The CLI-managed Codex workflow also persists a provider baseline before spawn
and reconciles the final aggregate delta to the bound folder when no visible
concurrent observation makes that attribution ambiguous.

## Persistence and privacy

The app reloads providers, windows, scopes, allocations, and ledger totals from
SQLite at startup. Data is stored in Tauri's operating-system-specific app-data
directory. The UI does not load remote fonts, analytics, or hosted assets.

For passive Codex Desktop attribution, the backend opens the newest local
`state_N.sqlite` database read-only and selects only thread ID, working folder,
cumulative token counter, and update time. It never selects or stores thread
title, preview, prompt, response, transcript, credential, or source-code
content. The stored cursor is local attribution metadata, not a copy of Codex
history.

## Honest limitations

This milestone does not:

- alter or copy coding-agent authentication;
- provide exact per-workspace tokens or confirmed causal attribution from an
  account-wide integer percentage;
- split activity across multiple or unmapped folders;
- accept manual usage estimates as a substitute for automatic attribution;
- terminate a Codex Desktop turn already in flight;
- archive scopes; or
- run as a background daemon.

The companion CLI canonicalizes the current directory, resolves its nearest
ancestor workspace binding, and reports its active allocation context. It can
dry-run Codex admission or reserve capacity and supervise one managed Codex
process with `aqm run codex`.
See the [CLI guide](cli.md) and [Codex-first roadmap](roadmap.md).
