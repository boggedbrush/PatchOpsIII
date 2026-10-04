#!/usr/bin/env python3
"""Generate deterministic Python-backend parity goldens for patchops-core.

The generator deliberately uses only the Python standard library plus the
repository's existing ``requests`` and ``vdf`` dependencies.  When FastAPI and
Pydantic are unavailable it installs tiny import-time shims and invokes the
same async endpoint functions directly.  No network request or real Steam
process is used.
"""

from __future__ import annotations

import argparse
import asyncio
import base64
import hashlib
import importlib
import json
import os
import platform
import re
import shutil
import sys
import tempfile
import types
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable


REPO_ROOT = Path(__file__).resolve().parents[1]
OUTPUT_DIR = REPO_ROOT / "native" / "core" / "tests" / "parity"
STEAM_ID = "76561198000000000"
CURRENT_EXE_SHA256 = "51ca63bbc660e0826943c60da67606f6bcb4b3b519528b5e0548c68c9423a323"
COMPATIBLE_EXE_SHA256 = "66b95eb4667bd5b3b3d230e7bed1d29ccd261d48ca2699f01216c863be24ff44"
TIMESTAMP_RE = re.compile(r"\b\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\b")


def _install_import_shims() -> bool:
    """Return True when endpoint calls must use the direct-call fallback."""
    try:
        import fastapi  # noqa: F401
        import pydantic  # noqa: F401

        return False
    except ImportError:
        pass

    class FieldInfo:
        def __init__(self, default: Any = ..., **_: Any) -> None:
            self.default = default

    class BaseModel:
        def __init__(self, **values: Any) -> None:
            annotations: dict[str, Any] = {}
            for cls in reversed(type(self).mro()):
                annotations.update(getattr(cls, "__annotations__", {}))
            for name in annotations:
                if name in values:
                    value = values.pop(name)
                else:
                    default = getattr(type(self), name, ...)
                    value = default.default if isinstance(default, FieldInfo) else default
                if value is ...:
                    raise TypeError(f"missing required field: {name}")
                setattr(self, name, value)
            if values:
                raise TypeError(f"unexpected fields: {', '.join(sorted(values))}")

    def Field(default: Any = ..., **kwargs: Any) -> FieldInfo:  # noqa: N802
        return FieldInfo(default, **kwargs)

    class FastAPI:
        def __init__(self, *_: Any, **__: Any) -> None:
            pass

        def add_middleware(self, *_: Any, **__: Any) -> None:
            pass

        def _decorator(self, *_: Any, **__: Any) -> Callable[[Any], Any]:
            return lambda function: function

        get = post = websocket = on_event = _decorator

    class WebSocket:
        pass

    class WebSocketDisconnect(Exception):
        pass

    class CORSMiddleware:
        pass

    async def run_in_threadpool(function: Callable[..., Any], *args: Any, **kwargs: Any) -> Any:
        return function(*args, **kwargs)

    fastapi = types.ModuleType("fastapi")
    fastapi.FastAPI = FastAPI
    fastapi.WebSocket = WebSocket
    fastapi.WebSocketDisconnect = WebSocketDisconnect
    concurrency = types.ModuleType("fastapi.concurrency")
    concurrency.run_in_threadpool = run_in_threadpool
    middleware = types.ModuleType("fastapi.middleware")
    cors = types.ModuleType("fastapi.middleware.cors")
    cors.CORSMiddleware = CORSMiddleware
    pydantic = types.ModuleType("pydantic")
    pydantic.BaseModel = BaseModel
    pydantic.Field = Field
    sys.modules.update(
        {
            "fastapi": fastapi,
            "fastapi.concurrency": concurrency,
            "fastapi.middleware": middleware,
            "fastapi.middleware.cors": cors,
            "pydantic": pydantic,
        }
    )
    return True


DIRECT_CALL_FALLBACK = _install_import_shims()
sys.path.insert(0, str(REPO_ROOT))
api = importlib.import_module("backend.api")
utils = importlib.import_module("utils")
t7_patch = importlib.import_module("t7_patch")
enhanced = importlib.import_module("bo3_enhanced")

# A fail-closed network guard: release discovery temporarily replaces requests.get
# with fixture responses, but any accidental real request is an error.
def reject_network(*_: Any, **__: Any) -> Any:
    raise AssertionError("parity fixtures must not access the network")

api.requests.sessions.Session.request = reject_network

try:
    if DIRECT_CALL_FALLBACK:
        raise ImportError("FastAPI unavailable")
    from fastapi.testclient import TestClient
    CLIENT = TestClient(api.app)
except (ImportError, TypeError):
    CLIENT = None


class ImmediateLogTarget:
    """Keep endpoint log ordering stable when endpoints are called directly."""

    def handle_write_log(
        self, *, full_message: str, category: str, plain_message: str, **_: str
    ) -> None:
        api.log_bus._recent.append(
            {"message": plain_message, "category": category, "line": full_message}
        )


@dataclass
class Harness:
    root: Path
    data: Path
    steam: Path
    library: Path
    game: Path
    logical_hashes: dict[Path, str]


def write(path: Path, contents: str | bytes, mode: int = 0o644) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(contents.encode("utf-8") if isinstance(contents, str) else contents)
    path.chmod(mode)


def localconfig(options: str = "") -> str:
    escaped = options.replace("\\", "\\\\").replace('"', '\\"')
    return (
        '"UserLocalConfigStore"\n{\n'
        '  "Software"\n  {\n    "Valve"\n    {\n      "Steam"\n      {\n'
        '        "apps"\n        {\n          "311210"\n          {\n'
        f'            "LaunchOptions" "{escaped}"\n'
        "          }\n        }\n      }\n    }\n  }\n}\n"
    )


def base_config() -> str:
    return """// fixture configuration
MaxFPS = "165" // original
FOV = "80" // original
FullScreenMode = "1" // original
WindowSize = "1920x1080" // original
RefreshRate = "60" // original
ResolutionPercent = "100" // original
Vsync = "1" // original
DrawFPS = "0" // original
SmoothFramerate = "0" // original
RestrictGraphicsOptions = "1" // original
SerializeRender = "0" // original
MaxFrameLatency = "1" // original
VideoMemory = "1" // original
StreamMinResident = "0" // original
"""


def make_harness(root: Path, *, saved_game: bool = True, game_in_library: bool = False) -> Harness:
    data = root / "data"
    steam = root / "steam"
    library = root / "secondary-library"
    game = (
        library / "steamapps" / "common" / "Call of Duty Black Ops III"
        if game_in_library
        else root / "game"
    )
    data.mkdir(parents=True)
    steam.mkdir(parents=True)
    library.mkdir(parents=True)
    settings: dict[str, Any] = {"release_channel": "stable"}
    if saved_game:
        settings["game_dir"] = str(game)
    write(data / "electron-settings.json", json.dumps(settings, indent=2))
    write(root / "resources" / "presets.json", (REPO_ROOT / "presets.json").read_bytes())
    write(
        steam / "steamapps" / "libraryfolders.vdf",
        (
            '"libraryfolders"\n{\n'
            f'  "0"\n  {{\n    "path" "{steam}"\n  }}\n'
            f'  "1"\n  {{\n    "path" "{library}"\n    "apps"\n    {{\n      "311210" "1"\n    }}\n  }}\n'
            '}\n'
        ),
    )
    write(steam / "userdata" / STEAM_ID / "config" / "localconfig.vdf", localconfig())
    write(
        library / "steamapps" / "workshop" / "appworkshop_311210.acf",
        '"AppWorkshop"\n{\n  "WorkshopItemsInstalled"\n  {\n  }\n  "WorkshopItemDetails"\n  {\n  }\n}\n',
    )
    return Harness(root, data, steam, library, game, {})


def add_game(harness: Harness, *, exe_profile: str = "unverified") -> None:
    marker = f"PATCHOPS_FAKE_EXE:{exe_profile}\n".encode()
    write(harness.game / "BlackOpsIII.exe", marker)
    if exe_profile == "current":
        harness.logical_hashes[harness.game / "BlackOpsIII.exe"] = CURRENT_EXE_SHA256
    elif exe_profile == "compatible":
        harness.logical_hashes[harness.game / "BlackOpsIII.exe"] = COMPATIBLE_EXE_SHA256
    write(harness.game / "players" / "config.ini", base_config())
    write(harness.game / "video" / "BO3_Global_Logo_LogoSequence.mkv", b"intro-one")
    write(harness.game / "video" / "Frontend_Intro.mkv", b"intro-two")
    write(harness.game / "d3dcompiler_46.dll", b"legacy-compiler")


def configure_modules(harness: Harness) -> None:
    api.SETTINGS_PATH = harness.data / "electron-settings.json"
    api.MOD_FILES_DIR = harness.data / "BO3 Mod Files"
    api.PRESETS_PATH = harness.root / "resources" / "presets.json"
    api._settings_cache = None
    api._game_dir_cache = None
    api._preset_names_cache = None
    api._presets_cache = None
    api.log_bus._recent.clear()
    api.log_target = ImmediateLogTarget()
    utils.steam_userdata_path = str(harness.steam / "userdata")
    utils.steam_exe_path = str(harness.steam / "steam")
    utils._steam_library_paths_cache = None
    utils._candidate_steam_roots = lambda: [str(harness.steam)]
    # Isolate the API's fallback home-directory discovery without changing HOME.
    Path.home = classmethod(lambda cls: harness.root / "home")
    utils.get_app_data_dir = lambda: str(harness.data)
    api.get_app_data_dir = lambda: str(harness.data)
    utils.close_steam = lambda _target: None
    utils.open_steam = lambda _target: None

    real_hash = utils.file_sha256

    def fixture_hash(path: str | os.PathLike[str]) -> str | None:
        candidate = Path(path)
        logical = harness.logical_hashes.get(candidate)
        return logical or real_hash(str(candidate))

    api.file_sha256 = fixture_hash


def normalize_string(value: str, root: Path) -> str:
    value = value.replace(str(root), "$ROOT")
    value = value.replace(str(root).replace("/", "\\"), "$ROOT")
    value = value.replace(platform.release(), "$KERNEL")
    return TIMESTAMP_RE.sub("$TIMESTAMP", value)


def normalize(value: Any, root: Path) -> Any:
    if isinstance(value, str):
        return normalize_string(value, root)
    if isinstance(value, list):
        return [normalize(item, root) for item in value]
    if isinstance(value, tuple):
        return [normalize(item, root) for item in value]
    if isinstance(value, dict):
        return {normalize_string(key, root): normalize(item, root) for key, item in value.items()}
    return value


def file_entry(path: Path, root: Path, logical_hashes: dict[Path, str]) -> dict[str, Any]:
    raw = path.read_bytes()
    entry: dict[str, Any] = {
        "path": path.relative_to(root).as_posix(),
        "mode": path.stat().st_mode & 0o777,
    }
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError:
        normalized_raw = raw
        entry["base64"] = base64.b64encode(raw).decode("ascii")
    else:
        text = normalize_string(text, root)
        normalized_raw = text.encode("utf-8")
        entry["text"] = text
    entry["size"] = len(normalized_raw)
    entry["sha256"] = hashlib.sha256(normalized_raw).hexdigest()
    if path in logical_hashes:
        entry["logicalSha256"] = logical_hashes[path]
    return entry


def snapshot(root: Path, logical_hashes: dict[Path, str]) -> dict[str, Any]:
    directories = sorted(
        path.relative_to(root).as_posix()
        for path in root.rglob("*")
        if path.is_dir()
    )
    files = [
        file_entry(path, root, logical_hashes)
        for path in sorted(root.rglob("*"))
        if path.is_file()
    ]
    return {"directories": directories, "files": files}


ENDPOINTS: dict[tuple[str, str], tuple[Callable[..., Any], type[Any] | None]] = {
    ("GET", "/api/status"): (api.status, None),
    ("POST", "/api/game-directory"): (api.set_game_directory, api.GameDirectoryPayload),
    ("POST", "/api/config"): (api.set_config, api.ConfigValuePayload),
    ("POST", "/api/vram-target"): (api.vram_target, api.VramPayload),
    ("POST", "/api/t7-config"): (api.set_t7_config, api.T7ConfigPayload),
    ("POST", "/api/dxvk-config"): (api.dxvk_config, api.DxvkConfigPayload),
    ("POST", "/api/launch-options"): (api.launch_options, api.LaunchOptionsPayload),
    ("POST", "/api/intro-skip"): (api.intro_skip, api.TogglePayload),
    ("POST", "/api/d3dcompiler"): (api.d3dcompiler, api.TogglePayload),
    ("POST", "/api/all-intros-skip"): (api.all_intros_skip, api.TogglePayload),
    ("POST", "/api/config-readonly"): (api.config_readonly, api.TogglePayload),
}


async def call_endpoint(operation: dict[str, Any]) -> Any:
    function, model = ENDPOINTS[(operation["method"], operation["path"])]
    if model is None:
        return await function()
    return await function(model(**operation.get("body", {})))


def run_endpoint_operations(operations: list[dict[str, Any]]) -> list[Any]:
    if CLIENT is not None:
        results = []
        for operation in operations:
            response = CLIENT.request(operation["method"], operation["path"],
                                      json=operation.get("body"))
            response.raise_for_status()
            results.append(response.json())
        return results
    async def run() -> list[Any]:
        return [await call_endpoint(operation) for operation in operations]

    return asyncio.run(run())


class FakeResponse:
    def __init__(self, payload: dict[str, Any]) -> None:
        self.payload = payload

    def raise_for_status(self) -> None:
        return None

    def json(self) -> dict[str, Any]:
        return self.payload


def resolve_asset_cases() -> list[dict[str, Any]]:
    digest_a = "a" * 64
    digest_b = "b" * 64
    payloads = [
        {
            "label": "versioned legacy-compatible name",
            "assetKey": "compatible_archive",
            "release": {
                "assets": [
                    {
                        "name": "Linux.Steamdeck.and.Manual.Windows.Install.2.05.zip",
                        "browser_download_url": "https://fixtures.invalid/t7-2.05.zip",
                        "digest": f"sha256:{digest_a}",
                    }
                ]
            },
        },
        {
            "label": "renamed universal current asset",
            "assetKey": "current_archive",
            "release": {
                "assets": [
                    {
                        "name": "T7Patch-Linux-Windows-3.0.zip",
                        "browser_download_url": "https://fixtures.invalid/t7-3.0.zip",
                        "digest": f"sha256:{digest_b}",
                    }
                ]
            },
        },
    ]
    results = []
    original_get = t7_patch.requests.get
    try:
        for item in payloads:
            t7_patch._t7patch_release_assets_cache.clear()
            t7_patch.requests.get = lambda *_args, payload=item["release"], **_kwargs: FakeResponse(payload)
            url, hashes = t7_patch._resolve_t7patch_asset(item["assetKey"], api.log_target)
            results.append(
                {
                    "label": item["label"],
                    "assetKey": item["assetKey"],
                    "release": item["release"],
                    "resolved": {"url": url, "sha256": sorted(hashes)},
                }
            )
    finally:
        t7_patch.requests.get = original_get
        t7_patch._t7patch_release_assets_cache.clear()
    return results


def case_definitions() -> list[dict[str, Any]]:
    cases = [
        {"name": "status_no_game", "saved_game": False, "add_game": False,
         "operations": [{"method": "GET", "path": "/api/status"}]},
        {"name": "status_unverified_base", "exe_profile": "unverified",
         "operations": [{"method": "GET", "path": "/api/status"}]},
        {"name": "status_september_2026_current", "exe_profile": "current",
         "operations": [{"method": "GET", "path": "/api/status"}],
         "rustIgnore": "fixture uses a logical SHA-256 alias because redistributing the game EXE is impossible"},
        {"name": "status_compatible_build", "exe_profile": "compatible",
         "operations": [{"method": "GET", "path": "/api/status"}],
         "rustIgnore": "fixture uses a logical SHA-256 alias because redistributing the game EXE is impossible"},
        {"name": "status_steam_secondary_library", "saved_game": False,
         "game_in_library": True, "exe_profile": "unverified",
         "operations": [{"method": "GET", "path": "/api/status"}]},
        {"name": "game_directory_validation_invalid", "add_game": False,
         "operations": [{"method": "POST", "path": "/api/game-directory",
                         "body": {"path": "$ROOT/not-a-game"}}]},
        {"name": "game_directory_validation_valid", "saved_game": False,
         "operations": [{"method": "POST", "path": "/api/game-directory",
                         "body": {"path": "$ROOT/game"}}]},
        {"name": "config_graphics_advanced_write",
         "operations": [
             {"method": "POST", "path": "/api/config", "body": {"key": "MaxFPS", "value": 240, "comment": "fixture max fps"}},
             {"method": "POST", "path": "/api/config", "body": {"key": "FOV", "value": 110, "comment": "fixture fov"}},
             {"method": "POST", "path": "/api/config", "body": {"key": "DrawFPS", "value": 1, "comment": "fixture counter"}},
             {"method": "POST", "path": "/api/config", "body": {"key": "SerializeRender", "value": 2, "comment": "fixture cpu"}},
             {"method": "POST", "path": "/api/vram-target", "body": {"limited": True, "target": 85}},
         ]},
        {"name": "t7_config_write", "setup": "t7",
         "operations": [{"method": "POST", "path": "/api/t7-config",
                         "body": {"gamertag": "FixturePlayer", "colorCode": "^3",
                                  "networkPassword": "secret", "friendsOnly": True}}]},
        {"name": "dxvk_config_write", "setup": "dxvk",
         "operations": [{"method": "POST", "path": "/api/dxvk-config",
                         "body": {"enableAsync": False, "gplAsyncCache": False,
                                  "numCompilerThreads": 6, "maxFrameRate": 144,
                                  "maxFrameLatency": 2, "tearFree": "Auto", "hudEnabled": True}}]},
        {"name": "launch_options_apply", "launch_options": 'WINEDLLOVERRIDES="dsound=n,b" %command% -novid +set fs_game oldmod',
         "operations": [
             {"method": "POST", "path": "/api/launch-options",
              "body": {"options": "+set fs_game offlinemp", "preserve_fs_game": False}},
             {"method": "POST", "path": "/api/launch-options",
              "body": {"options": "", "preserve_fs_game": True}},
         ]},
        {"name": "qol_toggles",
         "operations": [
             {"method": "POST", "path": "/api/intro-skip", "body": {"enabled": True}},
             {"method": "POST", "path": "/api/d3dcompiler", "body": {"enabled": True}},
             {"method": "POST", "path": "/api/all-intros-skip", "body": {"enabled": True}},
         ]},
        {"name": "t7_release_asset_discovery", "add_game": False,
         "internal": "t7_release_asset_discovery",
         "rustIgnore": "release selection is private to the Rust install path and would otherwise download an archive"},
    ]
    cases += [
        {"name": "status_t7_installed", "setup": "t7"},
        {"name": "status_dxvk_installed", "setup": "dxvk"},
        {"name": "status_enhanced_installed", "setup": "enhanced"},
        {"name": "status_empty_config", "config": ""},
        {"name": "status_config_variants", "config":
         '// malformed and decimal input\nMaxFPS = "144.9"\nFOV = "bad"\n'
         'RefreshRate = "143.98"\nVideoMemory = "0.85"\nStreamMinResident = "1"\n'
         'RestrictGraphicsOptions = "0"\nSerializeRender = "2"\n'},
        {"name": "config_append_and_crlf", "config": 'FOV = "80"\r\n',
         "operations": [{"method": "POST", "path": "/api/config",
                         "body": {"key": "MaxFPS", "value": 200, "comment": "fixture appended"}}]},
        {"name": "config_missing_file", "remove_config": True,
         "operations": [{"method": "POST", "path": "/api/config",
                         "body": {"key": "FOV", "value": 90}}]},
        {"name": "t7_config_missing", "operations": [
            {"method": "POST", "path": "/api/t7-config", "body": {"gamertag": "Player"}}]},
        {"name": "t7_config_clear_and_append", "setup": "t7",
         "t7_config": 'unknown=keep\n', "operations": [
            {"method": "POST", "path": "/api/t7-config", "body": {
                "gamertag": "  ^1ColorName  ", "colorCode": "^2",
                "networkPassword": "  ", "friendsOnly": False}}]},
        {"name": "t7_config_invalid_name", "setup": "t7", "operations": [
            {"method": "POST", "path": "/api/t7-config", "body": {"gamertag": "  "}},
            {"method": "POST", "path": "/api/t7-config", "body": {"gamertag": "a" * 21}}]},
        {"name": "launch_options_unsupported", "operations": [
            {"method": "POST", "path": "/api/launch-options", "body": {"options": "-bad"}}]},
        {"name": "qol_restore_legacy", "setup": "legacy_backups", "operations": [
            {"method": "POST", "path": "/api/intro-skip", "body": {"enabled": False}},
            {"method": "POST", "path": "/api/d3dcompiler", "body": {"enabled": False}},
            {"method": "POST", "path": "/api/all-intros-skip", "body": {"enabled": False}}]},
        {"name": "qol_toggle_roundtrip", "operations": [
            {"method": "POST", "path": path, "body": {"enabled": enabled}}
            for enabled in (True, True, False)
            for path in ("/api/intro-skip", "/api/d3dcompiler", "/api/all-intros-skip")]},
        {"name": "exe_untrusted_backup_detection", "setup": "untrusted_backups"},
        {"name": "exe_preserved_enhanced_detection", "setup": "preserved_enhanced"},
        {"name": "status_alternate_executable", "alternate_exe": True},
        {"name": "config_readonly_roundtrip", "operations": [
            {"method": "POST", "path": "/api/config-readonly", "body": {"enabled": True}},
            {"method": "POST", "path": "/api/config", "body": {"key": "FOV", "value": 90}},
            {"method": "POST", "path": "/api/config-readonly", "body": {"enabled": False}}]},
        {"name": "vram_target_roundtrip", "operations": [
            {"method": "POST", "path": "/api/vram-target", "body": {"limited": True, "target": 100}},
            {"method": "POST", "path": "/api/vram-target", "body": {"limited": False, "target": 85}}]},
    ]
    for case in cases:
        case.setdefault("operations", [{"method": "GET", "path": "/api/status"}])
    return cases


def apply_setup(harness: Harness, definition: dict[str, Any]) -> None:
    setup = definition.get("setup")
    if setup == "t7":
        write(
            harness.game / "t7patch.conf",
            "playername=^1OldName\nnetworkpassword=old\nisfriendsonly=0\nunknown=keep\n",
        )
        write(harness.game / "t7patch.dll", b"fixture-t7")
        write(harness.game / "t7patchloader.dll", b"fixture-loader")
    elif setup == "dxvk":
        write(harness.game / "dxgi.dll", b"\x00\xfffixture-dxgi")
        write(harness.game / "d3d11.dll", b"fixture-d3d11")
        write(harness.game / "dxvk.conf", "dxvk.enableAsync=true\ndxgi.maxFrameLatency=1\n")
    elif setup == "enhanced":
        for filename in sorted(enhanced.EXPECTED_ENHANCED_FILES):
            write(harness.game / filename, b"fixture-enhanced")
        write(harness.data / enhanced.STATE_FILENAME, json.dumps({
            "installed": True, "detected_at": "2026-09-10T12:00:00Z",
            "acknowledged_at": "2026-09-10T13:00:00Z",
            "installed_files": sorted(enhanced.EXPECTED_ENHANCED_FILES)}))
    elif setup == "legacy_backups":
        for name in ("video/BO3_Global_Logo_LogoSequence.mkv", "video/Frontend_Intro.mkv", "d3dcompiler_46.dll"):
            path = harness.game / name
            path.rename(str(path) + utils.LEGACY_BACKUP_SUFFIX)
    elif setup == "untrusted_backups":
        for name in ("BlackOpsIII.exe.patchops.bak", "BlackOpsIII.24784313.bak", "BlackOpsIII.10650222.bak", "BlackOpsIII.enhanced.bak"):
            write(harness.game / name, b"not-a-trusted-executable")
    elif setup == "preserved_enhanced":
        raw = (harness.game / "BlackOpsIII.exe").read_bytes()
        settings_path = harness.data / "electron-settings.json"
        settings = json.loads(settings_path.read_text())
        settings["enhanced_exe_hashes"] = {str(harness.game): [hashlib.sha256(raw).hexdigest()]}
        write(settings_path, json.dumps(settings, indent=2))
        write(harness.game / "BlackOpsIII.enhanced.bak", raw)
    if "config" in definition:
        write(harness.game / "players/config.ini", definition["config"])
    if definition.get("remove_config"):
        (harness.game / "players/config.ini").unlink()
    if "t7_config" in definition:
        write(harness.game / "t7patch.conf", definition["t7_config"])
    if definition.get("alternate_exe"):
        (harness.game / "BlackOpsIII.exe").rename(harness.game / "BlackOps3.exe")

    if "launch_options" in definition:
        write(
            harness.steam / "userdata" / STEAM_ID / "config" / "localconfig.vdf",
            localconfig(definition["launch_options"]),
        )


def generate_case(definition: dict[str, Any], scratch: Path) -> dict[str, Any]:
    root = scratch / definition["name"]
    harness = make_harness(
        root,
        saved_game=definition.get("saved_game", True),
        game_in_library=definition.get("game_in_library", False),
    )
    if definition.get("add_game", True):
        add_game(harness, exe_profile=definition.get("exe_profile", "unverified"))
    (root / "not-a-game").mkdir(exist_ok=True)
    apply_setup(harness, definition)
    configure_modules(harness)

    fixture = snapshot(root, harness.logical_hashes)
    operations = normalize(definition.get("operations", []), root)
    runtime_operations = json.loads(json.dumps(operations).replace("$ROOT", str(root)))
    if definition.get("internal") == "t7_release_asset_discovery":
        responses: list[Any] = [resolve_asset_cases()]
        checkpoints = []
    else:
        responses = []
        checkpoints = []
        for operation in runtime_operations:
            responses.extend(run_endpoint_operations([operation]))
            checkpoints.append(snapshot(root, harness.logical_hashes))

    result: dict[str, Any] = {
        "schemaVersion": 1,
        "name": definition["name"],
        "sourceOfTruth": "Python backend on main",
        "runner": "FastAPI TestClient" if CLIENT is not None else "direct endpoint functions",
        "sourceHashes": {name: hashlib.sha256((REPO_ROOT / name).read_bytes()).hexdigest()
                         for name in ("backend/api.py", "utils.py", "t7_patch.py", "dxvk_manager.py", "bo3_enhanced.py")},
        "fixture": fixture,
        "operations": operations,
        "expected": {
            "responses": normalize(responses, root),
            "checkpoints": checkpoints,
            "filesystem": snapshot(root, harness.logical_hashes),
        },
    }
    if definition.get("internal"):
        result["internalOperation"] = definition["internal"]
    if definition.get("rustIgnore"):
        result["rustIgnore"] = definition["rustIgnore"]
    return result


def main() -> int:
    # Fixed group-writable creation mask makes new-file modes reproducible and
    # detects atomic replacements that fail to preserve a fixture's 0644 mode.
    os.umask(0o002)
    assert CURRENT_EXE_SHA256 == t7_patch.DEFAULT_STEAM_EXE_SHA256
    assert COMPATIBLE_EXE_SHA256 in api.COMPATIBLE_BUILD_SHA256
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail if checked-in goldens differ")
    args = parser.parse_args()
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="patchops-parity-") as temporary:
        scratch = Path(temporary)
        generated = {
            f"{definition['name']}.json": json.dumps(
                generate_case(definition, scratch), indent=2, sort_keys=True
            )
            + "\n"
            for definition in case_definitions()
        }

    failures = []
    for filename, contents in generated.items():
        target = OUTPUT_DIR / filename
        if args.check:
            if not target.exists() or target.read_text(encoding="utf-8") != contents:
                failures.append(filename)
        else:
            target.write_text(contents, encoding="utf-8")
            print(f"wrote {target.relative_to(REPO_ROOT)}")
    if args.check and failures:
        print("stale parity goldens: " + ", ".join(failures), file=sys.stderr)
        return 1
    if args.check:
        print(f"{len(generated)} parity goldens are deterministic and current")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
