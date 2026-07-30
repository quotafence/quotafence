# Local MVP

The desktop application now supports a complete provider-neutral local workflow
without requiring a provider account or API key.

## First-run workflow

Onboarding creates one quota source as a single transaction:

```text
provider → subscription account → quota pool → quota window
```

The user chooses a provider label, allowance, unit, and reset time. This is
manual configuration; selecting "Codex" or "Claude Code" does not claim that an
adapter is installed or that usage is being read automatically.

## Dashboard workflow

For the selected quota window, the UI can:

- show capacity, provider-level usage, remaining quota, and reset time;
- distinguish root allocations from unallocated capacity;
- create project, repository, and nested task allocations;
- update existing allocation limits;
- record observed usage against a scope or as unattributed usage;
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

- authenticate with a coding-agent provider;
- discover subscription quota automatically;
- launch or stop a managed coding-agent session;
- reconcile manual observations with provider-confirmed totals;
- delete or archive sources and scopes; or
- run as a background daemon.

The next vertical slice can add the first Codex adapter while keeping these
manual workflows available as a provider-neutral fallback.
