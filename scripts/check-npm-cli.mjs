import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import * as path from "node:path";
import { runInNewContext } from "node:vm";
import assert from "node:assert/strict";

const staging = mkdtempSync(join(tmpdir(), "quotafence-npm-cli-"));
try {
  execFileSync(
    process.execPath,
    ["scripts/stage-npm-cli.mjs", "cli", "", staging],
    { stdio: "inherit" },
  );
  // Run npm's JavaScript entry point directly: npm.cmd is not an executable
  // that execFileSync can launch on Windows without a shell.
  if (!process.env.npm_execpath) {
    throw new Error("Run this check with `npm run check:npm-cli`.");
  }
  execFileSync(process.execPath, [process.env.npm_execpath, "pack", join(staging, "cli"), "--dry-run"], {
    stdio: "inherit",
    env: {
      ...process.env,
      NPM_CONFIG_CACHE: join(staging, "npm-cache"),
    },
  });

  const launcher = readFileSync("npm/cli/bin/qfence.cjs", "utf8");
  // Exercise the portable wrapper for every target without pretending to run
  // foreign native binaries on the host machine.
  for (const [platform, arch, packageId, binary] of [
    ["darwin", "arm64", "cli-darwin-arm64", "qfence"],
    ["darwin", "x64", "cli-darwin-x64", "qfence"],
    ["linux", "x64", "cli-linux-x64-gnu", "qfence"],
    ["win32", "x64", "cli-win32-x64", "qfence.exe"],
  ]) {
    let invocation;
    let resolved;
    const requireMock = (module) => module === "node:path" ? path : {
      spawnSync: (...args) => { invocation = args; return { status: 7 }; },
    };
    requireMock.resolve = (module) => {
      resolved = module;
      return path.join(staging, packageId, "package.json");
    };
    const finished = Symbol("exit");
    let exitCode;
    try {
      runInNewContext(launcher, {
        require: requireMock,
        console,
        process: {
          platform, arch, argv: ["node", "qfence", "top", "--interval", "5"],
          exit: (code) => { exitCode = code; throw finished; },
        },
      });
    } catch (error) {
      if (error !== finished) throw error;
    }
    assert.equal(resolved, `@quotafence/${packageId}/package.json`);
    assert.equal(invocation[0], path.join(staging, packageId, "bin", binary));
    assert.equal(JSON.stringify(invocation[1]), JSON.stringify(["top", "--interval", "5"]));
    assert.equal(invocation[2].stdio, "inherit");
    assert.equal(exitCode, 7);
  }
  for (const packageId of [
    "cli-darwin-arm64",
    "cli-darwin-x64",
    "cli-linux-x64-gnu",
    "cli-win32-x64",
  ]) {
    const packageJson = JSON.parse(
      readFileSync(`npm/platforms/${packageId}/package.json`, "utf8"),
    );
    if (!launcher.includes(packageJson.name)) {
      throw new Error(`${packageJson.name} is not mapped by the npm CLI launcher`);
    }
  }
} finally {
  rmSync(staging, { recursive: true, force: true });
}
