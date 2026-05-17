import { spawnSync } from "node:child_process";
import path from "node:path";
import process from "node:process";

const root = process.cwd();
const python = path.join(root, ".venv", "Scripts", "python.exe");
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

const args = [
  "-m",
  "PyInstaller",
  "--noconfirm",
  "--onedir",
  "--noconsole",
  "--name",
  "patchops-backend",
  "--icon",
  path.join(root, "PatchOpsIII.ico"),
  "--version-file",
  path.join(root, "installer", "patchops-backend-version.pyi"),
  "--distpath",
  path.join(root, "dist", "backend"),
  "--workpath",
  path.join(root, "build", "pyinstaller"),
  "--specpath",
  path.join(root, "build", "pyinstaller"),
  ...hiddenImports.flatMap((name) => ["--hidden-import", name]),
  path.join(root, "backend", "api.py")
];

const result = spawnSync(python, args, { cwd: root, stdio: "inherit" });
if (result.error) {
  throw result.error;
}
process.exit(result.status ?? 1);
