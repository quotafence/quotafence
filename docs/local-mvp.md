# Current Local MVP

This document describes the implemented baseline, not the full product
positioning. The desktop application currently configures local budgets and
observes Codex quota. The CLI can own one Codex child process and reserve its
workspace capacity. Optional lifecycle hooks can infer folder usage for one
uncontended Codex desktop turn.

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

- show capacity, provider-level usage, remaining quota, and reset time;
- distinguish folder allocations from unallocated capacity;
- choose any local folder and create one workspace allocation for it;
- update existing allocation limits;
- show canonical folder paths on explicitly bound workspace allocations;
- refresh the selected Codex source on startup or on demand;
- display inferred folder usage reconciled by the optional Codex hook adapter;
- carry allocations into the next provider reset window;
- prevent duplicate active bindings to the same detected provider limit;
- archive a quota source without deleting its ledger history;
- show folder-level reservations and policy decisions;
- configure per-folder warn, confirmation, and stop thresholds;
- show an evidence-gated managed burn rate and depletion signal; and
- switch between locally configured quota sources.

Creating a workspace, its first allocation, and its folder binding is atomic.
An invalid or over-capacity allocation leaves none of those records behind.
The CLI-managed Codex workflow also persists a provider baseline before spawn
and reconciles the final aggregate delta to the bound folder when no visible
concurrent observation makes that attribution ambiguous.

## Persistence and privacy

The app reloads providers, windows, scopes, allocations, and ledger totals from
SQLite at startup. Data is stored in Tauri's operating-system-specific app-data
directory. The UI does not load remote fonts, analytics, or hosted assets.

## Honest limitations

This milestone does not:

- alter or copy coding-agent authentication;
- provide exact per-turn tokens or confirmed causal attribution from an
  account-wide integer percentage;
- split concurrent Codex turns across folders;
- accept manual usage estimates as a substitute for automatic attribution;
- enforce live in-flight quota movement;
- archive scopes; or
- run as a background daemon.

The companion CLI canonicalizes the current directory, resolves its nearest
ancestor workspace binding, and reports its active allocation context. It can
dry-run Codex admission or reserve capacity and supervise one managed Codex
process with `aqm run codex`.
See the [CLI guide](cli.md) and [Codex-first roadmap](roadmap.md).
