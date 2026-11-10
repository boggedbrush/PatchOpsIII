"""Machine info, git info, idle guard."""
from __future__ import annotations

import datetime
import os
import platform
import shutil
import subprocess
import sys
import time

from . import procfs


def now_iso() -> str:
    return datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")


def _cmd(args, cwd=None, timeout=20):
    try:
        r = subprocess.run(args, cwd=cwd, capture_output=True, text=True, timeout=timeout)
        return r.stdout.strip() if r.returncode == 0 else ""
    except Exception:
        return ""


def tool_version(name: str, *args: str) -> str:
    exe = shutil.which(name)
    if not exe:
        return ""
    return (_cmd([exe, *(args or ("--version",))]).splitlines() or [""])[0]


def machine_info() -> dict:
    cpu_model = ""
    try:
        with open("/proc/cpuinfo") as fh:
            for line in fh:
                if line.startswith("model name"):
                    cpu_model = line.split(":", 1)[1].strip()
                    break
    except OSError:
        pass
    mem = procfs.meminfo_kb()
    return {
        "hostname": platform.node(),
        "kernel": platform.release(),
        "platform": platform.platform(),
        "arch": platform.machine(),
        "cpu_model": cpu_model,
        "cpu_count": os.cpu_count(),
        "mem_total_mb": mem.get("MemTotal", 0) // 1024,
        "python": sys.version.split()[0],
        "tools": {
            "bun": tool_version("bun"),
            "node": tool_version("node"),
            "python3": tool_version("python3"),
            "cargo": tool_version("cargo"),
            "rustc": tool_version("rustc"),
            "Xvfb": "present" if shutil.which("Xvfb") else "",
            "xdotool": tool_version("xdotool", "version"),
        },
        "euid": os.geteuid(),
        "loadavg": list(procfs.loadavg()),
    }


def git_info(path: str) -> dict:
    info = {"path": os.path.abspath(path)}
    info["commit"] = _cmd(["git", "rev-parse", "HEAD"], cwd=path)
    info["branch"] = _cmd(["git", "rev-parse", "--abbrev-ref", "HEAD"], cwd=path)
    info["describe"] = _cmd(["git", "describe", "--always", "--dirty=-dirty", "--exclude=*"], cwd=path)
    dirty = _cmd(["git", "status", "--porcelain", "--untracked-files=no"], cwd=path)
    info["dirty_tracked_files"] = len(dirty.splitlines()) if dirty else 0
    return info


class IdleGuard:
    """Wait until the 1-min loadavg drops below a threshold before timed sections."""

    def __init__(self, threshold: float = 0.7, timeout_s: float = 900.0, enabled: bool = True, log=print):
        self.threshold = threshold
        self.timeout_s = timeout_s
        self.enabled = enabled
        self.log = log
        self.events: list[dict] = []
        self.quiet_stats = {"checks": 0, "waited_s": 0.0, "timeouts": 0, "limit_percent": 25.0}

    def wait(self, label: str) -> dict:
        start = time.monotonic()
        l1 = procfs.loadavg()[0]
        ev = {"label": label, "enabled": self.enabled, "load1_before": l1, "waited_s": 0.0, "timed_out": False}
        if self.enabled and l1 >= self.threshold:
            self.log(f"[idle-guard] {label}: load1={l1:.2f} >= {self.threshold}, waiting (timeout {self.timeout_s:.0f}s)")
            while True:
                time.sleep(2.0)
                l1 = procfs.loadavg()[0]
                if l1 < self.threshold:
                    break
                if time.monotonic() - start > self.timeout_s:
                    ev["timed_out"] = True
                    self.log(f"[idle-guard] {label}: TIMEOUT, continuing with load1={l1:.2f}")
                    break
        ev["waited_s"] = round(time.monotonic() - start, 1)
        ev["load1_at_start"] = l1
        self.events.append(ev)
        return ev

    def wait_quiet(self, label: str, limit: float = 25.0, need: int = 2, max_wait: float = 180.0) -> float:
        """Finer-grained guard used between workload groups: wait until the host-wide CPU busy % (from /proc/stat,
        0.5 s windows) stayed below `limit` for `need` consecutive windows. Returns seconds waited."""
        if not self.enabled:
            return 0.0
        t0 = time.monotonic()
        prev = procfs.cpu_times()
        ok = 0
        timed_out = False
        while True:
            time.sleep(0.5)
            cur = procfs.cpu_times()
            b = procfs.host_busy_percent(prev, cur)
            prev = cur
            ok = ok + 1 if (b is not None and b < limit) else 0
            if ok >= need:
                break
            if time.monotonic() - t0 > max_wait:
                timed_out = True
                break
        waited = time.monotonic() - t0
        self.quiet_stats["checks"] += 1
        self.quiet_stats["waited_s"] = round(self.quiet_stats["waited_s"] + waited, 1)
        self.quiet_stats["timeouts"] += int(timed_out)
        return waited
