# Contributing to Agent Quota Manager

Thank you for helping make subscription quota more predictable for coding-agent
users.

## Before starting

For a bug fix or small documentation improvement, a pull request is welcome
directly. For a new provider, data-model change, or user-visible policy, open an
issue first so the behavior and security boundaries can be agreed on before a
large implementation.

Never include provider credentials, session data, private prompts, or repository
content in an issue, fixture, log, or screenshot.

## Local setup

You need Node.js 22+, pnpm 11.17.0, stable Rust, and the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your
operating system.

```bash
corepack enable
pnpm install
pnpm tauri dev
```

## Making a change

1. Create a focused branch such as `feat/codex-adapter` or `fix/quota-rollover`.
2. Keep domain policy independent from provider-specific implementation details.
3. Add or update tests for behavior changes.
4. Run `pnpm check`.
5. Update documentation when concepts, configuration, or user-visible behavior change.

Commit messages should follow the
[Conventional Commits](https://www.conventionalcommits.org/) format, for example:

```text
feat(quota): add repository allocation policy
fix(codex): reconcile usage window rollover
docs: clarify adapter capabilities
```

## Pull requests

A pull request should explain:

- the problem being solved;
- the chosen behavior and important alternatives;
- how the change was validated; and
- any effect on local data, credentials, permissions, or provider communication.

Provider adapters must declare their capabilities and must not silently fall
back from hard enforcement to estimation. A user should always be able to tell
which values are provider-confirmed, observed, or estimated.

By contributing, you agree that your contribution is licensed under the
Apache License 2.0.
