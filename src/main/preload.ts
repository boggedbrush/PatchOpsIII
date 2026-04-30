import { contextBridge, ipcRenderer } from "electron";

contextBridge.exposeInMainWorld("patchOpsDesktop", {
  pickGameDirectory: () => ipcRenderer.invoke("desktop:pick-game-directory"),
  getBackendUrl: () => ipcRenderer.invoke("desktop:backend-url"),
  getPlatform: () => ipcRenderer.invoke("desktop:platform")
});
