import { existsSync, statSync } from "node:fs";
import { spawnSync } from "node:child_process";
import path from "node:path";
import process from "node:process";

const root = process.cwd();
const binariesDir = path.join(root, "src-tauri", "binaries");
const extension = process.platform === "win32" ? ".exe" : "";

function hostTriple() {
  const result = spawnSync("rustc", ["-Vv"], { encoding: "utf-8" });
  if (result.status !== 0) {
    throw new Error(result.stderr || "rustc -Vv failed");
  }
  const host = result.stdout
    .split(/\r?\n/)
    .find((line) => line.startsWith("host:"))
    ?.split(":", 2)[1]
    ?.trim();
  if (!host) {
    throw new Error("Could not read Rust host target triple.");
  }
  return host;
}

function verifySidecar(name: string, triple: string) {
  const filePath = path.join(binariesDir, `${name}-${triple}${extension}`);
  if (!existsSync(filePath)) {
    throw new Error(`Missing Tauri sidecar: ${path.relative(root, filePath)}`);
  }
  const stats = statSync(filePath);
  if (!stats.isFile() || stats.size <= 0) {
    throw new Error(`Invalid Tauri sidecar: ${path.relative(root, filePath)}`);
  }
  console.log(`Verified ${path.relative(root, filePath)} (${stats.size} bytes)`);
}

const triple = hostTriple();
verifySidecar("patchops-backend", triple);
verifySidecar("patchops-core", triple);
