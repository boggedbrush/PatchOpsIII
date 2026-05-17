import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, rmSync, statSync } from "node:fs";
import { spawnSync } from "node:child_process";
import path from "node:path";
import process from "node:process";

const root = process.cwd();
const binariesDir = path.join(root, "src-tauri", "binaries");
const backendRuntimeDir = path.join(root, "src-tauri", "backend-runtime");
const extension = process.platform === "win32" ? ".exe" : "";
const devMode = process.argv.includes("--dev");

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

function requireFile(candidates: string[], label: string) {
  const found = candidates.find((candidate) => existsSync(candidate));
  if (!found) {
    throw new Error(`${label} was not found. Checked:\n${candidates.join("\n")}`);
  }
  return found;
}

function copySidecar(source: string, name: string, triple: string) {
  mkdirSync(binariesDir, { recursive: true });
  const target = path.join(binariesDir, `${name}-${triple}${extension}`);
  copyFileSync(source, target);
  if (process.platform !== "win32") {
    chmodSync(target, 0o755);
  }
  console.log(`Prepared ${path.relative(root, target)}`);
}

function prepareBackendRuntime(source: string) {
  rmSync(backendRuntimeDir, { recursive: true, force: true });
  mkdirSync(backendRuntimeDir, { recursive: true });

  if (statSync(source).isDirectory()) {
    cpSync(source, backendRuntimeDir, { recursive: true });
  } else {
    copyFileSync(source, path.join(backendRuntimeDir, `patchops-backend${extension}`));
  }

  if (process.platform !== "win32") {
    const backendExe = path.join(backendRuntimeDir, `patchops-backend${extension}`);
    if (existsSync(backendExe)) {
      chmodSync(backendExe, 0o755);
    }
  }
  console.log(`Prepared ${path.relative(root, backendRuntimeDir)}`);
}

const triple = hostTriple();
const core = requireFile(
  [
    path.join(root, "target", "release", `patchops-core${extension}`),
    path.join(root, "crates", "patchops-core", "target", "release", `patchops-core${extension}`)
  ],
  "Rust core sidecar"
);

const backendCandidates = [
  path.join(root, "dist", "backend", "patchops-backend"),
  path.join(root, "dist", "backend", `patchops-backend${extension}`),
  path.join(root, "backend-bin", `patchops-backend${extension}`)
];
const backend = backendCandidates.find((candidate) => existsSync(candidate));
if (!backend && !devMode) {
  throw new Error(`PyInstaller backend sidecar was not found. Checked:\n${backendCandidates.join("\n")}`);
}

prepareBackendRuntime(backend ?? core);
copySidecar(core, "patchops-core", triple);
if (!backend && devMode) {
  console.log("Prepared dev-only backend runtime placeholder. Tauri dev still runs Python through uvicorn.");
}
