# Codex Desktop Beta Recovery Matrix

This matrix records the safety-critical Codex Desktop recovery behavior required
before the v0.1 beta. Automated rows run in CI; live rows are checked against an
installed, signed-in Codex Desktop client without reading prompts or source
files.

| Scenario | Expected behavior | Coverage | Status |
| --- | --- | --- | --- |
| Provider window rollover | Carry allocation targets, reset usage, never reconcile across windows | Adapter, application, hook tests | Pass |
| Missing `Stop` | Reconcile the prior turn at the next prompt or abandon an unavailable baseline | Hook integration tests | Pass |
| Helper working directory | Keep a session on its bound workspace when Codex reports a helper CWD | Hook integration test | Pass |
| App restart with pending turn | Persist the observation and expose pending/stale recovery health | Storage and LocalState tests | Pass |
| Disabled or untrusted hooks | Report passive/degraded protection without claiming enforcement | Hook status and UI health logic | Pass |
| Concurrent Desktop turns | Keep aggregate movement unassigned rather than split by guesswork | Storage, application, and live health check | Pass |
| Provider correction below local attribution | Normalize effective workspace usage and avoid false blocking | Application and hook regression tests | Pass |
| UI-generated provider ID | Resolve `codex` by provider display identity for `admit` and `run` | CLI regression and live admission | Pass |

## Live check — 12 August 2026

- Full production build and test suite passed from merged `main`.
- `aqm context --json` resolved the current folder to its generated Codex source
  and an active allocation.
- `aqm admit codex --json` refreshed the real provider checkpoint and returned
  `allow` with exit code `0` after resolving the UI-generated provider ID.
- Codex protection hooks were configured, and recent local receipts included
  successful `UserPromptSubmit` and `Stop` decisions.
- One current contended observation remained pending and was retained rather
  than deleted; aggregate movement during overlap stays unassigned.

The live check inspects local lifecycle metadata only. It does not record hook
payloads, prompts, responses, or workspace contents.
