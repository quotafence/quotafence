# Quota Model

Providers expose percentages, rolling limits, credits, spend, concurrency, or
opaque capacity signals. AQM therefore models current Codex quota without
assuming tokens, while keeping unlike resource behaviors explicit.

## Core concepts

| Concept | Meaning |
| --- | --- |
| Provider | A coding-agent subscription service, such as Codex |
| Account | A locally available signed-in identity for one provider |
| Quota pool | One provider-defined allowance, such as a weekly usage window |
| Window | The period, reset boundary, and capacity applied to a quota pool |
| Workspace | One explicitly selected local folder |
| Allocation | The maximum share assigned to a scope for a window |
| Reservation | Capacity held temporarily for work in progress |
| Usage event | An immutable local record that debits a scope |
| Policy | The warn, confirm, or stop behavior at thresholds |
| Confidence | Whether a value is confirmed, observed, inferred, or estimated |

The Rust implementation of these provider-neutral types lives in
`src-tauri/src/domain`. IDs and display names reject empty values, quota windows
are half-open intervals, and usage events and reservations require positive
amounts.

## Units

The canonical stored value is a provider-native amount plus its unit and source.
Examples could include `percent_of_weekly_pool`, `provider_credit`, or another
documented signal.

A UI may display normalized **quota points** to make allocation easier inside
one pool—for example, 100 points representing that pool's full window. Points
from different providers or different pools are not exchangeable and must not
be summed as if they were money or tokens.

`QuotaUnit` is intentionally free-form today, but a label alone does not define
resource behavior. The following distinction guides future extensions:

| Dimension | Semantics | Modeling direction |
| --- | --- | --- |
| Rate limit or credit | Consumable capacity over a provider window | Existing pool/window/allocation model |
| USD spend | Consumable monetary capacity | Integer minor units plus currency metadata |
| Concurrency | Capacity occupied and later released | Reservation/admission model, not cumulative usage |
| Priority | Relative importance of proposed work | Workload and policy input |
| Deadline | Time constraint on proposed work | Workload and routing input |

Priority and deadline are not quota units. Concurrency should not be forced into
a cumulative percentage model. AQM will introduce a resource-kind abstraction
only when implementing behavior that needs it; the Codex slice does not require
a domain rewrite.

## Folder allocations

A pool is divided directly into independent folder allocations. AQM does not
ask users to create a second project or task hierarchy inside a folder.

```text
Codex weekly pool (100)
├── /code/product-a (50)
├── /code/product-b (30)
└── Unallocated reserve (20)
```

The domain validates that the sum of active folder allocations does not exceed
the window capacity. Each managed session is attributed to the nearest bound
folder, and therefore debits exactly one workspace allocation plus the shared
provider pool.

The persisted scope codec still reads legacy project/task rows so an older
local database can be opened without deleting history. Those legacy kinds are
not exposed as new allocation choices.

## Available capacity

For a scope in a specific window:

```text
available = allocation - attributed usage - active reservations
```

Provider-level availability also accounts for unattributed usage discovered
during reconciliation. Negative values are valid audit facts and must not be
silently clamped in storage, though the UI may present zero as spendable.

The domain exposes both values: signed `remaining` preserves overage, while
non-negative `spendable` is suitable for admission decisions and display.

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
usage. When a provider exposes only an aggregate total, a before/after delta is
an observation rather than proof of causality. AQM may associate it with a lone
managed session at observed confidence; ambiguous concurrent or external usage
remains unattributed.

## Enforcement

Policies may define warning, confirmation, and stop thresholds. A hard stop is
only valid for a session controlled by an adapter with the required capability.
External sessions can consume quota outside local enforcement, so no adapter
should promise an absolute account-wide limit unless the provider itself offers
that guarantee.

In v0.1, `stop` may refuse admission to an AQM-managed launch. Stopping an
already-running process is a distinct capability and requires a timely,
trustworthy consumption signal.
