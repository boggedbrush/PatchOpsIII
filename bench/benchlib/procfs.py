"""Minimal /proc readers: process tree, RSS/PSS, threads, CPU ticks, load average."""
from __future__ import annotations

import os
import time

CLK_TCK = os.sysconf("SC_CLK_TCK")
PAGE_SIZE = os.sysconf("SC_PAGE_SIZE")


def loadavg() -> tuple[float, float, float]:
    with open("/proc/loadavg") as fh:
        a, b, c = fh.read().split()[:3]
    return float(a), float(b), float(c)


def meminfo_kb() -> dict[str, int]:
    out: dict[str, int] = {}
    with open("/proc/meminfo") as fh:
        for line in fh:
            k, _, v = line.partition(":")
            parts = v.split()
            if parts:
                out[k] = int(parts[0])
    return out


def read_stat(pid: int):
    """Return (ppid, comm, ticks, threads, state, starttime) or None."""
    try:
        with open(f"/proc/{pid}/stat", "rb") as fh:
            raw = fh.read().decode("utf-8", "replace")
    except OSError:
        return None
    rp = raw.rfind(")")
    lp = raw.find("(")
    if rp < 0 or lp < 0:
        return None
    comm = raw[lp + 1 : rp]
    rest = raw[rp + 2 :].split()
    try:
        state = rest[0]
        ppid = int(rest[1])
        utime = int(rest[11])
        stime = int(rest[12])
        threads = int(rest[17])
        start = int(rest[19])
    except (IndexError, ValueError):
        return None
    return ppid, comm, utime + stime, threads, state, start


def all_procs() -> dict[int, tuple]:
    table = {}
    for name in os.listdir("/proc"):
        if name.isdigit():
            pid = int(name)
            st = read_stat(pid)
            if st:
                table[pid] = st
    return table


def descendants(root: int, table: dict | None = None, include_root: bool = True) -> list[int]:
    table = table if table is not None else all_procs()
    kids: dict[int, list[int]] = {}
    for pid, st in table.items():
        kids.setdefault(st[0], []).append(pid)
    out = [root] if include_root and root in table else []
    stack = [root]
    while stack:
        cur = stack.pop()
        for k in kids.get(cur, []):
            out.append(k)
            stack.append(k)
    return out


def cmdline(pid: int) -> list[str]:
    try:
        with open(f"/proc/{pid}/cmdline", "rb") as fh:
            return [p.decode("utf-8", "replace") for p in fh.read().split(b"\0") if p]
    except OSError:
        return []


def rollup_kb(pid: int) -> tuple[int, int] | None:
    """(pss_kb, rss_kb) from smaps_rollup; None if unreadable."""
    try:
        pss = rss = None
        with open(f"/proc/{pid}/smaps_rollup") as fh:
            for line in fh:
                if line.startswith("Pss:"):
                    pss = int(line.split()[1])
                elif line.startswith("Rss:"):
                    rss = int(line.split()[1])
        if pss is None or rss is None:
            return None
        return pss, rss
    except OSError:
        return None


def statm_rss_kb(pid: int) -> int | None:
    try:
        with open(f"/proc/{pid}/statm") as fh:
            return int(fh.read().split()[1]) * PAGE_SIZE // 1024
    except (OSError, ValueError, IndexError):
        return None


def role_of(pid: int, comm: str) -> str:
    for a in " ".join(cmdline(pid)).split():     # Chromium rewrites its title into a single argv[0]
        if a.startswith("--type="):
            return a.split("=", 1)[1]
    return comm


def tree_sample(root: int, with_pss: bool = True, detail: bool = False) -> dict:
    """One sample of the whole process tree rooted at `root` (root included)."""
    table = all_procs()
    pids = descendants(root, table)
    pss = rss = threads = ticks = 0
    procs = []
    for pid in pids:
        st = table.get(pid)
        if not st or st[4] == "Z":
            continue
        ticks += st[2]
        threads += st[3]
        p_kb = r_kb = None
        if with_pss:
            roll = rollup_kb(pid)
            if roll:
                p_kb, r_kb = roll
        if r_kb is None:
            r_kb = statm_rss_kb(pid)
        if r_kb:
            rss += r_kb
        if p_kb:
            pss += p_kb
        if detail:
            procs.append({"pid": pid, "comm": st[1], "role": "main" if pid == root else role_of(pid, st[1]), "pss_kb": p_kb, "rss_kb": r_kb, "threads": st[3]})
    s = {"t": time.monotonic(), "procs": len([p for p in pids if table.get(p) and table[p][4] != "Z"]),
         "threads": threads, "rss_kb": rss, "pss_kb": pss, "cpu_ticks": ticks}
    if detail:
        s["detail"] = procs
    return s


def sweep_tag(tag: str, sig: int = 9, exclude: set[int] | None = None) -> list[int]:
    """Kill every process whose environment carries PATCHOPS_BENCH_TAG=<tag>. Returns killed pids."""
    needle = f"PATCHOPS_BENCH_TAG={tag}".encode()
    killed = []
    me = os.getpid()
    for name in os.listdir("/proc"):
        if not name.isdigit():
            continue
        pid = int(name)
        if pid == me or (exclude and pid in exclude):
            continue
        try:
            with open(f"/proc/{pid}/environ", "rb") as fh:
                env = fh.read()
        except OSError:
            continue
        if needle in env.split(b"\0"):
            try:
                os.kill(pid, sig)
                killed.append(pid)
            except OSError:
                pass
    return killed


def tagged_pids(tag: str) -> list[int]:
    needle = f"PATCHOPS_BENCH_TAG={tag}".encode()
    out = []
    for name in os.listdir("/proc"):
        if name.isdigit():
            try:
                with open(f"/proc/{name}/environ", "rb") as fh:
                    if needle in fh.read().split(b"\0"):
                        st = read_stat(int(name))
                        if st and st[4] != "Z":
                            out.append(int(name))
            except OSError:
                pass
    return out


def cpu_times() -> tuple[int, int]:
    """(busy_ticks, total_ticks) summed over all CPUs from /proc/stat (busy = total - idle - iowait)."""
    with open("/proc/stat") as fh:
        parts = fh.readline().split()[1:]
    vals = [int(x) for x in parts]
    idle = vals[3] + (vals[4] if len(vals) > 4 else 0)
    total = sum(vals[:8])
    return total - idle, total


def host_busy_percent(a: tuple[int, int], b: tuple[int, int]) -> float | None:
    dt = b[1] - a[1]
    return round(100.0 * (b[0] - a[0]) / dt, 2) if dt > 0 else None


def steam_processes(own_tags=None) -> list[tuple[int, bool]]:
    """All processes whose name is exactly `steam`: (pid, is ours). "Ours" means it carries the PATCHOPS_BENCH_TAG of a run
    of *this* harness process (`own_tags`); a process tagged by another harness instance is foreign."""
    out = []
    for name in os.listdir("/proc"):
        if not name.isdigit():
            continue
        st = read_stat(int(name))
        if not st or st[1] != "steam" or st[4] == "Z":
            continue
        tagged = False
        try:
            with open(f"/proc/{name}/environ", "rb") as fh:
                env = fh.read().split(b"\0")
            tagged = any(f"PATCHOPS_BENCH_TAG={t}".encode() in env for t in (own_tags or ()))
        except OSError:
            pass
        out.append((int(name), tagged))
    return out


def all_tagged_pids(tags) -> list[int]:
    """Every live process carrying PATCHOPS_BENCH_TAG=<t> for one of `tags` (the tags of this harness process's own runs).
    Never matches other tags: other agents/harness instances may be running on the same machine."""
    out = []
    for t in tags:
        out.extend(p for p in tagged_pids(t) if p != os.getpid() and p not in out)
    return out


class LoadMeter:
    """Host CPU accounting over a window: total busy %, and the part *not* explained by the benchmarked process tree or
    the harness itself ("foreign" load, e.g. a compiler running in another session). All percentages are of all cores."""

    def __init__(self, root_pid: int | None = None):
        self.root = root_pid
        self.reset()

    def _tree_ticks(self) -> int:
        if self.root is None:
            return 0
        try:
            return tree_sample(self.root, with_pss=False)["cpu_ticks"]
        except Exception:
            return 0

    def reset(self):
        self.h0 = cpu_times()
        self.t0 = self._tree_ticks()
        t = os.times()
        self.me0 = t.user + t.system

    def read(self) -> dict:
        h1 = cpu_times()
        total = h1[1] - self.h0[1]
        if total <= 0:
            return {"host_busy_percent": None, "foreign_busy_percent": None}
        busy = h1[0] - self.h0[0]
        t = os.times()
        me = (t.user + t.system - self.me0) * CLK_TCK
        app = self._tree_ticks() - self.t0
        foreign = max(0.0, busy - max(app, 0) - me)
        return {"host_busy_percent": round(100.0 * busy / total, 2), "foreign_busy_percent": round(100.0 * foreign / total, 2)}
