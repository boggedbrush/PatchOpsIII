import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type LogEntry = {
  category: "Info" | "Success" | "Warning" | "Error" | string;
  message: string;
  line: string;
};

export type DxvkSettings = {
  enableAsync: boolean;
  gplAsyncCache: boolean;
  numCompilerThreads: number;
  maxFrameRate: number;
  maxFrameLatency: number;
  tearFree: string;
  hudEnabled: boolean;
};

export type PatchOpsState = {
  appVersion: string;
  platform: string;
  gameDir: string | null;
  gameDetected: boolean;
  configExists: boolean;
  steamUserId: string | null;
  logPath: string;
  presets: string[];
  currentLaunchOptions: string | null;
  activeLaunchProfile: string;
  releaseChannel: "stable" | "beta";
  launchProfiles: Array<{ id: string; label: string; option: string; active: boolean; installed: boolean; subscribed: boolean; state: string; path?: string | null }>;
  enhanced: { installed: boolean; detectedAt: string | null; acknowledgedAt: string | null; launchOptionsActive: boolean; dumpSource: string; filesInstalled: number; backupStatus: string };
  exeSwap: {
    profile: string; modeLabel: string; patchLabel: string; displayLabel: string; state: string;
    activeBuildId: string; activeBuildDate: string; currentBuildId: string; currentBuildDate: string;
    compatibleBuildId: string; compatibleBuildDate: string; enhancedBuildId: string; enhancedBuildDate: string;
    executable: string; executableName: string; executableHash: string; trustedExecutable: boolean;
    integrityStatus: string; integrityMessage: string; backupAvailable: boolean; latestAvailable: boolean;
    compatibleAvailable: boolean; enhancedAvailable: boolean; compatibleActive: boolean;
    enhancedExeActive: boolean; enhancedActive: boolean;
  };
  t7: { installed: boolean; confExists: boolean; gamertag: string; plainName: string; colorCode: string; networkPassword: string; friendsOnly: boolean; mode: string };
  dxvk: { installed: boolean; confExists: boolean; settings: DxvkSettings };
  qol: { d3dcompiler: boolean; intro: boolean; allIntros: boolean };
  graphics: { maxFps: number; fov: number; displayMode: number; resolution: string; refreshRate: number; renderResolution: number; vsync: boolean; drawFps: boolean };
  advanced: { smoothFramerate: boolean; unlockOptions: boolean; reduceCpu: boolean; maxFrameLatency: number; vramLimited: boolean; vramTarget: number; configReadonly: boolean };
  maintenance: { modFilesDir: string };
  mods: { t7Patch: boolean; dxvk: boolean; enhanced: boolean };
  logs: LogEntry[];
};

export type CompatibleExeResult = { state: PatchOpsState; depotCommand: string | null };
export type DepotStatus = { available: boolean };
export type EnhancedValidation = { valid: boolean; message: string; state: PatchOpsState };
export type WindowState = { maximized: boolean };
export type ExternalTarget = "steamConsole" | "enhancedGuide";

async function command<T>(name: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(name, args);
  } catch (error) {
    if (error instanceof Error) throw error;
    throw new Error(typeof error === "string" ? error : JSON.stringify(error));
  }
}

export const getState = () => command<PatchOpsState>("get_state");
export const checkForUpdates = () => command<PatchOpsState>("check_for_updates");
export const setReleaseChannel = async (channel: PatchOpsState["releaseChannel"]) => ({
  releaseChannel: await command<PatchOpsState["releaseChannel"]>("set_release_channel", { channel }),
});
export const activateCompatibleExe = () => command<CompatibleExeResult>("activate_compatible_exe");
export const getCompatibleDepotStatus = () => command<DepotStatus>("get_compatible_depot_status");
export const activateCurrentExe = () => command<PatchOpsState>("activate_current_exe");
export const activateEnhancedExe = () => command<PatchOpsState>("activate_enhanced_exe");
export const setConfigValue = (key: string, value: string | number | boolean) => command<PatchOpsState>("set_config_value", { key, value });
export const applyLaunchProfile = (profileId: string) => command<PatchOpsState>("apply_launch_profile", { profileId });
export const installWorkshopProfile = (profileId: string) => command<PatchOpsState>("install_workshop_profile", { profileId });
export const setIntroSkip = (enabled: boolean) => command<PatchOpsState>("set_intro_skip", { enabled });
export const setD3dcompilerWorkaround = (enabled: boolean) => command<PatchOpsState>("set_d3dcompiler_workaround", { enabled });
export const setAllIntroSkip = (enabled: boolean) => command<PatchOpsState>("set_all_intro_skip", { enabled });
export const setAllQol = (enabled: boolean) => command<PatchOpsState>("set_all_qol", { enabled });
export const configureT7 = (values: { gamertag?: string; colorCode?: string; networkPassword?: string; friendsOnly?: boolean }) => command<PatchOpsState>("configure_t7", values);
export const applyPreset = (name: string) => command<PatchOpsState>("apply_preset", { name });
export const installT7 = () => command<PatchOpsState>("install_t7");
export const uninstallT7 = () => command<PatchOpsState>("uninstall_t7");
export const validateEnhancedSource = (dumpSource: string) => command<EnhancedValidation>("validate_enhanced_source", { dumpSource });
export const installEnhanced = (dumpSource: string) => command<PatchOpsState>("install_enhanced", { dumpSource });
export const uninstallEnhanced = () => command<PatchOpsState>("uninstall_enhanced");
export const configureDxvk = (settings: DxvkSettings) => command<PatchOpsState>("configure_dxvk", { settings });
export const installDxvk = (settings: DxvkSettings) => command<PatchOpsState>("install_dxvk", { settings });
export const uninstallDxvk = () => command<PatchOpsState>("uninstall_dxvk");
export const setConfigReadonly = (enabled: boolean) => command<PatchOpsState>("set_config_readonly", { enabled });
export const setVramTarget = (limited: boolean, target: number) => command<PatchOpsState>("set_vram_target", { limited, target });
export const getLogPayload = () => command<string>("get_log_payload");
export const clearLogs = () => command<PatchOpsState>("clear_logs");
export const clearModFiles = () => command<PatchOpsState>("clear_mod_files");
export const resetToStock = () => command<PatchOpsState>("reset_to_stock");
export const setGameDirectory = (path: string) => command<PatchOpsState>("set_game_directory", { path });
export const launchGame = () => command<PatchOpsState>("launch_game");

export const getPlatform = () => command<string>("get_platform");
export const getWindowState = () => command<WindowState>("get_window_state");
export const minimizeWindow = () => command<void>("minimize_window");
export const toggleMaximizeWindow = () => command<WindowState>("toggle_maximize_window");
export const closeWindow = () => command<void>("close_window");
export const pickGameDirectory = () => command<string | null>("pick_game_directory");
export const pickDumpSource = () => command<string | null>("pick_dump_source");
export const pickDumpArchive = () => command<string | null>("pick_dump_archive");
export const openExternal = (target: ExternalTarget) => command<void>("open_external", { target });
export const onLog = (callback: (entry: LogEntry) => void): Promise<UnlistenFn> => listen<LogEntry>("patchops-log", (event) => callback(event.payload));
export const onWindowState = (callback: (state: WindowState) => void): Promise<UnlistenFn> => listen<WindowState>("patchops-window-state", (event) => callback(event.payload));
export const onFileDrop = (callback: (paths: string[]) => void): Promise<UnlistenFn> => listen<string[]>("patchops-file-drop", (event) => callback(event.payload));
