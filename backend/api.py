#!/usr/bin/env python
from __future__ import annotations

import asyncio
import json
import os
import platform
import re
import shutil
import stat
import string
import subprocess
import sys
import zipfile
from collections import deque
from pathlib import Path
from typing import Any

import requests
from fastapi import FastAPI, WebSocket, WebSocketDisconnect
from fastapi.concurrency import run_in_threadpool
from fastapi.middleware.cors import CORSMiddleware
from pydantic import BaseModel, Field

from utils import (
    _read_launch_options,
    app_id,
    apply_launch_options,
    clear_log_file,
    cleanup_bo3_enhanced_linux,
    configure_bo3_enhanced_linux,
    existing_backup_path,
    file_sha256,
    find_steam_user_id,
    get_app_data_dir,
    get_log_file_path,
    get_steam_library_paths,
    get_workshop_item_state,
    LEGACY_BACKUP_SUFFIX,
    launch_game_via_steam,
    PATCHOPS_BACKUP_SUFFIX,
    patchops_backup_path,
    read_exe_variant,
    write_exe_variant,
    write_log,
)
from version import APP_VERSION
from bo3_enhanced import (
    detect_enhanced_install,
    download_latest_enhanced,
    install_enhanced_files,
    status_summary,
    uninstall_enhanced_files,
    validate_dump_source,
)
from t7_patch import (
    T7PATCH_ASSETS,
    T7PATCH_LEGACY_ONLY_FILES,
    T7PATCH_PROFILE_CURRENT,
    T7PATCH_PROFILES,
    _expected_asset_sha256,
    add_defender_exclusion,
    backup_lpc_files,
    check_t7_patch_status,
    describe_t7_patch_target,
    download_file,
    install_lpc_files,
    is_admin,
    is_t7_patch_installed,
    restore_lpc_backups,
    update_t7patch_conf,
)
from dxvk_manager import (
    DXVK_ASYNC_FILES,
    _build_dxvk_conf,
    _preset_settings,
    _supports_gpl_async_cache,
    extract_archive,
    get_download_url,
    get_latest_release,
    is_dxvk_async_installed,
)


APP_ROOT = Path(sys.executable).resolve().parents[1] if getattr(sys, "frozen", False) else Path(__file__).resolve().parents[1]
SETTINGS_PATH = Path(get_app_data_dir()) / "electron-settings.json"
PRESETS_PATH = APP_ROOT / "presets.json"
MOD_FILES_DIR = Path(get_app_data_dir()) / "BO3 Mod Files"
GAME_EXECUTABLE_NAMES = ("BlackOpsIII.exe", "BlackOps3.exe")
T7_GAME_FILES = ("t7patch.dll", "t7patch.conf", "discord_game_sdk.dll", "dsound.dll", "t7patchloader.dll", "zbr2.dll")
WORKSHOP_PROFILES = {
    "all_around": {
        "name": "All-around Enhancement Lite",
        "workshop_id": "2994481309",
        "launch_option": "+set fs_game 2994481309",
    },
    "ultimate": {
        "name": "Ultimate Experience Mod",
        "workshop_id": "2942053577",
        "launch_option": "+set fs_game 2942053577",
    },
}
GITHUB_LATEST_RELEASE_URL = "https://api.github.com/repos/boggedbrush/PatchOpsIII/releases/latest"
GITHUB_RELEASE_PAGE_URL = "https://github.com/boggedbrush/PatchOpsIII/releases/latest"
SUPPORTED_LAUNCH_OPTIONS = {"", "+set fs_game offlinemp"} | {
    profile["launch_option"] for profile in WORKSHOP_PROFILES.values()
}


class LogBus:
    def __init__(self) -> None:
        self._clients: set[WebSocket] = set()
        self._recent: deque[dict[str, str]] = deque(maxlen=300)

    @property
    def recent(self) -> list[dict[str, str]]:
        return list(self._recent)

    async def connect(self, websocket: WebSocket) -> None:
        await websocket.accept()
        self._clients.add(websocket)
        for entry in self._recent:
            await websocket.send_json({"type": "log", "payload": entry})

    def disconnect(self, websocket: WebSocket) -> None:
        self._clients.discard(websocket)

    async def publish(self, entry: dict[str, str]) -> None:
        self._recent.append(entry)
        stale: list[WebSocket] = []
        for websocket in self._clients:
            try:
                await websocket.send_json({"type": "log", "payload": entry})
            except Exception:
                stale.append(websocket)
        for websocket in stale:
            self.disconnect(websocket)


class ApiLogTarget:
    def __init__(self, bus: LogBus) -> None:
        self.bus = bus
        self.loop: asyncio.AbstractEventLoop | None = None

    def set_loop(self, loop: asyncio.AbstractEventLoop) -> None:
        self.loop = loop

    def handle_write_log(self, *, full_message: str, category: str, plain_message: str, **_: str) -> None:
        entry = {"message": plain_message, "category": category, "line": full_message}
        try:
            loop = asyncio.get_running_loop()
            loop.create_task(self.bus.publish(entry))
        except RuntimeError:
            if self.loop and self.loop.is_running():
                self.loop.call_soon_threadsafe(lambda: self.loop and self.loop.create_task(self.bus.publish(entry)))
            else:
                self.bus._recent.append(entry)


log_bus = LogBus()
log_target = ApiLogTarget(log_bus)
app = FastAPI(title="PatchOpsIII Local API", version=APP_VERSION)


def _cors_origins() -> list[str]:
    defaults = [
        "http://localhost:5173",
        "http://127.0.0.1:5173",
        "http://localhost:5174",
        "http://127.0.0.1:5174",
        "app://patchopsiii",
    ]
    extra = [origin.strip() for origin in os.environ.get("PATCHOPSIII_CORS_ORIGINS", "").split(",") if origin.strip()]
    return [*defaults, *extra]


app.add_middleware(
    CORSMiddleware,
    allow_origins=_cors_origins(),
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)


class GameDirectoryPayload(BaseModel):
    path: str = Field(..., min_length=1)


class LaunchOptionsPayload(BaseModel):
    options: str = ""
    preserve_fs_game: bool = False


class WorkshopInstallPayload(BaseModel):
    profileId: str = Field(..., min_length=1)


class ConfigValuePayload(BaseModel):
    key: str = Field(..., min_length=1)
    value: str | int | float | bool
    comment: str = "Managed by PatchOpsIII"


class TogglePayload(BaseModel):
    enabled: bool


class VramPayload(BaseModel):
    limited: bool
    target: int = Field(75, ge=75, le=100)


class T7ConfigPayload(BaseModel):
    gamertag: str | None = None
    colorCode: str = ""
    networkPassword: str | None = None
    friendsOnly: bool | None = None


class EnhancedInstallPayload(BaseModel):
    dumpSource: str = Field(..., min_length=1)


class DxvkConfigPayload(BaseModel):
    enableAsync: bool = True
    gplAsyncCache: bool = True
    numCompilerThreads: int = Field(0, ge=0, le=64)
    maxFrameRate: int = Field(0, ge=0, le=360)
    maxFrameLatency: int = Field(1, ge=0, le=16)
    tearFree: str = "True"
    hudEnabled: bool = False


class PresetPayload(BaseModel):
    name: str


_settings_cache: tuple[float | None, dict[str, Any]] | None = None
_game_dir_cache: tuple[float | None, str | None] | None = None
_preset_names_cache: tuple[float | None, list[str]] | None = None
_presets_cache: tuple[float | None, dict[str, Any]] | None = None


def _file_mtime(path: Path) -> float | None:
    try:
        return path.stat().st_mtime
    except OSError:
        return None


async def _blocking(func, /, *args, **kwargs):
    return await run_in_threadpool(func, *args, **kwargs)


async def _current_state_async() -> dict[str, Any]:
    return await _blocking(_current_state)


async def _find_game_directory_async() -> str | None:
    return await _blocking(_find_game_directory)


async def _config_path_async(game_dir: str | None = None) -> Path | None:
    return await _blocking(_config_path, game_dir)


def _load_settings() -> dict[str, Any]:
    global _settings_cache
    mtime = _file_mtime(SETTINGS_PATH)
    if _settings_cache and _settings_cache[0] == mtime:
        return dict(_settings_cache[1])
    try:
        loaded = json.loads(SETTINGS_PATH.read_text(encoding="utf-8"))
        settings = loaded if isinstance(loaded, dict) else {}
    except Exception:
        settings = {}
    _settings_cache = (mtime, dict(settings))
    return settings


def _save_settings(settings: dict[str, Any]) -> None:
    global _settings_cache, _game_dir_cache
    SETTINGS_PATH.parent.mkdir(parents=True, exist_ok=True)
    SETTINGS_PATH.write_text(json.dumps(settings, indent=2), encoding="utf-8")
    _settings_cache = (_file_mtime(SETTINGS_PATH), dict(settings))
    _game_dir_cache = None


def _has_game_executable(directory: str | Path | None) -> bool:
    if not directory:
        return False
    path = Path(directory)
    return path.is_dir() and any((path / name).exists() for name in GAME_EXECUTABLE_NAMES)


def _find_game_directory() -> str | None:
    global _game_dir_cache
    settings_mtime = _file_mtime(SETTINGS_PATH)
    if _game_dir_cache and _game_dir_cache[0] == settings_mtime:
        cached = _game_dir_cache[1]
        if cached is None or _has_game_executable(cached):
            return cached
        _game_dir_cache = None

    saved = _load_settings().get("game_dir")
    if isinstance(saved, str) and _has_game_executable(saved):
        found = str(Path(saved))
        _game_dir_cache = (settings_mtime, found)
        return found

    for library in get_steam_library_paths():
        candidate = Path(library) / "steamapps" / "common" / "Call of Duty Black Ops III"
        if _has_game_executable(candidate):
            found = str(candidate)
            _game_dir_cache = (settings_mtime, found)
            return found

    system = platform.system()
    home = Path.home()
    candidates = []
    if system == "Windows":
        candidates.extend(
            [
                Path(os.environ.get("PROGRAMFILES(X86)", r"C:\Program Files (x86)")) / "Steam/steamapps/common/Call of Duty Black Ops III",
                Path(os.environ.get("PROGRAMFILES", r"C:\Program Files")) / "Steam/steamapps/common/Call of Duty Black Ops III",
            ]
        )
    elif system == "Darwin":
        candidates.append(home / "Library/Application Support/Steam/steamapps/common/Call of Duty Black Ops III")
    else:
        candidates.extend(
            [
                home / ".steam/steam/steamapps/common/Call of Duty Black Ops III",
                home / ".local/share/Steam/steamapps/common/Call of Duty Black Ops III",
            ]
        )
    for candidate in candidates:
        if _has_game_executable(candidate):
            found = str(candidate)
            _game_dir_cache = (settings_mtime, found)
            return found
    _game_dir_cache = (settings_mtime, None)
    return None


def _config_path(game_dir: str | None = None) -> Path | None:
    directory = game_dir or _find_game_directory()
    if not directory:
        return None
    return Path(directory) / "players" / "config.ini"


def _read_config(game_dir: str | None = None) -> str:
    path = _config_path(game_dir)
    if not path or not path.exists():
        return ""
    try:
        return path.read_text(encoding="utf-8", errors="ignore")
    except Exception:
        return ""


def _extract_config(content: str, key: str, default: Any) -> Any:
    match = re.search(rf'^\s*{re.escape(key)}\s*=\s*"([^"]+)"', content, re.MULTILINE)
    return match.group(1) if match else default


def _config_int(content: str, key: str, default: int) -> int:
    try:
        return int(float(_extract_config(content, key, default)))
    except (TypeError, ValueError):
        return default


def _config_float(content: str, key: str, default: float) -> float:
    try:
        return float(_extract_config(content, key, default))
    except (TypeError, ValueError):
        return default


def _config_bool(content: str, key: str, enabled_value: str = "1", default: str = "0") -> bool:
    return str(_extract_config(content, key, default)) == enabled_value


def _write_config_value(game_dir: str, key: str, value: str | int | float | bool, comment: str) -> None:
    _write_config_values(game_dir, [(key, value, comment)])
    write_log(f"Set {key} to {value}.", "Success", log_target)


def _write_config_values(game_dir: str, updates: list[tuple[str, str | int | float | bool, str]]) -> None:
    config = _config_path(game_dir)
    if not config or not config.exists():
        raise FileNotFoundError(f"config.ini not found at {config}")
    text = config.read_text(encoding="utf-8", errors="ignore")
    for key, value, comment in updates:
        replacement = f'{key} = "{value}" // {comment}'
        pattern = re.compile(rf'^\s*{re.escape(key)}\s*=.*$', re.MULTILINE)
        if pattern.search(text):
            text = pattern.sub(replacement, text)
        else:
            text = f"{text.rstrip()}\n{replacement}\n"
    config.write_text(text, encoding="utf-8")


def _config_is_readonly(game_dir: str | None) -> bool:
    config = _config_path(game_dir)
    return bool(config and config.exists() and not os.access(config, os.W_OK))


def _preset_names() -> list[str]:
    global _preset_names_cache
    mtime = _file_mtime(PRESETS_PATH)
    if _preset_names_cache and _preset_names_cache[0] == mtime:
        return list(_preset_names_cache[1])
    try:
        data = _load_presets()
    except Exception:
        return []
    names = list(data.keys()) if isinstance(data, dict) else []
    _preset_names_cache = (mtime, list(names))
    return names


def _load_presets() -> dict[str, Any]:
    global _presets_cache
    mtime = _file_mtime(PRESETS_PATH)
    if _presets_cache and _presets_cache[0] == mtime:
        return dict(_presets_cache[1])
    data = json.loads(PRESETS_PATH.read_text(encoding="utf-8"))
    presets = data if isinstance(data, dict) else {}
    _presets_cache = (mtime, dict(presets))
    return presets


def _qol_status(game_dir: str | None) -> dict[str, bool]:
    if not game_dir:
        return {
            "d3dcompiler": False,
            "intro": False,
            "allIntros": False,
        }

    game_path = Path(game_dir)
    video_dir = game_path / "video"
    intro = video_dir / "BO3_Global_Logo_LogoSequence.mkv"
    dll = game_path / "d3dcompiler_46.dll"
    all_intros = False

    if video_dir.exists():
        try:
            mkv_files = [item for item in video_dir.iterdir() if item.name.endswith(".mkv")]
            backup_files = [
                item
                for item in video_dir.iterdir()
                if item.name.endswith(f".mkv{PATCHOPS_BACKUP_SUFFIX}") or item.name.endswith(f".mkv{LEGACY_BACKUP_SUFFIX}")
            ]
            all_intros = len(backup_files) > 0 and len(mkv_files) == 0
        except OSError:
            all_intros = False

    return {
        "d3dcompiler": existing_backup_path(str(dll)) is not None,
        "intro": existing_backup_path(str(intro)) is not None,
        "allIntros": all_intros,
    }


def _find_bo3_executable(game_dir: str | None) -> Path | None:
    if not game_dir:
        return None
    for name in GAME_EXECUTABLE_NAMES:
        candidate = Path(game_dir) / name
        if candidate.exists():
            return candidate
    return None


def _t7_mode(game_dir: str | None) -> str:
    if not game_dir:
        return "Unknown"
    return describe_t7_patch_target(game_dir)["display_label"]


def _t7_status(game_dir: str | None) -> dict[str, Any]:
    if not game_dir:
        return {
            "installed": False,
            "confExists": False,
            "gamertag": "",
            "plainName": "",
            "colorCode": "",
            "networkPassword": "",
            "friendsOnly": False,
            "mode": "Unknown",
        }

    patch_status = check_t7_patch_status(game_dir)
    return {
        "installed": is_t7_patch_installed(game_dir),
        "confExists": (Path(game_dir) / "t7patch.conf").exists(),
        "gamertag": patch_status.get("gamertag", ""),
        "plainName": patch_status.get("plain_name", ""),
        "colorCode": patch_status.get("color_code", ""),
        "networkPassword": patch_status.get("password", ""),
        "friendsOnly": bool(patch_status.get("friends_only")),
        "mode": _t7_mode(game_dir),
    }


def _enhanced_status(game_dir: str | None) -> dict[str, Any]:
    if not game_dir:
        return {
            "installed": False,
            "detectedAt": None,
            "acknowledgedAt": None,
            "launchOptionsActive": False,
            "dumpSource": _load_settings().get("enhanced_dump_source", ""),
        }

    summary = status_summary(game_dir, get_app_data_dir())
    current_options = _current_launch_options() or ""
    return {
        "installed": bool(summary.get("installed")),
        "detectedAt": summary.get("detected_at"),
        "acknowledgedAt": summary.get("acknowledged_at"),
        "launchOptionsActive": "windowscodecs=n,b" in current_options.lower(),
        "dumpSource": _load_settings().get("enhanced_dump_source", ""),
    }


def _read_dxvk_settings(game_dir: str | None) -> dict[str, Any]:
    settings = _preset_settings("recommended")
    if not game_dir:
        return {
            "enableAsync": settings["enable_async"],
            "gplAsyncCache": settings["gpl_async_cache"],
            "numCompilerThreads": settings["num_compiler_threads"],
            "maxFrameRate": settings["max_frame_rate"],
            "maxFrameLatency": settings["max_frame_latency"],
            "tearFree": settings["tear_free"],
            "hudEnabled": settings["hud_enabled"],
        }

    conf = Path(game_dir) / "dxvk.conf"
    if conf.exists():
        try:
            for line in conf.read_text(encoding="utf-8", errors="ignore").splitlines():
                if "=" not in line or line.strip().startswith("#"):
                    continue
                key, value = [part.strip() for part in line.split("=", 1)]
                lowered = value.lower()
                if key == "dxvk.enableAsync":
                    settings["enable_async"] = lowered == "true"
                elif key == "dxvk.gplAsyncCache":
                    settings["gpl_async_cache"] = lowered == "true"
                elif key == "dxvk.numCompilerThreads":
                    settings["num_compiler_threads"] = int(value)
                elif key == "dxgi.maxFrameRate":
                    settings["max_frame_rate"] = int(value)
                elif key == "dxgi.maxFrameLatency":
                    settings["max_frame_latency"] = int(value)
                elif key == "dxvk.tearFree":
                    settings["tear_free"] = value
                elif key == "dxvk.hud":
                    settings["hud_enabled"] = bool(value)
        except Exception as exc:
            write_log(f"Failed to read dxvk.conf: {exc}", "Warning", log_target)

    return {
        "enableAsync": bool(settings["enable_async"]),
        "gplAsyncCache": bool(settings["gpl_async_cache"]),
        "numCompilerThreads": int(settings["num_compiler_threads"]),
        "maxFrameRate": int(settings["max_frame_rate"]),
        "maxFrameLatency": int(settings["max_frame_latency"]),
        "tearFree": str(settings["tear_free"]),
        "hudEnabled": bool(settings["hud_enabled"]),
    }


def _dxvk_payload_to_settings(payload: DxvkConfigPayload) -> dict[str, Any]:
    tear_free = payload.tearFree if payload.tearFree in {"Auto", "True", "False"} else "Auto"
    return {
        "enable_async": payload.enableAsync,
        "gpl_async_cache": payload.gplAsyncCache,
        "num_compiler_threads": payload.numCompilerThreads,
        "max_frame_rate": payload.maxFrameRate,
        "max_frame_latency": payload.maxFrameLatency,
        "tear_free": tear_free,
        "hud_enabled": payload.hudEnabled,
    }


def _write_dxvk_conf(game_dir: str, settings: dict[str, Any], include_gpl_async_cache: bool = True) -> None:
    conf = Path(game_dir) / "dxvk.conf"
    conf.write_text(_build_dxvk_conf(settings, include_gpl_async_cache=include_gpl_async_cache), encoding="utf-8")
    write_log("Updated dxvk.conf from DXVK settings.", "Success", log_target)


def _dxvk_status(game_dir: str | None) -> dict[str, Any]:
    return {
        "installed": bool(game_dir and is_dxvk_async_installed(game_dir)),
        "confExists": bool(game_dir and (Path(game_dir) / "dxvk.conf").exists()),
        "settings": _read_dxvk_settings(game_dir),
    }


def _install_dxvk(game_dir: str, payload: DxvkConfigPayload) -> None:
    MOD_FILES_DIR.mkdir(parents=True, exist_ok=True)
    release = get_latest_release()
    dxvk_url = get_download_url(release)
    archive_path = MOD_FILES_DIR / "dxvk-gplasync"
    extract_dir = MOD_FILES_DIR / "dxvk_extracted"

    if extract_dir.exists():
        shutil.rmtree(extract_dir)
    extract_dir.mkdir(parents=True, exist_ok=True)

    downloaded_archive: str | None = None
    try:
        write_log("Downloading DXVK-GPLAsync...", "Info", log_target)
        downloaded_archive = str(_download_file(dxvk_url, str(archive_path)))
        write_log("Downloaded DXVK-GPLAsync successfully.", "Success", log_target)
        extract_archive(downloaded_archive, str(extract_dir))
        write_log("Extracted DXVK-GPLAsync successfully.", "Success", log_target)

        source_dir: Path | None = None
        for root, _, files in os.walk(extract_dir):
            if all(filename in files for filename in DXVK_ASYNC_FILES):
                source_dir = Path(root)
                break
        if not source_dir:
            raise RuntimeError("Required DXVK files were not found in the archive.")

        for filename in DXVK_ASYNC_FILES:
            shutil.copy2(source_dir / filename, Path(game_dir) / filename)
            write_log(f"Installed {filename}.", "Success", log_target)

        settings = _dxvk_payload_to_settings(payload)
        _write_dxvk_conf(game_dir, settings, include_gpl_async_cache=_supports_gpl_async_cache(release))
        write_log("DXVK-GPLAsync installed successfully.", "Success", log_target)
    finally:
        if downloaded_archive and os.path.exists(downloaded_archive):
            try:
                os.remove(downloaded_archive)
            except OSError:
                pass
        if extract_dir.exists():
            shutil.rmtree(extract_dir, ignore_errors=True)


def _download_file(url: str, filename: str) -> str:
    with requests.get(url, stream=True, timeout=30) as response:
        response.raise_for_status()
        original_filename = os.path.basename(url.split("?", 1)[0])
        final_filename = os.path.join(os.path.dirname(filename), original_filename or os.path.basename(filename))
        with open(final_filename, "wb") as handle:
            for chunk in response.iter_content(chunk_size=8192):
                if chunk:
                    handle.write(chunk)
    return final_filename


def _uninstall_dxvk(game_dir: str) -> None:
    changed = False
    for filename in DXVK_ASYNC_FILES:
        target = Path(game_dir) / filename
        if target.exists():
            target.unlink()
            changed = True
            write_log(f"Removed {filename}.", "Success", log_target)
    conf = Path(game_dir) / "dxvk.conf"
    if conf.exists():
        conf.unlink()
        changed = True
        write_log("Removed dxvk.conf.", "Success", log_target)
    write_log("DXVK-GPLAsync has been uninstalled." if changed else "DXVK-GPLAsync was not installed.", "Success" if changed else "Info", log_target)


def _resolve_enhanced_linux_tool_source() -> str | None:
    candidates = [
        APP_ROOT / "bo3-enhanced-proton" / "BO3 Enhanced",
        Path(get_app_data_dir()) / "bo3-enhanced-proton-cache" / "BO3 Enhanced",
    ]
    for candidate in candidates:
        if candidate.is_dir():
            return str(candidate)
    return None


def _install_enhanced(game_dir: str, dump_source: str) -> None:
    normalized_source = str(Path(dump_source).expanduser())
    if not os.path.exists(normalized_source):
        raise FileNotFoundError("The selected dump source does not exist.")
    if not validate_dump_source(normalized_source):
        raise ValueError("The dump source is missing required files.")

    settings = _load_settings()
    settings["enhanced_dump_source"] = normalized_source
    _save_settings(settings)

    MOD_FILES_DIR.mkdir(parents=True, exist_ok=True)
    write_log("Fetching latest BO3 Enhanced release...", "Info", log_target)
    enhanced_path = download_latest_enhanced(str(MOD_FILES_DIR), get_app_data_dir())
    if not enhanced_path:
        raise RuntimeError("Failed to download BO3 Enhanced.")

    write_log("Installing BO3 Enhanced files...", "Info", log_target)
    installed = install_enhanced_files(game_dir, str(MOD_FILES_DIR), get_app_data_dir(), normalized_source, log_widget=log_target)
    if not installed:
        raise RuntimeError("BO3 Enhanced installation failed.")

    if platform.system() == "Linux":
        configured = configure_bo3_enhanced_linux(_resolve_enhanced_linux_tool_source(), get_app_data_dir(), log_target)
        if not configured:
            raise RuntimeError("Installed files, but Linux compatibility setup failed.")

    write_exe_variant(game_dir, "enhanced")
    write_log("Installed BO3 Enhanced successfully.", "Success", log_target)


def _uninstall_enhanced(game_dir: str) -> None:
    removed = uninstall_enhanced_files(game_dir, str(MOD_FILES_DIR), get_app_data_dir(), log_widget=log_target)
    if not removed:
        raise RuntimeError("BO3 Enhanced uninstall failed.")

    if platform.system() == "Linux":
        cleanup_ok = cleanup_bo3_enhanced_linux(log_target)
        if not cleanup_ok:
            raise RuntimeError("BO3 Enhanced files were removed, but Linux compatibility cleanup was incomplete.")

    write_exe_variant(game_dir, "default")
    write_log("Uninstalled BO3 Enhanced successfully.", "Success", log_target)


def _install_t7_patch(game_dir: str) -> None:
    if platform.system() == "Windows" and not is_admin():
        raise PermissionError("Run PatchOpsIII as administrator to install or update T7 Patch.")

    MOD_FILES_DIR.mkdir(parents=True, exist_ok=True)
    if platform.system() == "Windows":
        add_defender_exclusion(str(MOD_FILES_DIR), log_target)
        add_defender_exclusion(game_dir, log_target)

    target = describe_t7_patch_target(game_dir)
    profile = T7PATCH_PROFILES[target["profile"]]
    archive_asset_key = profile["archive_asset"]
    archive_asset = T7PATCH_ASSETS[archive_asset_key]
    zip_url = archive_asset["download_url"]
    zip_dest = MOD_FILES_DIR / "T7Patch.zip"
    source_dir = MOD_FILES_DIR / "linux"

    write_log(f'Detected {target["mode_label"]}. Installing {profile["patch_label"]}...', "Info", log_target)
    if zip_dest.exists():
        zip_dest.unlink()
    if source_dir.exists():
        shutil.rmtree(source_dir)

    expected_hashes = _expected_asset_sha256(archive_asset_key, log_target)
    if not expected_hashes:
        raise RuntimeError("No trusted SHA-256 available for T7 Patch archive.")
    download_file(zip_url, str(zip_dest), log_target, expected_sha256=expected_hashes)
    write_log("Downloaded T7 Patch successfully.", "Success", log_target)

    with zipfile.ZipFile(zip_dest, "r") as archive:
        archive.extractall(MOD_FILES_DIR)
    write_log("Extracted T7 Patch successfully.", "Success", log_target)

    if not source_dir.exists():
        raise RuntimeError("T7 Patch archive did not contain the expected linux folder.")

    for root, _, files in os.walk(source_dir):
        relative = os.path.relpath(root, source_dir)
        destination = Path(game_dir) if relative == "." else Path(game_dir) / relative
        destination.mkdir(parents=True, exist_ok=True)
        for filename in files:
            if filename.lower() == "t7patch.conf" and (destination / filename).exists():
                continue
            shutil.copy2(Path(root) / filename, destination / filename)

    if target["profile"] == T7PATCH_PROFILE_CURRENT:
        for filename in T7PATCH_LEGACY_ONLY_FILES:
            stale_path = Path(game_dir) / filename
            if stale_path.exists():
                stale_path.unlink()

    if not backup_lpc_files(game_dir, log_target):
        raise RuntimeError("Failed to back up LPC files.")
    if not install_lpc_files(game_dir, str(MOD_FILES_DIR), log_target):
        raise RuntimeError("Failed to install LPC files.")
    write_log("Installed T7 Patch successfully.", "Success", log_target)


def _uninstall_t7_patch(game_dir: str) -> None:
    removed_game = False
    for filename in T7_GAME_FILES:
        target = Path(game_dir) / filename
        if target.exists():
            target.unlink()
            removed_game = True
    if removed_game:
        write_log("Uninstalled T7 Patch files from the game directory.", "Success", log_target)

    linux_dir = MOD_FILES_DIR / "linux"
    removed_mod = False
    if linux_dir.exists():
        for filename in T7_GAME_FILES:
            target = linux_dir / filename
            if target.exists():
                target.unlink()
                removed_mod = True
        if linux_dir.exists() and not any(linux_dir.iterdir()):
            linux_dir.rmdir()

    zip_path = MOD_FILES_DIR / "T7Patch.zip"
    if zip_path.exists():
        zip_path.unlink()
        removed_mod = True

    if removed_mod:
        write_log("Removed cached T7 Patch files.", "Success", log_target)
    restore_lpc_backups(game_dir, log_target)
    write_log("T7 Patch has been completely uninstalled.", "Success", log_target)


def _current_launch_options() -> str | None:
    user_id = find_steam_user_id()
    if not user_id:
        return None
    return _read_launch_options(user_id, app_id)


def _version_parts(value: str) -> tuple[int, ...]:
    parts = [int(part) for part in re.split(r"[^0-9]+", value.lstrip("vV")) if part.isdigit()]
    return tuple(parts or [0])


def _select_update_asset(release_data: dict[str, Any]) -> dict[str, Any] | None:
    assets = release_data.get("assets") or []
    system = platform.system()
    asset_names = ("PatchOpsIII.exe",) if system == "Windows" else ("PatchOpsIII.AppImage",)
    for expected_name in asset_names:
        for asset in assets:
            name = str(asset.get("name") or "")
            url = asset.get("browser_download_url")
            if name.lower() == expected_name.lower() and url:
                return {
                    "name": name,
                    "url": url,
                    "size": asset.get("size") or 0,
                    "contentType": asset.get("content_type") or "application/octet-stream",
                }
    return None


def _check_for_update() -> dict[str, Any]:
    response = requests.get(GITHUB_LATEST_RELEASE_URL, timeout=30)
    response.raise_for_status()
    release_data = response.json()
    version = str(release_data.get("tag_name") or release_data.get("name") or "0.0.0")
    page_url = release_data.get("html_url") or GITHUB_RELEASE_PAGE_URL
    asset = _select_update_asset(release_data)
    available = bool(
        asset
        and not release_data.get("draft")
        and not release_data.get("prerelease")
        and _version_parts(version) > _version_parts(APP_VERSION)
    )
    return {
        "available": available,
        "currentVersion": APP_VERSION,
        "latestVersion": version,
        "name": release_data.get("name") or "PatchOpsIII",
        "body": release_data.get("body") or "",
        "pageUrl": page_url,
        "asset": asset,
    }


def _launch_profiles(current_options: str | None) -> list[dict[str, Any]]:
    current = current_options or ""
    profiles: list[dict[str, Any]] = [
        {
            "id": "default",
            "label": "Default (None)",
            "option": "",
            "active": current.strip() == "",
            "installed": True,
            "subscribed": True,
            "state": "Ready",
        },
        {
            "id": "offline",
            "label": "Play Offline",
            "option": "+set fs_game offlinemp",
            "active": "+set fs_game offlinemp" in current,
            "installed": True,
            "subscribed": True,
            "state": "Ready",
        },
    ]

    for key, profile in WORKSHOP_PROFILES.items():
        workshop_state = get_workshop_item_state(app_id, profile["workshop_id"])
        profiles.append(
            {
                "id": key,
                "label": profile["name"],
                "option": profile["launch_option"],
                "active": profile["launch_option"] in current,
                "installed": bool(workshop_state.get("installed")),
                "subscribed": bool(workshop_state.get("subscribed")),
                "state": workshop_state.get("state") or "Unknown",
                "path": workshop_state.get("path"),
            }
        )
    return profiles


def _open_external_url(url: str) -> None:
    system = platform.system()
    if system == "Windows":
        os.startfile(url)  # type: ignore[attr-defined]
    elif system == "Darwin":
        subprocess.Popen(["open", url], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    else:
        launcher = shutil.which("xdg-open") or shutil.which("steam")
        if not launcher:
            raise RuntimeError("No system URL launcher was found.")
        subprocess.Popen([launcher, url], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def _current_state() -> dict[str, Any]:
    game_dir = _find_game_directory()
    content = _read_config(game_dir)
    config_exists = bool(content)
    log_path = get_log_file_path()
    qol = _qol_status(game_dir)
    current_launch_options = _current_launch_options()
    launch_profiles = _launch_profiles(current_launch_options)
    enhanced_summary = status_summary(game_dir, get_app_data_dir()) if game_dir else {"installed": False}
    return {
        "appVersion": APP_VERSION,
        "platform": platform.system(),
        "gameDir": game_dir,
        "gameDetected": bool(game_dir),
        "configExists": config_exists,
        "steamUserId": find_steam_user_id(),
        "logPath": log_path,
        "presets": _preset_names(),
        "currentLaunchOptions": current_launch_options,
        "activeLaunchProfile": next((profile["id"] for profile in launch_profiles if profile["active"]), "custom" if current_launch_options else "default"),
        "launchProfiles": launch_profiles,
        "enhanced": _enhanced_status(game_dir),
        "t7": _t7_status(game_dir),
        "dxvk": _dxvk_status(game_dir),
        "qol": qol,
        "graphics": {
            "maxFps": _config_int(content, "MaxFPS", 165),
            "fov": _config_int(content, "FOV", 80),
            "displayMode": _config_int(content, "FullScreenMode", 1),
            "resolution": _extract_config(content, "WindowSize", "1920x1080"),
            "refreshRate": _config_float(content, "RefreshRate", 60),
            "renderResolution": _config_int(content, "ResolutionPercent", 100),
            "vsync": _config_bool(content, "Vsync", "1", "1"),
            "drawFps": _config_bool(content, "DrawFPS", "1", "0"),
        },
        "advanced": {
            "smoothFramerate": _config_bool(content, "SmoothFramerate", "1", "0"),
            "unlockOptions": _config_bool(content, "RestrictGraphicsOptions", "0", "1"),
            "reduceCpu": _config_bool(content, "SerializeRender", "2", "0"),
            "maxFrameLatency": _config_int(content, "MaxFrameLatency", 1),
            "vramLimited": not (
                str(_extract_config(content, "VideoMemory", "1")) == "1"
                and str(_extract_config(content, "StreamMinResident", "0")) == "0"
            ),
            "vramTarget": int(_config_float(content, "VideoMemory", 0.75) * 100),
            "configReadonly": _config_is_readonly(game_dir),
        },
        "maintenance": {
            "modFilesDir": str(MOD_FILES_DIR),
            "logPayload": _log_payload(),
        },
        "mods": {
            "t7Patch": bool(game_dir and ((Path(game_dir) / "t7patch.exe").exists() or (Path(game_dir) / "t7patch.dll").exists())),
            "dxvk": bool(game_dir and is_dxvk_async_installed(game_dir)),
            "enhanced": bool(enhanced_summary.get("installed")),
        },
        "logs": log_bus.recent[-80:],
    }


def _browse_roots() -> list[dict[str, str]]:
    roots: list[dict[str, str]] = []
    system = platform.system()
    if system == "Windows":
        for letter in string.ascii_uppercase:
            drive = f"{letter}:\\"
            if os.path.isdir(drive):
                roots.append({"label": drive, "path": drive})
    else:
        roots.append({"label": "/", "path": "/"})
    return roots


def _browse_shortcuts() -> list[dict[str, str]]:
    shortcuts: list[dict[str, str]] = []
    seen: set[str] = set()

    def add(label: str, path: str | Path | None) -> None:
        if not path:
            return
        try:
            resolved = str(Path(path).expanduser().resolve())
        except Exception:
            resolved = str(path)
        normalized = os.path.normcase(os.path.normpath(resolved))
        if normalized in seen or not os.path.isdir(resolved):
            return
        seen.add(normalized)
        shortcuts.append({"label": label, "path": resolved})

    home = Path.home()
    add("Home", home)
    add("Desktop", home / "Desktop")
    add("Downloads", home / "Downloads")
    add("Saved game folder", _load_settings().get("game_dir"))
    detected = _find_game_directory()
    add("Detected BO3 folder", detected)
    for index, library in enumerate(get_steam_library_paths(), start=1):
        add(f"Steam Library {index}", library)
    return shortcuts


def _directory_entries(directory: Path) -> list[dict[str, Any]]:
    entries: list[dict[str, Any]] = []
    try:
        children = list(directory.iterdir())
    except (OSError, PermissionError):
        return entries

    for child in children:
        try:
            if not child.is_dir():
                continue
        except OSError:
            continue
        entries.append(
            {
                "name": child.name,
                "path": str(child),
                "hasGameExecutable": _has_game_executable(child),
            }
        )

    entries.sort(key=lambda item: (not item["hasGameExecutable"], item["name"].lower()))
    return entries[:300]


def _browse_payload(path: str | None = None) -> dict[str, Any]:
    selected = path or _load_settings().get("game_dir") or str(Path.home())
    try:
        current = Path(str(selected)).expanduser()
        if not current.exists() or not current.is_dir():
            current = Path.home()
        current = current.resolve()
    except Exception:
        current = Path.home().resolve()

    parent = current.parent if current.parent != current else None
    return {
        "path": str(current),
        "parent": str(parent) if parent else None,
        "hasGameExecutable": _has_game_executable(current),
        "roots": _browse_roots(),
        "shortcuts": _browse_shortcuts(),
        "entries": _directory_entries(current),
    }


def _restore_backup(target: Path, success_message: str) -> bool:
    backup = existing_backup_path(str(target))
    if backup and os.path.exists(backup):
        if target.exists():
            target.unlink()
        Path(backup).rename(target)
        write_log(success_message, "Success", log_target)
        return True
    return False


def _restore_intro_videos(game_dir: str) -> None:
    video_dir = Path(game_dir) / "video"
    if not video_dir.exists():
        return
    restored = 0
    for backup_file in list(video_dir.glob(f"*.mkv{PATCHOPS_BACKUP_SUFFIX}")) + list(video_dir.glob(f"*.mkv{LEGACY_BACKUP_SUFFIX}")):
        original_name = backup_file.name
        if original_name.endswith(PATCHOPS_BACKUP_SUFFIX):
            original_name = original_name[: -len(PATCHOPS_BACKUP_SUFFIX)]
        elif original_name.endswith(LEGACY_BACKUP_SUFFIX):
            original_name = original_name[: -len(LEGACY_BACKUP_SUFFIX)]
        target = video_dir / original_name
        if not target.exists():
            backup_file.rename(target)
            restored += 1
    if restored:
        write_log(f"Restored {restored} intro video file(s).", "Success", log_target)


def _reset_to_stock(game_dir: str) -> None:
    if detect_enhanced_install(game_dir):
        try:
            _uninstall_enhanced(game_dir)
        except Exception as exc:
            write_log(f"Enhanced reset step failed: {exc}", "Warning", log_target)

    executable = _find_bo3_executable(game_dir) or (Path(game_dir) / "BlackOps3.exe")
    try:
        if _restore_backup(executable, "Restored original executable from backup."):
            write_exe_variant(game_dir, "default")
    except Exception as exc:
        write_log(f"Executable reset step failed: {exc}", "Warning", log_target)

    try:
        _uninstall_t7_patch(game_dir)
    except Exception as exc:
        write_log(f"T7 Patch reset step failed: {exc}", "Warning", log_target)

    try:
        _uninstall_dxvk(game_dir)
    except Exception as exc:
        write_log(f"DXVK reset step failed: {exc}", "Warning", log_target)

    try:
        _restore_backup(Path(game_dir) / "d3dcompiler_46.dll", "Restored d3dcompiler_46.dll.")
        _restore_intro_videos(game_dir)
    except Exception as exc:
        write_log(f"Quality of Life reset step failed: {exc}", "Warning", log_target)

    try:
        _write_config_values(
            game_dir,
            [
                ("SmoothFramerate", "0", "0 or 1"),
                ("VideoMemory", "1", "0.75 to 1"),
                ("StreamMinResident", "0", "0 or 1"),
                ("MaxFrameLatency", "1", "0 to 4"),
                ("SerializeRender", "0", "0 to 2"),
                ("RestrictGraphicsOptions", "1", "0 or 1"),
            ],
        )
        write_log("Reset stock graphics and advanced config values.", "Success", log_target)
    except Exception as exc:
        write_log(f"Config reset step failed: {exc}", "Warning", log_target)

    try:
        apply_launch_options("", log_target)
        write_log("Cleared Steam launch options.", "Success", log_target)
    except Exception as exc:
        write_log(f"Launch options reset failed: {exc}", "Warning", log_target)

    write_log("Reset to stock complete.", "Success", log_target)


def _log_payload() -> str:
    log_path = get_log_file_path()
    try:
        body = Path(log_path).read_text(encoding="utf-8").strip()
    except Exception:
        body = ""
    platform_label = f"{platform.system()} {platform.release()} ({platform.machine()})".strip()
    return f"PatchOpsIII {APP_VERSION} - {platform_label} logs:\n```\n{body or '(no log entries found)'}\n```"


def _clear_mod_files() -> None:
    MOD_FILES_DIR.mkdir(parents=True, exist_ok=True)
    for item in MOD_FILES_DIR.iterdir():
        if item.is_dir():
            shutil.rmtree(item)
        else:
            item.unlink()
    write_log(f"Cleared mod files in {MOD_FILES_DIR}", "Success", log_target)


def _apply_preset_values(game_dir: str, preset_name: str) -> None:
    presets = _load_presets()
    preset = presets[preset_name]
    updates: list[tuple[str, str | int | float | bool, str]] = []
    for key, item in preset.items():
        if key == "ReduceStutter":
            dll = Path(game_dir) / "d3dcompiler_46.dll"
            backup = Path(patchops_backup_path(str(dll)))
            if str(item[0]) == "1" and dll.exists() and not backup.exists():
                dll.rename(backup)
            continue
        value, comment = item
        updates.append((key, value, comment))
    if updates:
        _write_config_values(game_dir, updates)
    write_log(f"Applied preset '{preset_name}'.", "Success", log_target)


@app.on_event("startup")
async def startup() -> None:
    log_target.set_loop(asyncio.get_running_loop())


@app.get("/api/health")
async def health() -> dict[str, Any]:
    return {"ok": True, "version": APP_VERSION}


@app.get("/api/status")
async def status() -> dict[str, Any]:
    return await _current_state_async()


@app.get("/api/browse")
async def browse(path: str | None = None) -> dict[str, Any]:
    return await _blocking(_browse_payload, path)


@app.post("/api/game-directory")
async def set_game_directory(payload: GameDirectoryPayload) -> dict[str, Any]:
    if not _has_game_executable(payload.path):
        write_log(f"Selected directory is not a Black Ops III install: {payload.path}", "Error", log_target)
        return {"ok": False, "error": "BlackOps3.exe or BlackOpsIII.exe was not found."}
    settings = _load_settings()
    settings["game_dir"] = str(Path(payload.path))
    _save_settings(settings)
    write_log(f"Game directory set to {payload.path}", "Success", log_target)
    return {"ok": True, "state": await _current_state_async()}


@app.post("/api/launch")
async def launch_game() -> dict[str, Any]:
    await _blocking(launch_game_via_steam, app_id, log_target)
    return {"ok": True}


@app.post("/api/launch-options")
async def launch_options(payload: LaunchOptionsPayload) -> dict[str, Any]:
    if payload.options not in SUPPORTED_LAUNCH_OPTIONS:
        error = "Unsupported launch option."
        write_log(error, "Warning", log_target)
        return {"ok": False, "error": error, "state": await _current_state_async()}
    ok = await _blocking(apply_launch_options, payload.options, log_target, preserve_fs_game=payload.preserve_fs_game)
    return {"ok": bool(ok), "state": await _current_state_async()}


@app.post("/api/workshop-install")
async def workshop_install(payload: WorkshopInstallPayload) -> dict[str, Any]:
    profile = WORKSHOP_PROFILES.get(payload.profileId)
    if not profile:
        return {"ok": False, "error": "Select an installable Workshop mod."}
    try:
        ok = await _blocking(apply_launch_options, profile["launch_option"], log_target, preserve_fs_game=False)
        if not ok:
            return {"ok": False, "error": "Failed to apply launch options."}
        url = f"steam://openurl/https://steamcommunity.com/sharedfiles/filedetails/?id={profile['workshop_id']}"
        await _blocking(_open_external_url, url)
        write_log(f"Opened {profile['name']} Workshop page in Steam.", "Info", log_target)
    except Exception as exc:
        write_log(f"Workshop install flow failed: {exc}", "Error", log_target)
        return {"ok": False, "error": str(exc)}
    return {"ok": True, "state": await _current_state_async()}


@app.post("/api/update-check")
async def update_check() -> dict[str, Any]:
    try:
        result = await _blocking(_check_for_update)
        if result["available"]:
            write_log(f"Update available: {result['latestVersion']}", "Success", log_target)
        else:
            write_log("No updates available.", "Info", log_target)
        return {"ok": True, "update": result, "state": await _current_state_async()}
    except Exception as exc:
        write_log(f"Update check failed: {exc}", "Error", log_target)
        return {"ok": False, "error": str(exc)}


@app.post("/api/config")
async def set_config(payload: ConfigValuePayload) -> dict[str, Any]:
    game_dir = await _find_game_directory_async()
    if not game_dir:
        return {"ok": False, "error": "Game directory is not set."}
    try:
        await _blocking(_write_config_value, game_dir, payload.key, payload.value, payload.comment)
    except Exception as exc:
        write_log(str(exc), "Error", log_target)
        return {"ok": False, "error": str(exc)}
    return {"ok": True, "state": await _current_state_async()}


@app.post("/api/dxvk-install")
async def install_dxvk(payload: DxvkConfigPayload) -> dict[str, Any]:
    game_dir = await _find_game_directory_async()
    if not game_dir:
        return {"ok": False, "error": "Game directory is not set."}
    try:
        await _blocking(_install_dxvk, game_dir, payload)
    except Exception as exc:
        write_log(f"DXVK-GPLAsync install failed: {exc}", "Error", log_target)
        return {"ok": False, "error": str(exc)}
    return {"ok": True, "state": await _current_state_async()}


@app.post("/api/dxvk-uninstall")
async def uninstall_dxvk() -> dict[str, Any]:
    game_dir = await _find_game_directory_async()
    if not game_dir:
        return {"ok": False, "error": "Game directory is not set."}
    try:
        await _blocking(_uninstall_dxvk, game_dir)
    except Exception as exc:
        write_log(f"DXVK-GPLAsync uninstall failed: {exc}", "Error", log_target)
        return {"ok": False, "error": str(exc)}
    return {"ok": True, "state": await _current_state_async()}


@app.post("/api/dxvk-config")
async def dxvk_config(payload: DxvkConfigPayload) -> dict[str, Any]:
    game_dir = await _find_game_directory_async()
    if not game_dir:
        return {"ok": False, "error": "Game directory is not set."}
    try:
        await _blocking(_write_dxvk_conf, game_dir, _dxvk_payload_to_settings(payload))
    except Exception as exc:
        write_log(f"Failed to update DXVK settings: {exc}", "Error", log_target)
        return {"ok": False, "error": str(exc)}
    return {"ok": True, "state": await _current_state_async()}


@app.post("/api/t7-config")
async def set_t7_config(payload: T7ConfigPayload) -> dict[str, Any]:
    game_dir = await _find_game_directory_async()
    if not game_dir:
        return {"ok": False, "error": "Game directory is not set."}

    t7_conf = Path(game_dir) / "t7patch.conf"
    try:
        if payload.gamertag is not None:
            plain_name = payload.gamertag.strip()
            if not t7_conf.exists():
                return {"ok": False, "error": "t7patch.conf was not found. Install T7 Patch before updating the gamertag."}
            if not plain_name:
                return {"ok": False, "error": "Gamertag cannot be empty."}
            if len(plain_name) > 20:
                return {"ok": False, "error": "Gamertag cannot exceed 20 characters."}
            await _blocking(update_t7patch_conf, game_dir, new_name=f"{payload.colorCode}{plain_name}", log_widget=log_target)

        if payload.networkPassword is not None:
            password = payload.networkPassword.strip()
            if not t7_conf.exists():
                return {"ok": False, "error": "t7patch.conf was not found. Install T7 Patch before changing the network password."}
            await _blocking(update_t7patch_conf, game_dir, new_password=password, log_widget=log_target)

        if payload.friendsOnly is not None:
            if not t7_conf.exists():
                return {"ok": False, "error": "t7patch.conf was not found. Install T7 Patch before changing Friends Only mode."}
            await _blocking(update_t7patch_conf, game_dir, friends_only=payload.friendsOnly, log_widget=log_target)
    except Exception as exc:
        write_log(f"Failed to update T7 Patch settings: {exc}", "Error", log_target)
        return {"ok": False, "error": str(exc)}

    return {"ok": True, "state": await _current_state_async()}


@app.post("/api/t7-install")
async def install_t7_patch() -> dict[str, Any]:
    game_dir = await _find_game_directory_async()
    if not game_dir:
        return {"ok": False, "error": "Game directory is not set."}
    try:
        await _blocking(_install_t7_patch, game_dir)
    except Exception as exc:
        write_log(f"T7 Patch install failed: {exc}", "Error", log_target)
        return {"ok": False, "error": str(exc)}
    return {"ok": True, "state": await _current_state_async()}


@app.post("/api/t7-uninstall")
async def uninstall_t7_patch() -> dict[str, Any]:
    game_dir = await _find_game_directory_async()
    if not game_dir:
        return {"ok": False, "error": "Game directory is not set."}
    try:
        await _blocking(_uninstall_t7_patch, game_dir)
    except Exception as exc:
        write_log(f"T7 Patch uninstall failed: {exc}", "Error", log_target)
        return {"ok": False, "error": str(exc)}
    return {"ok": True, "state": await _current_state_async()}


@app.post("/api/enhanced-install")
async def install_enhanced(payload: EnhancedInstallPayload) -> dict[str, Any]:
    game_dir = await _find_game_directory_async()
    if not game_dir:
        return {"ok": False, "error": "Game directory is not set."}
    try:
        await _blocking(_install_enhanced, game_dir, payload.dumpSource)
    except Exception as exc:
        write_log(f"BO3 Enhanced install failed: {exc}", "Error", log_target)
        return {"ok": False, "error": str(exc)}
    return {"ok": True, "state": await _current_state_async()}


@app.post("/api/enhanced-uninstall")
async def uninstall_enhanced() -> dict[str, Any]:
    game_dir = await _find_game_directory_async()
    if not game_dir:
        return {"ok": False, "error": "Game directory is not set."}
    try:
        await _blocking(_uninstall_enhanced, game_dir)
    except Exception as exc:
        write_log(f"BO3 Enhanced uninstall failed: {exc}", "Error", log_target)
        return {"ok": False, "error": str(exc)}
    return {"ok": True, "state": await _current_state_async()}


@app.post("/api/presets/apply")
async def apply_preset(payload: PresetPayload) -> dict[str, Any]:
    game_dir = await _find_game_directory_async()
    if not game_dir:
        return {"ok": False, "error": "Game directory is not set."}
    try:
        await _blocking(_apply_preset_values, game_dir, payload.name)
    except Exception as exc:
        write_log(f"Failed to apply preset: {exc}", "Error", log_target)
        return {"ok": False, "error": str(exc)}
    return {"ok": True, "state": await _current_state_async()}


@app.post("/api/intro-skip")
async def intro_skip(payload: TogglePayload) -> dict[str, Any]:
    game_dir = await _find_game_directory_async()
    if not game_dir:
        return {"ok": False, "error": "Game directory is not set."}
    video_dir = Path(game_dir) / "video"
    intro = video_dir / "BO3_Global_Logo_LogoSequence.mkv"
    backup = Path(patchops_backup_path(str(intro)))
    try:
        if payload.enabled:
            if intro.exists():
                await _blocking(intro.rename, backup)
            write_log("Intro video skipped.", "Success", log_target)
        else:
            existing = await _blocking(existing_backup_path, str(intro))
            if existing:
                await _blocking(Path(existing).rename, intro)
            write_log("Intro video restored.", "Success", log_target)
    except Exception as exc:
        write_log(f"Failed to update intro video: {exc}", "Error", log_target)
        return {"ok": False, "error": str(exc)}
    return {"ok": True, "state": await _current_state_async()}


@app.post("/api/d3dcompiler")
async def d3dcompiler(payload: TogglePayload) -> dict[str, Any]:
    game_dir = await _find_game_directory_async()
    if not game_dir:
        return {"ok": False, "error": "Game directory is not set."}

    dll = Path(game_dir) / "d3dcompiler_46.dll"
    backup = Path(patchops_backup_path(str(dll)))
    try:
        if payload.enabled:
            if dll.exists():
                await _blocking(dll.rename, backup)
                write_log("Renamed d3dcompiler_46.dll to reduce stuttering.", "Success", log_target)
            elif await _blocking(existing_backup_path, str(dll)):
                write_log("Already using latest d3dcompiler.", "Success", log_target)
            else:
                return {"ok": False, "error": "d3dcompiler_46.dll was not found."}
        else:
            existing = await _blocking(existing_backup_path, str(dll))
            if existing:
                await _blocking(Path(existing).rename, dll)
                write_log("Restored d3dcompiler_46.dll.", "Success", log_target)
            else:
                return {"ok": False, "error": "No d3dcompiler backup was found."}
    except Exception as exc:
        write_log(f"Failed to update d3dcompiler_46.dll: {exc}", "Error", log_target)
        return {"ok": False, "error": str(exc)}
    return {"ok": True, "state": await _current_state_async()}


@app.post("/api/all-intros-skip")
async def all_intros_skip(payload: TogglePayload) -> dict[str, Any]:
    game_dir = await _find_game_directory_async()
    if not game_dir:
        return {"ok": False, "error": "Game directory is not set."}

    video_dir = Path(game_dir) / "video"
    if not video_dir.exists():
        return {"ok": False, "error": "Video directory was not found."}

    try:
        if payload.enabled:
            changed = 0
            for mkv_file in await _blocking(lambda: list(video_dir.glob("*.mkv"))):
                backup = Path(patchops_backup_path(str(mkv_file)))
                if not backup.exists():
                    await _blocking(mkv_file.rename, backup)
                    changed += 1
            write_log("All intro videos skipped." if changed else "Intro videos were already skipped.", "Success", log_target)
        else:
            changed = 0
            backup_files = await _blocking(lambda: list(video_dir.glob(f"*.mkv{PATCHOPS_BACKUP_SUFFIX}")) + list(video_dir.glob(f"*.mkv{LEGACY_BACKUP_SUFFIX}")))
            for backup_file in backup_files:
                original_name = backup_file.name
                if original_name.endswith(PATCHOPS_BACKUP_SUFFIX):
                    original_name = original_name[: -len(PATCHOPS_BACKUP_SUFFIX)]
                elif original_name.endswith(LEGACY_BACKUP_SUFFIX):
                    original_name = original_name[: -len(LEGACY_BACKUP_SUFFIX)]
                target = video_dir / original_name
                if not target.exists():
                    await _blocking(backup_file.rename, target)
                    changed += 1
            write_log("Intro videos restored." if changed else "No intro video backups were found.", "Success", log_target)
    except Exception as exc:
        write_log(f"Failed to update intro videos: {exc}", "Error", log_target)
        return {"ok": False, "error": str(exc)}
    return {"ok": True, "state": await _current_state_async()}


@app.post("/api/config-readonly")
async def config_readonly(payload: TogglePayload) -> dict[str, Any]:
    path = await _config_path_async()
    if not path or not path.exists():
        return {"ok": False, "error": "config.ini was not found."}
    try:
        if payload.enabled:
            await _blocking(path.chmod, stat.S_IREAD)
        else:
            await _blocking(path.chmod, stat.S_IWRITE | stat.S_IREAD)
        write_log(f"config.ini set to {'read-only' if payload.enabled else 'writable'}.", "Success", log_target)
    except Exception as exc:
        write_log(f"Failed to change config.ini permissions: {exc}", "Error", log_target)
        return {"ok": False, "error": str(exc)}
    return {"ok": True, "state": await _current_state_async()}


@app.post("/api/vram-target")
async def vram_target(payload: VramPayload) -> dict[str, Any]:
    game_dir = await _find_game_directory_async()
    if not game_dir:
        return {"ok": False, "error": "Game directory is not set."}
    try:
        if payload.limited:
            decimal_value = payload.target / 100
            await _blocking(_write_config_values, game_dir, [
                ("VideoMemory", decimal_value, "0.75 to 1"),
                ("StreamMinResident", "1", "0 or 1"),
            ])
            write_log(f"Limited VRAM usage set to {payload.target}%.", "Success", log_target)
        else:
            await _blocking(_write_config_values, game_dir, [
                ("VideoMemory", "1", "0.75 to 1"),
                ("StreamMinResident", "0", "0 or 1"),
            ])
            write_log("Enabled full VRAM usage.", "Success", log_target)
    except Exception as exc:
        write_log(f"Failed to update VRAM target: {exc}", "Error", log_target)
        return {"ok": False, "error": str(exc)}
    return {"ok": True, "state": await _current_state_async()}


@app.get("/api/logs/payload")
async def logs_payload() -> dict[str, Any]:
    return {"ok": True, "payload": await _blocking(_log_payload)}


@app.post("/api/logs/clear")
async def logs_clear() -> dict[str, Any]:
    if not await _blocking(clear_log_file):
        return {"ok": False, "error": "Failed to clear log file."}
    log_bus._recent.clear()
    write_log("Logs cleared.", "Success", log_target)
    return {"ok": True, "state": await _current_state_async()}


@app.post("/api/mod-files/clear")
async def clear_mod_files() -> dict[str, Any]:
    try:
        await _blocking(_clear_mod_files)
    except Exception as exc:
        write_log(f"Failed to clear mod files: {exc}", "Error", log_target)
        return {"ok": False, "error": str(exc)}
    return {"ok": True, "state": await _current_state_async()}


@app.post("/api/reset-stock")
async def reset_stock() -> dict[str, Any]:
    game_dir = await _find_game_directory_async()
    if not game_dir:
        return {"ok": False, "error": "Game directory is not set."}
    try:
        await _blocking(_reset_to_stock, game_dir)
    except Exception as exc:
        write_log(f"Reset to stock failed: {exc}", "Error", log_target)
        return {"ok": False, "error": str(exc)}
    return {"ok": True, "state": await _current_state_async()}


@app.websocket("/ws")
async def logs(websocket: WebSocket) -> None:
    await log_bus.connect(websocket)
    try:
        while True:
            await websocket.receive_text()
    except WebSocketDisconnect:
        log_bus.disconnect(websocket)


if __name__ == "__main__":
    import uvicorn

    uvicorn.run(
        app,
        host=os.environ.get("PATCHOPSIII_BACKEND_HOST", "127.0.0.1"),
        port=int(os.environ.get("PATCHOPSIII_BACKEND_PORT", "8765")),
    )
