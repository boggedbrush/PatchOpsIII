import { app, BrowserWindow, dialog, ipcMain, shell } from "electron";
import { spawn, spawnSync, type ChildProcessWithoutNullStreams } from "node:child_process";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const appRoot = app.isPackaged ? process.resourcesPath : path.resolve(__dirname, "../..");
const rendererRoot = app.isPackaged ? app.getAppPath() : appRoot;
const backendHost = "127.0.0.1";
const backendPort = Number(process.env.PATCHOPSIII_BACKEND_PORT ?? 8765);
const backendUrl = `http://${backendHost}:${backendPort}`;
let mainWindow: BrowserWindow | null = null;
let backendProcess: ChildProcessWithoutNullStreams | null = null;
let stoppingBackend = false;

function sendWindowState() {
  mainWindow?.webContents.send("desktop:window-state", {
    maximized: mainWindow.isMaximized()
  });
}

function pythonCommand() {
  if (process.env.PATCHOPSIII_PYTHON) {
    return process.env.PATCHOPSIII_PYTHON;
  }

  const venvPython =
    process.platform === "win32"
      ? path.join(appRoot, ".venv", "Scripts", "python.exe")
      : path.join(appRoot, ".venv", "bin", "python");

  if (existsSync(venvPython)) {
    return venvPython;
  }

  return process.platform === "win32" ? "python" : "python3";
}

function startBackend() {
  if (backendProcess) {
    return;
  }
  const packagedBackend =
    process.platform === "win32"
      ? path.join(appRoot, "backend-bin", "patchops-backend.exe")
      : path.join(appRoot, "backend-bin", "patchops-backend");
  const usePackagedBackend = app.isPackaged && existsSync(packagedBackend);
  const command = usePackagedBackend ? packagedBackend : pythonCommand();
  const args = usePackagedBackend
    ? []
    : ["-m", "uvicorn", "backend.api:app", "--host", backendHost, "--port", String(backendPort)];

  stoppingBackend = false;
  backendProcess = spawn(command, args, {
    cwd: appRoot,
    env: {
      ...process.env,
      PATCHOPSIII_BACKEND_HOST: backendHost,
      PATCHOPSIII_BACKEND_PORT: String(backendPort),
      PYTHONUNBUFFERED: "1"
    }
  });

  backendProcess.stdout.on("data", (chunk) => console.log(`[backend] ${chunk.toString().trim()}`));
  backendProcess.stderr.on("data", (chunk) => console.error(`[backend] ${chunk.toString().trim()}`));
  backendProcess.on("exit", (code) => {
    console.log(`[backend] exited with code ${code}`);
    backendProcess = null;
    stoppingBackend = false;
  });
}

function stopBackend() {
  if (!backendProcess?.pid || stoppingBackend) {
    return;
  }

  stoppingBackend = true;
  if (process.platform === "win32") {
    spawnSync("taskkill", ["/pid", String(backendProcess.pid), "/t", "/f"], { stdio: "ignore" });
    return;
  }

  backendProcess.kill("SIGTERM");
}

async function waitForBackend(timeoutMs = 12000) {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      const response = await fetch(`${backendUrl}/api/health`);
      if (response.ok) {
        return;
      }
    } catch {
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
  }
  throw new Error("PatchOpsIII backend did not become ready.");
}

async function createWindow() {
  startBackend();
  await waitForBackend();

  mainWindow = new BrowserWindow({
    width: 1320,
    height: 860,
    minWidth: 1100,
    minHeight: 720,
    title: "PatchOpsIII",
    backgroundColor: "#080806",
    frame: process.platform === "darwin",
    titleBarStyle: process.platform === "darwin" ? "hiddenInset" : "default",
    trafficLightPosition: process.platform === "darwin" ? { x: 18, y: 16 } : undefined,
    webPreferences: {
      preload: path.join(__dirname, "preload.cjs"),
      contextIsolation: true,
      nodeIntegration: false
    }
  });

  mainWindow.on("maximize", sendWindowState);
  mainWindow.on("unmaximize", sendWindowState);
  mainWindow.on("restore", sendWindowState);

  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    shell.openExternal(url);
    return { action: "deny" };
  });

  if (process.env.VITE_DEV_SERVER_URL) {
    await mainWindow.loadURL(process.env.VITE_DEV_SERVER_URL);
  } else {
    await mainWindow.loadFile(path.join(rendererRoot, "dist/renderer/index.html"));
  }
}

ipcMain.handle("desktop:pick-game-directory", async () => {
  if (!mainWindow) {
    return null;
  }
  const result = await dialog.showOpenDialog(mainWindow, {
    title: "Select Black Ops III folder",
    properties: ["openDirectory"]
  });
  return result.canceled ? null : result.filePaths[0] ?? null;
});

ipcMain.handle("desktop:backend-url", () => backendUrl);
ipcMain.handle("desktop:platform", () => process.platform);
ipcMain.handle("desktop:window-state", () => ({ maximized: Boolean(mainWindow?.isMaximized()) }));
ipcMain.handle("desktop:window-minimize", () => mainWindow?.minimize());
ipcMain.handle("desktop:window-toggle-maximize", () => {
  if (!mainWindow) {
    return { maximized: false };
  }
  if (mainWindow.isMaximized()) {
    mainWindow.unmaximize();
  } else {
    mainWindow.maximize();
  }
  const state = { maximized: mainWindow.isMaximized() };
  mainWindow.webContents.send("desktop:window-state", state);
  return state;
});
ipcMain.handle("desktop:window-close", () => mainWindow?.close());

app.whenReady().then(createWindow);
app.on("activate", () => {
  if (BrowserWindow.getAllWindows().length === 0) {
    void createWindow();
  }
});
app.on("before-quit", () => {
  stopBackend();
});
app.on("will-quit", () => {
  stopBackend();
});
app.on("window-all-closed", () => {
  if (process.platform !== "darwin") {
    app.quit();
  }
});
