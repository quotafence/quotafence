# Changelog

All notable changes to QuotaFence are documented in this file. The
project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0-beta.1] - Unreleased

First source-distributed macOS beta of the Codex vertical slice.

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

### Known limitations

- Codex is the only provider in this beta.
- Aggregate provider percentages cannot yield exact per-workspace token usage.
- Concurrent or ambiguous activity remains unassigned by design.
- Prompt protection requires separately trusted and enabled Codex hooks and
  cannot terminate an already-running turn.
- The beta is tested on macOS and currently ships from source, without signed
  installers or automatic updates.
