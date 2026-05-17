import { existsSync, readdirSync, statSync } from "node:fs";
import { spawnSync } from "node:child_process";
import path from "node:path";
import process from "node:process";

const root = process.cwd();
const force = process.argv.includes("--force") || process.argv.includes("--release");
const platformArg = valueAfter("--platform");
const platform = platformArg ?? (process.platform === "win32" ? "win" : "linux");

if (platform !== "win" && platform !== "linux") {
  throw new Error("Usage: bun scripts/build-backend.ts --platform <win|linux> [--force]");
}

const hiddenImports = [
  "uvicorn.logging",
  "uvicorn.loops",
  "uvicorn.loops.auto",
  "uvicorn.protocols",
  "uvicorn.protocols.http",
  "uvicorn.protocols.http.auto",
  "uvicorn.protocols.websockets",
  "uvicorn.protocols.websockets.auto",
  "uvicorn.lifespan",
  "uvicorn.lifespan.on"
];

function valueAfter(flag: string) {
  const index = process.argv.indexOf(flag);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function listFiles(directory: string, predicate: (filePath: string) => boolean) {
  const files: string[] = [];
  if (!existsSync(directory)) {
    return files;
  }
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const filePath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...listFiles(filePath, predicate));
    } else if (predicate(filePath)) {
      files.push(filePath);
    }
  }
  return files;
}

function newestMtime(files: string[]) {
  return files
    .filter((file) => existsSync(file))
    .map((file) => statSync(file).mtimeMs)
    .reduce((newest, mtime) => Math.max(newest, mtime), 0);
}

function backendOutput() {
  return platform === "win"
    ? path.join(root, "dist", "backend", "patchops-backend", "patchops-backend.exe")
    : path.join(root, "dist", "backend", "patchops-backend");
}

function backendInputs() {
  return [
    ...listFiles(path.join(root, "backend"), (file) => file.endsWith(".py")),
    path.join(root, "utils.py"),
    path.join(root, "bo3_enhanced.py"),
    path.join(root, "dxvk_manager.py"),
    path.join(root, "t7_patch.py"),
    path.join(root, "requirements.txt"),
    path.join(root, "PatchOpsIII.ico"),
    path.join(root, "installer", "patchops-backend-version.pyi"),
    path.join(root, "scripts", "build-backend.ts"),
    path.join(root, "scripts", "build-backend-win.ts"),
    path.join(root, "scripts", "check-backend-build.ts")
  ];
}

function run(command: string, args: string[]) {
  const result = spawnSync(command, args, { cwd: root, stdio: "inherit" });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

const output = backendOutput();
const inputMtime = newestMtime(backendInputs());
const outputMtime = existsSync(output) ? statSync(output).mtimeMs : 0;

if (!force && outputMtime >= inputMtime) {
  console.log(`Reused ${path.relative(root, output)}; backend inputs unchanged.`);
  process.exit(0);
}

run("bun", ["scripts/check-backend-build.ts", platform]);

const python = platform === "win" ? path.join(root, ".venv", "Scripts", "python.exe") : path.join(root, ".venv", "bin", "python");
const commonArgs = [
  "-m",
  "PyInstaller",
  "--noconfirm",
  "--name",
  "patchops-backend",
  "--distpath",
  path.join(root, "dist", "backend"),
  "--workpath",
  path.join(root, "build", "pyinstaller"),
  "--specpath",
  path.join(root, "build", "pyinstaller"),
  ...hiddenImports.flatMap((name) => ["--hidden-import", name]),
  path.join(root, "backend", "api.py")
];

const args =
  platform === "win"
    ? [
        ...commonArgs.slice(0, 2),
        "--onedir",
        "--noconsole",
        "--icon",
        path.join(root, "PatchOpsIII.ico"),
        "--version-file",
        path.join(root, "installer", "patchops-backend-version.pyi"),
        ...commonArgs.slice(2)
      ]
    : ["-m", "PyInstaller", "--noconfirm", "--onefile", "--name", "patchops-backend", "--distpath", path.join(root, "dist", "backend"), "--workpath", path.join(root, "build", "pyinstaller"), "--specpath", path.join(root, "build", "pyinstaller"), ...hiddenImports.flatMap((name) => ["--hidden-import", name]), path.join(root, "backend", "api.py")];

run(python, args);
