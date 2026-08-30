import {
  closeSync,
  mkdirSync,
  openSync,
  writeFileSync,
  writeSync,
} from "node:fs";
import { resolve } from "node:path";

const root = resolve(process.argv[2] ?? "/tmp/patchops-benchmark-fixture");
mkdirSync(root);
for (const directory of [
  "data/PatchOpsIII",
  "game/players",
  "game/video",
  "library/steamapps/common",
  "steam/steamapps",
]) {
  mkdirSync(resolve(root, directory), { recursive: true });
}

writeFileSync(
  resolve(root, "data/PatchOpsIII/electron-settings.json"),
  `${JSON.stringify({ game_dir: resolve(root, "game"), release_channel: "stable" }, null, 2)}\n`,
);
writeFileSync(
  resolve(root, "steam/steamapps/libraryfolders.vdf"),
  `"libraryfolders"
{
  "0"
  {
    "path" "${resolve(root, "steam")}"
  }
  "1"
  {
    "path" "${resolve(root, "library")}"
  }
}
`,
);
writeFileSync(
  resolve(root, "game/players/config.ini"),
  `// representative PatchOpsIII benchmark config
MaxFPS = "240" // Maximum frames per second
FOV = "100" // Field of view
WindowSize = "2560x1440" // Screen resolution
FullScreenMode = "1" // Display mode
RefreshRate = "144" // Refresh rate
RenderResolution = "100" // Render resolution percentage
ResolutionPercent = "100" // Render resolution percentage
Vsync = "0" // Vertical sync
DrawFPS = "1" // Show FPS
SmoothFramerate = "0" // Smooth framerate
RestrictGraphicsOptions = "0" // Unlock graphics options
SerializeRender = "0" // Reduce CPU pressure
MaxFrameLatency = "1" // Maximum frame latency
VideoMemory = "0.95" // VRAM target
StreamMinResident = "1" // VRAM limiter
`,
);
writeFileSync(
  resolve(root, "game/dxvk.conf"),
  `dxvk.enableAsync = True
dxvk.numCompilerThreads = 4
dxgi.maxFrameRate = 240
dxgi.maxFrameLatency = 1
dxvk.tearFree = False
dxvk.hud = compiler
`,
);
writeFileSync(
  resolve(root, "game/t7patch.conf"),
  "playername=Benchmark^1\nnetworkpassword=test-only\nfriends_only=1\n",
);
for (const marker of ["d3d11.dll", "dxgi.dll", "t7patch.dll", "t7patchloader.dll"]) {
  writeFileSync(resolve(root, `game/${marker}`), "benchmark\n");
}

const executable = openSync(resolve(root, "game/BlackOps3.exe"), "wx");
const zeroMiB = Buffer.alloc(1024 * 1024);
for (let index = 0; index < 256; index += 1) {
  writeSync(executable, zeroMiB);
}
closeSync(executable);

console.log(root);
