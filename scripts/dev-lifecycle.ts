import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { spawnSync, type ChildProcess } from "node:child_process";
import process from "node:process";

type ProcessRecord = {
  command: string;
  name: string;
  pid: number;
  session: string;
  startedAt: string;
};

const stateDir = path.resolve(".patchopsiii-dev");

function recordPath(session: string, name: string) {
  return path.join(stateDir, `${session}-${name}.json`);
}

function removeRecord(filePath: string) {
  try {
    rmSync(filePath, { force: true });
  } catch {
    // Best effort cleanup only.
  }
}

function readRecord(filePath: string): ProcessRecord | null {
  try {
    return JSON.parse(readFileSync(filePath, "utf-8")) as ProcessRecord;
  } catch {
    removeRecord(filePath);
    return null;
  }
}

function processExists(pid: number) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

export function killPidTree(pid: number) {
  if (!processExists(pid)) {
    return;
  }

  if (process.platform === "win32") {
    spawnSync("taskkill", ["/pid", String(pid), "/t", "/f"], { stdio: "ignore" });
    return;
  }

  try {
    process.kill(pid, "SIGTERM");
  } catch {
    // The process may have already exited.
  }
}

export function killChildTree(child: ChildProcess | null) {
  if (!child?.pid || child.killed) {
    return;
  }
  killPidTree(child.pid);
}

export function cleanupSession(session: string) {
  if (!existsSync(stateDir)) {
    return;
  }

  for (const entry of readdirSync(stateDir)) {
    if (!entry.startsWith(`${session}-`) || !entry.endsWith(".json")) {
      continue;
    }
    const filePath = path.join(stateDir, entry);
    const record = readRecord(filePath);
    if (record?.pid) {
      killPidTree(record.pid);
    }
    removeRecord(filePath);
  }
}

export function registerProcess(session: string, name: string, child: ChildProcess, command: string) {
  if (!child.pid) {
    return;
  }

  mkdirSync(stateDir, { recursive: true });
  const filePath = recordPath(session, name);
  const record: ProcessRecord = {
    command,
    name,
    pid: child.pid,
    session,
    startedAt: new Date().toISOString()
  };

  writeFileSync(filePath, JSON.stringify(record, null, 2), "utf-8");
  child.once("exit", () => removeRecord(filePath));
}

function pidsListeningOnPorts(ports: string[]) {
  if (process.platform !== "win32" || ports.length === 0) {
    return [];
  }

  const portList = ports.map((port) => `[int]${port}`).join(",");
  const result = spawnSync(
    "powershell",
    [
      "-NoProfile",
      "-Command",
      `$ports=@(${portList}); Get-NetTCPConnection -LocalPort $ports -State Listen -ErrorAction SilentlyContinue | Select-Object -ExpandProperty OwningProcess -Unique`
    ],
    { encoding: "utf-8" }
  );

  return result.stdout
    .split(/\r?\n/)
    .map((line) => Number(line.trim()))
    .filter((pid) => Number.isInteger(pid) && pid > 0);
}

export function cleanupAllDevProcesses(ports: string[]) {
  if (existsSync(stateDir)) {
    for (const entry of readdirSync(stateDir)) {
      if (!entry.endsWith(".json")) {
        continue;
      }
      const filePath = path.join(stateDir, entry);
      const record = readRecord(filePath);
      if (record?.pid) {
        killPidTree(record.pid);
      }
      removeRecord(filePath);
    }
  }

  for (const pid of pidsListeningOnPorts(ports)) {
    killPidTree(pid);
  }
}
