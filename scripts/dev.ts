import { spawn } from "node:child_process";
import process from "node:process";

const vite = spawn("bun", ["x", "vite", "--host", "127.0.0.1"], {
  stdio: "inherit",
  shell: process.platform === "win32"
});

let electron: ReturnType<typeof spawn> | null = null;

async function waitForVite() {
  const start = Date.now();
  while (Date.now() - start < 15000) {
    try {
      const response = await fetch("http://127.0.0.1:5173");
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
        VITE_DEV_SERVER_URL: "http://127.0.0.1:5173"
      }
    });

    electron.on("exit", (code) => shutdown(code ?? 0));
  })
  .catch((error) => {
    console.error(error);
    shutdown(1);
  });

function shutdown(code = 0) {
  vite.kill();
  electron?.kill();
  process.exit(code);
}

process.on("SIGINT", () => shutdown(0));
process.on("SIGTERM", () => shutdown(0));
