# v0.1.0 release execution report

Target: `0.1.0`. This records completed distribution work and the remaining
native verification required for the initial public release.

## Completed locally

- Linux/npm distribution and public installation documentation are merged.
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
- `@quotafence/cli@0.1.0-beta.1` was published and clean-installed successfully
  from the npm registry on macOS Apple silicon.

## Not yet verified or published

- The final `0.1.0` native artifacts have not been built from merged `main` yet.
- Native Windows installer and Linux AppImage/Debian GUI smoke tests still need
  clean native accounts. Compilation is not a substitute for these tests.
- No `v0.1.0` tag or public GitHub release has been created.
- `@quotafence/cli@0.1.0` has not been published yet.

## Operator steps before publication

1. Merge the `0.1.0` version PR and build all artifacts from that exact commit.
2. Run one hosted native build pass for macOS, Windows, and Linux.
3. Record dated native smoke results using the macOS/Windows/Linux guides.
4. Configure the `npm` environment and publishing credential through GitHub's
   secret UI; never paste credentials into the task or repository.
5. Validate the matching version/tag and create the draft GitHub release.
   Inspect and attach native artifacts only after testing.
6. Publish npm with the `latest` dist-tag, then verify registry installation on
   each supported platform.
7. Invite users and resolve installation, sync, and false-block regressions
   before accepting Pro subscriptions.
