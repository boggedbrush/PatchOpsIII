#!/usr/bin/env python3
"""Build or run the single-process Rust/GPUI desktop app."""
from __future__ import annotations

import argparse
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
WORKSPACE = ROOT / "native"
MANIFEST = WORKSPACE / "Cargo.toml"


def target_directory() -> Path:
    target = Path(os.environ.get("CARGO_TARGET_DIR", WORKSPACE / "target"))
    return target if target.is_absolute() else ROOT / target


def stop(process: subprocess.Popen) -> None:
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def run(executable: Path) -> int:
    if not executable.is_file():
        raise RuntimeError(f"GPUI executable not found: {executable}. Run the build command first.")
    app = subprocess.Popen([str(executable)], cwd=ROOT)
    try:
        return app.wait()
    finally:
        stop(app)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["build", "run", "test", "check"])
    parser.add_argument("--release", action="store_true")
    parser.add_argument("--executable", type=Path, help="Run a prebuilt GPUI executable")
    args = parser.parse_args()
    if args.command in {"build", "test", "check"}:
        command = ["cargo", args.command, "--locked", "--manifest-path", str(MANIFEST)]
        command += ["--workspace"] if args.command == "test" else ["-p", "patchopsiii-gpui"]
        if args.release:
            command.append("--release")
        env = {**os.environ, "CARGO_TARGET_DIR": str(target_directory())}
        return subprocess.call(command, cwd=WORKSPACE, env=env)
    executable = args.executable or target_directory() / ("release" if args.release else "debug") / ("patchopsiii-gpui.exe" if os.name == "nt" else "patchopsiii-gpui")
    return run(executable.resolve())


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        raise SystemExit(130)
    except (OSError, RuntimeError) as error:
        print(f"GPUI: {error}", file=sys.stderr)
        raise SystemExit(1)
