import { execFileSync, spawn } from "node:child_process";
import {
  chmodSync,
  closeSync,
  copyFileSync,
  cpSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readFileSync,
  readdirSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { dlopen, FFIType } from "bun:ffi";

const app = resolve(process.argv[2]);
const appDir = dirname(app);
const run = Number(process.argv[3] ?? 1);
const backendPort = 18760 + run;
const debugPort = 19760 + run;
const fixture = process.env.PATCHOPSIII_BENCHMARK_FIXTURE ?? "/tmp/patchops-benchmark-fixture";
const runRoot = mkdtempSync(join(tmpdir(), "patchops-electron-benchmark-"));
const home = join(runRoot, "home");
const dataHome = join(runRoot, "data");
const cacheHome = join(runRoot, "cache");
const configHome = join(runRoot, "config");
const runtimeDir = join(runRoot, "runtime");
for (const directory of [home, dataHome, cacheHome, configHome]) {
  mkdirSync(directory, { recursive: true });
}
mkdirSync(runtimeDir, { recursive: true, mode: 0o700 });
chmodSync(runtimeDir, 0o700);

const appData = join(dataHome, "PatchOpsIII");
mkdirSync(appData, { recursive: true });
copyFileSync(
  join(fixture, "data/PatchOpsIII/electron-settings.json"),
  join(appData, "electron-settings.json"),
);
const steamHome = join(home, ".steam/steam");
mkdirSync(dirname(steamHome), { recursive: true });
cpSync(join(fixture, "steam"), steamHome, { recursive: true });

const { symbols: libc } = dlopen("libc.so.6", {
  posix_fadvise: {
    args: [FFIType.i32, FFIType.i64, FFIType.i64, FFIType.i32],
    returns: FFIType.i32,
  },
});

function regularFiles(root) {
  const files = [];
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const path = join(root, entry.name);
    if (entry.isDirectory()) files.push(...regularFiles(path));
    else if (entry.isFile()) files.push(path);
  }
  return files;
}

if (process.env.PATCHOPS_BENCH_PAYLOAD_COLD === "1") {
  for (const path of regularFiles(appDir)) {
    const descriptor = openSync(path, "r");
    const result = libc.posix_fadvise(descriptor, 0n, 0n, 4);
    closeSync(descriptor);
    if (result !== 0) throw new Error(`posix_fadvise failed for ${path}: ${result}`);
  }
}

const started = process.hrtime.bigint();
let peakRssKb = 0;
let peakPssKb = 0;

const child = spawn(app, [`--remote-debugging-port=${debugPort}`], {
  cwd: appDir,
  env: {
    ...process.env,
    APPDIR: appDir,
    GDK_BACKEND: "x11",
    HOME: home,
    PATCHOPSIII_BACKEND_PORT: String(backendPort),
    ELECTRON_ENABLE_LOGGING: "1",
    XDG_CACHE_HOME: cacheHome,
    XDG_CONFIG_HOME: configHome,
    XDG_DATA_HOME: dataHome,
    XDG_RUNTIME_DIR: runtimeDir,
  },
  stdio: ["ignore", "pipe", "pipe"],
});

let output = "";
child.stdout.on("data", (chunk) => (output += chunk));
child.stderr.on("data", (chunk) => (output += chunk));

const elapsedMs = () => Number(process.hrtime.bigint() - started) / 1e6;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function descendants(rootPid) {
  const found = [];
  const queue = [rootPid];
  while (queue.length) {
    const pid = queue.shift();
    if (!pid || found.includes(pid)) continue;
    found.push(pid);
    try {
      const children = readFileSync(`/proc/${pid}/task/${pid}/children`, "utf8")
        .trim()
        .split(/\s+/)
        .filter(Boolean)
        .map(Number);
      queue.push(...children);
    } catch {
      // The process exited between discovery and sampling.
    }
  }
  return found;
}

function rssKb(pids) {
  return pids.reduce((sum, pid) => {
    try {
      const match = readFileSync(`/proc/${pid}/status`, "utf8").match(/^VmRSS:\s+(\d+)\s+kB$/m);
      return sum + Number(match?.[1] ?? 0);
    } catch {
      return sum;
    }
  }, 0);
}

function pssKb(pids) {
  return pids.reduce((sum, pid) => {
    try {
      const match = readFileSync(`/proc/${pid}/smaps_rollup`, "utf8").match(/^Pss:\s+(\d+)\s+kB$/m);
      return sum + Number(match?.[1] ?? 0);
    } catch {
      return sum;
    }
  }, 0);
}

function cpuTicks(pids) {
  return pids.reduce((sum, pid) => {
    try {
      const stat = readFileSync(`/proc/${pid}/stat`, "utf8");
      const fields = stat.slice(stat.lastIndexOf(")") + 2).trim().split(/\s+/);
      return sum + Number(fields[11]) + Number(fields[12]);
    } catch {
      return sum;
    }
  }, 0);
}

const peakTimer = setInterval(() => {
  const pids = descendants(child.pid);
  peakRssKb = Math.max(peakRssKb, rssKb(pids));
  peakPssKb = Math.max(peakPssKb, pssKb(pids));
}, 50);

async function waitFor(test, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const value = await test().catch(() => null);
    if (value) return value;
    await sleep(20);
  }
  throw new Error(`timed out after ${timeoutMs}ms`);
}

try {
  const backendMs = await waitFor(async () => {
    const response = await fetch(`http://127.0.0.1:${backendPort}/api/health`);
    return response.ok ? elapsedMs() : null;
  }, 20_000);

  const target = await waitFor(async () => {
    const response = await fetch(`http://127.0.0.1:${debugPort}/json/list`);
    const targets = await response.json();
    return targets.find((item) => item.type === "page" && item.webSocketDebuggerUrl);
  }, 20_000);

  const interactiveMs = await waitFor(
    async () => (/GET \/api\/status HTTP\/1\.1" 200 OK/.test(output) ? elapsedMs() : null),
    20_000,
  );

  await sleep(2_000);
  const idlePids = descendants(child.pid);
  const idleRssKb = rssKb(idlePids);
  const idlePssKb = pssKb(idlePids);
  const ticksBefore = cpuTicks(idlePids);
  const cpuWindowMs = Number(process.env.PATCHOPS_BENCH_CPU_WINDOW_MS ?? 3_000);
  await sleep(cpuWindowMs);
  const ticksAfter = cpuTicks(descendants(child.pid));
  const clockTicks = Number(execFileSync("getconf", ["CLK_TCK"], { encoding: "utf8" }).trim());
  const idleCpuPercent = ((ticksAfter - ticksBefore) / clockTicks / (cpuWindowMs / 1_000)) * 100;

  const shutdownStarted = process.hrtime.bigint();
  const closeResponse = await fetch(`http://127.0.0.1:${debugPort}/json/close/${target.id}`);
  if (!closeResponse.ok) throw new Error(`CDP close failed: ${closeResponse.status}`);
  await Promise.race([
    new Promise((resolve) => child.once("exit", resolve)),
    sleep(10_000).then(() => {
      throw new Error("shutdown timeout");
    }),
  ]);
  const shutdownMs = Number(process.hrtime.bigint() - shutdownStarted) / 1e6;

  clearInterval(peakTimer);
  console.log(
    JSON.stringify({
      run,
      backendMs,
      interactiveMs,
      idleRssKb,
      peakRssKb,
      idlePssKb,
      peakPssKb,
      idleCpuPercent,
      processCount: idlePids.length,
      shutdownMs,
    }),
  );
} catch (error) {
  clearInterval(peakTimer);
  child.kill("SIGKILL");
  console.error(String(error));
  console.error(output.slice(-4_000));
  process.exitCode = 1;
} finally {
  rmSync(runRoot, { recursive: true, force: true });
}
