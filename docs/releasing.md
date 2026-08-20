# macOS Beta Release

The release workflow builds one universal macOS application for Apple Silicon
and Intel, including the CLI companion expected by Tauri's multi-binary bundle,
packages a DMG, produces SHA-256 checksums, and creates a **draft prerelease**.
A human must inspect and publish the draft.

Manual workflow runs build and retain the same artifacts for 14 days but do not
create a GitHub release. Tag pushes create the draft release only when the tag
matches every application version file.

## Release modes

### Ad-hoc beta build

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
   `src-tauri/tauri.conf.json`.
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
   git tag -a v0.1.0-beta.1 -m "QuotaFence v0.1.0-beta.1"
   git push origin v0.1.0-beta.1
   ```

6. Inspect the workflow result and draft release:
   - verify `BUILD-INFO.txt` reports the expected signing mode;
   - verify `shasum -a 256 -c SHA256SUMS.txt` succeeds;
   - install and smoke-test the DMG on a clean macOS account;
   - confirm Codex discovery, sync, folder allocation, protection health, and
     one allow/block decision;
   - edit generated release notes and publish only after those checks pass.

## Rollback

Keep a failed draft unpublished. If a published beta is unsafe, mark it clearly
in the release notes, remove it from recommendations, and ship a new version;
do not silently replace assets under an existing tag because that invalidates
checksums and provenance.
