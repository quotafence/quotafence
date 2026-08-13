# Codex Desktop Beta Recovery Matrix

This matrix records the safety-critical Codex Desktop recovery behavior required
before the v0.1 beta. Automated rows run in CI; live rows are checked against an
installed, signed-in Codex Desktop client without reading prompts or source
files.

The complete release procedure is the
[Codex beta exit checklist](beta-exit-checklist.md). Rows below record evidence;
they do not replace the checklist.

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

## P4 installed-app gate

Status: **Deferred by maintainer on 13 August 2026. Not passed.**

| Journey | Result | Evidence/date |
| --- | --- | --- |
| First setup and verified protection | Pending | — |
| Provider sync parity | Pending | — |
| Isolated folder attribution | Pending | — |
| Concurrent turns stay unassigned | Pending | — |
| Warning remains advisory | Pending | — |
| Stop blocks only a new prompt | Pending | — |
| Provider reset restores carried allocations | Pending | — |
| Missing `Stop` recovery | Pending | — |
| Helper CWD keeps session mapping | Pending | — |
| AQM restart rehydrates diagnostics | Pending | — |
| Disable and uninstall are honest | Pending | — |
| Allocation removal preserves history | Pending | — |
| Provider correction avoids false exhaustion | Pending | — |

The beta may continue with these rows deferred, but it must not be described as
production-ready or fully validated. When the gate resumes, update this table
with `Pass`, `Fail (#issue)`, or `Blocked (reason)`; never infer a pass from
automated coverage.

## Live smoke check — 13 August 2026

- The full production build, Rust formatting, locked check, clippy, 139 library
  tests, and 12 CLI tests passed from merged `main` at `c16334d`.
- `aqm context --json` resolved the repository root to the expected active
  allocation and returned an `allow` decision.
- A direct official App Server `account/rateLimits/read` request returned the
  active Codex weekly window without reading credentials, prompts, responses,
  transcripts, or workspace contents.
- `aqm admit codex --json` refreshed that window, returned `synced`, preserved
  the window identity, and allowed the mapped workspace with exit code `0`.
- The allocation and provider remaining values moved together during current
  single-workspace activity. This is supporting evidence only; the isolated
  folder-attribution checklist row remains Pending until a timestamped test
  turn is compared before and after Sync.
- `aqm hooks status codex` confirmed that AQM hook entries were configured.
  Verified trust and enablement still require the Codex Desktop check.

The first sandboxed refresh could not initialize Codex's local runtime because
the test shell denied writes under the Codex home directory. Repeating the same
official App Server request with normal local permissions succeeded, so this
was classified as a test-environment restriction rather than a provider or AQM
sync regression.
