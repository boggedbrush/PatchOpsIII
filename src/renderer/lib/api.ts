export type LogEntry = {
  category: "Info" | "Success" | "Warning" | "Error" | string;
  message: string;
  line: string;
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
  launchProfiles: Array<{
    id: string;
    label: string;
    option: string;
    active: boolean;
    installed: boolean;
    subscribed: boolean;
    state: string;
    path?: string | null;
  }>;
  enhanced: {
    installed: boolean;
    detectedAt: string | null;
    acknowledgedAt: string | null;
    launchOptionsActive: boolean;
    dumpSource: string;
  };
  t7: {
    installed: boolean;
    confExists: boolean;
    gamertag: string;
    plainName: string;
    colorCode: string;
    networkPassword: string;
    friendsOnly: boolean;
    mode: string;
    reforgedActive: boolean;
    reforged: {
      networkPass: string;
      forceRanked: boolean;
      steamAchievements: boolean;
    };
  };
  dxvk: {
    installed: boolean;
    confExists: boolean;
    settings: {
      enableAsync: boolean;
      gplAsyncCache: boolean;
      numCompilerThreads: number;
      maxFrameRate: number;
      maxFrameLatency: number;
      tearFree: string;
      hudEnabled: boolean;
    };
  };
  qol: {
    d3dcompiler: boolean;
    intro: boolean;
    allIntros: boolean;
  };
  graphics: {
    maxFps: number;
    fov: number;
    displayMode: number;
    resolution: string;
    refreshRate: number;
    renderResolution: number;
    vsync: boolean;
    drawFps: boolean;
  };
  advanced: {
    smoothFramerate: boolean;
    unlockOptions: boolean;
    reduceCpu: boolean;
    maxFrameLatency: number;
    vramLimited: boolean;
    vramTarget: number;
    configReadonly: boolean;
  };
  maintenance: {
    modFilesDir: string;
    logPayload: string;
  };
  mods: {
    t7Patch: boolean;
    dxvk: boolean;
    enhanced: boolean;
    reforged: boolean;
  };
  logs: LogEntry[];
};

export type ApiResult<T = unknown> = {
  ok: boolean;
  error?: string;
  state?: PatchOpsState;
} & T;

export async function resolveBackendUrl() {
  if (window.patchOpsDesktop) {
    return window.patchOpsDesktop.getBackendUrl();
  }
  return import.meta.env.VITE_PATCHOPSIII_BACKEND_URL ?? "http://127.0.0.1:8765";
}

export async function apiRequest<T>(path: string, init?: RequestInit): Promise<T> {
  const backendUrl = await resolveBackendUrl();
  const response = await fetch(`${backendUrl}${path}`, {
    ...init,
    headers: {
      "Content-Type": "application/json",
      ...init?.headers
    }
  });
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}`);
  }
  return response.json() as Promise<T>;
}

export async function makeSocket() {
  const backendUrl = await resolveBackendUrl();
  const wsUrl = backendUrl.replace(/^http/, "ws");
  return new WebSocket(`${wsUrl}/ws`);
}
