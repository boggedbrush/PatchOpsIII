import type { IpcRendererEvent } from "electron";

const { contextBridge, ipcRenderer } = require("electron") as typeof import("electron");

contextBridge.exposeInMainWorld("patchOpsDesktop", {
  pickGameDirectory: () => ipcRenderer.invoke("desktop:pick-game-directory"),
  getBackendUrl: () => ipcRenderer.invoke("desktop:backend-url"),
  getPlatform: () => ipcRenderer.invoke("desktop:platform"),
  getWindowState: () => ipcRenderer.invoke("desktop:window-state"),
  minimizeWindow: () => ipcRenderer.invoke("desktop:window-minimize"),
  toggleMaximizeWindow: () => ipcRenderer.invoke("desktop:window-toggle-maximize"),
  closeWindow: () => ipcRenderer.invoke("desktop:window-close"),
  onWindowStateChange: (callback: (state: { maximized: boolean }) => void) => {
    const listener = (_event: IpcRendererEvent, state: { maximized: boolean }) => callback(state);
    ipcRenderer.on("desktop:window-state", listener);
    return () => ipcRenderer.removeListener("desktop:window-state", listener);
  }
});
