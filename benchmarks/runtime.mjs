import { dlopen, FFIType } from "bun:ffi";
import {
  chmodSync,
  cpSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  closeSync,
  readFileSync,
  readdirSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";

const sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));
const appRun = process.argv[2] ? resolve(process.argv[2]) : undefined;
const closeHelper = process.argv[3] ? resolve(process.argv[3]) : undefined;
const fixture = process.env.PATCHOPSIII_BENCHMARK_FIXTURE ?? "/tmp/patchops-benchmark-fixture";
const warmRuns = Number(process.env.PATCHOPSIII_BENCHMARK_WARM_RUNS ?? 5);
const coldRuns = Number(process.env.PATCHOPSIII_BENCHMARK_COLD_RUNS ?? 3);
const cpuRuns = Number(process.env.PATCHOPSIII_BENCHMARK_CPU_RUNS ?? 3);
const idleCpuSeconds = Number(process.env.PATCHOPSIII_BENCHMARK_CPU_SECONDS ?? 30);

if (!appRun || !closeHelper || !existsSync(appRun) || !existsSync(closeHelper)) {
  throw new Error("usage: bun benchmarks/runtime.mjs <AppRun> <close-window-helper>");
}
const appDir = dirname(appRun);

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

function dropPayloadCache(root) {
  for (const path of regularFiles(root)) {
    const descriptor = openSync(path, "r");
    const result = libc.posix_fadvise(descriptor, 0n, 0n, 4);
    closeSync(descriptor);
    if (result !== 0) throw new Error(`posix_fadvise failed for ${path}: ${result}`);
  }
}

function procText(pid, name) {
  try {
    return readFileSync(`/proc/${pid}/${name}`, "utf8");
  } catch {
    return "";
  }
}

function processTree(rootPid) {
  const found = [];
  const pending = [rootPid];
  const seen = new Set();
  while (pending.length > 0) {
    const pid = pending.pop();
    if (!pid || seen.has(pid) || !existsSync(`/proc/${pid}`)) continue;
    seen.add(pid);
    found.push(pid);
    const children = procText(pid, `task/${pid}/children`).trim();
    if (children) pending.push(...children.split(/\s+/).map(Number));
  }
  return found;
}

function processSample(rootPid) {
  const pids = processTree(rootPid);
  let pssKiB = 0;
  let rssKiB = 0;
  let cpuTicks = 0;
  for (const pid of pids) {
    const smaps = procText(pid, "smaps_rollup");
    pssKiB += Number(smaps.match(/^Pss:\s+(\d+)/m)?.[1] ?? 0);
    const status = procText(pid, "status");
    rssKiB += Number(status.match(/^VmRSS:\s+(\d+)/m)?.[1] ?? 0);
    const stat = procText(pid, "stat");
    const tail = stat.slice(stat.lastIndexOf(")") + 2).trim().split(/\s+/);
    cpuTicks += Number(tail[11] ?? 0) + Number(tail[12] ?? 0);
  }
  return { pids, pssKiB, rssKiB, cpuTicks };
}

async function waitUntil(predicate, timeoutMs, message) {
  const deadline = performance.now() + timeoutMs;
  while (performance.now() < deadline) {
    const value = predicate();
    if (value) return value;
    await sleep(5);
  }
  throw new Error(message);
}

async function drain(stream, onText = () => undefined) {
  const reader = stream.getReader();
  const decoder = new TextDecoder();
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    onText(decoder.decode(value, { stream: true }));
  }
}

async function oneRun({ cold, sampleCpu }) {
  if (cold) dropPayloadCache(appDir);
  const runRoot = mkdtempSync(join(tmpdir(), "patchops-tauri-benchmark-"));
  try {
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
    copyFileSync(join(fixture, "data/PatchOpsIII/electron-settings.json"), join(appData, "electron-settings.json"));
    const steamHome = join(home, ".steam/steam");
    mkdirSync(dirname(steamHome), { recursive: true });
    cpSync(join(fixture, "steam"), steamHome, { recursive: true });

    const started = performance.now();
    let interactiveAt = null;
    const child = Bun.spawn([appRun], {
      cwd: appDir,
      env: {
        ...process.env,
        GDK_BACKEND: "x11",
        HOME: home,
        PATCHOPSIII_BENCHMARK: "1",
        XDG_CACHE_HOME: cacheHome,
        XDG_CONFIG_HOME: configHome,
        XDG_DATA_HOME: dataHome,
        XDG_RUNTIME_DIR: runtimeDir,
      },
      stdout: "pipe",
      stderr: "pipe",
    });
    const stdoutDrain = drain(child.stdout);
    let stderrTail = "";
    const stderrDrain = drain(child.stderr, (text) => {
      const combined = `${stderrTail}${text}`;
      if (interactiveAt === null && combined.includes("PATCHOPSIII_BENCHMARK_INTERACTIVE")) {
        interactiveAt = performance.now();
      }
      stderrTail = combined.slice(-256);
    });

    let sampling = true;
    let peakPssKiB = 0;
    let peakRssKiB = 0;
    const observedPids = new Set([child.pid]);
    const sampler = (async () => {
      while (sampling) {
        const sample = processSample(child.pid);
        sample.pids.forEach((pid) => observedPids.add(pid));
        peakPssKiB = Math.max(peakPssKiB, sample.pssKiB);
        peakRssKiB = Math.max(peakRssKiB, sample.rssKiB);
        await sleep(20);
      }
    })();

    try {
      const logPath = join(appData, "PatchOpsIII.log");
      await waitUntil(
        () => existsSync(logPath) && readFileSync(logPath, "utf8").includes("PatchOpsIII started."),
        15_000,
        "native-core readiness marker was not observed",
      );
      const coreReadyMs = performance.now() - started;
      await waitUntil(() => interactiveAt, 15_000, "interactive readiness marker was not observed");
      const interactiveMs = interactiveAt - started;
      await sleep(2_000);
      const idle = processSample(child.pid);
      idle.pids.forEach((pid) => observedPids.add(pid));

      let idleCpuPercent = null;
      if (sampleCpu) {
        const cpuStart = idle.cpuTicks;
        const cpuStarted = performance.now();
        await sleep(idleCpuSeconds * 1_000);
        const cpuEnd = processSample(child.pid).cpuTicks;
        const elapsedSeconds = (performance.now() - cpuStarted) / 1_000;
        const ticksPerSecond = Number(Bun.spawnSync(["getconf", "CLK_TCK"]).stdout.toString().trim());
        idleCpuPercent = ((cpuEnd - cpuStart) / ticksPerSecond / elapsedSeconds) * 100;
      }

      const shutdownStarted = performance.now();
      const close = Bun.spawnSync([closeHelper, "PatchOpsIII"], { stdout: "pipe", stderr: "pipe" });
      if (close.exitCode !== 0) {
        throw new Error(`native close failed: ${close.stderr.toString()}`);
      }
      await waitUntil(
        () => [...observedPids].every((pid) => !existsSync(`/proc/${pid}`)),
        10_000,
        "process tree did not exit",
      );
      const shutdownMs = performance.now() - shutdownStarted;
      await child.exited;
      sampling = false;
      await sampler;
      await Promise.all([stdoutDrain, stderrDrain]);

      return {
        coreReadyMs,
        interactiveMs,
        idlePssKiB: idle.pssKiB,
        peakPssKiB,
        idleRssKiB: idle.rssKiB,
        peakRssKiB,
        idleCpuPercent,
        processCount: idle.pids.length,
        shutdownMs,
      };
    } catch (error) {
      sampling = false;
      try {
        child.kill("SIGKILL");
      } catch {
        // The process already exited.
      }
      await Promise.allSettled([child.exited, sampler, stdoutDrain, stderrDrain]);
      throw error;
    }
  } finally {
    rmSync(runRoot, { recursive: true, force: true });
  }
}

const warm = [];
for (let index = 0; index < warmRuns; index++) {
  warm.push(await oneRun({ cold: false, sampleCpu: index < cpuRuns }));
}
const cold = [];
for (let index = 0; index < coldRuns; index++) {
  cold.push(await oneRun({ cold: true, sampleCpu: false }));
}

const values = (runs, key) => runs.map((run) => run[key]).filter((value) => value !== null);
const median = (items) => [...items].sort((left, right) => left - right)[Math.floor(items.length / 2)];
const result = {
  method: `Pre-extracted AppImage; per-run isolated HOME and XDG directories with fixture settings and Steam metadata; native-ready log marker; initial get_state completion marker; aggregate process-tree PSS/RSS after two idle seconds; CPU over ${idleCpuSeconds} seconds; native WM_DELETE_WINDOW shutdown`,
  artifact: basename(appDir),
  warmRuns: Object.fromEntries(Object.keys(warm[0]).map((key) => [key, values(warm, key)])),
  payloadColdInteractiveMs: values(cold, "interactiveMs"),
  medians: Object.fromEntries(Object.keys(warm[0]).map((key) => [key, median(values(warm, key))])),
};
console.log(JSON.stringify(result, null, 2));
