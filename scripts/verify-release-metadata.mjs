import { readFileSync } from "node:fs";

const LEGACY_MSI_UPGRADE_CODE = "a9a7e56f-6704-5167-96c9-3df300383a42";
const repoRoot = new URL("../", import.meta.url);
const args = process.argv.slice(2);
const requestedChannel = args.find((arg) => !arg.startsWith("--"));
const checkTags = args.includes("--check-tags");

if (
  args.some((arg) => arg.startsWith("--") && arg !== "--check-tags") ||
  args.filter((arg) => !arg.startsWith("--")).length > 1 ||
  (requestedChannel && !["beta", "stable"].includes(requestedChannel))
) {
  fail("usage: node scripts/verify-release-metadata.mjs [beta|stable] [--check-tags]");
}

const packageMetadata = readJson("package.json");
const tauriConfig = readJson("src-tauri/tauri.conf.json");
const cargoManifest = readFileSync(new URL("src-tauri/Cargo.toml", repoRoot), "utf8");
const cargoVersion = cargoManifest.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
const version = parseVersion(packageMetadata.version, "package.json version");
const channel = version.beta === null ? "stable" : "beta";

if (requestedChannel && requestedChannel !== channel) {
  fail(`expected a ${requestedChannel} release, found ${packageMetadata.version}`);
}

const expectedCargoVersion =
  version.beta === null
    ? packageMetadata.version
    : `${version.major}.${version.minor}.${version.patch}-beta.${version.beta}`;
if (cargoVersion !== expectedCargoVersion) {
  fail(`Cargo.toml version must be ${expectedCargoVersion}, found ${cargoVersion ?? "none"}`);
}

const expectedMsiVersion = msiVersion(version);
const wix = tauriConfig.bundle?.windows?.wix;
if (wix?.version !== expectedMsiVersion) {
  fail(`WiX version must be ${expectedMsiVersion}, found ${wix?.version ?? "none"}`);
}
if (wix?.upgradeCode?.toLowerCase() !== LEGACY_MSI_UPGRADE_CODE) {
  fail(`WiX UpgradeCode must remain ${LEGACY_MSI_UPGRADE_CODE}`);
}

if (checkTags) {
  verifyTagOrdering(version, packageMetadata.version);
}

console.log(
  `Release metadata OK: ${packageMetadata.version} (${channel}) -> MSI ${expectedMsiVersion}`,
);

function readJson(path) {
  return JSON.parse(readFileSync(new URL(path, repoRoot), "utf8"));
}

function parseVersion(value, label, required = true) {
  const match = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-beta([1-9]\d*))?$/.exec(
    value ?? "",
  );
  if (!match) {
    if (required) {
      fail(`${label} must use M.m.p or M.m.p-betaN`);
    }
    return null;
  }

  const parsed = {
    major: Number(match[1]),
    minor: Number(match[2]),
    patch: Number(match[3]),
    beta: match[4] === undefined ? null : Number(match[4]),
  };
  if (parsed.major > 255 || parsed.minor > 255 || parsed.patch > 255) {
    fail(`${label} exceeds MSI's supported major, minor, or patch range`);
  }
  if (parsed.beta !== null && (parsed.beta < 1 || parsed.beta > 254)) {
    fail(`${label} beta number must be between 1 and 254`);
  }
  return parsed;
}

function msiVersion(version) {
  const build = version.patch * 256 + (version.beta ?? 255);
  return `${version.major}.${version.minor}.${build}`;
}

function compareVersions(left, right) {
  for (const key of ["major", "minor", "patch"]) {
    if (left[key] !== right[key]) {
      return left[key] - right[key];
    }
  }
  if (left.beta === right.beta) return 0;
  if (left.beta === null) return 1;
  if (right.beta === null) return -1;
  return left.beta - right.beta;
}

function verifyTagOrdering(current, currentText) {
  if (!("RELEASE_VERSION_TAGS" in process.env)) {
    fail("--check-tags requires RELEASE_VERSION_TAGS from git tag --list 'v*'");
  }
  const tags = process.env.RELEASE_VERSION_TAGS
    .split(/\r?\n/)
    .filter(Boolean);

  for (const tag of tags) {
    const taggedText = tag.slice(1);
    if (taggedText === currentText) continue;
    const tagged = parseVersion(taggedText.replace(/-beta$/, "-beta1"), `tag ${tag}`, false);
    if (tagged && compareVersions(current, tagged) <= 0) {
      fail(`release ${currentText} must be newer than merged tag ${tag}`);
    }
  }
}

function fail(message) {
  console.error(`Release metadata error: ${message}`);
  process.exit(1);
}
