#!/usr/bin/env node

"use strict";

const { spawnSync } = require("node:child_process");
const { dirname, join } = require("node:path");

const targets = {
  "darwin-arm64": ["@quotafence/cli-darwin-arm64", "qfence"],
  "darwin-x64": ["@quotafence/cli-darwin-x64", "qfence"],
  "linux-x64": ["@quotafence/cli-linux-x64-gnu", "qfence"],
  "win32-x64": ["@quotafence/cli-win32-x64", "qfence.exe"],
};

const target = `${process.platform}-${process.arch}`;
const selected = targets[target];
if (!selected) {
  console.error(
    `QuotaFence does not publish a native CLI for ${process.platform}/${process.arch} yet.`,
  );
  process.exit(1);
}

const [packageName, executableName] = selected;
let executable;
try {
  executable = join(dirname(require.resolve(`${packageName}/package.json`)), "bin", executableName);
} catch {
  console.error(
    `QuotaFence could not find ${packageName}. Reinstall @quotafence/cli without omitting optional dependencies.`,
  );
  process.exit(1);
}

const result = spawnSync(executable, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error(`QuotaFence could not start its native CLI: ${result.error.message}`);
  process.exit(1);
}
if (result.signal) {
  process.kill(process.pid, result.signal);
}
process.exit(result.status ?? 1);
