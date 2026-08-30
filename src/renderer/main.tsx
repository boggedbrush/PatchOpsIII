import React, { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  AlertTriangle,
  CheckCircle2,
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
import * as desktop from "./lib/api";
import type { LogEntry, PatchOpsState } from "./lib/api";
import packageInfo from "../../package.json";
import "./styles/app.css";

const logoUrl = new URL("../../website/assets/img/patchopsiii.png", import.meta.url).href;
const packageVersion = packageInfo.version;
const captionIconUrls = {
  close: new URL("./assets/caption-buttons/close.svg", import.meta.url).href,
  maximize: new URL("./assets/caption-buttons/maximize.svg", import.meta.url).href,
  minimize: new URL("./assets/caption-buttons/minimize.svg", import.meta.url).href,
  restore: new URL("./assets/caption-buttons/restore.svg", import.meta.url).href
};

const navItems = [
  { id: "dashboard", label: "Dashboard", icon: LayoutDashboard },
  { id: "t7", label: "T7 Patch", icon: ShieldCheck },
  { id: "exe", label: "EXE Swapper", icon: RefreshCw },
  { id: "enhanced", label: "Enhanced", icon: Gem },
  // removed for now as the developer has decided to turn this mod into a paid service
  { id: "graphics", label: "Graphics", icon: Image },
  { id: "tools", label: "Tools", icon: Wrench }
] as const;

type ViewId = (typeof navItems)[number]["id"];
type GraphicsTabId = "graphics" | "dxvk";
type DxvkSettings = PatchOpsState["dxvk"]["settings"];
type AppStatus = "starting" | "ready" | "failed";

type DepotPromptState = {
  command: string;
  copied: boolean;
  watching: boolean;
};

type EnhancedValidationState = {
  label: string;
  ok: boolean | null;
  checkedAt: string | null;
};

const defaultExeSwap: PatchOpsState["exeSwap"] = {
  profile: "",
  modeLabel: "Unknown",
  patchLabel: "",
  displayLabel: "EXE Swapper is unavailable while PatchOpsIII is starting.",
  state: "unavailable",
  activeBuildId: "Unknown",
  activeBuildDate: "",
  currentBuildId: "21201493",
  currentBuildDate: "Feb 19, 2026",
  compatibleBuildId: "10650222",
  compatibleBuildDate: "Mar 3, 2023",
  enhancedBuildId: "Enhanced",
  enhancedBuildDate: "",
  executable: "",
  executableName: "",
  executableHash: "",
  trustedExecutable: false,
  integrityStatus: "unavailable",
  integrityMessage: "EXE Swapper is unavailable while PatchOpsIII is starting.",
  backupAvailable: false,
  latestAvailable: false,
  compatibleAvailable: false,
  enhancedAvailable: false,
  compatibleActive: false,
  enhancedExeActive: false,
  enhancedActive: false
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

type CaptionGlyphStyle = React.CSSProperties & {
  "--caption-icon": string;
};

function CaptionGlyph({ iconUrl }: { iconUrl: string }) {
  return <span className="window-glyph" style={{ "--caption-icon": `url("${iconUrl}")` } as CaptionGlyphStyle} aria-hidden="true" />;
}

function TitleBar({ appVersion, updateDisabled, onCheckForUpdates }: { appVersion: string; updateDisabled: boolean; onCheckForUpdates: () => void }) {
  const [platform, setPlatform] = useState(initialDesktopPlatform);
  const [maximized, setMaximized] = useState(false);
  const isMac = platform === "darwin";
  const displayVersion = appVersion.toLowerCase().startsWith("v") ? appVersion : `v${appVersion}`;
  const usesNativeWindowControls = false;
  const showWindowControls = true;

  useEffect(() => {
    let removeWindowStateListener: (() => void) | undefined;
    void desktop.getPlatform().then(setPlatform).catch(() => undefined);
    void desktop.getWindowState().then((state) => setMaximized(state.maximized));
    void desktop.onWindowState((state) => setMaximized(state.maximized)).then((remove) => {
      removeWindowStateListener = remove;
    });
    return () => removeWindowStateListener?.();
  }, []);

  async function minimizeWindow() {
    await desktop.minimizeWindow();
  }

  async function toggleMaximize() {
    const state = await desktop.toggleMaximizeWindow();
    setMaximized(state.maximized);
  }

  function closeWindow() {
    void desktop.closeWindow();
  }

  return (
    <div className={cx("titlebar", isMac ? "titlebar-mac" : "titlebar-desktop", usesNativeWindowControls && "titlebar-native-overlay titlebar-mica")} data-tauri-drag-region>
      <div className="titlebar-grip" aria-hidden="true" data-tauri-drag-region />
      <div className="titlebar-brand">
        <img src={logoUrl} alt="" className="titlebar-logo" />
        <div className="titlebar-copy">
          <strong>PatchOpsIII</strong>
          <button type="button" className="titlebar-update" title="Check for Updates" aria-label="Check for Updates" onClick={onCheckForUpdates} disabled={updateDisabled}>
            <span className="titlebar-version">{displayVersion}</span>
            <RefreshCw className="titlebar-update-icon" size={13} strokeWidth={2.4} aria-hidden="true" />
          </button>
        </div>
      </div>
      <div className="titlebar-drag" data-tauri-drag-region />
      {showWindowControls && (
        <div className="window-controls" aria-label="Window controls">
          <button type="button" className="window-control" aria-label="Minimize window" onClick={minimizeWindow}>
            <CaptionGlyph iconUrl={captionIconUrls.minimize} />
          </button>
          <button type="button" className="window-control" aria-label={maximized ? "Restore window" : "Maximize window"} onClick={toggleMaximize}>
            <CaptionGlyph iconUrl={maximized ? captionIconUrls.restore : captionIconUrls.maximize} />
          </button>
          <button type="button" className="window-control close" aria-label="Close window" onClick={closeWindow}>
            <CaptionGlyph iconUrl={captionIconUrls.close} />
          </button>
        </div>
      )}
    </div>
  );
}

function logKey(entry: LogEntry) {
  return `${entry.line}|${entry.category}|${entry.message}`;
}

function uniqueLogs(entries: LogEntry[]) {
  const seen = new Set<string>();
  return entries.filter((entry) => {
    const key = logKey(entry);
    if (seen.has(key)) {
      return false;
    }
    seen.add(key);
    return true;
  });
}

function appendUniqueLog(current: LogEntry[], entry: LogEntry) {
  const key = logKey(entry);
  if (current.some((item) => logKey(item) === key)) {
    return current;
  }
  return [...current.slice(-179), entry];
}

function detectedCompilerThreads() {
  const logicalCores = navigator.hardwareConcurrency || 0;
  if (!logicalCores) {
    return null;
  }
  return Math.max(1, logicalCores - 2);
}

function runtimeUnavailableMessage(status: AppStatus) {
  return status === "failed" ? "PatchOpsIII took longer than expected. Restart PatchOpsIII and try again." : "PatchOpsIII is still getting ready.";
}

function normalizeState(next: PatchOpsState) {
  return {
    ...next,
    enhanced: {
      ...next.enhanced,
      filesInstalled: next.enhanced.filesInstalled ?? 0,
      backupStatus: next.enhanced.backupStatus ?? "Not created"
    },
    exeSwap: next.exeSwap ?? defaultExeSwap
  };
}

function formatTimestamp(value: string | null | undefined) {
  if (!value) {
    return "Never";
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return date.toLocaleString();
}

function StartupScreen({ status, onRetry }: { status: AppStatus; onRetry: () => void }) {
  const failed = status === "failed";
  return (
    <section className={cx("startup-screen", failed && "startup-screen-failed")} aria-live="polite">
      <div className="startup-mark" aria-hidden="true">
        <img src={logoUrl} alt="" />
      </div>
      <div className="startup-copy">
        <strong>{failed ? "Startup took too long" : "Opening PatchOpsIII"}</strong>
        <span>{failed ? "Try again, or close and reopen PatchOpsIII." : "Loading your settings..."}</span>
      </div>
      {!failed && <div className="startup-track" aria-hidden="true" />}
      {failed && (
        <button type="button" className="small-button startup-retry" onClick={onRetry}>
          <RefreshCw size={16} />
          Try Again
        </button>
      )}
    </section>
  );
}

function App() {
  const [state, setState] = useState<PatchOpsState | null>(null);
  const [appStatus, setAppStatus] = useState<AppStatus>("starting");
  const [bootAttempt, setBootAttempt] = useState(0);
  const [activeView, setActiveView] = useState<ViewId>("dashboard");
  const [activeGraphicsTab, setActiveGraphicsTab] = useState<GraphicsTabId>("graphics");
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [selectedProfile, setSelectedProfile] = useState("default");
  const [error, setError] = useState<string | null>(null);
  const [depotPrompt, setDepotPrompt] = useState<DepotPromptState | null>(null);
  const depotWatchTimer = useRef<number | null>(null);
  const [t7Gamertag, setT7Gamertag] = useState("");
  const [t7ColorCode, setT7ColorCode] = useState("");
  const [t7Password, setT7Password] = useState("");
  const [t7NetworkPasswordEnabled, setT7NetworkPasswordEnabled] = useState(false);
  const [t7PasswordTouched, setT7PasswordTouched] = useState(false);
  const [showT7Password, setShowT7Password] = useState(false);
  const [showT7PasswordEdit, setShowT7PasswordEdit] = useState(false);
  const [enhancedDumpSource, setEnhancedDumpSource] = useState("");
  const [enhancedValidation, setEnhancedValidation] = useState<EnhancedValidationState>({ label: "Not run", ok: null, checkedAt: null });
  const [dxvkSettings, setDxvkSettings] = useState<DxvkSettings>(dxvkPresets.Recommended);

  async function refresh() {
    const next = await desktop.getState();
    setState(normalizeState(next));
    setLogs(uniqueLogs(next.logs));
  }

  async function checkForUpdates() {
    await runAction("update-check", desktop.checkForUpdates);
  }

  async function setReleaseChannel(channel: PatchOpsState["releaseChannel"]) {
    await runAction("release-channel", () => desktop.setReleaseChannel(channel));
  }

  async function useCompatibleExe() {
    if (appStatus !== "ready") {
      setError(runtimeUnavailableMessage(appStatus));
      return;
    }
    setBusy("exe-compatible");
    setError(null);
    try {
      const result = await desktop.activateCompatibleExe();
      setState(normalizeState(result.state));
      setLogs(uniqueLogs(result.state.logs));
      if (result.depotCommand) {
        setDepotPrompt({ command: result.depotCommand, copied: false, watching: false });
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Compatible EXE swap failed.");
    } finally {
      setBusy(null);
    }
  }

  async function copyDepotCommand() {
    if (!depotPrompt) {
      return;
    }
    await navigator.clipboard.writeText(depotPrompt.command);
    setDepotPrompt((current) => current ? { ...current, copied: true } : current);
  }

  async function continueDepotDownload() {
    if (!depotPrompt) {
      return;
    }
    setDepotPrompt((current) => current ? { ...current, watching: true } : current);
    await desktop.openExternal("steamConsole");
  }

  async function pollCompatibleDepot() {
    try {
      const depotResult = await desktop.getCompatibleDepotStatus();
      setState(normalizeState(depotResult.state));
      setLogs(uniqueLogs(depotResult.state.logs));
      if (!depotResult.available) {
        return;
      }

      const swapResult = await desktop.activateCompatibleExe();
      setState(normalizeState(swapResult.state));
      setLogs(uniqueLogs(swapResult.state.logs));
      if (swapResult.depotCommand) {
        return;
      }
      if (depotWatchTimer.current !== null) {
        window.clearInterval(depotWatchTimer.current);
        depotWatchTimer.current = null;
      }
      setDepotPrompt(null);
      setActiveView("exe");
    } catch (err) {
      if (depotWatchTimer.current !== null) {
        window.clearInterval(depotWatchTimer.current);
        depotWatchTimer.current = null;
      }
      setDepotPrompt((current) => current ? { ...current, watching: false } : current);
      setError(err instanceof Error ? err.message : "Compatible EXE swap failed.");
    }
  }

  async function useCurrentExe() {
    await runAction("exe-current", desktop.activateCurrentExe);
  }

  async function useEnhancedExe() {
    await runAction("exe-enhanced", desktop.activateEnhancedExe);
  }

  async function runAction(id: string, action: () => Promise<PatchOpsState>) {
    if (appStatus !== "ready") {
      setError(runtimeUnavailableMessage(appStatus));
      return;
    }
    setBusy(id);
    setError(null);
    try {
      const result = await action();
      setState(normalizeState(result));
      setLogs(uniqueLogs(result.logs));
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
    await runAction(id, () => desktop.setConfigValue(item.key, normalizedValue));
  }

  async function applyProfile() {
    await runAction("launch-options", () => desktop.applyLaunchProfile(selectedProfile));
  }

  async function installSelectedProfile() {
    await runAction("workshop-install", () => desktop.installWorkshopProfile(selectedProfile));
  }

  async function toggleIntro(enabled: boolean) {
    await runAction("intro", () => desktop.setIntroSkip(enabled));
  }

  async function toggleD3dcompiler(enabled: boolean) {
    await runAction("d3dcompiler", () => desktop.setD3dcompilerWorkaround(enabled));
  }

  async function toggleAllIntros(enabled: boolean) {
    await runAction("all-intros", () => desktop.setAllIntroSkip(enabled));
  }

  async function toggleAllQol(enabled: boolean) {
    await runAction("qol-all", () => desktop.setAllQol(enabled));
  }

  async function updateT7Gamertag() {
    await runAction("t7-gamertag", () => desktop.configureT7({ gamertag: t7Gamertag, colorCode: t7ColorCode }));
  }

  function resetT7GamertagEdits() {
    if (!state?.t7) {
      return;
    }
    setT7Gamertag(state.t7.plainName);
    setT7ColorCode(state.t7.colorCode);
  }

  async function applyPreset(name: string) {
    await runAction("preset", () => desktop.applyPreset(name));
  }

  async function updateT7Password() {
    await runAction("t7-password", () => desktop.configureT7({ networkPassword: t7Password }));
    setT7PasswordTouched(false);
  }

  function resetT7SecurityEdits() {
    if (!state?.t7) {
      return;
    }
    setT7Password(state.t7.networkPassword);
    setT7NetworkPasswordEnabled(Boolean(state.t7.networkPassword));
    setT7PasswordTouched(false);
  }

  async function updateT7NetworkPasswordEnabled(enabled: boolean) {
    setT7NetworkPasswordEnabled(enabled);
    setError(null);
    if (enabled) {
      return;
    }
    setT7PasswordTouched(true);
    await runAction("t7-password-toggle", () => desktop.configureT7({ networkPassword: "" }));
  }

  async function updateT7FriendsOnly(friendsOnly: boolean) {
    await runAction("t7-friends", () => desktop.configureT7({ friendsOnly }));
  }

  async function installT7Patch() {
    await runAction("t7-install", desktop.installT7);
  }

  async function uninstallT7Patch() {
    if (!window.confirm("Uninstall T7 Patch files and restore LPC backups?")) {
      return;
    }
    await runAction("t7-uninstall", desktop.uninstallT7);
  }

  async function installEnhanced() {
    await runAction("enhanced-install", () => desktop.installEnhanced(enhancedDumpSource));
  }

  async function validateEnhancedSource() {
    if (appStatus !== "ready") {
      setError(runtimeUnavailableMessage(appStatus));
      return;
    }
    const dumpSource = enhancedDumpSource.trim();
    if (!dumpSource) {
      setEnhancedValidation({ label: "Select a source first", ok: false, checkedAt: new Date().toISOString() });
      return;
    }
    setBusy("enhanced-validate");
    setError(null);
    try {
      const result = await desktop.validateEnhancedSource(dumpSource);
      setState(normalizeState(result.state));
      setLogs(uniqueLogs(result.state.logs));
      setEnhancedValidation({
        label: result.message ?? (result.valid ? "Ready" : "Not valid"),
        ok: result.valid,
        checkedAt: new Date().toISOString()
      });
    } catch (err) {
      setEnhancedValidation({ label: "Validation failed", ok: false, checkedAt: new Date().toISOString() });
      setError(err instanceof Error ? err.message : "Validation failed.");
    } finally {
      setBusy(null);
    }
  }

  async function uninstallEnhanced() {
    if (!window.confirm("Uninstall BO3 Enhanced files and restore backups?")) {
      return;
    }
    await runAction("enhanced-uninstall", desktop.uninstallEnhanced);
  }

  async function applyDxvkSettings(nextSettings = dxvkSettings) {
    await runAction("dxvk-config", () => desktop.configureDxvk(nextSettings));
  }

  async function installDxvk() {
    await runAction("dxvk-install", () => desktop.installDxvk(dxvkSettings));
  }

  async function uninstallDxvk() {
    if (!window.confirm("Uninstall DXVK-GPLAsync and remove dxvk.conf?")) {
      return;
    }
    await runAction("dxvk-uninstall", desktop.uninstallDxvk);
  }

  async function setConfigReadonly(enabled: boolean) {
    await runAction("config-readonly", () => desktop.setConfigReadonly(enabled));
  }

  async function setVramTarget(limited: boolean, target = state?.advanced.vramTarget ?? 75) {
    await runAction("vram-target", () => desktop.setVramTarget(limited, target));
  }

  async function copyLogs() {
    if (appStatus !== "ready") {
      setError(runtimeUnavailableMessage(appStatus));
      return;
    }
    setBusy("copy-logs");
    setError(null);
    try {
      await navigator.clipboard.writeText(await desktop.getLogPayload());
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Unable to copy logs.");
    } finally {
      setBusy(null);
    }
  }

  async function clearLogs() {
    await runAction("clear-logs", desktop.clearLogs);
  }

  async function clearModFiles() {
    if (!window.confirm("Clear cached downloaded mod files?")) {
      return;
    }
    await runAction("clear-mod-files", desktop.clearModFiles);
  }

  async function resetToStock() {
    if (!window.confirm("Reset PatchOpsIII-managed files and settings to stock?")) {
      return;
    }
    await runAction("reset-stock", desktop.resetToStock);
  }

  async function openBrowseMenu(mode: "game" | "dumpFolder" | "dumpArchive" = "game") {
    if (appStatus !== "ready") {
      setError(runtimeUnavailableMessage(appStatus));
      return;
    }
    try {
      const path = mode === "dumpFolder"
        ? await desktop.pickDumpSource()
        : mode === "dumpArchive"
          ? await desktop.pickDumpArchive()
          : await desktop.pickGameDirectory();
      if (!path) return;
      if (mode !== "game") {
        setEnhancedDumpSource(path);
        setEnhancedValidation({ label: "Not run", ok: null, checkedAt: null });
      } else {
        await runAction("directory", () => desktop.setGameDirectory(path));
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Unable to select a local source.");
    }
  }

  useEffect(() => {
    let cancelled = false;
    let removeLogListener: (() => void) | undefined;
    async function boot() {
      setAppStatus("starting");
      try {
        removeLogListener = await desktop.onLog((entry) => {
          setLogs((current) => appendUniqueLog(current, entry));
        });
        if (cancelled) {
          removeLogListener();
          return;
        }
        const next = await desktop.getState();
        if (cancelled) {
          return;
        }
        setState(normalizeState(next));
        setLogs(uniqueLogs(next.logs));
        setAppStatus("ready");
      } catch (err) {
        if (!cancelled) {
          setAppStatus("failed");
          setError(err instanceof Error ? err.message : runtimeUnavailableMessage("failed"));
        }
      }
    }
    void boot();
    return () => {
      cancelled = true;
      removeLogListener?.();
    };
  }, [bootAttempt]);

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
    if (!t7PasswordTouched) {
      setT7Password(state.t7.networkPassword);
      setT7NetworkPasswordEnabled(Boolean(state.t7.networkPassword));
    }
  }, [state?.t7, t7PasswordTouched]);

  useEffect(() => {
    if (state?.enhanced.dumpSource !== undefined) {
      setEnhancedDumpSource(state.enhanced.dumpSource);
    }
  }, [state?.enhanced.dumpSource]);

  useEffect(() => {
    let removeFileDropListener: (() => void) | undefined;
    void desktop.onFileDrop((paths) => {
      const nextSource = paths[0];
      if (nextSource) {
        setEnhancedDumpSource(nextSource);
        setEnhancedValidation({ label: "Not run", ok: null, checkedAt: null });
      }
    }).then((remove) => {
      removeFileDropListener = remove;
    });
    return () => removeFileDropListener?.();
  }, []);

  useEffect(() => {
    if (state?.dxvk.settings) {
      setDxvkSettings(state.dxvk.settings);
    }
  }, [state?.dxvk.settings]);

  useEffect(() => {
    if (!depotPrompt?.watching || depotWatchTimer.current !== null) {
      return;
    }
    void pollCompatibleDepot();
    depotWatchTimer.current = window.setInterval(() => {
      void pollCompatibleDepot();
    }, 5000);
    return () => {
      if (depotWatchTimer.current !== null) {
        window.clearInterval(depotWatchTimer.current);
        depotWatchTimer.current = null;
      }
    };
  }, [depotPrompt?.watching]);

  const activeLaunchProfileLabel =
    state?.activeLaunchProfile === "custom"
      ? "Custom"
      : state?.activeLaunchProfile === "default"
        ? undefined
        : state?.launchProfiles.find((item) => item.id === state.activeLaunchProfile)?.label;
  const selectedLaunchProfile = state?.launchProfiles.find((profile) => profile.id === selectedProfile);
  const selectedProfileInstallable = Boolean(selectedLaunchProfile && selectedLaunchProfile.id !== "default" && selectedLaunchProfile.id !== "offline");
  const selectedT7Color = gamertagColors.find((color) => color.code === t7ColorCode) ?? gamertagColors[0];
  const t7PreviewName = t7Gamertag.trim() || state?.t7.plainName || "None";
  const t7PasswordControlsDisabled = !state?.t7.confExists || !t7NetworkPasswordEnabled;
  const savedT7Color = gamertagColors.find((color) => color.code === state?.t7.colorCode) ?? gamertagColors[0];
  const savedT7Name = state?.t7.plainName || "None";
  const t7GamertagPending = Boolean(state?.t7 && (t7Gamertag !== state.t7.plainName || t7ColorCode !== state.t7.colorCode));
  const t7SecurityPending = Boolean(state?.t7 && (t7NetworkPasswordEnabled !== Boolean(state.t7.networkPassword) || t7Password !== state.t7.networkPassword));
  const dxvkDetectedThreads = detectedCompilerThreads();
  const appReady = appStatus === "ready";
  const appVersion = state?.appVersion ?? packageVersion;
  const latestError = logs
    .slice()
    .reverse()
    .find((entry) => entry.category === "Error");
  const releaseChannelLabel = state?.releaseChannel === "beta" ? "Beta" : "Stable";
  const activeExeProfile = state?.exeSwap.profile ?? "";
  const activeExeTrusted = Boolean(state?.exeSwap.trustedExecutable);
  const compatibleDisabled = !state?.gameDetected || activeExeProfile === "compatible" || busy === "exe-compatible";
  const latestDisabled = !state?.gameDetected || activeExeProfile === "current" || !state.exeSwap.latestAvailable || busy === "exe-current";
  const enhancedDisabled = !state?.gameDetected || activeExeProfile === "enhanced" || !state.exeSwap.enhancedAvailable || busy === "exe-enhanced";
  const enhancedSourceLabel = enhancedDumpSource.trim() || "None";
  const enhancedValidationLabel = enhancedValidation.checkedAt
    ? `${enhancedValidation.label} (${formatTimestamp(enhancedValidation.checkedAt)})`
    : enhancedValidation.label;

  function retryStartup() {
    setError(null);
    setAppStatus("starting");
    setBootAttempt((current) => current + 1);
  }

  if (!state && appStatus !== "ready") {
    return (
      <main className="app-shell">
        <TitleBar appVersion={appVersion} updateDisabled onCheckForUpdates={checkForUpdates} />
        <StartupScreen status={appStatus} onRetry={retryStartup} />
      </main>
    );
  }

  return (
    <main className="app-shell">
      <TitleBar appVersion={appVersion} updateDisabled={!appReady || busy === "update-check"} onCheckForUpdates={checkForUpdates} />

      <div className="directory-row">
        <label>
          Game Directory:
          <input readOnly value={state?.gameDir ?? ""} placeholder="Black Ops III directory" />
        </label>
        <button
          className="tool-button browse"
          disabled={!appReady}
          onClick={() => void openBrowseMenu("game")}
        >
          <FolderOpen size={17} />
          Browse...
        </button>
        <button className="tool-button primary launch" disabled={!appReady} onClick={() => runAction("launch", desktop.launchGame)}>
          <PlaySquare size={16} />
          Launch Game
        </button>
      </div>

      {appStatus !== "ready" && (
        <div className="error-strip">
          <AlertTriangle size={16} />
          <span>{appStatus === "starting" ? "PatchOpsIII is still getting ready." : runtimeUnavailableMessage("failed")}</span>
        </div>
      )}

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
                    <button className="small-button" disabled={!selectedProfileInstallable || busy === "workshop-install"} onClick={installSelectedProfile}>
                      Install Selected Mod
                    </button>
                    <button className="small-button" onClick={() => runAction("refresh", desktop.getState)}>
                      Refresh
                    </button>
                    <button className="small-button" onClick={applyProfile}>
                      Apply
                    </button>
                  </div>
                </Panel>
              </div>
            </>
          )}

            {activeView === "t7" && state && (
              <>
                <Panel title="T7 Patch" className="t7-overview-panel">
                  <div className="t7-overview-grid">
                    <ModuleRow icon={ShieldCheck} title="T7 Patch" active={state.t7.installed} />
                    <StatusPill label="Config" value={state.t7.confExists ? "t7patch.conf found" : "Config missing"} ok={state.t7.confExists} />
                    <StatusPill label="Game Mode" value={state.t7.mode} ok={state.t7.mode !== "Unknown"} />
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
                  </div>
                </Panel>

                <div className="t7-dashboard-split">
                  <Panel title="Gamertag" className="t7-gamertag-panel">
                    <div className="t7-card-content">
                      <div className="gamertag-preview">
                        <strong style={{ color: selectedT7Color.swatch }}>{t7PreviewName}</strong>
                      </div>
                      <div className="field-grid">
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
                      <div className="t7-state-grid">
                        <div className="saved-gamertag-state">
                          <span>Currently Saved</span>
                          <div className="saved-gamertag-line">
                            <strong>{savedT7Name}</strong>
                            <i
                              aria-label={`${savedT7Color.label} saved color`}
                              className="saved-color-swatch"
                              style={{ backgroundColor: savedT7Color.swatch }}
                            />
                            <em>{savedT7Name.length}/20</em>
                          </div>
                        </div>
                        <div className="inline-state">
                          <span>
                            Status
                            <strong className={t7GamertagPending ? "warn" : "ok"}>{t7GamertagPending ? "Unsaved" : "Saved"}</strong>
                          </span>
                        </div>
                      </div>
                      <div className="button-row t7-card-actions">
                        <button className="small-button" disabled={!t7GamertagPending || busy === "t7-gamertag"} onClick={resetT7GamertagEdits}>
                          Reset Edits
                        </button>
                        <button className="small-button action-button" disabled={!state.t7.confExists || busy === "t7-gamertag"} onClick={updateT7Gamertag}>
                          <Save size={15} />
                          Update Gamertag
                        </button>
                      </div>
                    </div>
                  </Panel>

                  <Panel title="Security" className="t7-network-panel">
                    <div className="t7-card-content">
                      <div className="setting-row compact-setting">
                        <span>Friends Only Mode</span>
                        <Toggle label="Friends Only Mode" checked={state.t7.friendsOnly} disabled={!state.t7.confExists} onChange={updateT7FriendsOnly} />
                      </div>
                      <div
                        className="setting-row compact-setting clickable-setting"
                        role="button"
                        tabIndex={state.t7.confExists && busy !== "t7-password-toggle" ? 0 : -1}
                        onClick={() => {
                          if (state.t7.confExists && busy !== "t7-password-toggle") {
                            void updateT7NetworkPasswordEnabled(!t7NetworkPasswordEnabled);
                          }
                        }}
                        onKeyDown={(event) => {
                          if ((event.key === "Enter" || event.key === " ") && state.t7.confExists && busy !== "t7-password-toggle") {
                            event.preventDefault();
                            void updateT7NetworkPasswordEnabled(!t7NetworkPasswordEnabled);
                          }
                        }}
                      >
                        <span>Network Password</span>
                        <Toggle
                          label="Network Password"
                          checked={t7NetworkPasswordEnabled}
                          disabled={!state.t7.confExists || busy === "t7-password-toggle"}
                          onChange={updateT7NetworkPasswordEnabled}
                        />
                      </div>
                      <div className="field-grid">
                        <label>
                          Current Password
                          <div className="input-action">
                            <input readOnly disabled={t7PasswordControlsDisabled} type={showT7Password ? "text" : "password"} value={state.t7.networkPassword} placeholder="None" />
                            <button type="button" className="icon-action" disabled={t7PasswordControlsDisabled} onClick={() => setShowT7Password((current) => !current)} title={showT7Password ? "Hide password" : "Show password"}>
                              <KeyRound size={15} />
                            </button>
                          </div>
                        </label>
                        <label>
                          Password
                          <div className="input-action">
                            <input disabled={t7PasswordControlsDisabled} type={showT7PasswordEdit ? "text" : "password"} value={t7Password} onChange={(event) => {
                              setT7Password(event.target.value);
                              setT7PasswordTouched(true);
                            }} placeholder="Enter network password" />
                            <button type="button" className="icon-action" disabled={t7PasswordControlsDisabled} onClick={() => setShowT7PasswordEdit((current) => !current)} title={showT7PasswordEdit ? "Hide password" : "Show password"}>
                              <KeyRound size={15} />
                            </button>
                          </div>
                        </label>
                      </div>
                      <div className="t7-state-grid">
                        <div>
                          <span>Friends Only</span>
                          <strong className={state.t7.friendsOnly ? "ok" : ""}>{state.t7.friendsOnly ? "Enabled" : "Disabled"}</strong>
                        </div>
                        <div>
                          <span>Password</span>
                          <strong className={state.t7.networkPassword ? "ok" : ""}>{state.t7.networkPassword ? "Set" : "Not set"}</strong>
                        </div>
                        <div>
                          <span>Status</span>
                          <strong className={t7SecurityPending ? "warn" : "ok"}>{t7SecurityPending ? "Unsaved" : "Saved"}</strong>
                        </div>
                      </div>
                      <div className="button-row t7-card-actions">
                        <button className="small-button" disabled={!t7SecurityPending || busy === "t7-password" || busy === "t7-password-toggle"} onClick={resetT7SecurityEdits}>
                          Reset Edits
                        </button>
                        <button className="small-button action-button" disabled={!state.t7.confExists || !t7NetworkPasswordEnabled || busy === "t7-password"} onClick={updateT7Password}>
                          <Save size={15} />
                          Update Security
                        </button>
                      </div>
                    </div>
                  </Panel>
                </div>
              </>
            )}

            {activeView === "exe" && state && (
              <Panel title="EXE Swapper" className="exe-swap-panel">
                <div className="exe-swap-layout">
                  <section className="exe-swap-hero">
                    <div className="exe-swap-title">
                      <RefreshCw size={20} />
                      <div>
                        <span>Active build</span>
                        <strong>
                          {state.exeSwap.trustedExecutable && state.exeSwap.profile === "compatible"
                            ? `Compatible Build (${state.exeSwap.activeBuildId})`
                            : state.exeSwap.trustedExecutable && state.exeSwap.profile === "current"
                              ? `Latest Build (${state.exeSwap.activeBuildId})`
                              : state.exeSwap.trustedExecutable && state.exeSwap.profile === "enhanced"
                                ? `BO3 Enhanced (${state.exeSwap.activeBuildId})`
                                : "Unverified EXE"}
                        </strong>
                      </div>
                    </div>
                    <div className="exe-swap-status-grid">
                      <StatusPill label={`Latest - ${state.exeSwap.currentBuildDate}`} value={state.exeSwap.currentBuildId} ok={state.exeSwap.trustedExecutable && state.exeSwap.profile === "current"} />
                      <StatusPill label={`Compatible - ${state.exeSwap.compatibleBuildDate}`} value={state.exeSwap.compatibleBuildId} ok={state.exeSwap.compatibleActive} />
                      <StatusPill label="Enhanced" value={state.exeSwap.enhancedExeActive ? "Active" : state.exeSwap.enhancedAvailable ? "Available" : "Not found"} ok={state.exeSwap.enhancedAvailable} />
                    </div>
                  </section>

                  <section className="exe-swap-options" aria-label="EXE build comparison">
                    <div className={cx("exe-swap-option", activeExeProfile === "compatible" && activeExeTrusted && "active", compatibleDisabled && "disabled")}>
                      <header>
                        <div>
                          <h3>Compatible Build</h3>
                          <p>March 3, 2023 Steam depot</p>
                        </div>
                        <span className={cx("option-badge", activeExeProfile === "compatible" && activeExeTrusted && "active")}>{activeExeProfile === "compatible" && activeExeTrusted ? "Active" : "Best for mods"}</span>
                      </header>
                      <div className="exe-swap-pros">
                        <span className="pro"><CheckCircle2 size={15} /><strong>Vanilla experience:</strong> Supported</span>
                        <span className="pro"><CheckCircle2 size={15} /><strong>Modded experience:</strong> Best support</span>
                        <span className="warn"><AlertTriangle size={15} /><strong>Performance:</strong> Standard Steam performance</span>
                      </div>
                      <button className="tool-button primary" disabled={compatibleDisabled} onClick={useCompatibleExe}>
                        <Download size={16} />
                        Use Compatible Build
                      </button>
                    </div>

                    <div className={cx("exe-swap-option", activeExeProfile === "current" && activeExeTrusted && "active", latestDisabled && "disabled")}>
                      <header>
                        <div>
                          <h3>Latest Build</h3>
                          <p>February 19, 2026 Steam build</p>
                        </div>
                        <span className={cx("option-badge", activeExeProfile === "current" && activeExeTrusted && "active")}>{activeExeProfile === "current" && activeExeTrusted ? "Active" : "Steam default"}</span>
                      </header>
                      <div className="exe-swap-pros">
                        <span className="pro"><CheckCircle2 size={15} /><strong>Vanilla experience:</strong> Best Steam match</span>
                        <span className="warn"><AlertTriangle size={15} /><strong>Modded experience:</strong> May or may not work</span>
                        <span className="warn"><AlertTriangle size={15} /><strong>Performance:</strong> Standard Steam performance</span>
                      </div>
                      <button className="tool-button" disabled={latestDisabled} onClick={useCurrentExe}>
                        <RotateCcw size={16} />
                        Use Latest Build
                      </button>
                    </div>

                    <div className={cx("exe-swap-option", activeExeProfile === "enhanced" && activeExeTrusted && "active", enhancedDisabled && "disabled")}>
                      <header>
                        <div>
                          <h3>BO3 Enhanced</h3>
                          <p>Windows Store build modded for Steam</p>
                        </div>
                        <span className={cx("option-badge", activeExeProfile === "enhanced" && activeExeTrusted && "active")}>{activeExeProfile === "enhanced" && activeExeTrusted ? "Active" : state.exeSwap.enhancedAvailable ? "Best performance" : "Not installed"}</span>
                      </header>
                      <div className="exe-swap-pros">
                        <span className="pro"><CheckCircle2 size={15} /><strong>Vanilla experience:</strong> Supported</span>
                        <span className="con"><X size={15} /><strong>Modded experience:</strong> Most mods unlikely</span>
                        <span className="pro"><CheckCircle2 size={15} /><strong>Performance:</strong> Highest performance</span>
                      </div>
                      <button className="tool-button" disabled={enhancedDisabled} onClick={useEnhancedExe}>
                        <Gem size={16} />
                        Use BO3 Enhanced
                      </button>
                    </div>
                  </section>
                </div>
              </Panel>
            )}

            {activeView === "enhanced" && (
              <Panel title="Enhanced" className="enhanced-panel">
                {state && (
                  <div className="enhanced-layout">
                    <section className="enhanced-card enhanced-status-card">
                      <h3>
                        <Gem size={16} />
                        BO3 Enhanced Status
                      </h3>
                      <div className="enhanced-metric-grid">
                        <div className="enhanced-metric">
                          <span>Status</span>
                          <strong className={cx(state.enhanced.installed && "ok")}>{state.enhanced.installed ? "Installed" : "Not installed"}</strong>
                        </div>
                        <div className="enhanced-metric">
                          <span>Launch override</span>
                          <strong className={cx(state.enhanced.launchOptionsActive && "ok")}>{state.enhanced.launchOptionsActive ? "Active" : "Inactive"}</strong>
                        </div>
                        <div className="enhanced-metric">
                          <span>Game</span>
                          <strong className={cx(state.gameDetected && "ok")}>{state.gameDetected ? "Detected" : "Not detected"}</strong>
                        </div>
                        <div className="enhanced-metric">
                          <span>Tracking</span>
                          <strong className={cx(state.enhanced.detectedAt && "ok")}>{state.enhanced.detectedAt ? "Tracked" : "Not tracked"}</strong>
                        </div>
                      </div>
                    </section>

                    <div className="enhanced-main-grid">
                      <section className="enhanced-card enhanced-source-card">
                        <h3>
                          <Download size={16} />
                          Install Source
                        </h3>
                        <div className="enhanced-source-body">
                          <div className="enhanced-drop-zone">
                            <strong>Drop DUMP.zip or extracted folder here</strong>
                            <span>or browse to the dump source manually</span>
                          </div>
                          <div className="enhanced-source-controls">
                            <div className="enhanced-source-actions">
                              <button type="button" className="tool-button" onClick={() => void openBrowseMenu("dumpFolder")}>
                                <FolderOpen size={16} />
                                Browse Folder
                              </button>
                              <button type="button" className="tool-button" onClick={() => void openBrowseMenu("dumpArchive")}>
                                <Download size={16} />
                                Browse ZIP
                              </button>
                              <button type="button" className="tool-button" disabled={!enhancedDumpSource.trim() || busy === "enhanced-validate"} onClick={validateEnhancedSource}>
                                <CheckCircle2 size={16} />
                                Validate Source
                              </button>
                            </div>
                            <div className="enhanced-source-meta">
                              <div>
                                <span>Source</span>
                                <strong title={enhancedSourceLabel}>{enhancedSourceLabel}</strong>
                              </div>
                              <div>
                                <span>Validation</span>
                                <strong className={cx(enhancedValidation.ok === true && "ok", enhancedValidation.ok === false && "warn")}>{enhancedValidationLabel}</strong>
                              </div>
                            </div>
                            <button className="tool-button install-primary" disabled={!enhancedDumpSource.trim() || enhancedValidation.ok !== true || busy === "enhanced-install"} onClick={installEnhanced}>
                              <Download size={16} />
                              Install / Update BO3 Enhanced
                            </button>
                            {enhancedValidation.ok !== true && (
                              <p className="enhanced-action-note">Install requires a valid DUMP.zip or extracted UWP dump folder.</p>
                            )}
                          </div>
                        </div>
                      </section>

                      <section className="enhanced-card enhanced-help-card">
                        <h3>
                          <AlertTriangle size={16} />
                          Help / Requirements
                        </h3>
                        <div className="enhanced-help-group">
                          <span className={cx("requirement-line", state.gameDetected && "ok")}>
                            <CheckCircle2 size={15} />
                            Game detected
                          </span>
                          <span className={cx("requirement-line", enhancedValidation.ok === true && "ok", enhancedValidation.ok !== true && "warn")}>
                            <AlertTriangle size={15} />
                            Dump required
                          </span>
                        </div>
                        <div className="enhanced-help-summary">
                          <span><strong>Sources:</strong> DUMP.zip or extracted folder</span>
                          <span><strong>Checks:</strong> files, read/write, version</span>
                        </div>
                        <button type="button" className="tool-button enhanced-guide-button" onClick={() => void desktop.openExternal("enhancedGuide")}>
                          <ExternalLink size={16} />
                          Open Dump Guide
                        </button>
                      </section>
                    </div>

                    <section className="enhanced-card enhanced-details-card">
                      <h3>Install Details / Diagnostics</h3>
                      <div className="enhanced-detail-grid">
                        <div>
                          <span>Last install</span>
                          <strong>{formatTimestamp(state.enhanced.detectedAt)}</strong>
                        </div>
                        <div>
                          <span>Last validation</span>
                          <strong>{formatTimestamp(enhancedValidation.checkedAt)}</strong>
                        </div>
                        <div>
                          <span>Files installed</span>
                          <strong>{state.enhanced.filesInstalled}</strong>
                        </div>
                        <div>
                          <span>Backup status</span>
                          <strong>{state.enhanced.backupStatus}</strong>
                        </div>
                        <div>
                          <span>Install path</span>
                          <strong title={state.gameDir ?? ""}>{state.enhanced.installed && state.gameDir ? state.gameDir : "Not created"}</strong>
                        </div>
                        <div>
                          <span>Launch target</span>
                          <strong>{state.enhanced.launchOptionsActive ? "BO3 Enhanced" : "Stock BO3"}</strong>
                        </div>
                        <div>
                          <span>Config file</span>
                          <strong>{state.enhanced.installed ? "Configured" : "Missing"}</strong>
                        </div>
                        <div>
                          <span>Restore point</span>
                          <strong>{state.enhanced.backupStatus === "Created" ? "Available" : "Not available"}</strong>
                        </div>
                      </div>
                    </section>

                    <details className="enhanced-card enhanced-danger-card">
                      <summary>Danger Zone</summary>
                      <button className="tool-button danger" disabled={!state.enhanced.installed || busy === "enhanced-uninstall"} onClick={uninstallEnhanced}>
                        <Trash2 size={16} />
                        Uninstall BO3 Enhanced
                      </button>
                    </details>

                  </div>
                )}
              </Panel>
            )}

            {activeView === "graphics" && state && (
              <>
                <div className="subtab-bar" role="tablist" aria-label="Graphics sections">
                  <button className={cx(activeGraphicsTab === "graphics" && "active")} role="tab" aria-selected={activeGraphicsTab === "graphics"} onClick={() => setActiveGraphicsTab("graphics")}>
                    Graphics Settings
                  </button>
                  <button className={cx(activeGraphicsTab === "dxvk" && "active")} role="tab" aria-selected={activeGraphicsTab === "dxvk"} onClick={() => setActiveGraphicsTab("dxvk")}>
                    DXVK
                  </button>
                </div>

                {activeGraphicsTab === "graphics" && (
                  <Panel title="Graphics Settings" className="graphics-settings-panel">
                    <div className="graphics-settings-shell">
                      <div className="graphics-quick-grid">
                        <SelectRow label="Preset" value="" options={state.presets.map((preset) => ({ value: preset, label: preset }))} placeholder="Choose preset" disabled={state.advanced.configReadonly} onChange={applyPreset} />
                        <SelectRow label="Display Mode" value={String(state.graphics.displayMode)} options={displayModes.map((mode) => ({ value: String(mode.value), label: mode.label }))} disabled={state.advanced.configReadonly} onChange={(value) => setConfig("displayMode", Number(value))} />
                        <TextRow label="Resolution" value={state.graphics.resolution} disabled={state.advanced.configReadonly} onCommit={(value) => setConfig("resolution", value)} />
                        <NumberRow label="Refresh Rate" value={state.graphics.refreshRate} min={1} max={240} disabled={state.advanced.configReadonly} onCommit={(value) => setConfig("refreshRate", value)} />
                      </div>

                      <div className="graphics-section-grid">
                        <section className="graphics-settings-group">
                          <h3>Performance</h3>
                          <NumberRow label="Max FPS" value={state.graphics.maxFps} min={0} max={1000} disabled={state.advanced.configReadonly} onCommit={(value) => setConfig("maxFps", value)} />
                          <ToggleRow label="Vertical sync" checked={state.graphics.vsync} disabled={state.advanced.configReadonly} onChange={(value) => setConfig("vsync", value)} />
                          <ToggleRow label="FPS counter" checked={state.graphics.drawFps} disabled={state.advanced.configReadonly} onChange={(value) => setConfig("drawFps", value)} />
                        </section>

                        <section className="graphics-settings-group">
                          <h3>Quality</h3>
                          <SliderControl label="Render Resolution" value={state.graphics.renderResolution} min={50} max={200} disabled={state.advanced.configReadonly} onCommit={(value) => setConfig("renderResolution", value)} />
                          <SliderControl label="Field of View" value={state.graphics.fov} min={65} max={120} disabled={state.advanced.configReadonly} onCommit={(value) => setConfig("fov", value)} />
                        </section>
                      </div>

                      <details className="graphics-advanced-drawer">
                        <summary>
                          <span>
                            <strong>Advanced</strong>
                            <small>Config, CPU, latency, and VRAM controls</small>
                          </span>
                        </summary>
                        <div className="graphics-advanced-grid">
                          <ToggleRow label="Smooth framerate" checked={state.advanced.smoothFramerate} disabled={state.advanced.configReadonly} onChange={(value) => setConfig("smoothFramerate", value)} />
                          <ToggleRow label="Expose hidden graphics" checked={state.advanced.unlockOptions} disabled={state.advanced.configReadonly} onChange={(value) => setConfig("unlockOptions", value)} />
                          <ToggleRow label="Reduce CPU pressure" checked={state.advanced.reduceCpu} disabled={state.advanced.configReadonly} onChange={(value) => setConfig("reduceCpu", value)} />
                          <NumberRow label="Frame latency" value={state.advanced.maxFrameLatency} min={0} max={4} disabled={state.advanced.configReadonly} onCommit={(value) => setConfig("maxFrameLatency", value)} />
                          <ToggleRow label="Limit VRAM target" checked={state.advanced.vramLimited} disabled={state.advanced.configReadonly} onChange={(value) => setVramTarget(value)} />
                          <NumberRow label="VRAM target %" value={state.advanced.vramTarget} min={75} max={100} disabled={state.advanced.configReadonly || !state.advanced.vramLimited} onCommit={(value) => setVramTarget(true, value)} />
                          <ToggleRow label="Lock config.ini" checked={state.advanced.configReadonly} onChange={setConfigReadonly} />
                        </div>
                      </details>
                    </div>
                  </Panel>
                )}

                {activeGraphicsTab === "dxvk" && (
                  <Panel title="DXVK-GPLAsync" className="dxvk-panel">
                    <div className="graphics-settings-shell dxvk-settings-shell">
                      <div className="dxvk-control-bar">
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
                          <button className="small-button" disabled={!state.dxvk.installed || busy === "dxvk-config"} onClick={() => applyDxvkSettings()}>
                            <Save size={15} />
                            Apply
                          </button>
                        </div>
                      </div>

                      <div className="graphics-quick-grid dxvk-quick-grid">
                        <SelectRow
                          label="Preset"
                          value=""
                          options={Object.keys(dxvkPresets).map((preset) => ({ value: preset, label: preset }))}
                          placeholder="Choose preset"
                          onChange={(preset) => setDxvkSettings(dxvkPresets[preset])}
                        />
                        <div className="setting-row form-row number-row recommended-row">
                          <span>Compiler threads</span>
                          <input
                            type="number"
                            min={0}
                            max={64}
                            value={dxvkSettings.numCompilerThreads}
                            onChange={(event) => setDxvkSettings((current) => ({ ...current, numCompilerThreads: Number(event.target.value) }))}
                            onBlur={() => setDxvkSettings((current) => ({ ...current, numCompilerThreads: Math.max(0, Math.min(64, current.numCompilerThreads)) }))}
                          />
                          <button
                            type="button"
                            className="recommended-chip"
                            onClick={() => setDxvkSettings((current) => ({ ...current, numCompilerThreads: 0 }))}
                            title={dxvkDetectedThreads ? `Auto uses about ${dxvkDetectedThreads} compiler threads on this ${navigator.hardwareConcurrency}-thread CPU` : "Auto lets DXVK choose compiler threads"}
                          >
                            Recommended: Auto (0)
                          </button>
                          {dxvkDetectedThreads && (
                            <button
                              type="button"
                              className="recommended-chip secondary"
                              onClick={() => setDxvkSettings((current) => ({ ...current, numCompilerThreads: dxvkDetectedThreads }))}
                              title={`${navigator.hardwareConcurrency} logical CPU cores detected`}
                            >
                              Manual: {dxvkDetectedThreads}
                            </button>
                          )}
                        </div>
                      </div>

                      <div className="graphics-section-grid">
                        <section className="graphics-settings-group">
                          <h3>Shader Pipeline</h3>
                          <ToggleRow label="Async shader compilation" checked={dxvkSettings.enableAsync} onChange={(value) => setDxvkSettings((current) => ({ ...current, enableAsync: value }))} />
                          <ToggleRow label="GPL async cache" checked={dxvkSettings.gplAsyncCache} onChange={(value) => setDxvkSettings((current) => ({ ...current, gplAsyncCache: value }))} />
                          <ToggleRow label="FPS/GPU HUD" checked={dxvkSettings.hudEnabled} onChange={(value) => setDxvkSettings((current) => ({ ...current, hudEnabled: value }))} />
                        </section>

                        <section className="graphics-settings-group">
                          <h3>Frame Pacing</h3>
                          <SliderControl label="Frame rate cap" value={dxvkSettings.maxFrameRate} min={0} max={360} onCommit={(value) => setDxvkSettings((current) => ({ ...current, maxFrameRate: value }))} />
                          <NumberRow label="Frame latency" value={dxvkSettings.maxFrameLatency} min={0} max={16} onCommit={(value) => setDxvkSettings((current) => ({ ...current, maxFrameLatency: value }))} />
                          <SelectRow label="Tear Free" value={dxvkSettings.tearFree} options={["Auto", "True", "False"].map((item) => ({ value: item, label: item }))} onChange={(value) => setDxvkSettings((current) => ({ ...current, tearFree: value }))} />
                        </section>
                      </div>
                    </div>
                  </Panel>
                )}
              </>
            )}

            {activeView === "tools" && state && (
              <Panel title="Tools" className="tools-panel">
                <div className="tools-grid">
                  <ToolCard title="System" icon={Cpu}>
                    <div className="tool-metric-list">
                      <ToolMetric label="Platform" value={state.platform || "Unknown"} ok={Boolean(state.platform)} />
                      <ToolMetric label="Steam ID" value={state.steamUserId || "Not found"} ok={Boolean(state.steamUserId)} />
                      <ToolMetric label="PatchOps" value={appVersion} ok />
                    </div>
                  </ToolCard>

                  <ToolCard title="Updates" icon={RefreshCw}>
                    <div className="release-channel-control" role="group" aria-label="Release channel">
                      <div className="segmented-control">
                        {(["stable", "beta"] as const).map((channel) => (
                          <button
                            key={channel}
                            type="button"
                            className={cx(state.releaseChannel === channel && "active")}
                            disabled={busy === "release-channel"}
                            onClick={() => void setReleaseChannel(channel)}
                          >
                            {channel === "stable" ? "Stable" : "Beta"}
                          </button>
                        ))}
                      </div>
                    </div>
                    <div className="tool-metric-list updates-metrics">
                      <ToolMetric label="Current" value={releaseChannelLabel} ok />
                      <ToolMetric label="Last checked" value="On demand" />
                    </div>
                    <button type="button" className="small-button action-button" disabled={!appReady || busy === "update-check"} onClick={checkForUpdates}>
                      <RefreshCw size={15} />
                      Check for Updates
                    </button>
                  </ToolCard>

                  <ToolCard title="Mod Cache" icon={Download}>
                    <div className="tool-metric-list">
                      <ToolMetric label="Status" value={state.maintenance.modFilesDir ? "Configured" : "Missing"} ok={Boolean(state.maintenance.modFilesDir)} />
                      <ToolMetric label="Directory" value={state.maintenance.modFilesDir ? "Ready" : "Not found"} ok={Boolean(state.maintenance.modFilesDir)} />
                    </div>
                  </ToolCard>

                  <ToolCard title="Cache Actions" icon={Trash2}>
                    <div className="tool-action-grid">
                      <button className="small-button action-button" disabled={busy === "clear-mod-files"} onClick={clearModFiles}>
                        <Trash2 size={15} />
                        Clear Mod Files
                      </button>
                      <button className="small-button action-button danger" disabled={!state.gameDetected || busy === "reset-stock"} onClick={resetToStock}>
                        <RotateCcw size={15} />
                        Reset to Stock
                      </button>
                    </div>
                  </ToolCard>

                  <ToolCard title="Logs" icon={Clipboard}>
                    <div className="tool-metric-list">
                      <ToolMetric label="Log file" value={state.logPath ? "Available" : "Missing"} ok={Boolean(state.logPath)} />
                      <ToolMetric label="Last error" value={latestError ? "See Activity Log" : "None"} ok={!latestError} />
                      <ToolMetric label="Entries" value={String(logs.length)} ok={logs.length > 0} />
                    </div>
                  </ToolCard>

                  <ToolCard title="Log Actions" icon={Eraser}>
                    <div className="tool-action-grid">
                      <button className="small-button action-button" disabled={busy === "copy-logs"} onClick={copyLogs}>
                        <Clipboard size={15} />
                        Copy Logs
                      </button>
                      <button className="small-button action-button" disabled={busy === "clear-logs"} onClick={clearLogs}>
                        <Eraser size={15} />
                        Clear Logs
                      </button>
                    </div>
                  </ToolCard>
                </div>
              </Panel>
            )}
        </section>

        <Panel title="Activity Log" className="log-panel">
          <div className="terminal">
            {logs.length === 0 ? (
              <p className="log-empty">No activity yet.</p>
            ) : (
              logs.slice(-8).map((entry, index) => (
                <p key={`${entry.line}-${index}`} className={logTone(entry.category)}>
                  <span>{entry.category}</span>
                  {entry.message}
                </p>
              ))
            )}
          </div>
        </Panel>
      </section>

      {depotPrompt && (
        <div className="modal-backdrop depot-backdrop" role="presentation">
          <section className="depot-modal" role="dialog" aria-modal="true" aria-labelledby="depot-title" aria-describedby="depot-message">
            <div className="depot-mark">
              <Clipboard size={28} />
            </div>
            <div className="depot-copy">
              <h2 id="depot-title">Copy this</h2>
              <p id="depot-message">Run this command in the Steam console to download the compatible build.</p>
              <code>{depotPrompt.command}</code>
              {depotPrompt.watching && <p className="depot-watch">Watching for the depot. PatchOpsIII will install it when ready.</p>}
            </div>
            <div className="depot-actions">
              <button type="button" className="tool-button" onClick={() => setDepotPrompt(null)}>
                Cancel
              </button>
              <button type="button" className="tool-button" onClick={copyDepotCommand}>
                <Clipboard size={16} />
                {depotPrompt.copied ? "Copied" : "Copy"}
              </button>
              <button type="button" className="tool-button primary" onClick={continueDepotDownload}>
                <ExternalLink size={16} />
                Continue
              </button>
            </div>
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

function ToolCard({ title, icon: Icon, className, children }: { title: string; icon: LucideIcon; className?: string; children: React.ReactNode }) {
  return (
    <section className={cx("tool-card", className)}>
      <h3>
        <Icon size={16} />
        {title}
      </h3>
      {children}
    </section>
  );
}

function ToolMetric({ label, value, ok }: { label: string; value: string; ok?: boolean }) {
  return (
    <div className="tool-metric">
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
  disabled,
  onChange
}: {
  label: string;
  value: string;
  options: Array<{ value: string; label: string }>;
  placeholder?: string;
  disabled?: boolean;
  onChange: (value: string) => void;
}) {
  return (
    <div className="setting-row form-row">
      <span>{label}</span>
      <select value={value} disabled={disabled} onChange={(event) => event.target.value && onChange(event.target.value)}>
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

function TextRow({ label, value, disabled, onCommit }: { label: string; value: string; disabled?: boolean; onCommit: (value: string) => void }) {
  const [localValue, setLocalValue] = useState(value);
  useEffect(() => setLocalValue(value), [value]);
  return (
    <div className="setting-row form-row">
      <span>{label}</span>
      <input value={localValue} disabled={disabled} onChange={(event) => setLocalValue(event.target.value)} onBlur={() => onCommit(localValue)} onKeyDown={(event) => {
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

function SliderControl({ label, value, min, max, disabled, onCommit }: { label: string; value: number; min: number; max: number; disabled?: boolean; onCommit: (value: number) => void }) {
  const [localValue, setLocalValue] = useState(value);
  useEffect(() => setLocalValue(value), [value]);
  return (
    <div className="setting-row slider-row">
      <span>{label}</span>
      <input type="range" min={min} max={max} disabled={disabled} value={localValue} onChange={(event) => setLocalValue(Number(event.target.value))} onMouseUp={() => onCommit(localValue)} onTouchEnd={() => onCommit(localValue)} />
      <strong>{localValue}</strong>
    </div>
  );
}

const rootElement = document.getElementById("root")! as HTMLElement & { _patchOpsRoot?: ReturnType<typeof createRoot> };
const root = rootElement._patchOpsRoot ?? createRoot(rootElement);
rootElement._patchOpsRoot = root;

root.render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
