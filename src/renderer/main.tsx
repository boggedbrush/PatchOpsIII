import React, { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  AlertTriangle,
  Clipboard,
  Cpu,
  Download,
  Eraser,
  ExternalLink,
  FolderOpen,
  Gem,
  Image,
  KeyRound,
  LayoutDashboard,
  Maximize2,
  Minus,
  Minimize2,
  PlaySquare,
  RotateCcw,
  Save,
  ShieldCheck,
  RefreshCw,
  Trash2,
  UserRound,
  Wrench,
  X
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { Toggle } from "./components/Toggle";
import { apiRequest, makeSocket, type ApiResult, type LogEntry, type PatchOpsState } from "./lib/api";
import "./styles/app.css";

const logoUrl = new URL("../../website/assets/img/patchopsiii.png", import.meta.url).href;

const navItems = [
  { id: "dashboard", label: "Dashboard", icon: LayoutDashboard },
  { id: "t7", label: "T7 Patch", icon: ShieldCheck },
  { id: "enhanced", label: "Enhanced", icon: Gem },
  // removed for now as the developer has decided to turn this mod into a paid service
  { id: "graphics", label: "Graphics", icon: Image },
  { id: "tools", label: "Tools", icon: Wrench }
] as const;

type ViewId = (typeof navItems)[number]["id"];
type GraphicsTabId = "graphics" | "advanced" | "dxvk";
type DxvkSettings = PatchOpsState["dxvk"]["settings"];

type BrowseEntry = {
  name: string;
  path: string;
  hasGameExecutable: boolean;
};

type BrowseLocation = {
  label: string;
  path: string;
};

type BrowseState = {
  path: string;
  parent: string | null;
  hasGameExecutable: boolean;
  roots: BrowseLocation[];
  shortcuts: BrowseLocation[];
  entries: BrowseEntry[];
};

const configMap = {
  maxFps: { key: "MaxFPS", comment: "Maximum FPS cap" },
  fov: { key: "FOV", comment: "Field of view" },
  displayMode: { key: "FullScreenMode", comment: "0=Windowed,1=Fullscreen,2=Fullscreen Windowed" },
  resolution: { key: "WindowSize", comment: "any text" },
  refreshRate: { key: "RefreshRate", comment: "1 to 240" },
  renderResolution: { key: "ResolutionPercent", comment: "50 to 200" },
  vsync: { key: "Vsync", comment: "Vertical sync" },
  drawFps: { key: "DrawFPS", comment: "FPS counter" },
  smoothFramerate: { key: "SmoothFramerate", comment: "Frame smoothing" },
  unlockOptions: { key: "RestrictGraphicsOptions", comment: "Expose all graphics options" },
  reduceCpu: { key: "SerializeRender", comment: "Reduce CPU pressure" },
  maxFrameLatency: { key: "MaxFrameLatency", comment: "Maximum frame latency" }
} as const;

const gamertagColors = [
  { code: "", label: "Default", swatch: "#f6f6f6" },
  { code: "^1", label: "Red", swatch: "#ff453a" },
  { code: "^2", label: "Green", swatch: "#34c759" },
  { code: "^3", label: "Yellow", swatch: "#ffd60a" },
  { code: "^4", label: "Blue", swatch: "#5ac8fa" },
  { code: "^5", label: "Cyan", swatch: "#64d2ff" },
  { code: "^6", label: "Pink", swatch: "#ff2d55" },
  { code: "^7", label: "White", swatch: "#ffffff" },
  { code: "^8", label: "Mid Blue", swatch: "#0a84ff" },
  { code: "^9", label: "Cinnabar", swatch: "#ff6b35" },
  { code: "^0", label: "Black", swatch: "#151518" }
] as const;

const displayModes = [
  { value: 0, label: "Windowed" },
  { value: 1, label: "Fullscreen" },
  { value: 2, label: "Fullscreen Windowed" }
] as const;

const dxvkPresets: Record<string, DxvkSettings> = {
  Recommended: {
    enableAsync: true,
    gplAsyncCache: true,
    numCompilerThreads: 0,
    maxFrameRate: 0,
    maxFrameLatency: 1,
    tearFree: "True",
    hudEnabled: false
  },
  None: {
    enableAsync: true,
    gplAsyncCache: false,
    numCompilerThreads: 0,
    maxFrameRate: 0,
    maxFrameLatency: 0,
    tearFree: "Auto",
    hudEnabled: false
  }
};

function cx(...parts: Array<string | false | null | undefined>) {
  return parts.filter(Boolean).join(" ");
}

function logTone(category: string) {
  if (category === "Success") return "log-success";
  if (category === "Warning") return "log-warning";
  if (category === "Error") return "log-error";
  return "log-info";
}

function initialDesktopPlatform() {
  if (import.meta.env.VITE_PATCHOPSIII_TITLEBAR_PLATFORM) {
    return import.meta.env.VITE_PATCHOPSIII_TITLEBAR_PLATFORM;
  }
  if (navigator.platform.toLowerCase().includes("mac")) {
    return "darwin";
  }
  return "web";
}

function TitleBar({ appVersion, updateDisabled, onCheckForUpdates }: { appVersion: string; updateDisabled: boolean; onCheckForUpdates: () => void }) {
  const [platform, setPlatform] = useState(initialDesktopPlatform);
  const [maximized, setMaximized] = useState(false);
  const isMac = platform === "darwin";
  const hasDesktopChrome = Boolean(window.patchOpsDesktop);
  const showWindowControls = !isMac;

  useEffect(() => {
    let removeWindowStateListener: (() => void) | undefined;
    void window.patchOpsDesktop?.getPlatform().then(setPlatform).catch(() => undefined);
    void window.patchOpsDesktop?.getWindowState?.().then((state) => setMaximized(state.maximized));
    removeWindowStateListener = window.patchOpsDesktop?.onWindowStateChange?.((state) => setMaximized(state.maximized));
    return () => removeWindowStateListener?.();
  }, []);

  async function toggleMaximize() {
    const state = await window.patchOpsDesktop?.toggleMaximizeWindow?.();
    if (state) {
      setMaximized(state.maximized);
    }
  }

  return (
    <div className={cx("titlebar", isMac ? "titlebar-mac" : "titlebar-desktop")}>
      <div className="titlebar-grip" aria-hidden="true" />
      <div className="titlebar-brand">
        <img src={logoUrl} alt="" className="titlebar-logo" />
        <div className="titlebar-copy">
          <strong>PatchOpsIII</strong>
          <button type="button" className="titlebar-update" title="Check for Updates" aria-label="Check for Updates" onClick={onCheckForUpdates} disabled={updateDisabled}>
            <span className="titlebar-version">v{appVersion}</span>
            <RefreshCw className="titlebar-update-icon" size={13} strokeWidth={2.4} aria-hidden="true" />
          </button>
        </div>
      </div>
      <div className="titlebar-drag" />
      {showWindowControls && (
        <div className="window-controls" aria-label="Window controls">
          <button type="button" className="window-control" aria-label="Minimize window" onClick={() => window.patchOpsDesktop?.minimizeWindow?.()} tabIndex={hasDesktopChrome ? 0 : -1}>
            <Minus size={15} strokeWidth={2.4} />
          </button>
          <button type="button" className="window-control" aria-label={maximized ? "Restore window" : "Maximize window"} onClick={toggleMaximize} tabIndex={hasDesktopChrome ? 0 : -1}>
            {maximized ? <Minimize2 size={13} strokeWidth={2.2} /> : <Maximize2 size={13} strokeWidth={2.2} />}
          </button>
          <button type="button" className="window-control close" aria-label="Close window" onClick={() => window.patchOpsDesktop?.closeWindow?.()} tabIndex={hasDesktopChrome ? 0 : -1}>
            <X size={15} strokeWidth={2.4} />
          </button>
        </div>
      )}
    </div>
  );
}

function isVisibleLog(entry: LogEntry) {
  return entry.message !== "PatchOpsIII local API started.";
}

function logKey(entry: LogEntry) {
  return `${entry.line}|${entry.category}|${entry.message}`;
}

function uniqueVisibleLogs(entries: LogEntry[]) {
  const seen = new Set<string>();
  return entries.filter((entry) => {
    if (!isVisibleLog(entry)) {
      return false;
    }
    const key = logKey(entry);
    if (seen.has(key)) {
      return false;
    }
    seen.add(key);
    return true;
  });
}

function appendUniqueLog(current: LogEntry[], entry: LogEntry) {
  if (!isVisibleLog(entry)) {
    return current;
  }
  const key = logKey(entry);
  if (current.some((item) => logKey(item) === key)) {
    return current;
  }
  return [...current.slice(-179), entry];
}

function browseErrorMessage(error: unknown) {
  const message = error instanceof Error ? error.message : "";
  if (message.includes("404")) {
    return "Folder browser is unavailable. Restart PatchOpsIII and try again.";
  }
  if (message.includes("Failed to fetch")) {
    return "Folder browser is unavailable because the local service is not running.";
  }
  return "Unable to browse folders right now.";
}

function App() {
  const [state, setState] = useState<PatchOpsState | null>(null);
  const [activeView, setActiveView] = useState<ViewId>("dashboard");
  const [activeGraphicsTab, setActiveGraphicsTab] = useState<GraphicsTabId>("graphics");
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [selectedProfile, setSelectedProfile] = useState("default");
  const [error, setError] = useState<string | null>(null);
  const [browseOpen, setBrowseOpen] = useState(false);
  const [browseData, setBrowseData] = useState<BrowseState | null>(null);
  const [browseInput, setBrowseInput] = useState("");
  const [browseError, setBrowseError] = useState<string | null>(null);
  const [browseMode, setBrowseMode] = useState<"game" | "dump">("game");
  const [t7Gamertag, setT7Gamertag] = useState("");
  const [t7ColorCode, setT7ColorCode] = useState("");
  const [t7Password, setT7Password] = useState("");
  const [showT7Password, setShowT7Password] = useState(false);
  const [showT7PasswordEdit, setShowT7PasswordEdit] = useState(false);
  const [enhancedDumpSource, setEnhancedDumpSource] = useState("");
  const [dxvkSettings, setDxvkSettings] = useState<DxvkSettings>(dxvkPresets.Recommended);

  async function refresh() {
    const next = await apiRequest<PatchOpsState>("/api/status");
    setState(next);
    setLogs(uniqueVisibleLogs(next.logs));
  }

  async function checkForUpdates() {
    await runAction("update-check", () =>
      apiRequest<ApiResult<{ update: unknown }>>("/api/update-check", {
        method: "POST"
      })
    );
  }

  async function runAction<T>(id: string, action: () => Promise<ApiResult<T> | PatchOpsState | unknown>) {
    setBusy(id);
    setError(null);
    try {
      const result = await action();
      if (typeof result === "object" && result && "ok" in result) {
        const apiResult = result as ApiResult<T>;
        if (!apiResult.ok) {
          throw new Error(apiResult.error ?? "Action failed");
        }
        if (apiResult.state) {
          setState(apiResult.state);
          setLogs(uniqueVisibleLogs(apiResult.state.logs));
        } else {
          await refresh();
        }
      } else {
        await refresh();
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Unknown error");
    } finally {
      setBusy(null);
    }
  }

  async function setConfig(id: string, value: string | number | boolean) {
    const item = configMap[id as keyof typeof configMap];
    const normalizedValue =
      id === "vsync" || id === "drawFps" || id === "smoothFramerate"
        ? value
          ? "1"
          : "0"
        : id === "unlockOptions"
          ? value
            ? "0"
            : "1"
          : id === "reduceCpu"
            ? value
              ? "2"
              : "0"
            : value;
    await runAction(id, () =>
      apiRequest<ApiResult>("/api/config", {
        method: "POST",
        body: JSON.stringify({ key: item.key, value: normalizedValue, comment: item.comment })
      })
    );
  }

  async function applyProfile() {
    const profile = state?.launchProfiles.find((item) => item.id === selectedProfile);
    await runAction("launch-options", () =>
      apiRequest<ApiResult>("/api/launch-options", {
        method: "POST",
        body: JSON.stringify({ options: profile?.option ?? "", preserve_fs_game: false })
      })
    );
  }

  async function toggleIntro(enabled: boolean) {
    await runAction("intro", () =>
      apiRequest<ApiResult>("/api/intro-skip", {
        method: "POST",
        body: JSON.stringify({ enabled })
      })
    );
  }

  async function toggleD3dcompiler(enabled: boolean) {
    await runAction("d3dcompiler", () =>
      apiRequest<ApiResult>("/api/d3dcompiler", {
        method: "POST",
        body: JSON.stringify({ enabled })
      })
    );
  }

  async function toggleAllIntros(enabled: boolean) {
    await runAction("all-intros", () =>
      apiRequest<ApiResult>("/api/all-intros-skip", {
        method: "POST",
        body: JSON.stringify({ enabled })
      })
    );
  }

  async function toggleAllQol(enabled: boolean) {
    await runAction("qol-all", async () => {
      await apiRequest<ApiResult>("/api/d3dcompiler", {
        method: "POST",
        body: JSON.stringify({ enabled })
      });
      await apiRequest<ApiResult>("/api/intro-skip", {
        method: "POST",
        body: JSON.stringify({ enabled })
      });
      return apiRequest<ApiResult>("/api/all-intros-skip", {
        method: "POST",
        body: JSON.stringify({ enabled })
      });
    });
  }

  async function updateT7Gamertag() {
    await runAction("t7-gamertag", () =>
      apiRequest<ApiResult>("/api/t7-config", {
        method: "POST",
        body: JSON.stringify({ gamertag: t7Gamertag, colorCode: t7ColorCode })
      })
    );
  }

  async function installSelectedProfile() {
    await runAction("workshop-install", () =>
      apiRequest<ApiResult>("/api/workshop-install", {
        method: "POST",
        body: JSON.stringify({ profileId: selectedProfile })
      })
    );
  }

  async function applyPreset(name: string) {
    await runAction("preset", () =>
      apiRequest<ApiResult>("/api/presets/apply", {
        method: "POST",
        body: JSON.stringify({ name })
      })
    );
  }

  async function updateT7Password() {
    await runAction("t7-password", () =>
      apiRequest<ApiResult>("/api/t7-config", {
        method: "POST",
        body: JSON.stringify({ networkPassword: t7Password })
      })
    );
  }

  async function updateT7FriendsOnly(friendsOnly: boolean) {
    await runAction("t7-friends", () =>
      apiRequest<ApiResult>("/api/t7-config", {
        method: "POST",
        body: JSON.stringify({ friendsOnly })
      })
    );
  }

  async function installT7Patch() {
    await runAction("t7-install", () =>
      apiRequest<ApiResult>("/api/t7-install", {
        method: "POST"
      })
    );
  }

  async function uninstallT7Patch() {
    if (!window.confirm("Uninstall T7 Patch files and restore LPC backups?")) {
      return;
    }
    await runAction("t7-uninstall", () =>
      apiRequest<ApiResult>("/api/t7-uninstall", {
        method: "POST"
      })
    );
  }

  async function installEnhanced() {
    await runAction("enhanced-install", () =>
      apiRequest<ApiResult>("/api/enhanced-install", {
        method: "POST",
        body: JSON.stringify({ dumpSource: enhancedDumpSource })
      })
    );
  }

  async function uninstallEnhanced() {
    if (!window.confirm("Uninstall BO3 Enhanced files and restore backups?")) {
      return;
    }
    await runAction("enhanced-uninstall", () =>
      apiRequest<ApiResult>("/api/enhanced-uninstall", {
        method: "POST"
      })
    );
  }

  async function applyDxvkSettings(nextSettings = dxvkSettings) {
    await runAction("dxvk-config", () =>
      apiRequest<ApiResult>("/api/dxvk-config", {
        method: "POST",
        body: JSON.stringify(nextSettings)
      })
    );
  }

  async function installDxvk() {
    await runAction("dxvk-install", () =>
      apiRequest<ApiResult>("/api/dxvk-install", {
        method: "POST",
        body: JSON.stringify(dxvkSettings)
      })
    );
  }

  async function uninstallDxvk() {
    if (!window.confirm("Uninstall DXVK-GPLAsync and remove dxvk.conf?")) {
      return;
    }
    await runAction("dxvk-uninstall", () =>
      apiRequest<ApiResult>("/api/dxvk-uninstall", {
        method: "POST"
      })
    );
  }

  async function setConfigReadonly(enabled: boolean) {
    await runAction("config-readonly", () =>
      apiRequest<ApiResult>("/api/config-readonly", {
        method: "POST",
        body: JSON.stringify({ enabled })
      })
    );
  }

  async function setVramTarget(limited: boolean, target = state?.advanced.vramTarget ?? 75) {
    await runAction("vram-target", () =>
      apiRequest<ApiResult>("/api/vram-target", {
        method: "POST",
        body: JSON.stringify({ limited, target })
      })
    );
  }

  async function copyLogs() {
    setBusy("copy-logs");
    setError(null);
    try {
      const result = await apiRequest<{ ok: boolean; payload: string; error?: string }>("/api/logs/payload");
      if (!result.ok) {
        throw new Error(result.error ?? "Unable to read logs.");
      }
      await navigator.clipboard.writeText(result.payload);
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Unable to copy logs.");
    } finally {
      setBusy(null);
    }
  }

  async function clearLogs() {
    await runAction("clear-logs", () =>
      apiRequest<ApiResult>("/api/logs/clear", {
        method: "POST"
      })
    );
  }

  async function clearModFiles() {
    if (!window.confirm("Clear cached downloaded mod files?")) {
      return;
    }
    await runAction("clear-mod-files", () =>
      apiRequest<ApiResult>("/api/mod-files/clear", {
        method: "POST"
      })
    );
  }

  async function resetToStock() {
    if (!window.confirm("Reset PatchOpsIII-managed files and settings to stock?")) {
      return;
    }
    await runAction("reset-stock", () =>
      apiRequest<ApiResult>("/api/reset-stock", {
        method: "POST"
      })
    );
  }

  async function loadBrowse(path?: string) {
    setBrowseError(null);
    try {
      const query = path ? `?path=${encodeURIComponent(path)}` : "";
      const next = await apiRequest<BrowseState>(`/api/browse${query}`);
      setBrowseData(next);
      setBrowseInput(next.path);
    } catch (err) {
      setBrowseError(browseErrorMessage(err));
      setBrowseData((current) => current ?? {
        path: path ?? "",
        parent: null,
        hasGameExecutable: false,
        roots: [],
        shortcuts: [],
        entries: []
      });
      setBrowseInput(path ?? "");
    }
  }

  async function openBrowseMenu(mode: "game" | "dump" = "game") {
    setBrowseMode(mode);
    setBrowseOpen(true);
    await loadBrowse(mode === "dump" ? enhancedDumpSource || state?.gameDir || undefined : state?.gameDir ?? undefined);
  }

  async function selectBrowsePath(path: string) {
    if (browseMode === "dump") {
      setEnhancedDumpSource(path);
      setBrowseOpen(false);
      return;
    }
    await runAction("directory", () =>
      apiRequest<ApiResult>("/api/game-directory", {
        method: "POST",
        body: JSON.stringify({ path })
      })
    );
    setBrowseOpen(false);
  }

  useEffect(() => {
    void refresh().catch((err) => setError(err.message));
    let socket: WebSocket | null = null;
    void makeSocket().then((ws) => {
      socket = ws;
      ws.onmessage = (event) => {
        const data = JSON.parse(event.data);
        if (data.type === "log") {
          const entry = data.payload as LogEntry;
          setLogs((current) => appendUniqueLog(current, entry));
        }
      };
    });
    return () => socket?.close();
  }, []);

  useEffect(() => {
    if (state?.activeLaunchProfile) {
      setSelectedProfile(state.activeLaunchProfile === "custom" ? "default" : state.activeLaunchProfile);
    }
  }, [state?.activeLaunchProfile]);

  useEffect(() => {
    if (!state?.t7) {
      return;
    }
    setT7Gamertag(state.t7.plainName);
    setT7ColorCode(state.t7.colorCode);
    setT7Password(state.t7.networkPassword);
  }, [state?.t7]);

  useEffect(() => {
    if (state?.enhanced.dumpSource !== undefined) {
      setEnhancedDumpSource(state.enhanced.dumpSource);
    }
  }, [state?.enhanced.dumpSource]);

  useEffect(() => {
    if (state?.dxvk.settings) {
      setDxvkSettings(state.dxvk.settings);
    }
  }, [state?.dxvk.settings]);

  const activeLaunchProfileLabel =
    state?.activeLaunchProfile === "custom"
      ? "Custom"
      : state?.activeLaunchProfile === "default"
        ? undefined
        : state?.launchProfiles.find((item) => item.id === state.activeLaunchProfile)?.label;
  const selectedLaunchProfile = state?.launchProfiles.find((profile) => profile.id === selectedProfile);
  const selectedProfileInstallable = Boolean(selectedLaunchProfile && selectedLaunchProfile.id !== "default" && selectedLaunchProfile.id !== "offline");
  const workshopOptionsDisabled = Boolean(state?.enhanced.installed);

  return (
    <main className="app-shell">
      <TitleBar appVersion={state?.appVersion ?? "1.2.2"} updateDisabled={busy === "update-check"} onCheckForUpdates={checkForUpdates} />

      <div className="directory-row">
        <label>
          Game Directory:
          <input readOnly value={state?.gameDir ?? ""} placeholder="Black Ops III directory" />
        </label>
        <button
          className="tool-button browse"
          onClick={() => void openBrowseMenu("game")}
        >
          <FolderOpen size={17} />
          Browse...
        </button>
        <button className="tool-button primary launch" onClick={() => runAction("launch", () => apiRequest<ApiResult>("/api/launch", { method: "POST" }))}>
          <PlaySquare size={16} />
          Launch Game
        </button>
      </div>

      {error && (
        <div className="error-strip">
          <AlertTriangle size={16} />
          <span>{error}</span>
          <button type="button" className="error-dismiss" aria-label="Dismiss error" onClick={() => setError(null)}>
            <X size={15} strokeWidth={2.4} />
          </button>
        </div>
      )}

      <section className="main-grid">
        <aside className="nav-panel">
          {navItems.map((item) => (
            <NavButton key={item.id} item={item} active={activeView === item.id} onClick={() => setActiveView(item.id)} />
          ))}
        </aside>

        <section className="content-panel">
          {activeView === "dashboard" && state && (
            <>
              <Panel title="Status Overview" className="status-panel">
                <StatusRow label="T7 Patch" ok={state.mods.t7Patch} />
                <StatusRow label="DXVK-GPLAsync" ok={state.mods.dxvk} />
                <StatusRow label="BO3 Enhanced" ok={state.mods.enhanced} />
                <StatusRow
                  label="Launch Option"
                  ok={state.activeLaunchProfile !== "default"}
                  detail={activeLaunchProfileLabel}
                />
                <StatusRow label="Quality of Life" ok={state.qol.d3dcompiler || state.qol.intro || state.qol.allIntros} />
              </Panel>

              <div className="dashboard-split">
                <Panel title="Quality of Life" className="qol-panel">
                  <label className="check-row apply-all-row">
                    <input
                      type="checkbox"
                      checked={state.qol.d3dcompiler && state.qol.intro && state.qol.allIntros}
                      onChange={(event) => void toggleAllQol(event.target.checked)}
                    />
                    Apply All
                  </label>
                  <label className="check-row">
                    <input
                      type="checkbox"
                      checked={state.qol.d3dcompiler}
                      onChange={(event) => void toggleD3dcompiler(event.target.checked)}
                    />
                    Use latest d3dcompiler (d3dcompiler_46.dll)
                  </label>
                  <label className="check-row">
                    <input type="checkbox" checked={state.qol.intro} onChange={(event) => void toggleIntro(event.target.checked)} />
                    Skip Intro (BO3_Global_Logo_LogoSequence.mkv)
                  </label>
                  <label className="check-row">
                    <input
                      type="checkbox"
                      checked={state.qol.allIntros}
                      onChange={(event) => void toggleAllIntros(event.target.checked)}
                    />
                    Skip All Intros (Campaign, Zombies, etc.)
                  </label>
                </Panel>

                <Panel title="Launch Options">
                  <div className="radio-list">
                    {state.launchProfiles.map((profile) => (
                      <label key={profile.id} className="radio-row">
                        <input
                          type="radio"
                          name="launch-profile"
                          checked={selectedProfile === profile.id}
                          disabled={workshopOptionsDisabled && profile.id !== "default" && profile.id !== "offline"}
                          onChange={() => setSelectedProfile(profile.id)}
                        />
                        <span>{profile.label}</span>
                        {profile.id !== "default" && profile.id !== "offline" && (
                          <b className={cx("profile-state", profile.installed && "installed", !profile.installed && profile.subscribed && "subscribed", !profile.subscribed && "not-subscribed")}>
                            {profile.state}
                          </b>
                        )}
                      </label>
                    ))}
                  </div>
                  <div className="button-row">
                    <button className="small-button" disabled={!selectedProfileInstallable || workshopOptionsDisabled || busy === "workshop-install"} onClick={installSelectedProfile}>
                      Install Selected Mod
                    </button>
                    <button className="small-button" onClick={() => runAction("refresh", refresh)}>
                      Refresh
                    </button>
                    <button className="small-button" disabled={workshopOptionsDisabled && selectedProfile !== "default" && selectedProfile !== "offline"} onClick={applyProfile}>
                      Apply
                    </button>
                  </div>
                </Panel>
              </div>
            </>
          )}

            {activeView === "t7" && state && (
              <Panel title="T7 Patch" className="t7-panel">
                <div className="t7-layout">
                  <div className="t7-summary">
                    <ModuleRow icon={ShieldCheck} title="T7 Patch" active={state.t7.installed} />
                    <StatusPill label="Config" value={state.t7.confExists ? "t7patch.conf found" : "Config missing"} ok={state.t7.confExists} />
                    <StatusPill label="Game Mode" value={state.t7.mode} ok={state.t7.mode !== "Unknown"} />
                  </div>

                  <div className="t7-actions">
                    <button className="tool-button primary" disabled={busy === "t7-install"} onClick={installT7Patch}>
                      <Download size={16} />
                      Install / Update T7 Patch
                    </button>
                    <button className="tool-button" disabled={!state.t7.installed || busy === "t7-uninstall"} onClick={uninstallT7Patch}>
                      <Trash2 size={16} />
                      Uninstall T7 Patch
                    </button>
                  </div>

                  <div className="t7-settings-grid">
                    <section className="t7-card">
                      <h3>
                        <UserRound size={16} />
                        Gamertag
                      </h3>
                      <div className="field-grid">
                        <label>
                          Current
                          <input readOnly value={state.t7.plainName || "None"} />
                        </label>
                        <label>
                          Name
                          <input value={t7Gamertag} onChange={(event) => setT7Gamertag(event.target.value)} disabled={!state.t7.confExists} maxLength={20} />
                        </label>
                      </div>
                      <div className="color-grid" aria-label="Gamertag color">
                        {gamertagColors.map((color) => (
                          <button
                            key={color.code || "default"}
                            className={cx("color-chip", t7ColorCode === color.code && "active")}
                            disabled={!state.t7.confExists}
                            onClick={() => setT7ColorCode(color.code)}
                            title={color.label}
                          >
                            <span style={{ background: color.swatch }} />
                            {color.label}
                          </button>
                        ))}
                      </div>
                      <button className="small-button action-button" disabled={!state.t7.confExists || busy === "t7-gamertag"} onClick={updateT7Gamertag}>
                        <Save size={15} />
                        Update Gamertag
                      </button>
                    </section>

                    <section className="t7-card">
                      <h3>
                        <KeyRound size={16} />
                        Network
                      </h3>
                      <div className="field-grid">
                        <label>
                          Current Password
                          <div className="input-action">
                            <input readOnly type={showT7Password ? "text" : "password"} value={state.t7.networkPassword} placeholder="None" />
                            <button type="button" className="icon-action" onClick={() => setShowT7Password((current) => !current)} title={showT7Password ? "Hide password" : "Show password"}>
                              <KeyRound size={15} />
                            </button>
                          </div>
                        </label>
                        <label>
                          Password
                          <div className="input-action">
                            <input type={showT7PasswordEdit ? "text" : "password"} value={t7Password} onChange={(event) => setT7Password(event.target.value)} placeholder="Enter network password" />
                            <button type="button" className="icon-action" onClick={() => setShowT7PasswordEdit((current) => !current)} title={showT7PasswordEdit ? "Hide password" : "Show password"}>
                              <KeyRound size={15} />
                            </button>
                          </div>
                        </label>
                      </div>
                      <div className="setting-row compact-setting">
                        <span>Friends Only Mode</span>
                        <Toggle checked={state.t7.friendsOnly} disabled={!state.t7.confExists} onChange={updateT7FriendsOnly} />
                      </div>
                      <button className="small-button action-button" disabled={busy === "t7-password"} onClick={updateT7Password}>
                        <Save size={15} />
                        Update Password
                      </button>
                    </section>
                  </div>
                </div>
              </Panel>
            )}

            {activeView === "enhanced" && (
              <Panel title="Enhanced" className="enhanced-panel">
                {state && (
                  <div className="enhanced-layout">
                    <div className="t7-summary">
                      <ModuleRow icon={Gem} title="BO3 Enhanced" active={state.enhanced.installed} />
                      <StatusPill label="Launch Override" value={state.enhanced.launchOptionsActive ? "Active" : "Inactive"} ok={state.enhanced.launchOptionsActive} />
                      <StatusPill label="Detected" value={state.enhanced.detectedAt ? "Tracked" : "Not tracked"} ok={Boolean(state.enhanced.detectedAt)} />
                    </div>

                    <section className="enhanced-card">
                      <h3>
                        <Download size={16} />
                        Install Source
                      </h3>
                      <div className="field-grid">
                        <label>
                          Dump Source
                          <div className="input-action">
                            <input
                              value={enhancedDumpSource}
                              onChange={(event) => setEnhancedDumpSource(event.target.value)}
                              placeholder="DUMP.zip or extracted dump folder"
                            />
                            <button type="button" className="small-button" onClick={() => void openBrowseMenu("dump")}>
                              <FolderOpen size={15} />
                              Browse
                            </button>
                          </div>
                        </label>
                      </div>
                      <p className="info-note">
                        PatchOpsIII requires a manually supplied UWP game dump for BO3 Enhanced.
                        <a href="https://youtu.be/rBZZTcSJ9_s?si=41p0r_Enten3h5AQ" target="_blank" rel="noreferrer">
                          <ExternalLink size={14} />
                          Dump guide
                        </a>
                      </p>
                      <div className="t7-actions">
                        <button className="tool-button primary" disabled={!enhancedDumpSource.trim() || busy === "enhanced-install"} onClick={installEnhanced}>
                          <Download size={16} />
                          Install / Update BO3 Enhanced
                        </button>
                        <button className="tool-button" disabled={!state.enhanced.installed || busy === "enhanced-uninstall"} onClick={uninstallEnhanced}>
                          <Trash2 size={16} />
                          Uninstall BO3 Enhanced
                        </button>
                      </div>
                    </section>

                    <div className="enhanced-details-grid">
                      <StatusPill label="Game Directory" value={state.gameDetected ? "Detected" : "Missing"} ok={state.gameDetected} />
                      <StatusPill label="Dump Source" value={enhancedDumpSource.trim() ? "Selected" : "Required"} ok={Boolean(enhancedDumpSource.trim())} />
                      <StatusPill label="Workshop Mods" value={state.enhanced.installed ? "Disabled" : "Available"} ok={!state.enhanced.installed} />
                    </div>
                  </div>
                )}
              </Panel>
            )}

            {activeView === "graphics" && state && (
              <>
                <div className="subtab-bar" role="tablist" aria-label="Graphics sections">
                  <button className={cx(activeGraphicsTab === "graphics" && "active")} role="tab" aria-selected={activeGraphicsTab === "graphics"} onClick={() => setActiveGraphicsTab("graphics")}>
                    Graphics
                  </button>
                  <button className={cx(activeGraphicsTab === "advanced" && "active")} role="tab" aria-selected={activeGraphicsTab === "advanced"} onClick={() => setActiveGraphicsTab("advanced")}>
                    Advanced Graphics
                  </button>
                  <button className={cx(activeGraphicsTab === "dxvk" && "active")} role="tab" aria-selected={activeGraphicsTab === "dxvk"} onClick={() => setActiveGraphicsTab("dxvk")}>
                    DXVK
                  </button>
                </div>

                {activeGraphicsTab === "graphics" && (
                  <Panel title="Graphics">
                    <SelectRow label="Preset" value="" options={state.presets.map((preset) => ({ value: preset, label: preset }))} placeholder="Choose preset" onChange={applyPreset} />
                    <SelectRow label="Display Mode" value={String(state.graphics.displayMode)} options={displayModes.map((mode) => ({ value: String(mode.value), label: mode.label }))} onChange={(value) => setConfig("displayMode", Number(value))} />
                    <TextRow label="Resolution" value={state.graphics.resolution} onCommit={(value) => setConfig("resolution", value)} />
                    <NumberRow label="Refresh Rate" value={state.graphics.refreshRate} min={1} max={240} onCommit={(value) => setConfig("refreshRate", value)} />
                    <SliderControl label="FOV" value={state.graphics.fov} min={65} max={120} onCommit={(value) => setConfig("fov", value)} />
                    <NumberRow label="Max FPS" value={state.graphics.maxFps} min={0} max={1000} onCommit={(value) => setConfig("maxFps", value)} />
                    <NumberRow label="Render Resolution %" value={state.graphics.renderResolution} min={50} max={200} step={10} onCommit={(value) => setConfig("renderResolution", value)} />
                    <ToggleRow label="Vertical sync" checked={state.graphics.vsync} onChange={(value) => setConfig("vsync", value)} />
                    <ToggleRow label="FPS counter" checked={state.graphics.drawFps} onChange={(value) => setConfig("drawFps", value)} />
                  </Panel>
                )}

                {activeGraphicsTab === "advanced" && (
                  <Panel title="Advanced Graphics">
                    <ToggleRow label="Smooth framerate" checked={state.advanced.smoothFramerate} onChange={(value) => setConfig("smoothFramerate", value)} />
                    <ToggleRow label="Expose hidden graphics" checked={state.advanced.unlockOptions} onChange={(value) => setConfig("unlockOptions", value)} />
                    <ToggleRow label="Reduce CPU pressure" checked={state.advanced.reduceCpu} onChange={(value) => setConfig("reduceCpu", value)} />
                    <NumberRow label="Frame latency" value={state.advanced.maxFrameLatency} min={0} max={4} onCommit={(value) => setConfig("maxFrameLatency", value)} />
                    <ToggleRow label="Limit VRAM target" checked={state.advanced.vramLimited} onChange={(value) => setVramTarget(value)} />
                    <NumberRow label="VRAM target %" value={state.advanced.vramTarget} min={75} max={100} disabled={!state.advanced.vramLimited} onCommit={(value) => setVramTarget(true, value)} />
                    <ToggleRow label="Lock config.ini" checked={state.advanced.configReadonly} onChange={setConfigReadonly} />
                  </Panel>
                )}

                {activeGraphicsTab === "dxvk" && (
                  <Panel title="DXVK-GPLAsync" className="dxvk-panel">
                    <div className="dxvk-head">
                      <StatusPill label="Status" value={state.dxvk.installed ? "Installed" : "Not installed"} ok={state.dxvk.installed} />
                      <StatusPill label="dxvk.conf" value={state.dxvk.confExists ? "Configured" : "Missing"} ok={state.dxvk.confExists} />
                      <div className="button-row dxvk-actions">
                        <button className="small-button" disabled={busy === "dxvk-install"} onClick={installDxvk}>
                          <Download size={15} />
                          Install
                        </button>
                        <button className="small-button" disabled={!state.dxvk.installed || busy === "dxvk-uninstall"} onClick={uninstallDxvk}>
                          <Trash2 size={15} />
                          Uninstall
                        </button>
                        <button className="small-button" disabled={busy === "dxvk-config"} onClick={() => applyDxvkSettings()}>
                          <Save size={15} />
                          Apply
                        </button>
                      </div>
                    </div>
                    <SelectRow
                      label="Preset"
                      value=""
                      options={Object.keys(dxvkPresets).map((preset) => ({ value: preset, label: preset }))}
                      placeholder="Choose preset"
                      onChange={(preset) => setDxvkSettings(dxvkPresets[preset])}
                    />
                    <ToggleRow label="Async shader compilation" checked={dxvkSettings.enableAsync} onChange={(value) => setDxvkSettings((current) => ({ ...current, enableAsync: value }))} />
                    <ToggleRow label="GPL async cache" checked={dxvkSettings.gplAsyncCache} onChange={(value) => setDxvkSettings((current) => ({ ...current, gplAsyncCache: value }))} />
                    <ToggleRow label="FPS/GPU HUD" checked={dxvkSettings.hudEnabled} onChange={(value) => setDxvkSettings((current) => ({ ...current, hudEnabled: value }))} />
                    <NumberRow label="Compiler threads" value={dxvkSettings.numCompilerThreads} min={0} max={64} onCommit={(value) => setDxvkSettings((current) => ({ ...current, numCompilerThreads: value }))} />
                    <SliderControl label="Frame rate cap" value={dxvkSettings.maxFrameRate} min={0} max={360} onCommit={(value) => setDxvkSettings((current) => ({ ...current, maxFrameRate: value }))} />
                    <NumberRow label="Frame latency" value={dxvkSettings.maxFrameLatency} min={0} max={16} onCommit={(value) => setDxvkSettings((current) => ({ ...current, maxFrameLatency: value }))} />
                    <SelectRow label="Tear Free" value={dxvkSettings.tearFree} options={["Auto", "True", "False"].map((item) => ({ value: item, label: item }))} onChange={(value) => setDxvkSettings((current) => ({ ...current, tearFree: value }))} />
                  </Panel>
                )}
              </>
            )}

            {activeView === "tools" && state && (
              <Panel title="Tools" className="tools-panel">
                <div className="tools-grid">
                  <section className="t7-card">
                    <h3>
                      <Clipboard size={16} />
                      Logs
                    </h3>
                    <StatusPill label="Log file" value={state.logPath ? "Available" : "Missing"} ok={Boolean(state.logPath)} />
                    <div className="tool-stack">
                      <button className="small-button action-button" disabled={busy === "copy-logs"} onClick={copyLogs}>
                        <Clipboard size={15} />
                        Copy Logs
                      </button>
                      <button className="small-button action-button" disabled={busy === "clear-logs"} onClick={clearLogs}>
                        <Eraser size={15} />
                        Clear Logs
                      </button>
                    </div>
                  </section>

                  <section className="t7-card">
                    <h3>
                      <Download size={16} />
                      Mod Cache
                    </h3>
                    <StatusPill label="Directory" value={state.maintenance.modFilesDir ? "Configured" : "Missing"} ok={Boolean(state.maintenance.modFilesDir)} />
                    <button className="small-button action-button" disabled={busy === "clear-mod-files"} onClick={clearModFiles}>
                      <Trash2 size={15} />
                      Clear Mod Files
                    </button>
                  </section>

                  <section className="t7-card danger-card">
                    <h3>
                      <RotateCcw size={16} />
                      Reset
                    </h3>
                    <StatusPill label="Game Directory" value={state.gameDetected ? "Detected" : "Missing"} ok={state.gameDetected} />
                    <button className="small-button action-button danger" disabled={!state.gameDetected || busy === "reset-stock"} onClick={resetToStock}>
                      <RotateCcw size={15} />
                      Reset to Stock
                    </button>
                  </section>

                  <section className="t7-card">
                    <h3>
                      <Cpu size={16} />
                      System
                    </h3>
                    <StatusPill label="PatchOpsIII" value={`v${state.appVersion}`} ok />
                    <StatusPill label="Platform" value={state.platform || "Unknown"} ok={Boolean(state.platform)} />
                    <StatusPill label="Steam User" value={state.steamUserId || "Not found"} ok={Boolean(state.steamUserId)} />
                  </section>
                </div>
              </Panel>
            )}
        </section>

        <Panel title="Activity Log" className="log-panel">
          <div className="terminal">
            {logs.slice(-8).map((entry, index) => (
              <p key={`${entry.line}-${index}`} className={logTone(entry.category)}>
                <span>{entry.category}</span>
                {entry.message}
              </p>
            ))}
          </div>
        </Panel>
      </section>

      {browseOpen && (
        <div className="modal-backdrop" role="presentation" onMouseDown={() => setBrowseOpen(false)}>
          <section className="browse-modal" role="dialog" aria-modal="true" aria-labelledby="browse-title" onMouseDown={(event) => event.stopPropagation()}>
            <header className="browse-head">
              <div>
                <h2 id="browse-title">{browseMode === "game" ? "Select Game Directory" : "Select Dump Folder"}</h2>
                <p>{browseMode === "game" ? "Choose the folder that contains BlackOps3.exe or BlackOpsIII.exe." : "Choose an extracted dump folder, or type a DUMP.zip path above."}</p>
              </div>
              <button className="icon-action" aria-label="Close browser" onClick={() => setBrowseOpen(false)}>
                <X size={18} />
              </button>
            </header>

            <div className="browse-path-row">
              <input value={browseInput} onChange={(event) => setBrowseInput(event.target.value)} onKeyDown={(event) => {
                if (event.key === "Enter") {
                  void loadBrowse(browseInput);
                }
              }} />
              <button className="small-button" onClick={() => loadBrowse(browseInput)}>Go</button>
            </div>

            {browseError && (
              <div className="browse-error">
                <AlertTriangle size={15} />
                {browseError}
              </div>
            )}

            <div className="browse-grid">
              <aside className="browse-shortcuts">
                <h3>Locations</h3>
                {[...(browseData?.roots ?? []), ...(browseData?.shortcuts ?? [])].map((item) => (
                  <button key={`${item.label}-${item.path}`} onClick={() => loadBrowse(item.path)}>
                    {item.label}
                  </button>
                ))}
              </aside>

              <div className="browse-list">
                <div className="browse-current">
                  <span>{browseData?.path || (browseError ? "Folder browser unavailable" : "Loading...")}</span>
                  {browseData?.hasGameExecutable && <strong>BO3 detected</strong>}
                </div>
                <div className="folder-list">
                  {browseData?.parent && (
                    <button className="folder-row" onClick={() => loadBrowse(browseData.parent ?? undefined)}>
                      <FolderOpen size={17} />
                      ..
                    </button>
                  )}
                  {browseData?.entries.map((entry) => (
                    <button key={entry.path} className="folder-row" onClick={() => loadBrowse(entry.path)}>
                      <FolderOpen size={17} />
                      <span>{entry.name}</span>
                      {entry.hasGameExecutable && <strong>BO3</strong>}
                    </button>
                  ))}
                  {browseData && browseData.entries.length === 0 && <p className="empty-folder">No folders found.</p>}
                </div>
              </div>
            </div>

            <footer className="browse-actions">
              <button className="tool-button" onClick={() => setBrowseOpen(false)}>Cancel</button>
              <button className="tool-button primary" disabled={browseMode === "game" && !browseData?.hasGameExecutable} onClick={() => browseData && selectBrowsePath(browseData.path)}>
                {browseMode === "game" ? "Use Selected Folder" : "Use Dump Folder"}
              </button>
            </footer>
          </section>
        </div>
      )}
    </main>
  );
}

function NavButton({
  item,
  active,
  onClick
}: {
  item: { id: ViewId; label: string; icon: LucideIcon };
  active: boolean;
  onClick: () => void;
}) {
  const Icon = item.icon;
  return (
    <button className={cx("nav-button", active && "active")} onClick={onClick}>
      <Icon size={18} />
      <span>{item.label}</span>
    </button>
  );
}

function Panel({ title, className, children }: { title: string; className?: string; children: React.ReactNode }) {
  return (
    <section className={cx("panel", className)}>
      <h2>{title}</h2>
      <div className="panel-body">{children}</div>
    </section>
  );
}

function StatusRow({ label, ok, detail }: { label: string; ok: boolean; detail?: string }) {
  return (
    <div className="status-row">
      <span>{label}</span>
      <strong className={ok ? "ok" : ""}>
        <i />
        {ok ? (detail ?? "Configured") : "Not configured"}
      </strong>
    </div>
  );
}

function ModuleRow({ icon: Icon, title, active }: { icon: LucideIcon; title: string; active: boolean }) {
  return (
    <div className="module-row">
      <Icon size={22} />
      <span>{title}</span>
      <strong className={active ? "ok" : ""}>{active ? "Configured" : "Not configured"}</strong>
    </div>
  );
}

function StatusPill({ label, value, ok }: { label: string; value: string; ok: boolean }) {
  return (
    <div className="status-pill">
      <span>{label}</span>
      <strong className={ok ? "ok" : ""}>{value}</strong>
    </div>
  );
}

function ToggleRow({ label, checked, disabled, onChange }: { label: string; checked: boolean; disabled?: boolean; onChange: (checked: boolean) => void }) {
  return (
    <div className="setting-row">
      <span>{label}</span>
      <Toggle checked={checked} disabled={disabled} onChange={onChange} />
    </div>
  );
}

function SelectRow({
  label,
  value,
  options,
  placeholder,
  onChange
}: {
  label: string;
  value: string;
  options: Array<{ value: string; label: string }>;
  placeholder?: string;
  onChange: (value: string) => void;
}) {
  return (
    <div className="setting-row form-row">
      <span>{label}</span>
      <select value={value} onChange={(event) => event.target.value && onChange(event.target.value)}>
        {placeholder && <option value="">{placeholder}</option>}
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
    </div>
  );
}

function TextRow({ label, value, onCommit }: { label: string; value: string; onCommit: (value: string) => void }) {
  const [localValue, setLocalValue] = useState(value);
  useEffect(() => setLocalValue(value), [value]);
  return (
    <div className="setting-row form-row">
      <span>{label}</span>
      <input value={localValue} onChange={(event) => setLocalValue(event.target.value)} onBlur={() => onCommit(localValue)} onKeyDown={(event) => {
        if (event.key === "Enter") {
          onCommit(localValue);
        }
      }} />
    </div>
  );
}

function NumberRow({
  label,
  value,
  min,
  max,
  step = 1,
  disabled,
  onCommit
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  disabled?: boolean;
  onCommit: (value: number) => void;
}) {
  const [localValue, setLocalValue] = useState(value);
  useEffect(() => setLocalValue(value), [value]);
  return (
    <div className="setting-row form-row number-row">
      <span>{label}</span>
      <input
        type="number"
        min={min}
        max={max}
        step={step}
        disabled={disabled}
        value={localValue}
        onChange={(event) => setLocalValue(Number(event.target.value))}
        onBlur={() => onCommit(Math.max(min, Math.min(max, localValue)))}
      />
    </div>
  );
}

function SliderControl({ label, value, min, max, onCommit }: { label: string; value: number; min: number; max: number; onCommit: (value: number) => void }) {
  const [localValue, setLocalValue] = useState(value);
  useEffect(() => setLocalValue(value), [value]);
  return (
    <div className="setting-row slider-row">
      <span>{label}</span>
      <input type="range" min={min} max={max} value={localValue} onChange={(event) => setLocalValue(Number(event.target.value))} onMouseUp={() => onCommit(localValue)} onTouchEnd={() => onCommit(localValue)} />
      <strong>{localValue}</strong>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
