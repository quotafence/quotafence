# Beta release execution report

Target: `0.1.0-beta.1`. This is an execution record, not a stable-release claim.

## Completed locally

- PR #60 is confirmed merged; the remaining Linux/npm work needs a separate PR.
- Linux x64 release workflow builds AppImage, Debian package, both CLI aliases,
  checksums, and build metadata.
- npm launcher and four platform package templates are version-aligned.
- npm publishing is manual-only and protected by the `npm` environment.
- Windows npm-check invocation uses npm's JavaScript entry point rather than
  attempting to execute `npm.cmd` directly.
- Package staging accepts only the five known package IDs.
- Staged macOS npm binaries are explicitly ad-hoc signed and verified.
- Version validation, npm package dry-run, frontend build, and whitespace checks
  pass locally.
- Portable npm wrapper tests pass for all four targets, including arguments,
  inherited TTY, and child exit code.
- Fresh Rust rebuild passes 166 library tests and 25 CLI tests.
- Formatting and clippy with warnings denied pass after the fresh rebuild.
- New local branch: `codex/linux-npm-beta`.

## Not yet verified or published

- No new branch has been pushed and no new hosted release jobs have been run.
- No repository signing/npm secrets were present when inspected.
- The `npm` environment secrets API returned 404; publishing configuration is
  not independently verified.
- Native Windows installer and Linux AppImage/Debian GUI smoke tests still need
  clean native accounts. Compilation is not a substitute for these tests.
- No npm registry installation has been tested for this version.
- No release tag, public GitHub release, or npm publication has been created.
- `CHANGELOG.md` remains Unreleased until the release gate passes.

## Operator steps before publication

1. Confirm permission for the new PR push and one hosted native build pass,
   considering the previously requested Actions usage limit.
2. Merge the new PR and build all artifacts from that exact commit.
3. Record dated native smoke results using the macOS/Windows/Linux guides.
4. Configure the `npm` environment and publishing credential through GitHub's
   secret UI; never paste credentials into the task or repository.
5. Date the changelog, validate the matching version/tag, and create the draft
   GitHub prerelease. Inspect and attach native previews only after testing.
6. Publish npm with the `beta` dist-tag, then verify registry installation on
   each supported platform. Keep `latest` unchanged until stable promotion.
7. Invite testers and resolve installation, sync, and false-block regressions
   before stable release or accepting Pro subscriptions.
