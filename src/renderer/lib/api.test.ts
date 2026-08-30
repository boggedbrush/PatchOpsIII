import { beforeEach, expect, mock, test } from "bun:test";

const invoke = mock(async () => ({}));
const listen = mock(
  async (_event: string, _handler: (event: { payload: unknown }) => void) => () => undefined,
);

mock.module("@tauri-apps/api/core", () => ({ invoke }));
mock.module("@tauri-apps/api/event", () => ({ listen }));

const api = await import("./api");

beforeEach(() => {
  invoke.mockClear();
  listen.mockClear();
});

test("configuration writes use a named command and typed arguments", async () => {
  await api.setConfigValue("MaxFPS", 240);
  expect(invoke).toHaveBeenCalledWith("set_config_value", { key: "MaxFPS", value: 240 });
});

test("external actions expose an enum instead of arbitrary URLs", async () => {
  await api.openExternal("enhancedGuide");
  expect(invoke).toHaveBeenCalledWith("open_external", { target: "enhancedGuide" });
});

test("Enhanced source browse exposes both folder and ZIP pickers", async () => {
  await api.pickDumpSource();
  await api.pickDumpArchive();
  expect(invoke).toHaveBeenNthCalledWith(1, "pick_dump_source", undefined);
  expect(invoke).toHaveBeenNthCalledWith(2, "pick_dump_archive", undefined);
});

test("native events are subscribed to by their fixed names", async () => {
  const callback = () => undefined;
  await api.onLog(callback);
  expect(listen).toHaveBeenCalledTimes(1);
  expect(listen.mock.calls[0]?.[0]).toBe("patchops-log");
});

test("string command errors become Error instances", async () => {
  invoke.mockRejectedValueOnce("Game directory is not set.");
  await expect(api.launchGame()).rejects.toThrow("Game directory is not set.");
});
