"""Transport clients with a uniform interface.

call(op, args) -> Resp(envelope_ok, result, error, http_status, latency_s)

Envelope semantics (identical for every target, see OPS.md):
  envelope_ok=True  -> the op ran; `result` is the op's JSON body (which may itself say {"ok": false, "error": ...})
  envelope_ok=False -> transport / validation level failure; `error` starts with "validation:", "unknown op:" or "internal:"
"""
from __future__ import annotations

import http.client
import json
import os
import select
import time
import urllib.parse
from dataclasses import dataclass, field

# op name -> (HTTP method, legacy path)
LEGACY_ROUTES = {
    "health": ("GET", "/api/health"),
    "status": ("GET", "/api/status"),
    "browse": ("GET", "/api/browse"),
    "game-directory": ("POST", "/api/game-directory"),
    "launch": ("POST", "/api/launch"),
    "launch-options": ("POST", "/api/launch-options"),
    "workshop-install": ("POST", "/api/workshop-install"),
    "update-check": ("POST", "/api/update-check"),
    "release-channel": ("POST", "/api/release-channel"),
    "config": ("POST", "/api/config"),
    "dxvk-install": ("POST", "/api/dxvk-install"),
    "dxvk-uninstall": ("POST", "/api/dxvk-uninstall"),
    "dxvk-config": ("POST", "/api/dxvk-config"),
    "t7-config": ("POST", "/api/t7-config"),
    "t7-install": ("POST", "/api/t7-install"),
    "t7-uninstall": ("POST", "/api/t7-uninstall"),
    "enhanced-install": ("POST", "/api/enhanced-install"),
    "enhanced-validate": ("POST", "/api/enhanced-validate"),
    "enhanced-uninstall": ("POST", "/api/enhanced-uninstall"),
    "exe-swap/compatible": ("POST", "/api/exe-swap/compatible"),
    "exe-swap/compatible-depot": ("GET", "/api/exe-swap/compatible-depot"),
    "exe-swap/current": ("POST", "/api/exe-swap/current"),
    "exe-swap/enhanced": ("POST", "/api/exe-swap/enhanced"),
    "presets/apply": ("POST", "/api/presets/apply"),
    "intro-skip": ("POST", "/api/intro-skip"),
    "d3dcompiler": ("POST", "/api/d3dcompiler"),
    "all-intros-skip": ("POST", "/api/all-intros-skip"),
    "config-readonly": ("POST", "/api/config-readonly"),
    "vram-target": ("POST", "/api/vram-target"),
    "logs/payload": ("GET", "/api/logs/payload"),
    "logs/clear": ("POST", "/api/logs/clear"),
    "mod-files/clear": ("POST", "/api/mod-files/clear"),
    "reset-stock": ("POST", "/api/reset-stock"),
}


@dataclass
class Resp:
    envelope_ok: bool
    result: dict | None = None
    error: str | None = None
    http_status: int | None = None
    latency_s: float = 0.0

    def body(self) -> dict:
        """Canonical, comparable form of the response."""
        if self.envelope_ok:
            return {"envelope_ok": True, "result": self.result}
        cat = (self.error or "").split(":", 1)[0]
        return {"envelope_ok": False, "error_class": cat}


class LegacyHttpClient:
    def __init__(self, host: str, port: int, timeout: float = 180.0):
        self.host, self.port, self.timeout = host, port, timeout
        self.conn: http.client.HTTPConnection | None = None

    def _connect(self):
        self.conn = http.client.HTTPConnection(self.host, self.port, timeout=self.timeout)

    def close(self):
        if self.conn:
            try:
                self.conn.close()
            except Exception:
                pass
            self.conn = None

    def call(self, op: str, args: dict | None = None) -> Resp:
        if op not in LEGACY_ROUTES:
            return Resp(False, error=f"unknown op: {op}")
        method, path = LEGACY_ROUTES[op]
        args = args or {}
        headers = {"Accept": "application/json"}
        body = None
        if method == "GET":
            q = {k: v for k, v in args.items() if v is not None}
            if q:
                path += "?" + urllib.parse.urlencode(q)
        else:
            body = json.dumps(args).encode()
            headers["Content-Type"] = "application/json"
        t0 = time.perf_counter()
        last_exc = None
        for attempt in (0, 1):
            try:
                if self.conn is None:
                    self._connect()
                self.conn.request(method, path, body=body, headers=headers)
                r = self.conn.getresponse()
                data = r.read()
                break
            except (http.client.RemoteDisconnected, BrokenPipeError, ConnectionResetError, http.client.CannotSendRequest,
                    http.client.BadStatusLine) as exc:
                last_exc = exc
                self.close()
                t0 = time.perf_counter()  # do not bill the stale-connection retry to the op
                continue
            except Exception as exc:
                self.close()
                return Resp(False, error=f"transport: {exc!r}", latency_s=time.perf_counter() - t0)
        else:
            return Resp(False, error=f"transport: {last_exc!r}", latency_s=time.perf_counter() - t0)
        lat = time.perf_counter() - t0
        status = r.status
        if 200 <= status < 300:
            try:
                return Resp(True, result=json.loads(data), http_status=status, latency_s=lat)
            except ValueError:
                return Resp(False, error="internal: non-JSON response", http_status=status, latency_s=lat)
        if status == 422:
            return Resp(False, error="validation: " + data.decode("utf-8", "replace")[:300], http_status=status, latency_s=lat)
        if status == 404 or status == 405:
            return Resp(False, error=f"unknown op: {op}", http_status=status, latency_s=lat)
        return Resp(False, error=f"internal: HTTP {status}", http_status=status, latency_s=lat)


class RpcClient:
    """JSON-lines client over a child's stdin/stdout (gpui contract)."""

    def __init__(self, popen, timeout: float = 180.0):
        self.p = popen
        self.timeout = timeout
        self.next_id = 1
        self.buf = b""
        self.fd = popen.stdout.fileno()

    def close(self):
        pass

    def _read_line(self, deadline: float) -> bytes | None:
        while b"\n" not in self.buf:
            left = deadline - time.monotonic()
            if left <= 0:
                return None
            r, _, _ = select.select([self.fd], [], [], min(left, 1.0))
            if r:
                chunk = os.read(self.fd, 65536)
                if not chunk:
                    return None
                self.buf += chunk
            elif self.p.poll() is not None and b"\n" not in self.buf:
                return None
        line, _, self.buf = self.buf.partition(b"\n")
        return line

    def call(self, op: str, args: dict | None = None, timeout: float | None = None) -> Resp:
        rid = self.next_id
        self.next_id += 1
        msg = json.dumps({"id": rid, "op": op, "args": args or {}}, separators=(",", ":")) + "\n"
        t0 = time.perf_counter()
        try:
            self.p.stdin.write(msg.encode())
            self.p.stdin.flush()
        except Exception as exc:
            return Resp(False, error=f"transport: {exc!r}", latency_s=time.perf_counter() - t0)
        deadline = time.monotonic() + (timeout or self.timeout)
        while True:
            line = self._read_line(deadline)
            if line is None:
                return Resp(False, error="transport: no response (timeout or process exited)", latency_s=time.perf_counter() - t0)
            lat = time.perf_counter() - t0
            try:
                obj = json.loads(line)
            except ValueError:
                continue  # stray non-protocol output
            if obj.get("id") != rid:
                continue  # event / late reply
            if obj.get("ok"):
                return Resp(True, result=obj.get("result"), latency_s=lat)
            return Resp(False, error=str(obj.get("error", "")), latency_s=lat)
