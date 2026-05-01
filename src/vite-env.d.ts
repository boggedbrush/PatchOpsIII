/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_PATCHOPSIII_BACKEND_URL?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

interface DesktopBridge {
  pickGameDirectory: () => Promise<string | null>;
  getBackendUrl: () => Promise<string>;
  getPlatform: () => Promise<string>;
}

interface Window {
  patchOpsDesktop?: DesktopBridge;
}
