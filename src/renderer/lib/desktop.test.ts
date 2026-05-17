import { beforeEach, describe, expect, mock, test } from "bun:test";

import {
  closeWindow,
  desktopRuntime,
  getBackendUrl,
  getPlatform,
  getWindowState,
  hasDesktopBridge,
  minimizeWindow,
  onWindowStateChange,
  openExternalUrl,
  pickGameDirectory,
  toggleMaximizeWindow
} from "./desktop";

const invokeCalls: Array<{ command: string; args?: Record<string, unknown> }> = [];
const listenCalls: Array<{ event: string }> = [];
let invokeResponses: Record<string, unknown> = {};
let tauriUnlistenCalls = 0;

mock.module("@tauri-apps/api/core", () => ({
  invoke: async (command: string, args?: Record<string, unknown>) => {
    invokeCalls.push({ command, args });
    return invokeResponses[command];
  }
}));

mock.module("@tauri-apps/api/event", () => ({
  listen: async (event: string, callback: (event: { payload: { maximized: boolean } }) => void) => {
    listenCalls.push({ event });
    callback({ payload: { maximized: true } });
    return () => {
      tauriUnlistenCalls += 1;
    };
  }
}));

function setWindow(value: Partial<Window>) {
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value
  });
}

describe("desktop adapter", () => {
  beforeEach(() => {
    invokeCalls.length = 0;
    listenCalls.length = 0;
    invokeResponses = {};
    tauriUnlistenCalls = 0;
    setWindow({});
  });

  test("prefers Tauri commands when Tauri internals exist", async () => {
    invokeResponses = {
      backend_url: "http://127.0.0.1:8767",
      pick_game_directory: "C:/Games/Call of Duty Black Ops III",
      platform: "win32",
      window_state: { maximized: false },
      window_toggle_maximize: { maximized: true }
    };
    setWindow({
      __TAURI_INTERNALS__: {},
      patchOpsDesktop: {
        getBackendUrl: async () => "electron",
        pickGameDirectory: async () => null,
        getPlatform: async () => "electron",
        getWindowState: async () => ({ maximized: false }),
        minimizeWindow: async () => undefined,
        toggleMaximizeWindow: async () => ({ maximized: false }),
        closeWindow: async () => undefined,
        onWindowStateChange: () => () => undefined
      }
    });

    expect(desktopRuntime()).toBe("tauri");
    expect(hasDesktopBridge()).toBe(true);
    expect(await getBackendUrl()).toBe("http://127.0.0.1:8767");
    expect(await pickGameDirectory()).toBe("C:/Games/Call of Duty Black Ops III");
    expect(await getPlatform()).toBe("win32");
    expect(await getWindowState()).toEqual({ maximized: false });
    await minimizeWindow();
    expect(await toggleMaximizeWindow()).toEqual({ maximized: true });
    await closeWindow();
    await openExternalUrl("steam://open/console");

    expect(invokeCalls.map((call) => call.command)).toEqual([
      "backend_url",
      "pick_game_directory",
      "platform",
      "window_state",
      "window_minimize",
      "window_toggle_maximize",
      "window_close",
      "open_external_url"
    ]);
    expect(invokeCalls.at(-1)?.args).toEqual({ url: "steam://open/console" });
  });

  test("uses Electron bridge when Tauri is unavailable", async () => {
    const calls: string[] = [];
    setWindow({
      patchOpsDesktop: {
        getBackendUrl: async () => {
          calls.push("backend");
          return "http://127.0.0.1:8766";
        },
        pickGameDirectory: async () => {
          calls.push("picker");
          return "D:/Steam/steamapps/common/Call of Duty Black Ops III";
        },
        getPlatform: async () => "win32",
        getWindowState: async () => ({ maximized: false }),
        minimizeWindow: async () => {
          calls.push("minimize");
        },
        toggleMaximizeWindow: async () => {
          calls.push("toggle");
          return { maximized: true };
        },
        closeWindow: async () => {
          calls.push("close");
        },
        onWindowStateChange: (callback) => {
          callback({ maximized: true });
          return () => calls.push("unlisten");
        }
      }
    });

    expect(desktopRuntime()).toBe("electron");
    expect(await getBackendUrl()).toBe("http://127.0.0.1:8766");
    expect(await pickGameDirectory()).toBe("D:/Steam/steamapps/common/Call of Duty Black Ops III");
    await minimizeWindow();
    expect(await toggleMaximizeWindow()).toEqual({ maximized: true });
    await closeWindow();
    const unlisten = onWindowStateChange((state) => calls.push(`state:${state.maximized}`));
    unlisten();

    expect(invokeCalls).toEqual([]);
    expect(calls).toEqual(["backend", "picker", "minimize", "toggle", "close", "state:true", "unlisten"]);
  });

  test("falls back to web defaults without a desktop bridge", async () => {
    const opened: string[] = [];
    setWindow({
      open: (url: string) => {
        opened.push(url);
        return null;
      }
    });

    expect(desktopRuntime()).toBe("web");
    expect(hasDesktopBridge()).toBe(false);
    expect(await getBackendUrl()).toBe("http://127.0.0.1:8765");
    expect(await pickGameDirectory()).toBeNull();
    expect(await getPlatform()).toBe("web");
    expect(await getWindowState()).toEqual({ maximized: false });
    await openExternalUrl("https://example.com");
    expect(opened).toEqual(["https://example.com"]);
  });

  test("subscribes to Tauri window-state events and unlistens", async () => {
    invokeResponses = { window_state: { maximized: false } };
    setWindow({ __TAURI_INTERNALS__: {} });
    const states: boolean[] = [];

    const unlisten = onWindowStateChange((state) => states.push(state.maximized));
    await new Promise((resolve) => setTimeout(resolve, 0));
    unlisten();
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(listenCalls).toEqual([{ event: "desktop:window-state" }]);
    expect(states).toEqual([false, true]);
    expect(tauriUnlistenCalls).toBe(1);
  });
});
