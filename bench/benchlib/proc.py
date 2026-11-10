"""Process helpers: measured build commands, managed (killable) process trees, Xvfb."""
from __future__ import annotations

import os
import shutil
import signal
import socket
import struct
import subprocess
import threading
import time

from . import procfs


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def tail(path: str, n: int = 25) -> str:
    try:
        with open(path, "rb") as fh:
            return b"\n".join(fh.read().splitlines()[-n:]).decode("utf-8", "replace")
    except OSError:
        return ""


def run_measured(cmd, *, cwd=None, env=None, log_path: str, timeout: float = 3600, sample_s: float = 0.1,
                 shell: bool = False) -> dict:
    """Run a command to completion; return wall time, CPU time, peak process-tree RSS (sampled).

    CPU time and ru_maxrss come from wait4() on the direct child (includes reaped descendants).
    peak_rss_sum_kb is the maximum over samples (every `sample_s`) of the sum of RSS of the
    whole descendant tree - it is a sampled lower bound of the true peak and over-counts shared pages.
    """
    os.makedirs(os.path.dirname(log_path), exist_ok=True)
    t0 = time.monotonic()
    with open(log_path, "wb") as logfh:
        logfh.write(f"$ {cmd if isinstance(cmd, str) else ' '.join(map(str, cmd))}\n".encode())
        logfh.flush()
        p = subprocess.Popen(cmd, cwd=cwd, env=env, stdout=logfh, stderr=subprocess.STDOUT,
                             stdin=subprocess.DEVNULL, start_new_session=True, shell=shell)
        peak_rss = 0
        peak_procs = 0
        peak_threads = 0
        samples = 0
        status = None
        ru = None
        timed_out = False
        while True:
            pid_, st, ru_ = os.wait4(p.pid, os.WNOHANG)
            if pid_ != 0:
                status, ru = st, ru_
                break
            samples += 1
            try:
                s = procfs.tree_sample(p.pid, with_pss=False)
                peak_rss = max(peak_rss, s["rss_kb"])
                peak_procs = max(peak_procs, s["procs"])
                peak_threads = max(peak_threads, s["threads"])
            except Exception:
                pass
            if time.monotonic() - t0 > timeout:
                timed_out = True
                kill_tree(p.pid)
            time.sleep(sample_s)
        p.returncode = os.waitstatus_to_exitcode(status)
    wall = time.monotonic() - t0
    # reap stragglers of the process group
    try:
        os.killpg(p.pid, signal.SIGKILL)
    except OSError:
        pass
    return {
        "cmd": cmd if isinstance(cmd, str) else [str(c) for c in cmd],
        "exit_code": p.returncode,
        "timed_out": timed_out,
        "wall_s": round(wall, 3),
        "cpu_user_s": round(ru.ru_utime, 3),
        "cpu_sys_s": round(ru.ru_stime, 3),
        "cpu_total_s": round(ru.ru_utime + ru.ru_stime, 3),
        "peak_rss_sum_kb": peak_rss,
        "peak_procs": peak_procs,
        "peak_threads": peak_threads,
        "ru_maxrss_kb": ru.ru_maxrss,
        "rss_samples": samples,
        "log": log_path,
        "log_tail": tail(log_path, 12) if p.returncode != 0 else "",
    }


def kill_tree(pid: int, sig=signal.SIGKILL):
    """Kill a process and all of its descendants (collected first so re-parenting cannot hide them)."""
    pids = procfs.descendants(pid)
    for q in reversed(pids):
        try:
            os.kill(q, sig)
        except OSError:
            pass
    try:
        os.killpg(pid, sig)
    except OSError:
        pass


class ManagedProcess:
    """A long-running process (GUI or backend) in its own session, killable as a tree."""

    def __init__(self, cmd, *, cwd, env, tag: str, run_as: tuple[int, int] | None, stdin=subprocess.DEVNULL,
                 stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL):
        self.tag = tag
        self.cmd = cmd
        self._stderr = stderr
        kw = {}
        if run_as is not None and os.geteuid() == 0:
            kw.update(user=run_as[0], group=run_as[1], extra_groups=[])
        self.t_spawn = time.monotonic()
        self.popen = subprocess.Popen(cmd, cwd=cwd, env=env, stdin=stdin, stdout=stdout, stderr=self._stderr,
                                      start_new_session=True, **kw)
        self.pid = self.popen.pid

    def alive(self) -> bool:
        return self.popen.poll() is None

    def stop(self, grace: float = 3.0) -> dict:
        """SIGTERM the tree, wait, SIGKILL leftovers, sweep the tag. Returns leak report."""
        tree = procfs.descendants(self.pid)
        for q in tree:
            try:
                os.kill(q, signal.SIGTERM)
            except OSError:
                pass
        end = time.monotonic() + grace
        while time.monotonic() < end and (self.popen.poll() is None or any(procfs.read_stat(q) and procfs.read_stat(q)[4] != "Z" for q in tree)):
            time.sleep(0.05)
        leftovers = [q for q in tree if (procfs.read_stat(q) and procfs.read_stat(q)[4] != "Z")]
        kill_tree(self.pid)
        tagged = procfs.sweep_tag(self.tag)
        try:
            self.popen.wait(timeout=2)
        except Exception:
            pass
        if self._stderr not in (None, subprocess.DEVNULL):
            try:
                self._stderr.close()
            except Exception:
                pass
        time.sleep(0.15)
        still = procfs.tagged_pids(self.tag)
        return {"needed_sigkill": len(leftovers), "tag_swept": len(tagged), "still_alive_after": len(still)}


def _x_handshake(sock_path: str) -> bool:
    """True once the X server behind `sock_path` completes the connection setup (reply byte 1 = Success).

    The socket file appears (and even accepts connections) before the server has finished initialising, and under load a
    client that connects in that window (gpui's x11rb: "Failed to initialize X11 client ... Unknown connection error")
    can fail. Waiting for a real handshake replaces the former blind 150 ms sleep; it runs before the target is spawned, so
    it does not enter any startup timing."""
    s = socket.socket(socket.AF_UNIX)
    s.settimeout(2.0)
    try:
        s.connect(sock_path)
        s.sendall(struct.pack("<BxHHHHxx", 0x6C, 11, 0, 0, 0))  # little-endian, protocol 11.0, no auth
        return s.recv(1) == b"\x01"
    except OSError:
        return False
    finally:
        s.close()


class Xvfb:
    def __init__(self, tag: str, size="1920x1080x24"):
        self.tag = tag
        self.size = size
        self.proc: subprocess.Popen | None = None
        self.display = ""

    def start(self, env: dict) -> str:
        exe = shutil.which("Xvfb")
        if not exe:
            raise RuntimeError("Xvfb not installed")
        for num in range(90, 190):
            if os.path.exists(f"/tmp/.X11-unix/X{num}") or os.path.exists(f"/tmp/.X{num}-lock"):
                continue
            e = dict(env)
            self.proc = subprocess.Popen([exe, f":{num}", "-screen", "0", self.size, "-nolisten", "tcp", "-ac"],
                                         env=e, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                         stderr=subprocess.DEVNULL, start_new_session=True)
            sock = f"/tmp/.X11-unix/X{num}"
            end = time.monotonic() + 15
            while time.monotonic() < end:
                if self.proc.poll() is not None:
                    break
                if os.path.exists(sock) and _x_handshake(sock):
                    self.display = f":{num}"
                    return self.display
                time.sleep(0.01)
            self.stop()
        raise RuntimeError("could not start Xvfb")

    def stop(self):
        if self.proc and self.proc.poll() is None:
            try:
                os.killpg(self.proc.pid, signal.SIGTERM)
                self.proc.wait(timeout=3)
            except Exception:
                try:
                    os.killpg(self.proc.pid, signal.SIGKILL)
                except OSError:
                    pass
        if self.display:
            for p in (f"/tmp/.X11-unix/X{self.display[1:]}", f"/tmp/.X{self.display[1:]}-lock"):
                try:
                    os.unlink(p)
                except OSError:
                    pass
        self.proc = None


def window_probe(display: str, name_pattern: str = "PatchOps", min_size=(200, 150)) -> dict | None:
    """Return info about the first viewable top-level window >= min_size, or None.

    1. a viewable window whose title matches `name_pattern` (the app title is PatchOpsIII), via `xdotool search --onlyvisible`;
    2. fallback (title not set / different): any viewable direct child of the root window of this (app-private) Xvfb
       display that is >= min_size, via `xwininfo`.
    """
    env = {"DISPLAY": display, "PATH": os.environ.get("PATH", "")}

    def geom(wid):
        try:
            g = subprocess.run(["xdotool", "getwindowgeometry", wid], env=env, capture_output=True, text=True, timeout=5)
        except Exception:
            return 0, 0
        for line in g.stdout.splitlines():
            line = line.strip()
            if line.startswith("Geometry:"):
                try:
                    w, h = [int(x) for x in line.split(":", 1)[1].strip().split("x")]
                    return w, h
                except ValueError:
                    pass
        return 0, 0

    try:
        r = subprocess.run(["xdotool", "search", "--onlyvisible", "--name", name_pattern], env=env, capture_output=True,
                           text=True, timeout=5)
    except Exception:
        r = None
    for wid in (r.stdout.split() if r else []):
        w, h = geom(wid)
        if w >= min_size[0] and h >= min_size[1]:
            try:
                nm = subprocess.run(["xdotool", "getwindowname", wid], env=env, capture_output=True, text=True, timeout=5).stdout.strip()
            except Exception:
                nm = ""
            return {"window_id": wid, "width": w, "height": h, "name": nm, "matched_by": "title"}
    try:
        t = subprocess.run(["xwininfo", "-root", "-children"], env=env, capture_output=True, text=True, timeout=5)
    except Exception:
        return None
    for line in t.stdout.splitlines():
        line = line.strip()
        if not line.startswith("0x"):
            continue
        wid = line.split()[0]
        try:
            info = subprocess.run(["xwininfo", "-id", wid], env=env, capture_output=True, text=True, timeout=5).stdout
        except Exception:
            continue
        if "IsViewable" not in info:
            continue
        w = h = 0
        for l in info.splitlines():
            l = l.strip()
            if l.startswith("Width:"):
                w = int(l.split(":")[1])
            elif l.startswith("Height:"):
                h = int(l.split(":")[1])
        if w >= min_size[0] and h >= min_size[1]:
            return {"window_id": wid, "width": w, "height": h, "name": "", "matched_by": "top-level-geometry"}
    return None
