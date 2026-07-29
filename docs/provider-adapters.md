# Provider Adapters

Provider adapters isolate subscription-specific behavior from the quota core.
Codex is the first planned adapter; additional providers should use the same
contract while exposing their real differences.

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

The first adapter should prove the complete vertical slice:

1. detect an available Codex installation or supported local surface;
2. discover account quota and its reset window;
3. map a repository to an allocation;
4. launch one managed session;
5. attribute and reconcile its usage; and
6. enforce a visible policy boundary where technically supported.

Only after that slice is stable should the adapter contract be generalized from
real implementation evidence for a second provider.
