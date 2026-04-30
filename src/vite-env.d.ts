/// <reference types="vite/client" />

interface DesktopBridge {
  pickGameDirectory: () => Promise<string | null>;
  getBackendUrl: () => Promise<string>;
  getPlatform: () => Promise<string>;
}

interface Window {
  patchOpsDesktop?: DesktopBridge;
}
