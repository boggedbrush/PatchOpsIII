#!/usr/bin/env python3
"""Check that the native GPUI app maps a window on a Wayland compositor and exits cleanly.

By default a private headless compositor is started (weston --backend=headless,
or kwin_wayland --virtual) on its own socket, so no desktop session is needed:

    python scripts/smoke_gpui_wayland.py --executable path/to/patchopsiii-gpui

Use --compositor existing to run against the current session's WAYLAND_DISPLAY
(a window will briefly appear). GPUI draws through Vulkan; on machines without
a GPU install mesa-vulkan-drivers (lavapipe).

The app runs with PATCHOPSIII_GPUI_SMOKE_EXIT=1, which closes the window one
second after the first frame through the normal window-close path. The check
passes when

  * GPUI reports the Wayland platform (and not X11: DISPLAY is removed),
  * the compositor configured the surface and the client acked it
    (xdg_toplevel configure / xdg_surface ack_configure in WAYLAND_DEBUG),
  * the window title was set and the first frame was rendered, and
  * the process exits with status 0.

No game mutations are requested; the app gets a throw-away data directory.
"""
import argparse
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time

SOCKET = "patchops-smoke-wl"
FIRST_FRAME = "first frame: window mapped"
APP_TIMEOUT = 60


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def compositor_command(kind):
    """Command line for a headless compositor listening on SOCKET."""
    if kind == "weston":
        return ["weston", "--backend=headless", f"--socket={SOCKET}", "--idle-time=0", "--width=1280", "--height=900"]
    if kind == "kwin":
        return ["kwin_wayland", "--virtual", f"--socket={SOCKET}", "--no-lockscreen", "--width=1280", "--height=900"]
    raise ValueError(kind)


def pick_compositor(requested):
    if requested != "auto":
        return requested
    for kind, binary in (("weston", "weston"), ("kwin", "kwin_wayland")):
        if shutil.which(binary):
            return kind
    raise RuntimeError("No headless Wayland compositor found: install weston (or pass --compositor existing)")


def wait_for_socket(runtime, compositor, timeout=20):
    path = runtime / SOCKET
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if compositor.poll() is not None:
            raise RuntimeError(f"Wayland compositor exited early with {compositor.returncode}")
        if path.exists():
            time.sleep(0.5)
            return
        time.sleep(0.1)
    raise RuntimeError(f"Wayland socket {path} did not appear")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--executable", type=Path, required=True)
    parser.add_argument("--compositor", choices=["auto", "weston", "kwin", "existing"], default="auto")
    parser.add_argument("--timeout", type=int, default=APP_TIMEOUT)
    parser.add_argument("--keep-log", type=Path, help="Write the app's stderr (incl. the Wayland protocol trace) here")
    args = parser.parse_args()
    owned = []
    with tempfile.TemporaryDirectory(prefix="patchops-wayland-smoke-") as directory:
        temporary = Path(directory)
        data = temporary / "data"
        data.mkdir()
        env = {key: value for key, value in os.environ.items() if key not in ("DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET")}
        env.update(
            {
                "PATCHOPSIII_DATA_DIR": str(data),
                "PATCHOPSIII_GPUI_SMOKE_EXIT": "1",
                "RUST_LOG": "info",
                "WAYLAND_DEBUG": "client",
            }
        )
        try:
            if args.compositor == "existing":
                display = os.environ.get("WAYLAND_DISPLAY")
                if not display:
                    raise RuntimeError("--compositor existing needs WAYLAND_DISPLAY")
                env["WAYLAND_DISPLAY"] = display
            else:
                kind = pick_compositor(args.compositor)
                runtime = temporary / "run"
                runtime.mkdir(mode=0o700)
                env["XDG_RUNTIME_DIR"] = str(runtime)
                env["WAYLAND_DISPLAY"] = SOCKET
                compositor_env = {key: value for key, value in env.items() if key != "WAYLAND_DEBUG"}
                compositor_env.update({"WLR_BACKENDS": "headless", "QT_QPA_PLATFORM": "offscreen"})
                log = (temporary / "compositor.log").open("w")
                compositor = subprocess.Popen(compositor_command(kind), env=compositor_env, stdout=log, stderr=subprocess.STDOUT)
                owned.append(compositor)
                try:
                    wait_for_socket(runtime, compositor)
                except RuntimeError:
                    print((temporary / "compositor.log").read_text(errors="replace"))
                    raise
                print(f"Started headless {kind} on {SOCKET}")
            app = subprocess.run(
                [str(args.executable.resolve())],
                env=env,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                errors="replace",
                timeout=args.timeout,
            )
        finally:
            for process in reversed(owned):
                stop(process)
    trace = app.stderr
    if args.keep_log:
        args.keep_log.write_text(trace)
    lines = trace.splitlines()
    report = [line for line in lines if "patchopsiii_gpui::ui::chrome" in line]
    for line in report:
        print(line)

    def require(condition, message):
        if not condition:
            tail = "\n".join(line for line in lines if "WAYLAND_DEBUG" not in line and not line.startswith("[") or "ERROR" in line)[-4000:]
            raise RuntimeError(f"{message}\n--- app log tail ---\n{tail}")

    require(any("compositor: Wayland" in line for line in report), "GPUI did not choose the Wayland platform")
    # libwayland prints events as "xdg_toplevel#23.configure(...)" and requests with a leading "->".
    require(any(re.search(r"xdg_toplevel#\d+\.configure\(", line) and "->" not in line for line in lines), "The compositor never configured the toplevel")
    require(any("ack_configure" in line for line in lines), "The client never acked a configure")
    require(any("set_title" in line and "PatchOpsIII" in line for line in lines), "The window title was not set")
    decoration = [line for line in lines if re.search(r"zxdg_toplevel_decoration_v1#\d+\.configure\(", line)]
    print(f"xdg-decoration configure events: {len(decoration)} (mode 1 = client-side, 2 = server-side)")
    require(any(FIRST_FRAME in line for line in report), "GPUI never rendered a first frame")
    require(app.returncode == 0, f"GPUI exited with status {app.returncode}")
    print("GPUI native Wayland window mapped, rendered a frame and closed cleanly")


if __name__ == "__main__":
    main()
