"""Shared driver plumbing: instance lifecycle (spawn, readiness probing, memory sampling)."""
from __future__ import annotations

import os
import threading
import time

from .. import procfs, proc, net


class Instance:
    """A running GUI or headless process tree plus a client speaking the op protocol."""

    def __init__(self, mp: proc.ManagedProcess, client, kind: str, display: str | None, backend_port: int | None = None,
                 log_files=()):
        self.mp = mp
        self.client = client
        self.kind = kind
        self.display = display
        self.backend_port = backend_port
        self.log_files = log_files
        self.pid = mp.pid

    def wait_ready(self, ready_probe, timeout: float = 60.0, want_window: bool = True) -> dict:
        """Wait for: first viewable top-level window (thread), backend/RPC readiness, and (legacy GUI) the renderer's
        first connection to the backend. Times are milliseconds since the instant just before spawn.
        """
        t0 = self.mp.t_spawn
        res: dict = {"window_ms": None, "health_ms": None, "ready_ms": None, "ui_backend_connected_ms": None,
                     "window_info": None, "failed": None}
        need_window = want_window and self.display is not None
        stop = threading.Event()
        deadline = time.monotonic() + timeout

        def window_loop():
            while not stop.is_set() and time.monotonic() < deadline:
                w = proc.window_probe(self.display)
                if w:
                    res["window_ms"] = round((time.monotonic() - t0) * 1000, 1)
                    res["window_info"] = w
                    return
                stop.wait(0.02)

        th = None
        if need_window:
            th = threading.Thread(target=window_loop, daemon=True)
            th.start()
        ui_wait = bool(self.backend_port and self.kind == "gui")
        ready_done = False
        while time.monotonic() < deadline:
            if not self.mp.alive():
                res["failed"] = f"process exited early (code {self.mp.popen.returncode})"
                break
            if not ready_done:
                ready_done = bool(ready_probe(self, res, t0))
                if res["failed"]:
                    break
            if ui_wait and res["ui_backend_connected_ms"] is None:
                pids = procfs.descendants(self.pid)
                if net.client_connections_to_port(pids, self.backend_port) > 0:
                    res["ui_backend_connected_ms"] = round((time.monotonic() - t0) * 1000, 1)
            if ready_done and (not need_window or res["window_ms"] is not None) and (not ui_wait or res["ui_backend_connected_ms"] is not None):
                break
            time.sleep(0.01)
        else:
            res["failed"] = "timeout"
        stop.set()
        if th:
            th.join(timeout=2)
        if res["failed"] is None and (res["ready_ms"] is None or (need_window and res["window_ms"] is None)):
            res["failed"] = "timeout"
        res.pop("_probe_sent", None)
        return res

    def stop(self) -> dict:
        try:
            self.client.close()
        except Exception:
            pass
        rep = self.mp.stop()
        for f in self.log_files:
            try:
                f.close()
            except Exception:
                pass
        return rep


class MemSampler(threading.Thread):
    """Background sampler of the process tree (PSS, RSS, process and thread counts)."""

    def __init__(self, root_pid: int, interval: float = 0.5):
        super().__init__(daemon=True)
        self.root = root_pid
        self.interval = interval
        self.stop_evt = threading.Event()
        self.samples: list[dict] = []
        self.peak: dict | None = None

    def run(self):
        t0 = time.monotonic()
        while not self.stop_evt.is_set():
            try:
                s = procfs.tree_sample(self.root, with_pss=True, detail=False)
                s["t"] = round(time.monotonic() - t0, 2)
                self.samples.append(s)
                if self.peak is None or s["pss_kb"] > self.peak["pss_kb"]:
                    d = procfs.tree_sample(self.root, with_pss=True, detail=True)
                    d["t"] = s["t"]
                    self.peak = d
            except Exception:
                pass
            self.stop_evt.wait(self.interval)

    def finish(self) -> dict:
        self.stop_evt.set()
        self.join(timeout=5)
        return {
            "interval_s": self.interval,
            "n": len(self.samples),
            "peak_pss_kb": max((s["pss_kb"] for s in self.samples), default=None),
            "peak_rss_kb": max((s["rss_kb"] for s in self.samples), default=None),
            "peak_procs": max((s["procs"] for s in self.samples), default=None),
            "peak_threads": max((s["threads"] for s in self.samples), default=None),
            "peak_detail": self.peak,
            "series": [{"t": s["t"], "pss_kb": s["pss_kb"], "rss_kb": s["rss_kb"], "procs": s["procs"], "threads": s["threads"]}
                       for s in self.samples],
        }


def idle_window(root_pid: int, seconds: float = 10.0, hz: float = 1.0) -> dict:
    """Sample the idle process tree for `seconds`; CPU% from utime+stime deltas over the window."""
    samples = []
    t_start = time.monotonic()
    meter = procfs.LoadMeter(root_pid)
    first = procfs.tree_sample(root_pid, with_pss=True)
    samples.append(first)
    n = max(1, int(seconds * hz))
    for _ in range(n):
        time.sleep(1.0 / hz)
        samples.append(procfs.tree_sample(root_pid, with_pss=True))
    last = samples[-1]
    load = meter.read()
    elapsed = last["t"] - first["t"]
    cpu = (last["cpu_ticks"] - first["cpu_ticks"]) / procfs.CLK_TCK
    detail = procfs.tree_sample(root_pid, with_pss=True, detail=True)
    pss = [s["pss_kb"] for s in samples]
    rss = [s["rss_kb"] for s in samples]
    return {
        "window_s": round(elapsed, 2), "samples": len(samples),
        "pss_kb_mean": round(sum(pss) / len(pss)), "pss_kb_min": min(pss), "pss_kb_max": max(pss),
        "rss_kb_mean": round(sum(rss) / len(rss)),
        "procs": last["procs"], "threads": last["threads"],
        "cpu_percent": round(100.0 * cpu / elapsed, 2) if elapsed > 0 else None,
        "cpu_seconds": round(cpu, 3),
        "host_busy_percent_all_cores": load["host_busy_percent"],
        "foreign_busy_percent_all_cores": load["foreign_busy_percent"],
        "load1_end": procfs.loadavg()[0],
        "detail": detail["detail"],
        "pss_series_kb": pss,
    }
