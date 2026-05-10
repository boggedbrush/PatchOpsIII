import { spawn, type ChildProcess } from "node:child_process";
import process from "node:process";
import { cleanupSession, killChildTree, registerProcess } from "./dev-lifecycle";

const viteHost = process.env.PATCHOPSIII_VITE_HOST ?? "127.0.0.1";
const vitePort = process.env.PATCHOPSIII_VITE_PORT ?? "5174";
const backendPort = process.env.PATCHOPSIII_BACKEND_PORT ?? "8766";
const viteUrl = `http://${viteHost}:${vitePort}`;
const backendUrl = `http://127.0.0.1:${backendPort}`;

let electron: ChildProcess | null = null;
let shuttingDown = false;
cleanupSession("desktop");

const vite = spawn("bun", ["x", "vite", "--host", viteHost, "--port", vitePort, "--strictPort"], {
  stdio: "inherit",
  shell: process.platform === "win32",
  env: {
    ...process.env,
    VITE_PATCHOPSIII_BACKEND_URL: backendUrl
  }
});
registerProcess("desktop", "vite", vite, `bun x vite --host ${viteHost} --port ${vitePort} --strictPort`);
vite.on("exit", (code) => {
  if (!shuttingDown) {
    shutdown(code ?? 0);
  }
});

async function waitForVite() {
  const start = Date.now();
  while (Date.now() - start < 15000) {
    try {
      const response = await fetch(viteUrl);
      if (response.ok) {
        return;
      }
    } catch {
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
  }
  throw new Error("Vite dev server did not become ready.");
}

void waitForVite()
  .then(() => {
    electron = spawn("bun", ["x", "electron", "."], {
      stdio: "inherit",
      shell: process.platform === "win32",
      env: {
        ...process.env,
        PATCHOPSIII_BACKEND_PORT: backendPort,
        VITE_DEV_SERVER_URL: viteUrl
      }
    });
    registerProcess("desktop", "electron", electron, "bun x electron .");

    electron.on("exit", (code) => shutdown(code ?? 0));
  })
  .catch((error) => {
    console.error(error);
    shutdown(1);
  });

function shutdown(code = 0) {
  if (shuttingDown) {
    return;
  }

  shuttingDown = true;
  killChildTree(electron);
  killChildTree(vite);
  process.exit(code);
}

process.on("SIGINT", () => shutdown(0));
process.on("SIGTERM", () => shutdown(0));
process.on("SIGBREAK", () => shutdown(0));
process.on("SIGHUP", () => shutdown(0));
process.on("uncaughtException", (error) => {
  console.error(error);
  shutdown(1);
});
