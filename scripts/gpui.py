#!/usr/bin/env python3
"""Build/run the GPUI evaluation without changing Electron's processes or ports."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
from urllib.request import ProxyHandler, build_opener

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "native" / "gpui" / "Cargo.toml"


def target_directory() -> Path:
    target = Path(os.environ.get("CARGO_TARGET_DIR", MANIFEST.parent / "target"))
    return target if target.is_absolute() else ROOT / target


def stop(process: subprocess.Popen) -> None:
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def serve(ready: Path) -> None:
    # Bind once and hand the listening socket directly to Uvicorn. No port race,
    # no collision with Electron, and no need to modify the normal API launcher.
    import uvicorn
    from backend.api import app

    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(("127.0.0.1", 0))
        listener.listen(128)
        port = listener.getsockname()[1]
        temporary = ready.with_suffix(".tmp")
        temporary.write_text(json.dumps({"url": f"http://127.0.0.1:{port}"}), encoding="utf-8")
        temporary.replace(ready)
        server = uvicorn.Server(uvicorn.Config(app, host="127.0.0.1", port=port, log_level="warning"))
        server.run(sockets=[listener])


def wait_ready(process: subprocess.Popen, ready: Path, timeout: float = 30) -> str:
    deadline = time.monotonic() + timeout
    opener = build_opener(ProxyHandler({}))
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"Local service exited with code {process.returncode}")
        if ready.exists():
            url = json.loads(ready.read_text(encoding="utf-8"))["url"]
            try:
                with opener.open(f"{url}/api/health", timeout=1) as response:
                    if json.load(response).get("ok") is True:
                        return url
            except OSError:
                pass
        time.sleep(0.1)
    raise RuntimeError("Local service did not become healthy within 30 seconds")


def run(executable: Path) -> int:
    if not executable.is_file():
        raise RuntimeError(f"GPUI executable not found: {executable}. Run the build command first.")
    with tempfile.TemporaryDirectory(prefix="patchops-gpui-") as directory:
        ready = Path(directory) / "ready.json"
        # Source execution needs the repository root on Python's import path.
        env = {**os.environ, "PYTHONPATH": str(ROOT)}
        service = subprocess.Popen([sys.executable, str(Path(__file__).resolve()), "serve", "--ready", str(ready)], cwd=ROOT, env=env)
        app = None
        try:
            url = wait_ready(service, ready)
            app = subprocess.Popen([str(executable)], cwd=ROOT, env={**env, "PATCHOPSIII_GPUI_BACKEND_URL": url})
            while app.poll() is None:
                if service.poll() is not None:
                    raise RuntimeError("The local service stopped while GPUI was running")
                time.sleep(0.2)
            return app.returncode
        finally:
            if app is not None:
                stop(app)
            stop(service)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["build", "run", "serve", "test", "check"])
    parser.add_argument("--release", action="store_true")
    parser.add_argument("--executable", type=Path, help="Run a prebuilt GPUI executable")
    parser.add_argument("--ready", type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.command == "serve":
        if args.ready is None:
            parser.error("serve requires --ready")
        # When this file is run directly, sys.path starts at scripts/.
        sys.path.insert(0, str(ROOT))
        serve(args.ready)
        return 0
    if args.command in {"build", "test", "check"}:
        command = ["cargo", args.command, "--locked", "--manifest-path", str(MANIFEST)]
        if args.release:
            command.append("--release")
        env = {**os.environ, "CARGO_TARGET_DIR": str(target_directory())}
        return subprocess.call(command, cwd=MANIFEST.parent, env=env)
    target = target_directory()
    executable = args.executable or target / ("release" if args.release else "debug") / ("patchopsiii-gpui.exe" if os.name == "nt" else "patchopsiii-gpui")
    return run(executable.resolve())


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        raise SystemExit(130)
    except (OSError, RuntimeError) as error:
        print(f"GPUI: {error}", file=sys.stderr)
        raise SystemExit(1)
