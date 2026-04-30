import React, { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  AlertTriangle,
  BadgeCheck,
  Cog,
  Diamond,
  FolderOpen,
  Gem,
  Image,
  LayoutDashboard,
  PlaySquare,
  RefreshCw,
  Shield,
  X
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { Toggle } from "./components/Toggle";
import { apiRequest, makeSocket, type ApiResult, type LogEntry, type PatchOpsState } from "./lib/api";
import "./styles/app.css";

const logoUrl = new URL("../../website/assets/img/patchopsiii.png", import.meta.url).href;

const navItems = [
  { id: "dashboard", label: "Dashboard", icon: LayoutDashboard },
  { id: "t7", label: "T7 Patch", icon: BadgeCheck },
  { id: "enhanced", label: "Enhanced", icon: Gem },
  { id: "reforged", label: "Reforged", icon: Diamond },
  { id: "graphics", label: "Graphics", icon: Image },
  { id: "advanced", label: "Advanced", icon: Cog }
] as const;

type ViewId = (typeof navItems)[number]["id"];

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
  vsync: { key: "Vsync", comment: "Vertical sync" },
  drawFps: { key: "DrawFPS", comment: "FPS counter" },
  smoothFramerate: { key: "SmoothFramerate", comment: "Frame smoothing" },
  unlockOptions: { key: "RestrictGraphicsOptions", comment: "Expose all graphics options" },
  reduceCpu: { key: "SerializeRender", comment: "Reduce CPU pressure" },
  maxFrameLatency: { key: "MaxFrameLatency", comment: "Maximum frame latency" }
} as const;

function cx(...parts: Array<string | false | null | undefined>) {
  return parts.filter(Boolean).join(" ");
}

function logTone(category: string) {
  if (category === "Success") return "log-success";
  if (category === "Warning") return "log-warning";
  if (category === "Error") return "log-error";
  return "log-info";
}

function isVisibleLog(entry: LogEntry) {
  return entry.message !== "PatchOpsIII local API started.";
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
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [selectedProfile, setSelectedProfile] = useState("default");
  const [error, setError] = useState<string | null>(null);
  const [browseOpen, setBrowseOpen] = useState(false);
  const [browseData, setBrowseData] = useState<BrowseState | null>(null);
  const [browseInput, setBrowseInput] = useState("");
  const [browseError, setBrowseError] = useState<string | null>(null);

  async function refresh() {
    const next = await apiRequest<PatchOpsState>("/api/status");
    setState(next);
    setLogs(next.logs.filter(isVisibleLog));
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
          setLogs(apiResult.state.logs.filter(isVisibleLog));
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

  async function openBrowseMenu() {
    setBrowseOpen(true);
    await loadBrowse(state?.gameDir ?? undefined);
  }

  async function selectBrowsePath(path: string) {
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
          if (isVisibleLog(entry)) {
            setLogs((current) => [...current.slice(-179), entry]);
          }
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

  const activeProfileLabel = state?.launchProfiles.find((item) => item.id === selectedProfile)?.label ?? "Default (None)";

  return (
    <main className="app-shell">
      <header className="app-top">
        <div className="app-title">
          <img src={logoUrl} alt="" className="app-logo" />
          <div className="title-text">
            <strong>PatchOpsIII</strong>
            <span>v{state?.appVersion ?? "1.2.2"}</span>
          </div>
        </div>
        <div className="top-actions">
          <button className="tool-button" onClick={() => runAction("refresh", refresh)} disabled={busy === "refresh"}>
            <RefreshCw size={17} />
            Check for Updates
          </button>
          <button className="tool-button primary" onClick={() => runAction("launch", () => apiRequest<ApiResult>("/api/launch", { method: "POST" }))}>
            <PlaySquare size={16} />
            Launch Game
          </button>
        </div>
      </header>

      <div className="directory-row">
        <label>
          Game Directory:
          <input readOnly value={state?.gameDir ?? ""} placeholder="Black Ops III directory" />
        </label>
        <button
          className="tool-button browse"
          onClick={() => void openBrowseMenu()}
        >
          <FolderOpen size={17} />
          Browse...
        </button>
      </div>

      {error && (
        <div className="error-strip">
          <AlertTriangle size={16} />
          {error}
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
                <StatusRow label="BO3 Reforged" ok={state.mods.reforged} />
                <StatusRow
                  label="Launch Option"
                  ok={state.activeLaunchProfile !== "default"}
                  detail={state.activeLaunchProfile === "custom" ? "Custom" : state.activeLaunchProfile === "default" ? undefined : activeProfileLabel}
                />
                <StatusRow label="Quality of Life" ok={state.qol.d3dcompiler || state.qol.intro || state.qol.allIntros} />
              </Panel>

              <div className="dashboard-split">
                <Panel title="Quality of Life">
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
                        <input type="radio" name="launch-profile" checked={selectedProfile === profile.id} onChange={() => setSelectedProfile(profile.id)} />
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
                    <button className="small-button">Install Selected Mod</button>
                    <button className="small-button" onClick={() => runAction("refresh", refresh)}>
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
              <Panel title="T7 Patch">
                <ModuleRow icon={Shield} title="T7 Patch" active={state.mods.t7Patch} />
              </Panel>
            )}

            {activeView === "enhanced" && (
              <Panel title="Enhanced">
                <ModuleRow icon={Gem} title="BO3 Enhanced" active={Boolean(state?.mods.enhanced)} />
              </Panel>
            )}

            {activeView === "reforged" && state && (
              <Panel title="Reforged">
                <ModuleRow icon={Diamond} title="BO3 Reforged" active={state.mods.reforged} />
              </Panel>
            )}

            {activeView === "graphics" && state && (
              <Panel title="Graphics">
                <SliderControl label="FOV" value={state.graphics.fov} min={60} max={120} onCommit={(value) => setConfig("fov", value)} />
                <SliderControl label="Max FPS" value={state.graphics.maxFps} min={30} max={1000} onCommit={(value) => setConfig("maxFps", value)} />
                <ToggleRow label="Vertical sync" checked={state.graphics.vsync} onChange={(value) => setConfig("vsync", value)} />
                <ToggleRow label="FPS counter" checked={state.graphics.drawFps} onChange={(value) => setConfig("drawFps", value)} />
              </Panel>
            )}

            {activeView === "advanced" && state && (
              <Panel title="Advanced">
                <ToggleRow label="Smooth framerate" checked={state.advanced.smoothFramerate} onChange={(value) => setConfig("smoothFramerate", value)} />
                <ToggleRow label="Expose hidden graphics" checked={state.advanced.unlockOptions} onChange={(value) => setConfig("unlockOptions", value)} />
                <ToggleRow label="Reduce CPU pressure" checked={state.advanced.reduceCpu} onChange={(value) => setConfig("reduceCpu", value)} />
                <SliderControl label="Frame latency" value={state.advanced.maxFrameLatency} min={1} max={4} onCommit={(value) => setConfig("maxFrameLatency", value)} />
              </Panel>
            )}
        </section>
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

      {browseOpen && (
        <div className="modal-backdrop" role="presentation" onMouseDown={() => setBrowseOpen(false)}>
          <section className="browse-modal" role="dialog" aria-modal="true" aria-labelledby="browse-title" onMouseDown={(event) => event.stopPropagation()}>
            <header className="browse-head">
              <div>
                <h2 id="browse-title">Select Game Directory</h2>
                <p>Choose the folder that contains BlackOps3.exe or BlackOpsIII.exe.</p>
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
              <button className="tool-button primary" disabled={!browseData?.hasGameExecutable} onClick={() => browseData && selectBrowsePath(browseData.path)}>
                Use Selected Folder
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

function ToggleRow({ label, checked, onChange }: { label: string; checked: boolean; onChange: (checked: boolean) => void }) {
  return (
    <div className="setting-row">
      <span>{label}</span>
      <Toggle checked={checked} onChange={onChange} />
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
