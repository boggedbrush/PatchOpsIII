/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_PATCHOPSIII_BACKEND_URL?: string;
  readonly VITE_PATCHOPSIII_TITLEBAR_PLATFORM?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

interface DesktopBridge {
  pickGameDirectory: () => Promise<string | null>;
  getBackendUrl: () => Promise<string>;
  getPlatform: () => Promise<string>;
  getWindowState: () => Promise<{ maximized: boolean }>;
  minimizeWindow: () => Promise<void>;
  toggleMaximizeWindow: () => Promise<{ maximized: boolean }>;
  closeWindow: () => Promise<void>;
  onWindowStateChange: (callback: (state: { maximized: boolean }) => void) => () => void;
}

interface Window {
  patchOpsDesktop?: DesktopBridge;
  __TAURI_INTERNALS__?: unknown;
}
