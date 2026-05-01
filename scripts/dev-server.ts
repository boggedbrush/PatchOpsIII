import { existsSync } from "node:fs";
import path from "node:path";
import { spawn, type ChildProcess } from "node:child_process";
import process from "node:process";

const backendHost = process.env.PATCHOPSIII_BACKEND_HOST ?? "127.0.0.1";
const backendPort = process.env.PATCHOPSIII_BACKEND_PORT ?? "8765";
const viteHost = process.env.PATCHOPSIII_VITE_HOST ?? "127.0.0.1";
const vitePort = process.env.PATCHOPSIII_VITE_PORT ?? "5173";

function resolvePython() {
  if (process.env.PATCHOPSIII_PYTHON) {
    return process.env.PATCHOPSIII_PYTHON;
  }

  const venvPython =
    process.platform === "win32"
      ? path.join(".venv", "Scripts", "python.exe")
      : path.join(".venv", "bin", "python");

  if (existsSync(venvPython)) {
    return venvPython;
  }

  return process.platform === "win32" ? "python" : "python3";
}

const children: ChildProcess[] = [];

function start(name: string, command: string, args: string[]) {
  const child = spawn(command, args, {
    stdio: "inherit",
    shell: process.platform === "win32",
    env: {
      ...process.env,
      PYTHONUNBUFFERED: "1"
    }
  });

  children.push(child);
  child.on("exit", (code) => {
    if (code && code !== 0) {
      console.error(`${name} exited with code ${code}`);
      shutdown(code);
    }
  });
}

function shutdown(code = 0) {
  for (const child of children) {
    child.kill();
  }
  process.exit(code);
}

start("backend", resolvePython(), [
  "-m",
  "uvicorn",
  "backend.api:app",
  "--host",
  backendHost,
  "--port",
  backendPort
]);

start("vite", "bun", ["x", "vite", "--host", viteHost, "--port", vitePort, "--strictPort"]);

process.on("SIGINT", () => shutdown(0));
process.on("SIGTERM", () => shutdown(0));
