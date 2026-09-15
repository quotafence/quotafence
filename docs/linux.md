# Linux Preview

QuotaFence builds an x64 Linux desktop preview and standalone CLI on Ubuntu
22.04. The desktop artifacts are an AppImage and a Debian package. Linux ARM64,
RPM, Flatpak, Snap, and musl/Alpine packages are not part of the first beta.

## Test from source

Install Node.js 22, npm 10, stable Rust, and the Tauri system dependencies. On
Ubuntu 22.04 or a compatible Debian-based distribution:

```bash
sudo apt-get update
sudo apt-get install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev
npm ci
npm run tauri -- dev
```

Build the release artifacts with:

```bash
cargo build --locked --release --manifest-path src-tauri/Cargo.toml --bin quotafence
npm run tauri -- build --bundles appimage,deb
```

## Test like an end user

Download and extract the complete `quotafence-linux-x64` workflow artifact, then
verify it before running anything:

```bash
sha256sum --check SHA256SUMS.txt
```

Run the AppImage:

```bash
chmod +x ./*.AppImage
./*.AppImage
```

Or install the Debian package:

```bash
sudo apt install ./*.deb
```

Install the standalone CLI without Rust or Node.js:

```bash
./install-cli.sh ./qfence
qfence status
qfence top
```

Claude Code credentials are read from
`$CLAUDE_CONFIG_DIR/.credentials.json` when `CLAUDE_CONFIG_DIR` is set, or
`~/.claude/.credentials.json` otherwise. Codex must be available on `PATH` and
signed in. Never attach either credential file to an issue.

## Native smoke checklist

- Launch the AppImage and the Debian-installed app in a clean user account.
- Confirm Codex and Claude Code discovery after signing in normally.
- Compare both 5-hour and weekly reset values with each provider.
- Create, resize, reorder, and delete a disposable weekly allocation.
- Run `qfence top`, exercise its add/edit/delete dialogs, and quit with `q`.
- Install and uninstall each supported hook without altering unrelated hooks.
- Confirm the desktop app and CLI use the same local database.
- Remove the app and CLI using the installation guide.

Linux remains Preview until this checklist has dated evidence on a clean Linux
machine. A successful cross-platform compile alone is not native validation.
