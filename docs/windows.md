# Windows Support

Windows x64 is currently a preview target. The repository compiles, tests, and
builds a native NSIS installer on GitHub Actions. macOS can be used for most
implementation work, but the final installer, WebView2 runtime behavior,
provider clients, hooks, and process lifecycle must be exercised on Windows.

Normal users should start with the cross-platform
[install, upgrade, and removal guide](installing.md). The development setup
below is only for contributors building from source.

## Development prerequisites

Install these on Windows:

1. Microsoft C++ Build Tools with **Desktop development with C++** selected.
2. Microsoft Edge WebView2 Runtime when it is not already present.
3. Rust using the default MSVC toolchain.
4. Node.js 22 and npm.
5. Codex CLI and/or Claude Code if their integrations will be tested.

In PowerShell:

```powershell
npm ci
npm run tauri -- dev
```

Build and test the CLI directly with:

```powershell
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib --bin quotafence
cargo build --locked --release --manifest-path src-tauri/Cargo.toml --bin quotafence
src-tauri\target\release\quotafence.exe status
```

Build the desktop installer with:

```powershell
npm run tauri -- build --bundles nsis
```

The installer is written below `src-tauri\target\release\bundle\nsis`.

## Provider integration

QuotaFence resolves `codex.exe` and `claude.exe` from `PATH`. The existing
environment overrides still take precedence. Claude credentials are read from
`%USERPROFILE%\.claude\.credentials.json`, or from
`%CLAUDE_CONFIG_DIR%\.credentials.json` when `CLAUDE_CONFIG_DIR` is set. A
successful Claude token refresh is written back to that same credential file.
QuotaFence does not log or copy the token into its own database.

## CI and preview artifacts

Every pull request runs the Rust tests, smoke-tests both CLI command names, and
uploads the NSIS installer, CLI binary, and installer script for seven days.
This makes a PR build testable by a normal Windows user without installing a
Rust or Node.js toolchain.

The `Windows beta release` workflow runs manually and for version tags. Its
`quotafence-windows-x64` artifact contains:

- the unsigned NSIS installer;
- `quotafence.exe` and the short-name copy `qfence.exe`;
- `install-cli.ps1`, which installs both command names into the user `PATH`;
- `uninstall-cli.ps1`, which removes both commands and its `PATH` entry;
- `SHA256SUMS.txt`; and
- `BUILD-INFO.txt`.

The artifact is intentionally not attached to the public GitHub release yet.
Unsigned Windows applications can trigger Microsoft Defender SmartScreen. Code
signing is required before this becomes a normal end-user distribution path.

After extracting the artifact, install the CLI from PowerShell with:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\install-cli.ps1
qfence help
```

Upgrade by running `install-cli.ps1` from a newer verified artifact. Remove the
CLI without touching desktop data with:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\uninstall-cli.ps1
```

## Native smoke checklist

Run this on a clean Windows x64 account before calling a build supported:

1. Verify the SHA-256 checksums, install, launch, and uninstall the NSIS bundle.
2. Add and sync Codex; compare every displayed quota window and reset time with
   the provider.
3. Add and sync Claude Code; verify both 5-hour and weekly columns.
4. Run `qfence sync`, `qfence status`, `qfence here`, and one managed provider
   session from PowerShell and Windows Terminal.
5. Install and remove each lifecycle integration, confirming unrelated provider
   configuration remains unchanged.
6. Terminate a managed session and confirm the reservation is reconciled; then
   simulate an orphaned supervisor and confirm the next invocation recovers it.

Known preview gap: Windows process liveness is implemented, but console
Ctrl+C/termination forwarding has not yet passed this native checklist.
