import { spawnSync } from "node:child_process";
import process from "node:process";

const releaseMode = process.argv.includes("--release");
const fastMode = process.argv.includes("--fast") || !releaseMode;
const bundles = valueAfter("--bundles");

function valueAfter(flag: string) {
  const index = process.argv.indexOf(flag);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function backendPlatform() {
  if (bundles === "msi") {
    return "win";
  }
  if (bundles === "appimage") {
    return "linux";
  }
  return process.platform === "win32" ? "win" : "linux";
}

function run(command: string, args: string[]) {
  const result = spawnSync(command, args, {
    stdio: "inherit",
    shell: process.platform === "win32",
    env: process.env
  });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

console.log(`Building Tauri package in ${fastMode ? "fast" : "release"} mode${bundles ? ` for ${bundles}` : ""}.`);

run("bun", ["run", "check:tauri-version"]);
run("bun", ["scripts/build-backend.ts", "--platform", backendPlatform(), ...(releaseMode ? ["--force"] : [])]);
run("bun", ["run", "build:core"]);
run("bun", ["run", "prepare:tauri-sidecars"]);
run("bun", ["run", "verify:tauri-sidecars"]);
run("bun", ["run", "clean:tauri-bundles"]);
run("bun", ["x", "tauri", "build", ...(bundles ? ["--bundles", bundles] : [])]);

if (bundles === "msi" || (!bundles && process.platform === "win32")) {
  run("bun", ["run", "verify:tauri-win-package"]);
}
