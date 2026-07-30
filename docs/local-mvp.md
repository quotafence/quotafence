# Current Local MVP

This document describes the implemented baseline, not the full product
positioning. The desktop application currently configures local budgets and
observes Codex quota; it is not yet in the coding-agent execution path.

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
- distinguish root allocations from unallocated capacity;
- create project, repository, and nested task allocations;
- update existing allocation limits;
- show canonical repository roots on explicitly bound repository allocations;
- refresh the selected Codex source on startup or on demand;
- carry allocations into the next provider reset window;
- prevent duplicate active bindings to the same detected provider limit;
- archive a quota source without deleting its ledger history;
- show hierarchical debiting, reservations, and policy decisions; and
- switch between locally configured quota sources.

Creating a scope and its first allocation is atomic. An invalid or
over-capacity allocation leaves neither record behind.

## Persistence and privacy

The app reloads providers, windows, scopes, allocations, and ledger totals from
SQLite at startup. Data is stored in Tauri's operating-system-specific app-data
directory. The UI does not load remote fonts, analytics, or hosted assets.

## Honest limitations

This milestone does not:

- alter or copy coding-agent authentication;
- launch or stop a managed coding-agent session;
- attribute a provider-confirmed total to individual projects automatically;
- accept manual usage estimates as a substitute for automatic attribution;
- persist per-scope policy or enforce it against a launched process;
- forecast depletion from session history;
- archive scopes; or
- run as a background daemon.

The companion CLI resolves nested working directories to a canonical Git root,
binds that root explicitly to an existing repository allocation, and reports
its active allocation context. It can dry-run Codex admission after refreshing
the provider checkpoint, but does not yet reserve capacity or launch provider
work.
See the [CLI guide](cli.md) and [Codex-first roadmap](roadmap.md).
