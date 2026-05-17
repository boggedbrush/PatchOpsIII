import { chmodSync, copyFileSync, existsSync, mkdirSync, readdirSync, rmSync, statSync } from "node:fs";
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

function changed(source: string, target: string) {
  if (!existsSync(target)) {
    return true;
  }
  const sourceStats = statSync(source);
  const targetStats = statSync(target);
  return sourceStats.size !== targetStats.size || sourceStats.mtimeMs > targetStats.mtimeMs + 1;
}

function copyFileIfChanged(source: string, target: string) {
  mkdirSync(path.dirname(target), { recursive: true });
  if (!changed(source, target)) {
    return false;
  }
  copyFileSync(source, target);
  if (process.platform !== "win32") {
    chmodSync(target, 0o755);
  }
  return true;
}

function listFiles(directory: string) {
  const files: string[] = [];
  if (!existsSync(directory)) {
    return files;
  }
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const filePath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...listFiles(filePath));
    } else {
      files.push(filePath);
    }
  }
  return files;
}

function removeStaleFiles(sourceDir: string, targetDir: string) {
  const sourceFiles = new Set(listFiles(sourceDir).map((file) => path.relative(sourceDir, file)));
  for (const targetFile of listFiles(targetDir)) {
    const relative = path.relative(targetDir, targetFile);
    if (relative === ".gitkeep" || sourceFiles.has(relative)) {
      continue;
    }
    rmSync(targetFile, { force: true });
  }
}

function removeEmptyDirectories(directory: string) {
  if (!existsSync(directory)) {
    return;
  }
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (!entry.isDirectory()) {
      continue;
    }
    const child = path.join(directory, entry.name);
    removeEmptyDirectories(child);
    if (readdirSync(child).length === 0) {
      rmSync(child, { recursive: true, force: true });
    }
  }
}

function copySidecar(source: string, name: string, triple: string) {
  const target = path.join(binariesDir, `${name}-${triple}${extension}`);
  const didCopy = copyFileIfChanged(source, target);
  console.log(`${didCopy ? "Prepared" : "Reused"} ${path.relative(root, target)}`);
}

function prepareBackendRuntime(source: string) {
  mkdirSync(backendRuntimeDir, { recursive: true });

  if (statSync(source).isDirectory()) {
    removeStaleFiles(source, backendRuntimeDir);
    for (const sourceFile of listFiles(source)) {
      const target = path.join(backendRuntimeDir, path.relative(source, sourceFile));
      copyFileIfChanged(sourceFile, target);
    }
    removeEmptyDirectories(backendRuntimeDir);
  } else {
    for (const targetFile of listFiles(backendRuntimeDir)) {
      const relative = path.relative(backendRuntimeDir, targetFile);
      if (relative !== ".gitkeep" && relative !== `patchops-backend${extension}`) {
        rmSync(targetFile, { force: true });
      }
    }
    copyFileIfChanged(source, path.join(backendRuntimeDir, `patchops-backend${extension}`));
    removeEmptyDirectories(backendRuntimeDir);
  }

  console.log(`Prepared ${path.relative(root, backendRuntimeDir)}`);
}

const triple = hostTriple();
const core = requireFile(
  devMode
    ? [
        path.join(root, "target", "debug", `patchops-core${extension}`),
        path.join(root, "crates", "patchops-core", "target", "debug", `patchops-core${extension}`)
      ]
    : [
        path.join(root, "target", "release", `patchops-core${extension}`),
        path.join(root, "crates", "patchops-core", "target", "release", `patchops-core${extension}`)
      ],
  "Rust core sidecar"
);

copySidecar(core, "patchops-core", triple);

if (devMode) {
  console.log("Skipped backend runtime prep in dev mode. Tauri dev runs Python through uvicorn.");
  process.exit(0);
}

const backendCandidates = [
  path.join(root, "dist", "backend", "patchops-backend"),
  path.join(root, "dist", "backend", `patchops-backend${extension}`),
  path.join(root, "backend-bin", `patchops-backend${extension}`)
];
const backend = backendCandidates.find((candidate) => existsSync(candidate));
if (!backend) {
  throw new Error(`PyInstaller backend sidecar was not found. Checked:\n${backendCandidates.join("\n")}`);
}

prepareBackendRuntime(backend);
