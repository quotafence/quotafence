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
| Repository binding | Not implemented |
| Managed user session | Not implemented |
| Automatic attribution | Not implemented |
| Admission enforcement | Policy model exists; not wired to launch |
| Live hard stop | Not supported |

The current Codex adapter implements quota discovery and synchronization:

- resolve a Codex executable from `PATH`, common install locations, or the
  `AGENT_QUOTA_CODEX_BIN` override;
- start `codex app-server --stdio`;
- complete the documented JSON-RPC initialization handshake;
- call `account/rateLimits/read`;
- map every complete primary or secondary window into percentage capacity,
  provider-confirmed usage, duration, and reset time; and
- replace the absolute provider snapshot on startup or explicit refresh;
- carry allocations into a fresh local window after the provider reset; and
- stop the transient App Server process after the snapshot is returned.

Only structured quota metadata crosses the adapter boundary. It does not read
`auth.json`, Codex session JSONL, prompts, source files, or account email.
Malformed, incomplete, and out-of-range provider responses are rejected.

The remaining Codex vertical slice is:

1. map a repository to an allocation;
2. admit and reserve one managed session;
3. launch and supervise Codex through the CLI wrapper;
4. attribute and reconcile its usage; and
5. enforce a visible policy boundary where technically supported.

Only after that slice is stable should the adapter contract be generalized from
real implementation evidence for a second provider.

Detailed milestones and attribution constraints are documented in the
[Codex-first roadmap](roadmap.md).
