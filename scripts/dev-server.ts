import { existsSync } from "node:fs";
import path from "node:path";
import { spawn, type ChildProcess } from "node:child_process";
import process from "node:process";
import { cleanupSession, killChildTree, registerProcess } from "./dev-lifecycle";

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
let shuttingDown = false;
cleanupSession("browser");

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
  registerProcess("browser", name, child, `${command} ${args.join(" ")}`);
  child.on("exit", (code) => {
    if (shuttingDown) {
      return;
    }

    if (code && code !== 0) {
      console.error(`${name} exited with code ${code}`);
    } else {
      console.log(`${name} exited.`);
    }
    shutdown(code ?? 0);
  });
}

function shutdown(code = 0) {
  if (shuttingDown) {
    return;
  }

  shuttingDown = true;
  for (const child of children) {
    killChildTree(child);
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
process.on("SIGBREAK", () => shutdown(0));
process.on("SIGHUP", () => shutdown(0));
process.on("uncaughtException", (error) => {
  console.error(error);
  shutdown(1);
});
