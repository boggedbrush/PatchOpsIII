#!/usr/bin/env python3
"""Check in-process status and native X11 window close against a disposable game.

Requires DISPLAY, xdotool, and a window manager for the window check. Use
--status-only to verify the same archive binary without desktop automation.
No game mutations or external backend service are requested.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time


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
    parser.add_argument("--status-only", action="store_true")
    args = parser.parse_args()
    owned = []
    with tempfile.TemporaryDirectory(prefix="patchops-native-smoke-") as directory:
        temporary = Path(directory)
        game = temporary / "game"
        (game / "players").mkdir(parents=True)
        (game / "BlackOps3.exe").write_bytes(b"disposable smoke executable")
        (game / "players" / "config.ini").write_text('FOV = "90"\n', encoding="utf-8")
        data = temporary / "data"
        data.mkdir()
        (data / "electron-settings.json").write_text(json.dumps({"game_dir": str(game)}), encoding="utf-8")
        env = {**os.environ, "PATCHOPSIII_DATA_DIR": str(data)}
        # A native status request needs neither Python nor an HTTP listener.
        executable = str(args.executable.resolve())
        status_result = subprocess.run([executable, "--smoke-status"], env=env, capture_output=True, text=True, check=True, timeout=30)
        status = json.loads(status_result.stdout)
        if not status.get("gameDetected") or Path(status["gameDir"]) != game or status["graphics"]["fov"] != 90:
            raise RuntimeError(f"Native core failed to load disposable game status: {status}")
        print("GPUI in-process Rust status loaded from disposable game")
        if args.status_only:
            return
        try:
            if args.start_session:
                owned.append(subprocess.Popen(["openbox", "--sm-disable"]))
                owned.append(subprocess.Popen(["xcompmgr"]))
                time.sleep(1)
            app = subprocess.Popen([executable], env=env)
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
            # Normal window close exercises the real platform callback.
            subprocess.run(["xdotool", "windowactivate", "--sync", window, "key", "alt+F4"], check=True, timeout=5)
            result = app.wait(timeout=10)
            if result != 0:
                raise RuntimeError(f"Native window close failed with exit code {result}")
            print("GPUI native window startup and normal X11 close passed")
        finally:
            for process in reversed(owned):
                stop(process)


if __name__ == "__main__":
    main()
