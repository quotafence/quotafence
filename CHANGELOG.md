# Changelog

All notable changes to QuotaFence are documented in this file. The
project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0-beta.1] - Unreleased

First source-distributed multi-agent beta of the QuotaFence vertical slice.

### Added

- Codex subscription discovery and provider-confirmed quota refresh.
- Reset-window rollover with allocation targets carried into the new window.
- Folder allocations with drag-and-drop protection priority.
- Passive Codex Desktop attribution from minimal local thread metadata.
- Optional trusted Codex prompt hooks for pre-prompt workspace enforcement.
- `quotafence context`, `quotafence admit codex`, and managed `quotafence run codex` workflows.
- Workspace policy thresholds, provider reconciliation, burn-rate signals, and
  integration health diagnostics.
- Recovery coverage for missing lifecycle events, restarts, helper working
  directories, overlapping work, and provider corrections.
- Full QuotaFence product, CLI, hook, environment-variable, package, release,
  and application-identifier namespace.
- One-time local ledger and integration migration from pre-QuotaFence builds.
- Claude Code 5-hour and weekly allowance discovery, reversible lifecycle
  hooks, and managed CLI launches for either native window.
- Windows x64 CI, preview installer workflow, CLI assets, and platform-aware
  process recovery.
- A centralized capability/entitlement boundary that keeps the complete Free
  core available and falls back safely when future grants expire.
- A versioned capability snapshot in Desktop Settings and the read-only
  `qfence features [--json]` command.

### Known limitations

- Codex and Claude Code are beta providers; Gemini and other agents are not yet
  implemented.
- Aggregate provider percentages cannot yield exact per-workspace token usage.
- Concurrent or ambiguous activity remains unassigned by design.
- Prompt protection requires separately trusted and enabled Codex hooks and
  cannot terminate an already-running turn.
- macOS is the live-tested platform. Windows x64 builds in CI but native
  installed-app validation is not complete.
- Production-signed installers and automatic updates are not yet available.
