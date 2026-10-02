#!/usr/bin/env python3
"""Render a native window and close it through X11 against a read-only fake API.

Requires an X11 DISPLAY, xdotool, and a compositor/window manager. CI uses
xvfb-run with --start-session. No real game service or game mutations are used.
"""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import threading
import time


class ReadOnlyStatus(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path != "/api/status":
            self.send_error(404)
            return
        payload = json.dumps({"appVersion": "GPUI smoke", "platform": "Linux", "gameDetected": False, "gameDir": None, "launchProfiles": [], "logs": []}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def do_POST(self):
        self.send_error(405, "Smoke service is read-only")

    def log_message(self, *_):
        pass


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--executable", type=Path, required=True)
    parser.add_argument("--start-session", action="store_true")
    args = parser.parse_args()
    owned = []
    with ThreadingHTTPServer(("127.0.0.1", 0), ReadOnlyStatus) as server:
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            if args.start_session:
                owned.append(subprocess.Popen(["openbox", "--sm-disable"]))
                owned.append(subprocess.Popen(["xcompmgr"]))
                time.sleep(1)
            env = {**os.environ, "PATCHOPSIII_GPUI_BACKEND_URL": f"http://127.0.0.1:{server.server_port}"}
            app = subprocess.Popen([str(args.executable.resolve())], env=env)
            owned.append(app)
            deadline = time.monotonic() + 30
            window = None
            while time.monotonic() < deadline:
                if app.poll() is not None:
                    raise RuntimeError(f"GPUI exited during startup: {app.returncode}")
                result = subprocess.run(["xdotool", "search", "--onlyvisible", "--pid", str(app.pid), "--name", "PatchOpsIII"], capture_output=True, text=True)
                if result.returncode == 0 and result.stdout.strip():
                    window = result.stdout.splitlines()[0]
                    break
                time.sleep(0.1)
            if window is None:
                raise RuntimeError("GPUI did not map its native window")
            time.sleep(1)
            # Close this exact owned window through its WM keyboard shortcut.
            # This reproduces the platform callback path that TestAppContext
            # cannot exercise; direct XDestroyWindow is not normal app closure.
            subprocess.run(["xdotool", "windowactivate", "--sync", window, "key", "alt+F4"], check=True, timeout=5)
            result = app.wait(timeout=10)
            if result != 0:
                raise RuntimeError(f"Native window close failed with exit code {result}")
            print("GPUI native window startup and normal X11 close passed")
        finally:
            for process in reversed(owned):
                stop(process)
            server.shutdown()
            thread.join(timeout=5)


if __name__ == "__main__":
    main()
