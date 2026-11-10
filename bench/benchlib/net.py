"""Tiny /proc/net helpers."""
from __future__ import annotations

import os


def _socket_inodes(pids) -> set[str]:
    out = set()
    for pid in pids:
        d = f"/proc/{pid}/fd"
        try:
            for fd in os.listdir(d):
                try:
                    t = os.readlink(f"{d}/{fd}")
                except OSError:
                    continue
                if t.startswith("socket:["):
                    out.add(t[8:-1])
        except OSError:
            continue
    return out


def client_connections_to_port(pids, port: int) -> int:
    """Count ESTABLISHED TCP connections whose remote port is `port` and whose socket is owned by one of `pids`."""
    inodes = _socket_inodes(pids)
    if not inodes:
        return 0
    n = 0
    for f in ("/proc/net/tcp", "/proc/net/tcp6"):
        try:
            with open(f) as fh:
                next(fh)
                for line in fh:
                    p = line.split()
                    if len(p) < 10 or p[3] != "01":
                        continue
                    rem_port = int(p[2].rsplit(":", 1)[1], 16)
                    if rem_port == port and p[9] in inodes:
                        n += 1
        except (OSError, StopIteration):
            continue
    return n
