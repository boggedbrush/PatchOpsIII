import { readFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";

const root = process.cwd();

function readJson<T>(filePath: string): T {
  return JSON.parse(readFileSync(filePath, "utf-8")) as T;
}

function numericBaseVersion(version: string): string {
  const match = /^v?(\d+\.\d+\.\d+)(?:[-+].*)?$/.exec(version);
  if (!match) {
    throw new Error(`package.json version must look like v1.2.3 or v1.2.3-beta. Got: ${version}`);
  }
  return match[1];
}

type PackageJson = {
  version?: string;
};

type TauriConfig = {
  version?: string;
};

const packageJson = readJson<PackageJson>(path.join(root, "package.json"));
if (!packageJson.version) {
  throw new Error("package.json is missing version.");
}

const expectedTauriVersion = numericBaseVersion(packageJson.version);
const tauriConfig = readJson<TauriConfig>(path.join(root, "src-tauri", "tauri.conf.json"));
const tauriCargoToml = readFileSync(path.join(root, "src-tauri", "Cargo.toml"), "utf-8");
const tauriCargoVersion = /^\s*version\s*=\s*"([^"]+)"/m.exec(tauriCargoToml)?.[1];

const errors: string[] = [];
if (tauriConfig.version !== expectedTauriVersion) {
  errors.push(`src-tauri/tauri.conf.json version is ${tauriConfig.version}, expected ${expectedTauriVersion}`);
}
if (tauriCargoVersion !== expectedTauriVersion) {
  errors.push(`src-tauri/Cargo.toml version is ${tauriCargoVersion ?? "<missing>"}, expected ${expectedTauriVersion}`);
}

if (errors.length > 0) {
  throw new Error(errors.join("\n"));
}

console.log(`Tauri version metadata OK: ${expectedTauriVersion} from ${packageJson.version}`);
