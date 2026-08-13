# Provider Capability Matrix

This document defines the evidence required before AQM adds a second coding
agent. A provider name does not imply a capability: discovery, observation,
attribution, and enforcement are assessed independently and may vary by client,
account type, version, or operating system.

Last reviewed: 13 August 2026.

## Evidence levels

| Level | Meaning |
| --- | --- |
| Documented | A provider publishes the interface and its semantics |
| Probed | AQM can detect the interface without changing provider state |
| Implemented | AQM consumes the interface with schema and failure tests |
| Live-tested | The installed provider client passed a dated end-to-end check |

UI and policy decisions must use the current runtime capability, not the best
row ever observed for that provider.

## Current matrix

| Capability | Codex subscription | Claude Code subscription | AQM implication |
| --- | --- | --- | --- |
| Local executable probe | Implemented and live-tested | Documented through the `claude` CLI | Probe version and installation only; do not authenticate automatically |
| Subscription quota windows | Implemented through App Server `account/rateLimits/read` | Documented in status-line input as optional 5-hour and 7-day windows after the first response for Claude.ai Pro/Max; CLI and the Desktop Code tab share user settings and the same Claude Code engine | Observe both CLI and Desktop Code through one integration; Claude Chat and Cowork are outside this adapter |
| Reset timestamp | Implemented | Documented with each optional status-line rate-limit window | Preserve each provider window independently |
| Account checkpoint refresh | Implemented on demand | Not established as a standalone local API | Do not add a Claude Sync button until a supported refresh surface is proven |
| Workspace identity | Explicit canonical folder binding | Hooks and status-line data expose current working-directory/session context | Reuse AQM folder bindings; never derive identity from prompt text |
| Managed interactive launch | Implemented through `aqm run codex` | Documented `claude` interactive CLI | A future `aqm run claude` may own process lifecycle, but it is not required for the first slice |
| Structured managed result | Codex-specific managed reconciliation | Claude print/SDK modes document JSON results including session ID and estimated USD cost | Keep USD cost separate from subscription quota percentage |
| Lifecycle hooks | Implemented for supported Codex hooks | Documented across terminal, IDE, Desktop, and web with `UserPromptSubmit`, `Stop`, `StopFailure`, and `SessionEnd` | Prefer user-level Claude hooks for a daily Desktop/IDE-compatible workflow |
| Prompt admission | Implemented for verified Codex Desktop hooks | `UserPromptSubmit` can return a blocking decision | Possible after safe config installation, runtime health, and failure semantics are tested |
| Turn completion | Codex `Stop` reconciliation implemented | `Stop`, `StopFailure`, and `SessionEnd` are documented | Treat API failure separately from successful completion |
| Session usage signal | Aggregate provider delta plus minimal local activity evidence | Status line documents context usage, estimated session cost, session ID, workspace, and optional subscription limits | Context-window percentage is not subscription consumption; cost is not quota |
| Exact subscription tokens by folder | Not available | Not established | Never display invented token totals |
| External usage detection | Provider checkpoint movement can reveal unattributed use | Not established outside observed Claude sessions | Keep unexplained Claude consumption unassigned |
| Stop before new work | Implemented for verified Codex prompt gate and managed launch | Hook decision is documented; AQM integration not implemented | Do not claim enforcement until install/status/uninstall and failure tests pass |
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

## First Claude Code vertical slice

The first observation slice is implemented as a read-only, reversible adapter:

1. Probe the supported `claude` executable and version without starting an
   authenticated session.
2. Define typed parsers for the minimal status-line and lifecycle-hook fields
   needed by AQM. Ignore prompt, response, transcript, and tool payloads.
3. Observe optional 5-hour and 7-day subscription windows from a user-approved
   status-line integration after Claude supplies them.
4. Map the reported working directory to the nearest explicit AQM folder
   binding.
5. Display Claude as observed/degraded until a fresh supported window exists;
   do not offer manual percentage entry as if it were provider-confirmed.
6. Status-line install/status/uninstall preserves unrelated Claude settings and
   refuses to overwrite a custom status line.

Still deferred: lifecycle-hook admission, per-folder attribution, hard
enforcement, managed launch, and provider routing. Each requires a separate
capability-backed slice rather than inference from status-line quota alone.

Done means AQM can show a real Claude subscription window with its origin and
health, or clearly say why it cannot. It does not mean hard enforcement,
cross-provider routing, or production readiness.

## Privacy and security boundary

- Never read or store Claude credentials.
- Do not parse conversation transcripts to discover quota.
- Do not store prompts, responses, assistant messages, or tool inputs from hook
  payloads.
- Preserve unrelated user and project hooks when installing or removing AQM
  entries.
- Treat hook and status-line JSON as untrusted, versioned input.
- Require explicit user action before modifying `~/.claude/settings.json`.

## Sources

- [Codex App Server](https://developers.openai.com/codex/app-server)
- [Claude Code hooks reference](https://code.claude.com/docs/en/hooks)
- [Claude Code status-line reference](https://code.claude.com/docs/en/statusline)
- [Claude Code CLI reference](https://docs.anthropic.com/en/docs/claude-code/cli-usage)
