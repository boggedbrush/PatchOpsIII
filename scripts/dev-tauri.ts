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
if (process.env.PATCHOPSIII_SKIP_CORE_BUILD !== "1") {
  runStep("core", "bun", ["run", "build:core:dev"]);
}

const tauri = spawn("bun", ["x", "tauri", "dev", "--config", "src-tauri/tauri.dev.conf.json"], {
  stdio: "inherit",
  shell: process.platform === "win32",
  env: {
    ...process.env,
    PATCHOPSIII_BACKEND_PORT: backendPort
  }
});

registerProcess("tauri", "tauri", tauri, "bun x tauri dev --config src-tauri/tauri.dev.conf.json");
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
