export type WindowState = { maximized: boolean };
export type DesktopRuntime = "tauri" | "electron" | "web";

type TauriInvoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

function desktopWindow() {
  return typeof window === "undefined" ? undefined : window;
}

function hasTauriRuntime() {
  return Boolean(desktopWindow()?.__TAURI_INTERNALS__);
}

async function tauriInvoke<T>(command: string, args?: Record<string, unknown>) {
  const api = await import("@tauri-apps/api/core");
  return (api.invoke as TauriInvoke)<T>(command, args);
}

function electronBridge() {
  return desktopWindow()?.patchOpsDesktop;
}

export function hasDesktopBridge() {
  return hasTauriRuntime() || Boolean(electronBridge());
}

export function desktopRuntime(): DesktopRuntime {
  if (hasTauriRuntime()) {
    return "tauri";
  }
  if (electronBridge()) {
    return "electron";
  }
  return "web";
}

export async function getBackendUrl(): Promise<string> {
  if (hasTauriRuntime()) {
    return tauriInvoke<string>("backend_url");
  }
  const bridge = electronBridge();
  if (bridge) {
    return bridge.getBackendUrl();
  }
  return import.meta.env?.VITE_PATCHOPSIII_BACKEND_URL ?? "http://127.0.0.1:8765";
}

export async function pickGameDirectory(): Promise<string | null> {
  if (hasTauriRuntime()) {
    return tauriInvoke<string | null>("pick_game_directory");
  }
  return electronBridge()?.pickGameDirectory() ?? null;
}

export async function getPlatform(): Promise<string> {
  if (hasTauriRuntime()) {
    return tauriInvoke<string>("platform");
  }
  return electronBridge()?.getPlatform() ?? Promise.resolve("web");
}

export async function getWindowState(): Promise<WindowState> {
  if (hasTauriRuntime()) {
    return tauriInvoke<WindowState>("window_state");
  }
  return electronBridge()?.getWindowState() ?? Promise.resolve({ maximized: false });
}

export async function minimizeWindow(): Promise<void> {
  if (hasTauriRuntime()) {
    await tauriInvoke<void>("window_minimize");
    return;
  }
  await electronBridge()?.minimizeWindow();
}

export async function toggleMaximizeWindow(): Promise<WindowState> {
  if (hasTauriRuntime()) {
    return tauriInvoke<WindowState>("window_toggle_maximize");
  }
  return electronBridge()?.toggleMaximizeWindow() ?? Promise.resolve({ maximized: false });
}

export async function closeWindow(): Promise<void> {
  if (hasTauriRuntime()) {
    await tauriInvoke<void>("window_close");
    return;
  }
  await electronBridge()?.closeWindow();
}

export async function openExternalUrl(url: string): Promise<void> {
  if (hasTauriRuntime()) {
    await tauriInvoke<void>("open_external_url", { url });
    return;
  }
  desktopWindow()?.open(url, "_blank", "noopener,noreferrer");
}

export function onWindowStateChange(callback: (state: WindowState) => void): () => void {
  if (hasTauriRuntime()) {
    void getWindowState().then(callback).catch(() => undefined);
    let unlistenPromise = import("@tauri-apps/api/event")
      .then(({ listen }) => listen<WindowState>("desktop:window-state", (event) => callback(event.payload)))
      .catch(() => undefined);
    return () => {
      void unlistenPromise.then((unlisten) => unlisten?.());
    };
  }
  return electronBridge()?.onWindowStateChange?.(callback) ?? (() => undefined);
}
