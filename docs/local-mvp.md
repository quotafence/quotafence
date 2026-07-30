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
- record observed usage against a scope or as unattributed usage;
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
- map the current working repository to an allocation;
- attribute a provider-confirmed total to individual projects automatically;
- persist per-scope policy or enforce it during admission;
- forecast depletion from session history;
- archive scopes; or
- run as a background daemon.

The next vertical slice starts with repository binding and a read-only CLI
context command. See the [Codex-first roadmap](roadmap.md).
