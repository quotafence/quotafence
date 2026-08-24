# Provider Capability Matrix

This document defines the evidence required before QuotaFence adds a second coding
agent. A provider name does not imply a capability: discovery, observation,
attribution, and enforcement are assessed independently and may vary by client,
account type, version, or operating system.

Last reviewed: 20 August 2026.

## Evidence levels

| Level | Meaning |
| --- | --- |
| Documented | A provider publishes the interface and its semantics |
| Probed | QuotaFence can detect the interface without changing provider state |
| Implemented | QuotaFence consumes the interface with schema and failure tests |
| Live-tested | The installed provider client passed a dated end-to-end check |

UI and policy decisions must use the current runtime capability, not the best
row ever observed for that provider.

## Current matrix

| Capability | Codex subscription | Claude Code subscription | QuotaFence implication |
| --- | --- | --- | --- |
| Local executable probe | Implemented and live-tested | Documented through the `claude` CLI | Probe version and installation only; do not authenticate automatically |
| Subscription quota windows | Implemented through App Server `account/rateLimits/read` | Available from the shared Claude subscription usage endpoint using an explicitly authorized existing Claude login; CLI status-line input is a secondary passive source | Use one account checkpoint for CLI and Desktop Code while keeping OAuth tokens memory-only; Claude Chat and Cowork remain outside folder attribution |
| Reset timestamp | Implemented | Documented with each optional status-line rate-limit window | Preserve each provider window independently |
| Account checkpoint refresh | Implemented on demand | Implemented through the shared subscription usage endpoint after explicit local authorization | Keep tokens memory-only and surface refresh failures honestly |
| Workspace identity | Explicit canonical folder binding | Hooks and status-line data expose current working-directory/session context | Reuse QuotaFence folder bindings; never derive identity from prompt text |
| Managed interactive launch | Implemented through `qfence codex` | Implemented through `qfence claude`; defaults to weekly and supports `--window 5h` | One managed reservation tracks one explicitly selected native window |
| Structured managed result | Codex-specific managed reconciliation | Claude print/SDK modes document JSON results including session ID and estimated USD cost | Keep USD cost separate from subscription quota percentage |
| Lifecycle hooks | Implemented for supported Codex hooks | Documented across terminal, IDE, Desktop, and web with `UserPromptSubmit`, `Stop`, `StopFailure`, and `SessionEnd` | Prefer user-level Claude hooks for a daily Desktop/IDE-compatible workflow |
| Prompt admission | Implemented for verified Codex Desktop hooks | Implemented with reversible user-level `UserPromptSubmit` hooks | Installed is not equivalent to observed; the UI reports activation health |
| Turn completion | Codex `Stop` reconciliation implemented | `Stop`, `StopFailure`, and `SessionEnd` are documented | Treat API failure separately from successful completion |
| Session usage signal | Aggregate provider delta plus minimal local activity evidence | Status line documents context usage, estimated session cost, session ID, workspace, and optional subscription limits | Context-window percentage is not subscription consumption; cost is not quota |
| Exact subscription tokens by folder | Not available | Not established | Never display invented token totals |
| External usage detection | Provider checkpoint movement can reveal unattributed use | Not established outside observed Claude sessions | Keep unexplained Claude consumption unassigned |
| Stop before new work | Implemented for verified Codex prompt gate and managed launch | Implemented for new Claude prompts and managed launches | Neither adapter terminates work already running |
| Stop work already running | Not supported | Not established | Out of scope for the next slice |
| Provider routing | Not implemented | Not implemented | Defer until both providers expose reliable, comparable availability signals |

## Shared contract

The shared adapter boundary should describe facts, not simulate symmetry:

- probe result: executable, version, client surfaces, and authentication state
  only when exposed safely;
- quota observation: provider-native unit, window identity, used/remaining value,
  reset time, source, timestamp, and confidence;
- workspace observation: canonical folder, provider session/turn ID, lifecycle
  event, and timestamp;
- managed process capability: executable plus argument vector, lifecycle
  ownership, and supported structured result fields;
- enforcement capability: where a decision can be applied, whether the hook is
  verified, and whether failures are fail-open or fail-closed.

An adapter may omit any capability. The core must not synthesize a missing
account checkpoint from context-window usage, session cost, transcript tokens,
or UI scraping.

## Claude Code beta vertical slice

The adapter starts with read-only observation and adds reversible lifecycle
control only after the user enables it explicitly:

1. Probe the supported `claude` executable and version without starting an
   authenticated session.
2. Define typed parsers for the minimal status-line and lifecycle-hook fields
   needed by QuotaFence. Ignore prompt, response, transcript, and tool payloads.
3. Observe optional 5-hour and 7-day subscription windows from a user-approved
   status-line integration after Claude supplies them.
4. Map the reported working directory to the nearest explicit QuotaFence folder
   binding.
5. Display Claude as observed/degraded until a fresh supported window exists;
   do not offer manual percentage entry as if it were provider-confirmed.
6. Status-line install/status/uninstall preserves unrelated Claude settings and
   refuses to overwrite a custom status line.

The beta now installs reversible lifecycle hooks, attributes an unambiguous
provider delta to the current folder across both native windows, applies Warn
or Stop before a new prompt, and supports an explicitly selected managed CLI
window. Provider routing remains deferred because Codex weekly percentage and
Claude's 5-hour/weekly limits are not a single comparable budget.

Done means QuotaFence can show both real Claude subscription windows, attribute an
unambiguous delta to a folder, and refuse a new prompt at a verified boundary,
or clearly say which capability is unavailable. It does not mean live turn
termination, cross-provider routing, or production readiness.

## Privacy and security boundary

- Read an existing Claude OAuth token only after explicit refresh/connection,
  keep it in process memory, and never persist or log it.
- Do not parse conversation transcripts to discover quota.
- Do not store prompts, responses, assistant messages, or tool inputs from hook
  payloads.
- Preserve unrelated user and project hooks when installing or removing QuotaFence
  entries.
- Treat hook and status-line JSON as untrusted, versioned input.
- Require explicit user action before modifying `~/.claude/settings.json`.

## Sources

- [Codex App Server](https://developers.openai.com/codex/app-server)
- [Claude Code hooks reference](https://code.claude.com/docs/en/hooks)
- [Claude Code status-line reference](https://code.claude.com/docs/en/statusline)
- [Claude Code CLI reference](https://docs.anthropic.com/en/docs/claude-code/cli-usage)
