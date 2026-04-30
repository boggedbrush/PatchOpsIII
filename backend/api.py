#!/usr/bin/env python
from __future__ import annotations

import asyncio
import json
import os
import platform
import re
import stat
import string
from collections import deque
from pathlib import Path
from typing import Any

from fastapi import FastAPI, WebSocket, WebSocketDisconnect
from fastapi.middleware.cors import CORSMiddleware
from pydantic import BaseModel, Field

from utils import (
    _read_launch_options,
    app_id,
    apply_launch_options,
    existing_backup_path,
    find_steam_user_id,
    get_app_data_dir,
    get_log_file_path,
    get_steam_library_paths,
    get_workshop_item_state,
    LEGACY_BACKUP_SUFFIX,
    launch_game_via_steam,
    PATCHOPS_BACKUP_SUFFIX,
    patchops_backup_path,
    write_log,
)
from version import APP_VERSION
from bo3_enhanced import status_summary


APP_ROOT = Path(__file__).resolve().parents[1]
SETTINGS_PATH = Path(get_app_data_dir()) / "electron-settings.json"
PRESETS_PATH = APP_ROOT / "presets.json"
GAME_EXECUTABLE_NAMES = ("BlackOpsIII.exe", "BlackOps3.exe")
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
    "reforged": {
        "name": "Reforged",
        "workshop_id": "3667377161",
        "launch_option": "+set fs_game 3667377161",
    },
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

    def handle_write_log(self, *, full_message: str, category: str, plain_message: str, **_: str) -> None:
        entry = {"message": plain_message, "category": category, "line": full_message}
        try:
            loop = asyncio.get_running_loop()
            loop.create_task(self.bus.publish(entry))
        except RuntimeError:
            self.bus._recent.append(entry)


log_bus = LogBus()
log_target = ApiLogTarget(log_bus)
app = FastAPI(title="PatchOpsIII Local API", version=APP_VERSION)
app.add_middleware(
    CORSMiddleware,
    allow_origins=["http://localhost:5173", "http://127.0.0.1:5173", "app://patchopsiii"],
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)


class GameDirectoryPayload(BaseModel):
    path: str = Field(..., min_length=1)


class LaunchOptionsPayload(BaseModel):
    options: str = ""
    preserve_fs_game: bool = False


class ConfigValuePayload(BaseModel):
    key: str = Field(..., min_length=1)
    value: str | int | float | bool
    comment: str = "Managed by PatchOpsIII"


class TogglePayload(BaseModel):
    enabled: bool


class PresetPayload(BaseModel):
    name: str


def _load_settings() -> dict[str, Any]:
    try:
        return json.loads(SETTINGS_PATH.read_text(encoding="utf-8"))
    except Exception:
        return {}


def _save_settings(settings: dict[str, Any]) -> None:
    SETTINGS_PATH.parent.mkdir(parents=True, exist_ok=True)
    SETTINGS_PATH.write_text(json.dumps(settings, indent=2), encoding="utf-8")


def _has_game_executable(directory: str | Path | None) -> bool:
    if not directory:
        return False
    path = Path(directory)
    return path.is_dir() and any((path / name).exists() for name in GAME_EXECUTABLE_NAMES)


def _find_game_directory() -> str | None:
    saved = _load_settings().get("game_dir")
    if isinstance(saved, str) and _has_game_executable(saved):
        return str(Path(saved))

    for library in get_steam_library_paths():
        candidate = Path(library) / "steamapps" / "common" / "Call of Duty Black Ops III"
        if _has_game_executable(candidate):
            return str(candidate)

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
            return str(candidate)
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


def _write_config_value(game_dir: str, key: str, value: str | int | float | bool, comment: str) -> None:
    config = _config_path(game_dir)
    if not config or not config.exists():
        raise FileNotFoundError(f"config.ini not found at {config}")
    text = config.read_text(encoding="utf-8", errors="ignore")
    replacement = f'{key} = "{value}" // {comment}'
    pattern = re.compile(rf'^\s*{re.escape(key)}\s*=.*$', re.MULTILINE)
    if pattern.search(text):
        text = pattern.sub(replacement, text)
    else:
        text = f"{text.rstrip()}\n{replacement}\n"
    config.write_text(text, encoding="utf-8")
    write_log(f"Set {key} to {value}.", "Success", log_target)


def _preset_names() -> list[str]:
    try:
        data = json.loads(PRESETS_PATH.read_text(encoding="utf-8"))
    except Exception:
        return []
    return list(data.keys()) if isinstance(data, dict) else []


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


def _current_launch_options() -> str | None:
    user_id = find_steam_user_id()
    if not user_id:
        return None
    return _read_launch_options(user_id, app_id)


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
        "qol": qol,
        "graphics": {
            "maxFps": int(float(_extract_config(content, "MaxFPS", 165))),
            "fov": int(float(_extract_config(content, "FOV", 80))),
            "displayMode": int(float(_extract_config(content, "FullScreenMode", 1))),
            "resolution": _extract_config(content, "WindowSize", "1920x1080"),
            "refreshRate": float(_extract_config(content, "RefreshRate", 60)),
            "vsync": _extract_config(content, "Vsync", "1") == "1",
            "drawFps": _extract_config(content, "DrawFPS", "0") == "1",
        },
        "advanced": {
            "smoothFramerate": _extract_config(content, "SmoothFramerate", "0") == "1",
            "unlockOptions": _extract_config(content, "RestrictGraphicsOptions", "1") == "0",
            "reduceCpu": _extract_config(content, "SerializeRender", "0") == "2",
            "maxFrameLatency": int(float(_extract_config(content, "MaxFrameLatency", 1))),
        },
        "mods": {
            "t7Patch": bool(game_dir and ((Path(game_dir) / "t7patch.exe").exists() or (Path(game_dir) / "t7patch.dll").exists())),
            "dxvk": bool(game_dir and (Path(game_dir) / "dxgi.dll").exists() and (Path(game_dir) / "d3d11.dll").exists()),
            "enhanced": bool(enhanced_summary.get("installed")),
            "reforged": bool(game_dir and (Path(game_dir) / ".patchops_exe_variant.json").exists()),
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


@app.on_event("startup")
async def startup() -> None:
    pass


@app.get("/api/health")
async def health() -> dict[str, Any]:
    return {"ok": True, "version": APP_VERSION}


@app.get("/api/status")
async def status() -> dict[str, Any]:
    return _current_state()


@app.get("/api/browse")
async def browse(path: str | None = None) -> dict[str, Any]:
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


@app.post("/api/game-directory")
async def set_game_directory(payload: GameDirectoryPayload) -> dict[str, Any]:
    if not _has_game_executable(payload.path):
        write_log(f"Selected directory is not a Black Ops III install: {payload.path}", "Error", log_target)
        return {"ok": False, "error": "BlackOps3.exe or BlackOpsIII.exe was not found."}
    settings = _load_settings()
    settings["game_dir"] = str(Path(payload.path))
    _save_settings(settings)
    write_log(f"Game directory set to {payload.path}", "Success", log_target)
    return {"ok": True, "state": _current_state()}


@app.post("/api/launch")
async def launch_game() -> dict[str, Any]:
    launch_game_via_steam(app_id, log_target)
    return {"ok": True}


@app.post("/api/launch-options")
async def launch_options(payload: LaunchOptionsPayload) -> dict[str, Any]:
    ok = apply_launch_options(payload.options, log_target, preserve_fs_game=payload.preserve_fs_game)
    return {"ok": bool(ok)}


@app.post("/api/config")
async def set_config(payload: ConfigValuePayload) -> dict[str, Any]:
    game_dir = _find_game_directory()
    if not game_dir:
        return {"ok": False, "error": "Game directory is not set."}
    try:
        _write_config_value(game_dir, payload.key, payload.value, payload.comment)
    except Exception as exc:
        write_log(str(exc), "Error", log_target)
        return {"ok": False, "error": str(exc)}
    return {"ok": True, "state": _current_state()}


@app.post("/api/presets/apply")
async def apply_preset(payload: PresetPayload) -> dict[str, Any]:
    game_dir = _find_game_directory()
    if not game_dir:
        return {"ok": False, "error": "Game directory is not set."}
    try:
        presets = json.loads(PRESETS_PATH.read_text(encoding="utf-8"))
        preset = presets[payload.name]
        for key, item in preset.items():
            if key == "ReduceStutter":
                continue
            value, comment = item
            _write_config_value(game_dir, key, value, comment)
        write_log(f"Applied preset '{payload.name}'.", "Success", log_target)
    except Exception as exc:
        write_log(f"Failed to apply preset: {exc}", "Error", log_target)
        return {"ok": False, "error": str(exc)}
    return {"ok": True, "state": _current_state()}


@app.post("/api/intro-skip")
async def intro_skip(payload: TogglePayload) -> dict[str, Any]:
    game_dir = _find_game_directory()
    if not game_dir:
        return {"ok": False, "error": "Game directory is not set."}
    video_dir = Path(game_dir) / "video"
    intro = video_dir / "BO3_Global_Logo_LogoSequence.mkv"
    backup = Path(patchops_backup_path(str(intro)))
    try:
        if payload.enabled:
            if intro.exists():
                intro.rename(backup)
            write_log("Intro video skipped.", "Success", log_target)
        else:
            existing = existing_backup_path(str(intro))
            if existing:
                Path(existing).rename(intro)
            write_log("Intro video restored.", "Success", log_target)
    except Exception as exc:
        write_log(f"Failed to update intro video: {exc}", "Error", log_target)
        return {"ok": False, "error": str(exc)}
    return {"ok": True, "state": _current_state()}


@app.post("/api/d3dcompiler")
async def d3dcompiler(payload: TogglePayload) -> dict[str, Any]:
    game_dir = _find_game_directory()
    if not game_dir:
        return {"ok": False, "error": "Game directory is not set."}

    dll = Path(game_dir) / "d3dcompiler_46.dll"
    backup = Path(patchops_backup_path(str(dll)))
    try:
        if payload.enabled:
            if dll.exists():
                dll.rename(backup)
                write_log("Renamed d3dcompiler_46.dll to reduce stuttering.", "Success", log_target)
            elif existing_backup_path(str(dll)):
                write_log("Already using latest d3dcompiler.", "Success", log_target)
            else:
                return {"ok": False, "error": "d3dcompiler_46.dll was not found."}
        else:
            existing = existing_backup_path(str(dll))
            if existing:
                Path(existing).rename(dll)
                write_log("Restored d3dcompiler_46.dll.", "Success", log_target)
            else:
                return {"ok": False, "error": "No d3dcompiler backup was found."}
    except Exception as exc:
        write_log(f"Failed to update d3dcompiler_46.dll: {exc}", "Error", log_target)
        return {"ok": False, "error": str(exc)}
    return {"ok": True, "state": _current_state()}


@app.post("/api/all-intros-skip")
async def all_intros_skip(payload: TogglePayload) -> dict[str, Any]:
    game_dir = _find_game_directory()
    if not game_dir:
        return {"ok": False, "error": "Game directory is not set."}

    video_dir = Path(game_dir) / "video"
    if not video_dir.exists():
        return {"ok": False, "error": "Video directory was not found."}

    try:
        if payload.enabled:
            changed = 0
            for mkv_file in video_dir.glob("*.mkv"):
                backup = Path(patchops_backup_path(str(mkv_file)))
                if not backup.exists():
                    mkv_file.rename(backup)
                    changed += 1
            write_log("All intro videos skipped." if changed else "Intro videos were already skipped.", "Success", log_target)
        else:
            changed = 0
            for backup_file in list(video_dir.glob(f"*.mkv{PATCHOPS_BACKUP_SUFFIX}")) + list(video_dir.glob(f"*.mkv{LEGACY_BACKUP_SUFFIX}")):
                original_name = backup_file.name
                if original_name.endswith(PATCHOPS_BACKUP_SUFFIX):
                    original_name = original_name[: -len(PATCHOPS_BACKUP_SUFFIX)]
                elif original_name.endswith(LEGACY_BACKUP_SUFFIX):
                    original_name = original_name[: -len(LEGACY_BACKUP_SUFFIX)]
                target = video_dir / original_name
                if not target.exists():
                    backup_file.rename(target)
                    changed += 1
            write_log("Intro videos restored." if changed else "No intro video backups were found.", "Success", log_target)
    except Exception as exc:
        write_log(f"Failed to update intro videos: {exc}", "Error", log_target)
        return {"ok": False, "error": str(exc)}
    return {"ok": True, "state": _current_state()}


@app.post("/api/config-readonly")
async def config_readonly(payload: TogglePayload) -> dict[str, Any]:
    path = _config_path()
    if not path or not path.exists():
        return {"ok": False, "error": "config.ini was not found."}
    try:
        if payload.enabled:
            path.chmod(stat.S_IREAD)
        else:
            path.chmod(stat.S_IWRITE | stat.S_IREAD)
        write_log(f"config.ini set to {'read-only' if payload.enabled else 'writable'}.", "Success", log_target)
    except Exception as exc:
        write_log(f"Failed to change config.ini permissions: {exc}", "Error", log_target)
        return {"ok": False, "error": str(exc)}
    return {"ok": True, "state": _current_state()}


@app.websocket("/ws")
async def logs(websocket: WebSocket) -> None:
    await log_bus.connect(websocket)
    try:
        while True:
            await websocket.receive_text()
    except WebSocketDisconnect:
        log_bus.disconnect(websocket)
