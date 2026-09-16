# Release Guide

The macOS release workflow builds one universal macOS application for Apple Silicon
and Intel, including the CLI companion expected by Tauri's multi-binary bundle,
packages a DMG, produces SHA-256 checksums, and creates a **draft release**.
A human must inspect and publish the draft.

Manual workflow runs build and retain the same artifacts for 14 days but do not
create a GitHub release. Tag pushes create the draft release only when the tag
matches every application version file.

The separate Windows workflow builds an unsigned x64 NSIS installer plus
`quotafence.exe` and `qfence.exe`. The Linux workflow builds unsigned x64
AppImage and Debian desktop packages plus `quotafence` and `qfence`. Both retain
their files as GitHub Actions artifacts for 14 days; they are not attached to
the public release until their native smoke checks are complete. See the
[Windows guide](windows.md) and [Linux guide](linux.md).

Production signing is a distribution safety requirement, not a Pro
entitlement. Once configured, official signed/notarized downloads must remain
available to Free users. Automatic update convenience may be capability-gated
later, but important security fixes must always remain manually downloadable.

## Release modes

### Ad-hoc unsigned build

When Apple signing secrets are absent, the workflow uses the ad-hoc identity
`-`. This helps downloaded Apple Silicon bundles avoid appearing corrupted, but
it is neither Developer ID signing nor notarization. The generated
`BUILD-INFO.txt` says `Ad-hoc signed; not notarized`.

Use this mode only for early testers who understand the limitation.

### Developer ID and notarized build

Configure all of these repository Actions secrets:

| Secret | Purpose |
| --- | --- |
| `APPLE_CERTIFICATE` | Base64-encoded Developer ID Application `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | Password used when exporting the `.p12` |
| `APPLE_SIGNING_IDENTITY` | Exact Developer ID Application identity |
| `KEYCHAIN_PASSWORD` | Ephemeral CI keychain password |
| `APPLE_ID` | Apple account used for notarization |
| `APPLE_PASSWORD` | App-specific Apple password |
| `APPLE_TEAM_ID` | Apple Developer team identifier |

If a certificate is provided but the remaining signing/notarization values are
incomplete, the workflow fails instead of silently publishing a partly signed
artifact. Never place these values in repository files, logs, pull requests,
or issue reports.

## Prepare a release

1. Update `CHANGELOG.md` and replace `Unreleased` with the release date.
2. Set the same semantic version in `package.json`, `package-lock.json`,
   `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`, and
   `src-tauri/tauri.conf.json`, plus every package manifest below `npm/`.
3. Validate locally:

   ```bash
   npm ci
   npm run check
   npm run check:release-version
   ```

4. Merge the version PR into `main`.
5. Create and push the matching tag, for example:

   ```bash
   git switch main
   git pull --ff-only
   git tag -a v0.1.0 -m "QuotaFence v0.1.0"
   git push origin v0.1.0
   ```

6. Inspect the workflow result and draft release:
   - verify `BUILD-INFO.txt` reports the expected signing mode;
   - verify `shasum -a 256 -c SHA256SUMS.txt` succeeds;
   - install and smoke-test the DMG on a clean macOS account;
   - confirm Codex discovery, sync, folder allocation, protection health, and
     one allow/block decision;
   - edit generated release notes and publish only after those checks pass.

7. Inspect the `Windows release` workflow artifact independently. Verify
   `SHA256SUMS.txt`, install it in a Windows test account, and run the smoke
   checklist in the Windows guide. Do not copy the unsigned preview into the
   public release without explicitly labelling the SmartScreen warning.

8. Inspect the `Linux release` artifact independently. Verify its checksum,
   test both the AppImage and Debian package in a clean Linux account, and run
   the Linux native smoke checklist. Publish Linux as Preview until that evidence
   is recorded.

All platform artifacts must contain install and uninstall scripts for the CLI.
The workflows exercise `qfence help`, the compatible `quotafence help`, and CLI
removal before uploading an artifact. A missing or non-runnable command is a
release failure, even when the desktop bundle itself builds successfully.

## Publish the npm CLI

The npm CLI is intentionally a separate, manual workflow so ordinary pushes and
release tags do not spend three additional native runner jobs. Before the first
publish, create an `npm` GitHub environment, add an `NPM_TOKEN` secret that can
publish the five reserved `@quotafence` packages, and protect that environment
with the desired reviewer rule.

Run **Publish npm CLI** with the exact application version and the `latest`
distribution tag for a normal release. It builds and packs four native packages
first, then publishes `@quotafence/cli`, whose two command aliases select the
correct optional native dependency. Reserve `beta` for explicit prereleases.

After publishing, verify from clean macOS, Windows, and Linux accounts:

```bash
npm install --global @quotafence/cli
qfence help
qfence top
```

Never republish or replace files for an existing npm version. Fix a failed or
incomplete release with a new version.

## Rollback

Keep a failed draft unpublished. If a published release is unsafe, mark it clearly
in the release notes, remove it from recommendations, and ship a new version;
do not silently replace assets under an existing tag because that invalidates
checksums and provenance.
