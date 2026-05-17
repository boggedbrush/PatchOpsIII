import { spawn, spawnSync } from "node:child_process";
import process from "node:process";
import { cleanupSession, killChildTree, registerProcess } from "./dev-lifecycle";

function runStep(name: string, command: string, args: string[]) {
  const result = spawnSync(command, args, {
    stdio: "inherit",
    shell: process.platform === "win32",
    env: process.env
  });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

const backendPort = process.env.PATCHOPSIII_BACKEND_PORT ?? "8767";
let shuttingDown = false;

cleanupSession("tauri");
runStep("core", "bun", ["run", "build:core"]);
runStep("sidecars", "bun", ["scripts/prepare-tauri-sidecars.ts", "--dev"]);

const tauri = spawn("bun", ["x", "tauri", "dev"], {
  stdio: "inherit",
  shell: process.platform === "win32",
  env: {
    ...process.env,
    PATCHOPSIII_BACKEND_PORT: backendPort
  }
});

registerProcess("tauri", "tauri", tauri, "bun x tauri dev");
tauri.on("exit", (code) => shutdown(code ?? 0));

function shutdown(code = 0) {
  if (shuttingDown) {
    return;
  }

  shuttingDown = true;
  killChildTree(tauri);
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
