from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any


def _resolve_app_root() -> Path:
    for env_name in ("PATCHOPSIII_RESOURCE_DIR", "PATCHOPSIII_APP_ROOT"):
        value = os.environ.get(env_name, "").strip()
        if value:
            path = Path(value).resolve()
            if path.exists():
                for candidate in (path, path / "_up_"):
                    if (candidate / "patchops-core").exists() or (candidate / "patchops-core.exe").exists():
                        return candidate
                    try:
                        if any(candidate.glob("patchops-core*")):
                            return candidate
                    except OSError:
                        continue
                return path

    if getattr(sys, "frozen", False):
        executable = Path(sys.executable).resolve()
        candidates = [executable.parent]
        candidates.append(executable.parent / "_up_")
        candidates.extend(executable.parents)
        candidates.extend(parent / "_up_" for parent in executable.parents)
        for candidate in candidates:
            if (candidate / "patchops-core").exists() or (candidate / "patchops-core.exe").exists():
                return candidate
            try:
                if any(candidate.glob("patchops-core*")):
                    return candidate
            except OSError:
                continue
        return executable.parents[1]

    return Path(__file__).resolve().parents[1]


APP_ROOT = _resolve_app_root()


class CoreBridgeError(RuntimeError):
    pass


class CoreUnavailableError(CoreBridgeError):
    pass


def _binary_name() -> str:
    return "patchops-core.exe" if os.name == "nt" else "patchops-core"


def _candidate_paths() -> list[Path]:
    name = _binary_name()
    candidates: list[Path] = []

    override = os.environ.get("PATCHOPSIII_CORE_BINARY", "").strip()
    if override:
        candidates.append(Path(override))

    candidates.extend(
        [
            APP_ROOT / "target" / "release" / name,
            APP_ROOT / "target" / "debug" / name,
            APP_ROOT / "crates" / "patchops-core" / "target" / "release" / name,
            APP_ROOT / "crates" / "patchops-core" / "target" / "debug" / name,
            APP_ROOT / "backend-bin" / name,
            APP_ROOT / name,
            Path(sys.executable).resolve().parent / name,
        ]
    )

    for directory in (APP_ROOT / "src-tauri" / "binaries", Path(sys.executable).resolve().parent):
        try:
            candidates.extend(sorted(directory.glob(f"patchops-core*{'.exe' if os.name == 'nt' else ''}")))
        except OSError:
            pass

    return candidates


def _resolve_core_binary() -> Path | None:
    for candidate in _candidate_paths():
        try:
            if candidate.is_file():
                return candidate
        except OSError:
            continue
    return None


def _hidden_subprocess_kwargs() -> dict[str, Any]:
    if os.name != "nt":
        return {}
    return {"creationflags": subprocess.CREATE_NO_WINDOW}


def core_available() -> bool:
    return _resolve_core_binary() is not None


def _run_core(binary: Path, command: str, payload: dict[str, Any], timeout: int) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(binary), command],
        input=json.dumps(payload),
        text=True,
        capture_output=True,
        timeout=timeout,
        check=False,
        **_hidden_subprocess_kwargs(),
    )


def core_call(command: str, payload: dict[str, Any], timeout: int = 60) -> dict[str, Any]:
    binary = _resolve_core_binary()
    if not binary:
        raise CoreUnavailableError("patchops-core binary was not found")

    try:
        result = _run_core(binary, command, payload, timeout)
    except subprocess.TimeoutExpired as exc:
        raise CoreBridgeError(f"patchops-core {command} timed out after {timeout}s") from exc
    except OSError as exc:
        raise CoreBridgeError(f"failed to start patchops-core: {exc}") from exc

    if result.returncode != 0:
        message = (result.stderr or result.stdout or f"patchops-core exited with code {result.returncode}").strip()
        raise CoreBridgeError(message)

    try:
        parsed = json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise CoreBridgeError("patchops-core returned invalid JSON") from exc

    if not isinstance(parsed, dict):
        raise CoreBridgeError("patchops-core returned a non-object JSON payload")
    return parsed
