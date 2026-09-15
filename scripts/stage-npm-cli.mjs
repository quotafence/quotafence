import {
  chmodSync,
  copyFileSync,
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
} from "node:fs";
import { basename, join, resolve } from "node:path";

const [packageId, binaryArgument, outputArgument = "npm-stage"] = process.argv.slice(2);
const packageIds = ["cli", "cli-darwin-arm64", "cli-darwin-x64", "cli-linux-x64-gnu", "cli-win32-x64"];
if (!packageIds.includes(packageId)) {
  throw new Error(
    "usage: node scripts/stage-npm-cli.mjs <cli|platform-id> [binary] [output-directory]",
  );
}

const root = process.cwd();
const source = packageId === "cli"
  ? join(root, "npm", "cli")
  : join(root, "npm", "platforms", packageId);
if (!existsSync(source)) {
  throw new Error(`unknown npm CLI package ${JSON.stringify(packageId)}`);
}

const outputRoot = resolve(outputArgument);
const destination = join(outputRoot, packageId);
rmSync(destination, { recursive: true, force: true });
mkdirSync(destination, { recursive: true });
cpSync(source, destination, { recursive: true });
copyFileSync(join(root, "LICENSE"), join(destination, "LICENSE"));

const rootVersion = JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version;
const packageVersion = JSON.parse(
  readFileSync(join(destination, "package.json"), "utf8"),
).version;
if (packageVersion !== rootVersion) {
  throw new Error(
    `${packageId} version ${packageVersion} does not match application ${rootVersion}`,
  );
}

if (packageId !== "cli") {
  if (!binaryArgument) {
    throw new Error(`${packageId} requires a native binary path`);
  }
  const binary = resolve(binaryArgument);
  if (!existsSync(binary)) {
    throw new Error(`native binary does not exist: ${binary}`);
  }
  const executableName = packageId.startsWith("cli-win32-") ? "qfence.exe" : "qfence";
  const binDirectory = join(destination, "bin");
  mkdirSync(binDirectory, { recursive: true });
  const stagedBinary = join(binDirectory, executableName);
  cpSync(binary, stagedBinary);
  if (executableName !== "qfence.exe") {
    chmodSync(stagedBinary, 0o755);
  }
}

console.log(`${packageId} staged at ${destination} from ${basename(source)}`);
