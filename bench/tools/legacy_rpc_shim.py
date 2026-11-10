#!/usr/bin/env python3
"""Reference implementation of the gpui RPC contract on top of the *legacy* Python backend.

Used only to smoke-test the `gpui` driver end to end before the Rust binary exists:
  BENCH_SHIM_SRC=<legacy checkout> BENCH_SHIM_PY=<venv python> bench/tools/mock-patchops --rpc-stdio

It starts the FastAPI app in-process on an ephemeral port and translates JSON-lines requests to HTTP calls
(the same translation the legacy driver does), so its golden parity must be 100 %.
"""
import json
import os
import sys
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
SRC = os.environ["BENCH_SHIM_SRC"]
sys.path.insert(0, SRC)
os.chdir(SRC)

from benchlib.clients import LegacyHttpClient  # noqa: E402
from benchlib.proc import free_port  # noqa: E402
import uvicorn  # noqa: E402
from backend.api import app  # noqa: E402

port = free_port()
server = uvicorn.Server(uvicorn.Config(app, host="127.0.0.1", port=port, log_level="warning"))
threading.Thread(target=server.run, daemon=True).start()
while not server.started:
    time.sleep(0.01)
client = LegacyHttpClient("127.0.0.1", port)
out = sys.stdout
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        req = json.loads(line)
        resp = client.call(req["op"], req.get("args") or {})
        if resp.envelope_ok:
            msg = {"id": req.get("id"), "ok": True, "result": resp.result}
        else:
            msg = {"id": req.get("id"), "ok": False, "error": resp.error}
    except Exception as exc:  # malformed request
        msg = {"id": None, "ok": False, "error": f"validation: {exc}"}
    out.write(json.dumps(msg) + "\n")
    out.flush()
