# Quota Model

Subscription providers expose different usage signals: percentages, reset
times, rolling windows, request classes, or opaque limits. Agent Quota Manager
therefore models quota without assuming that every provider reports tokens.

## Core concepts

| Concept | Meaning |
| --- | --- |
| Provider | A coding-agent subscription service, such as Codex |
| Account | A locally available signed-in identity for one provider |
| Quota pool | One provider-defined allowance, such as a weekly usage window |
| Window | The period and reset rule applied to a quota pool |
| Scope | A project, repository, task, or reserved system bucket |
| Allocation | The maximum share assigned to a scope for a window |
| Reservation | Capacity held temporarily for work in progress |
| Usage event | An immutable local record that debits a scope |
| Policy | The warn, confirm, or stop behavior at thresholds |
| Confidence | Whether a value is confirmed, observed, inferred, or estimated |

## Units

The canonical stored value is a provider-native amount plus its unit and source.
Examples could include `percent_of_weekly_pool`, `provider_credit`, or another
documented signal.

A UI may display normalized **quota points** to make allocation easier inside
one pool—for example, 100 points representing that pool's full window. Points
from different providers or different pools are not exchangeable and must not
be summed as if they were money or tokens.

## Allocation hierarchy

A pool may be divided into project allocations, then optional task allocations.
Usage attributed to a task also debits its parent project and provider pool.

```text
Codex weekly pool (100)
├── Project A (50)
│   ├── Feature work (30)
│   └── Maintenance (20)
├── Project B (30)
└── Unallocated reserve (20)
```

The parent allocation is always the upper bound. Unused child quota is not
automatically borrowed by siblings unless an explicit borrowing policy allows
it.

## Available capacity

For a scope in a specific window:

```text
available = allocation - attributed usage - active reservations
```

Provider-level availability also accounts for unattributed usage discovered
during reconciliation. Negative values are valid audit facts and must not be
silently clamped in storage, though the UI may present zero as spendable.

## Reservations

Before managed work begins, the application can reserve an estimated amount.
The reservation prevents concurrent sessions from each seeing the same
remaining capacity. When work ends, the reservation is replaced by attributed
usage or released.

Expired reservations require an explicit recovery rule and an audit event; they
must not disappear without explanation.

## Reconciliation

Local attribution and provider totals are separate observations:

1. Read the last provider checkpoint.
2. Read the latest provider total for the same pool and window.
3. Compare the provider delta with locally attributed events.
4. Record any unexplained delta as unattributed usage.
5. Attach source and confidence to the new checkpoint.

A window reset creates a new window identity. It does not rewrite historical
usage.

## Enforcement

Policies may define warning, confirmation, and stop thresholds. A hard stop is
only valid for a session controlled by an adapter with the required capability.
External sessions can consume quota outside local enforcement, so no adapter
should promise an absolute account-wide limit unless the provider itself offers
that guarantee.
