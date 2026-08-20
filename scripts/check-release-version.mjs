import { readFileSync } from "node:fs";

const rootPackage = JSON.parse(readFileSync("package.json", "utf8"));
const packageLock = JSON.parse(readFileSync("package-lock.json", "utf8"));
const tauriConfig = JSON.parse(
  readFileSync("src-tauri/tauri.conf.json", "utf8"),
);
const cargoManifest = readFileSync("src-tauri/Cargo.toml", "utf8");
const cargoLock = readFileSync("src-tauri/Cargo.lock", "utf8");

const cargoVersion = cargoManifest.match(
  /^\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m,
)?.[1];
const lockedCargoVersion = cargoLock.match(
  /\[\[package\]\]\s+name = "quotafence"\s+version = "([^"]+)"/,
)?.[1];

const versions = new Map([
  ["package.json", rootPackage.version],
  ["package-lock.json", packageLock.version],
  ["package-lock root package", packageLock.packages?.[""]?.version],
  ["src-tauri/Cargo.toml", cargoVersion],
  ["src-tauri/Cargo.lock", lockedCargoVersion],
  ["src-tauri/tauri.conf.json", tauriConfig.version],
]);

const missing = [...versions].filter(([, version]) => !version);
if (missing.length > 0) {
  throw new Error(`Could not read version from: ${missing.map(([file]) => file).join(", ")}`);
}

const uniqueVersions = new Set(versions.values());
if (uniqueVersions.size !== 1) {
  const details = [...versions]
    .map(([file, version]) => `${file}=${version}`)
    .join(", ");
  throw new Error(`Release versions are not aligned: ${details}`);
}

const version = uniqueVersions.values().next().value;
const releaseTag = process.env.RELEASE_TAG;
if (releaseTag && releaseTag !== `v${version}`) {
  throw new Error(
    `Release tag ${releaseTag} does not match application version v${version}`,
  );
}

console.log(
  releaseTag
    ? `Release version ${version} matches ${releaseTag}.`
    : `Release versions are aligned at ${version}.`,
);
