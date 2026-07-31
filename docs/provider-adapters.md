# Provider Adapters

Provider adapters isolate subscription-specific behavior from the quota core.
Codex is the first implemented adapter. AQM will complete its managed workflow
before using another provider to generalize the contract.

## Capability discovery

An adapter reports capabilities at runtime rather than relying on a hard-coded
provider matrix.

| Capability | Meaning |
| --- | --- |
| Quota discovery | Read one or more provider quota pools and reset windows |
| Usage checkpoint | Read a provider-confirmed or provider-observed total |
| Managed session | Start work through a supported local provider surface |
| Usage attribution | Associate managed work with a local scope |
| Hard enforcement | Prevent further managed work at a policy boundary |
| External usage detection | Detect account usage not started by this app |

Capabilities may depend on provider version, operating system, authentication
state, or current adapter health. The UI must degrade visibly when a capability
is unavailable.

## Adapter lifecycle

1. **Probe:** detect supported local installations and versions without mutating them.
2. **Connect:** use the provider's supported authentication or local interface.
3. **Discover:** return pools, units, windows, and confidence metadata.
4. **Reserve:** ask the core to reserve capacity before managed work.
5. **Run:** start and supervise a session if supported.
6. **Observe:** emit provider-native usage observations.
7. **Reconcile:** compare local attribution with the latest provider checkpoint.
8. **Disconnect:** release processes and transient resources without deleting provider data.

## Adapter rules

- Do not copy browser cookies, session tokens, or credential files into app storage.
- Do not depend on undocumented private endpoints without an explicit design and
  user-facing risk disclosure.
- Treat provider output as untrusted and versioned input.
- Preserve provider-native values; normalization is a presentation concern.
- Declare the source and confidence of every usage observation.
- Fail closed for hard enforcement: an unhealthy adapter must not pretend a stop
  policy is still active.
- Record external or unexplained consumption as unattributed usage.

## Codex-first development

Current capability status:

| Capability | Codex status |
| --- | --- |
| Quota discovery | Implemented |
| Checkpoint refresh | Implemented |
| Reset rollover | Implemented |
| Workspace binding | Implemented for any local folder |
| Managed user session | Implemented through the CLI wrapper |
| Automatic attribution | Passive on desktop refresh when activity resolves to one workspace |
| Admission assessment | Implemented as dry run and managed launch gate |
| Desktop prompt gate | Implemented through an optional trusted lifecycle hook |
| Live hard stop | Not supported |

The current Codex adapter implements quota discovery and synchronization:

- resolve a Codex executable from `PATH`, common install locations, or the
  `AGENT_QUOTA_CODEX_BIN` override;
- start `codex app-server --stdio`;
- complete the documented JSON-RPC initialization handshake;
- call `account/rateLimits/read`;
- map every complete primary or secondary window into percentage capacity,
  provider-confirmed usage, duration, and reset time; and
- replace the absolute provider snapshot on startup or explicit refresh,
  identifying the connected source by adapter metadata rather than its local
  storage ID;
- carry allocations into a fresh local window after the provider reset; and
- stop the transient App Server process after the snapshot is returned.

The managed CLI path resolves the same supported Codex executable, reserves
the bound workspace, and starts it directly with inherited terminal streams.
AQM owns the child lifecycle and can refuse a new launch at a stop boundary,
but does not claim live in-flight quota enforcement.

Only structured quota metadata crosses the quota-detection adapter boundary.
It does not read `auth.json`, Codex session JSONL, prompts, source files, or
account email. Malformed, incomplete, and out-of-range provider responses are
rejected.

The passive Codex Desktop adapter discovers the highest-versioned local
`state_N.sqlite`, opens it read-only, verifies the `threads` schema, and selects
only thread ID, working directory, cumulative token counter, and update time.
It intentionally does not select title or preview columns and does not scan
rollout JSONL files. The first scan establishes a cursor baseline. Later token
deltas identify active folders, while `account/rateLimits/read` remains the
source of the quota amount.

If all pending activity maps through nearest-ancestor binding to one workspace,
AQM appends the provider percentage delta to that workspace at `inferred`
confidence. Multiple workspaces, any unmapped activity, reset rollover, or a
missing scan leave the provider change unattributed. A provider refresh that
cannot include a desktop scan invalidates pending attribution rather than
guessing across an observation gap.

### Related work

[OpenUsage](https://github.com/robinebers/openusage) demonstrates that local
Codex session data can support useful usage analysis. AQM follows the same
local-first principle but uses a narrower input for this feature: it queries
minimal thread counters from Codex's state database instead of parsing rollout
content, then debits only the separately refreshed provider quota delta.

The optional Desktop protection integration uses official Codex lifecycle
hooks:

- `UserPromptSubmit` blocks unallocated folders once any Codex workspace has an
  allocation, evaluates the mapped workspace boundary, and records a provider
  checkpoint baseline for allowed work;
- `Stop` refreshes and reconciles the provider delta;
- `SessionEnd` removes unfinished observations; and
- explicit policy decisions may block a new prompt, while parse, database, or
  provider failures remain fail-open.

Codex sends the complete lifecycle JSON to the command hook. AQM's typed input
intentionally ignores prompt, assistant-message, and transcript fields and
persists only lifecycle identifiers and folder metadata. Because the provider
checkpoint is an account-wide integer percentage, the resulting workspace
attribution is `inferred`, may remain unchanged for a small turn, and is never
split across concurrent turns.

The managed Codex wrapper now records a pre-spawn checkpoint, refreshes after
exit, and attributes a same-window non-contended delta at `observed`
confidence. Managed child hooks are bypassed through an inherited session
marker to prevent double observation. Visible concurrent work remains
unattributed; invisible external usage is why this signal is not
provider-confirmed.

Workspace policy overrides are now persisted and applied to dry-run admission,
managed launch, and the trusted Desktop prompt gate. For the hook, stop and
non-interactive confirmation mean refusing the next prompt. Aggregate Codex
checkpoints are not timely enough to justify live termination.

Only after that slice is stable should the adapter contract be generalized from
real implementation evidence for a second provider.

Detailed milestones and attribution constraints are documented in the
[Codex-first roadmap](roadmap.md).
