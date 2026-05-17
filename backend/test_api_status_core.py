from __future__ import annotations

import json
import os
import tempfile
import unittest
from contextlib import ExitStack
from pathlib import Path
from unittest.mock import patch

from backend import api


def _built_core_binary() -> Path | None:
    extension = ".exe" if os.name == "nt" else ""
    root = Path(__file__).resolve().parents[1]
    for mode in ("release", "debug"):
        candidate = root / "target" / mode / f"patchops-core{extension}"
        if candidate.is_file():
            return candidate
    return None


class StatusCoreIntegrationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.game_dir = self.root / "Call of Duty Black Ops III"
        self.players_dir = self.game_dir / "players"
        self.players_dir.mkdir(parents=True)
        self.exe = self.game_dir / "BlackOps3.exe"
        self.exe.write_bytes(b"bo3")
        self.config = self.players_dir / "config.ini"
        self.config.write_text('MaxFPS = "144"\nFOV = "90"\nDrawFPS = "1"\n', encoding="utf-8")
        self.settings_path = self.root / "settings.json"
        self.settings_path.write_text(json.dumps({"game_dir": str(self.game_dir)}), encoding="utf-8")
        self._reset_api_caches()

    def tearDown(self) -> None:
        self._reset_api_caches()
        self.tmp.cleanup()

    @staticmethod
    def _reset_api_caches() -> None:
        api._settings_cache = None
        api._game_dir_cache = None
        api._preset_names_cache = None
        api._presets_cache = None
        api._core_warning_logged = False

    def _status_patches(self, core_available: bool, core_call=None):
        patches = [
            patch.object(api, "SETTINGS_PATH", self.settings_path),
            patch.object(api, "core_available", return_value=core_available),
            patch.object(api, "get_steam_library_paths", return_value=[]),
            patch.object(api, "find_steam_user_id", return_value=None),
            patch.object(api, "get_workshop_item_state", return_value={}),
            patch.object(api, "_preset_names", return_value=[]),
            patch.object(api, "_enhanced_status", return_value={"installed": False}),
            patch.object(api, "status_summary", return_value={"installed": False}),
            patch.object(api, "detect_enhanced_install", return_value=False),
            patch.object(api, "read_exe_variant", return_value=""),
            patch.object(api, "_validated_latest_build_backup_path", return_value=None),
            patch.object(api, "_validated_preserved_compatible_exe", return_value=None),
            patch.object(api, "_validated_enhanced_backup_path", return_value=None),
        ]
        if core_call is not None:
            patches.append(patch.object(api, "core_call", side_effect=core_call))
        return patches

    def _current_state(self, core_available: bool, core_call=None) -> dict:
        with ExitStack() as stack:
            for item in self._status_patches(core_available, core_call):
                stack.enter_context(item)
            return api._current_state()

    def test_status_uses_python_fallback_when_core_is_unavailable(self) -> None:
        state = self._current_state(core_available=False)

        self.assertTrue(state["gameDetected"])
        self.assertTrue(state["configExists"])
        self.assertEqual(state["gameDir"], str(self.game_dir))
        self.assertEqual(state["graphics"]["maxFps"], 144)
        self.assertEqual(state["graphics"]["fov"], 90)
        self.assertFalse(state["dxvk"]["installed"])

    def test_status_merges_rust_core_status_and_config_when_available(self) -> None:
        def fake_core_call(command: str, payload: dict, timeout: int = 60) -> dict:
            if command == "status":
                return {
                    "ok": True,
                    "gameDetected": True,
                    "configExists": True,
                    "configReadonly": True,
                    "executable": str(self.exe),
                    "executableName": self.exe.name,
                    "executableHash": "abc123",
                    "t7Installed": True,
                    "t7ConfigExists": True,
                    "dxvkInstalled": True,
                }
            if command == "read-config":
                return {
                    "ok": True,
                    "configExists": True,
                    "path": str(self.config),
                    "values": {
                        "MaxFPS": "222",
                        "FOV": "111",
                        "DrawFPS": "0",
                    },
                }
            if command == "scan-steam":
                return {"ok": True, "gameDirs": []}
            raise AssertionError(f"unexpected core command: {command}")

        state = self._current_state(core_available=True, core_call=fake_core_call)

        self.assertTrue(state["gameDetected"])
        self.assertTrue(state["configExists"])
        self.assertTrue(state["advanced"]["configReadonly"])
        self.assertEqual(state["graphics"]["maxFps"], 222)
        self.assertEqual(state["graphics"]["fov"], 111)
        self.assertFalse(state["graphics"]["drawFps"])
        self.assertTrue(state["t7"]["installed"])
        self.assertTrue(state["t7"]["confExists"])
        self.assertTrue(state["dxvk"]["installed"])
        self.assertEqual(state["exeSwap"]["executableName"], self.exe.name)
        self.assertEqual(state["exeSwap"]["executableHash"], "abc123")

    def test_status_falls_back_to_python_when_rust_core_errors(self) -> None:
        def failing_core_call(command: str, payload: dict, timeout: int = 60) -> dict:
            raise api.CoreBridgeError(f"{command} failed")

        with patch.object(api, "write_log") as write_log:
            state = self._current_state(core_available=True, core_call=failing_core_call)

        self.assertTrue(state["gameDetected"])
        self.assertTrue(state["configExists"])
        self.assertEqual(state["graphics"]["maxFps"], 144)
        self.assertEqual(state["graphics"]["fov"], 90)
        self.assertFalse(state["advanced"]["configReadonly"])
        self.assertEqual(state["exeSwap"]["executableName"], self.exe.name)
        self.assertNotEqual(state["exeSwap"]["executableHash"], "abc123")
        self.assertTrue(write_log.called)
        warning_message = write_log.call_args.args[0]
        self.assertIn("Rust core", warning_message)
        self.assertIn("failed", warning_message)

    def test_core_warning_is_logged_once_across_repeated_failures(self) -> None:
        def failing_core_call(command: str, payload: dict, timeout: int = 60) -> dict:
            raise api.CoreBridgeError(f"{command} failed")

        with patch.object(api, "write_log") as write_log:
            self._current_state(core_available=True, core_call=failing_core_call)
            self._current_state(core_available=True, core_call=failing_core_call)

        self.assertEqual(write_log.call_count, 1)

    def test_status_uses_real_rust_core_when_binary_is_built(self) -> None:
        core_binary = _built_core_binary()
        if core_binary is None:
            self.skipTest("patchops-core binary is not built")

        (self.game_dir / "t7patch.dll").write_bytes(b"t7")
        (self.game_dir / "t7patch.conf").write_text("playername=PatchOps", encoding="utf-8")
        (self.game_dir / "dxgi.dll").write_bytes(b"dxgi")
        (self.game_dir / "d3d11.dll").write_bytes(b"d3d11")

        patches = [
            patch.object(api, "SETTINGS_PATH", self.settings_path),
            patch.object(api, "get_steam_library_paths", return_value=[]),
            patch.object(api, "find_steam_user_id", return_value=None),
            patch.object(api, "get_workshop_item_state", return_value={}),
            patch.object(api, "_preset_names", return_value=[]),
            patch.object(api, "_enhanced_status", return_value={"installed": False}),
            patch.object(api, "status_summary", return_value={"installed": False}),
            patch.object(api, "detect_enhanced_install", return_value=False),
            patch.object(api, "read_exe_variant", return_value=""),
            patch.object(api, "_validated_latest_build_backup_path", return_value=None),
            patch.object(api, "_validated_preserved_compatible_exe", return_value=None),
            patch.object(api, "_validated_enhanced_backup_path", return_value=None),
            patch.object(api, "_read_config", return_value=""),
            patch.object(api, "is_t7_patch_installed", return_value=False),
            patch.object(api, "is_dxvk_async_installed", return_value=False),
        ]

        with patch.dict(os.environ, {"PATCHOPSIII_CORE_BINARY": str(core_binary)}):
            with ExitStack() as stack:
                for item in patches:
                    stack.enter_context(item)
                state = api._current_state()

        self.assertTrue(state["gameDetected"])
        self.assertTrue(state["configExists"])
        self.assertEqual(state["graphics"]["maxFps"], 144)
        self.assertEqual(state["graphics"]["fov"], 90)
        self.assertTrue(state["t7"]["installed"])
        self.assertTrue(state["t7"]["confExists"])
        self.assertTrue(state["dxvk"]["installed"])


class CorsOriginsTests(unittest.TestCase):
    def test_cors_origins_include_all_dev_renderers(self) -> None:
        origins = set(api._cors_origins())

        self.assertIn("http://127.0.0.1:5173", origins)
        self.assertIn("http://127.0.0.1:5174", origins)
        self.assertIn("http://127.0.0.1:5175", origins)
        self.assertIn("http://tauri.localhost", origins)
        self.assertIn("tauri://localhost", origins)
        self.assertIn("app://patchopsiii", origins)


class SettingsPathTests(unittest.TestCase):
    def setUp(self) -> None:
        api._settings_cache = None
        api._game_dir_cache = None
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.settings_path = self.root / "patchops-settings.json"
        self.legacy_settings_path = self.root / "electron-settings.json"

    def tearDown(self) -> None:
        api._settings_cache = None
        api._game_dir_cache = None
        self.tmp.cleanup()

    def test_load_settings_reads_legacy_file_when_neutral_file_is_missing(self) -> None:
        self.legacy_settings_path.write_text(json.dumps({"game_dir": "legacy"}), encoding="utf-8")

        with patch.object(api, "SETTINGS_PATH", self.settings_path):
            with patch.object(api, "LEGACY_SETTINGS_PATH", self.legacy_settings_path):
                self.assertEqual(api._settings_read_path(), self.legacy_settings_path)
                self.assertEqual(api._load_settings()["game_dir"], "legacy")

    def test_load_settings_prefers_neutral_file_over_legacy_file(self) -> None:
        self.settings_path.write_text(json.dumps({"game_dir": "neutral"}), encoding="utf-8")
        self.legacy_settings_path.write_text(json.dumps({"game_dir": "legacy"}), encoding="utf-8")

        with patch.object(api, "SETTINGS_PATH", self.settings_path):
            with patch.object(api, "LEGACY_SETTINGS_PATH", self.legacy_settings_path):
                self.assertEqual(api._settings_read_path(), self.settings_path)
                self.assertEqual(api._load_settings()["game_dir"], "neutral")

    def test_settings_cache_tracks_source_path_when_neutral_file_appears(self) -> None:
        self.legacy_settings_path.write_text(json.dumps({"game_dir": "legacy"}), encoding="utf-8")

        with patch.object(api, "SETTINGS_PATH", self.settings_path):
            with patch.object(api, "LEGACY_SETTINGS_PATH", self.legacy_settings_path):
                self.assertEqual(api._load_settings()["game_dir"], "legacy")

                self.settings_path.write_text(json.dumps({"game_dir": "neutral"}), encoding="utf-8")
                legacy_mtime = api._file_mtime(self.legacy_settings_path)
                if legacy_mtime is not None:
                    os.utime(self.settings_path, (legacy_mtime, legacy_mtime))

                self.assertEqual(api._settings_read_path(), self.settings_path)
                self.assertEqual(api._load_settings()["game_dir"], "neutral")

    def test_game_directory_cache_tracks_source_path_when_neutral_file_appears(self) -> None:
        legacy_game = self.root / "legacy-game"
        neutral_game = self.root / "neutral-game"
        legacy_game.mkdir()
        neutral_game.mkdir()
        (legacy_game / "BlackOps3.exe").write_text("", encoding="utf-8")
        (neutral_game / "BlackOps3.exe").write_text("", encoding="utf-8")
        self.legacy_settings_path.write_text(json.dumps({"game_dir": str(legacy_game)}), encoding="utf-8")

        with patch.object(api, "SETTINGS_PATH", self.settings_path):
            with patch.object(api, "LEGACY_SETTINGS_PATH", self.legacy_settings_path):
                with patch.object(api, "get_steam_library_paths", return_value=[]):
                    with patch.object(api, "core_available", return_value=False):
                        self.assertEqual(api._find_game_directory(), str(legacy_game))

                        self.settings_path.write_text(json.dumps({"game_dir": str(neutral_game)}), encoding="utf-8")
                        legacy_mtime = api._file_mtime(self.legacy_settings_path)
                        if legacy_mtime is not None:
                            os.utime(self.settings_path, (legacy_mtime, legacy_mtime))

                        self.assertEqual(api._find_game_directory(), str(neutral_game))

    def test_save_settings_writes_neutral_file_without_rewriting_legacy_file(self) -> None:
        self.legacy_settings_path.write_text(json.dumps({"game_dir": "legacy"}), encoding="utf-8")

        with patch.object(api, "SETTINGS_PATH", self.settings_path):
            with patch.object(api, "LEGACY_SETTINGS_PATH", self.legacy_settings_path):
                api._save_settings({"game_dir": "neutral"})

        self.assertEqual(json.loads(self.settings_path.read_text(encoding="utf-8"))["game_dir"], "neutral")
        self.assertEqual(json.loads(self.legacy_settings_path.read_text(encoding="utf-8"))["game_dir"], "legacy")


class AppRootTests(unittest.TestCase):
    def test_app_root_can_come_from_resource_dir_env(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            with patch.dict(os.environ, {"PATCHOPSIII_RESOURCE_DIR": tmp}):
                self.assertEqual(api._resolve_app_root(), Path(tmp).resolve())

    def test_app_root_resource_env_can_point_at_tauri_parent(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            parent = Path(tmp)
            resource_dir = parent / "_up_"
            resource_dir.mkdir()
            (resource_dir / "presets.json").write_text("{}", encoding="utf-8")

            with patch.dict(os.environ, {"PATCHOPSIII_RESOURCE_DIR": str(parent)}):
                self.assertEqual(api._resolve_app_root(), resource_dir.resolve())

    def test_frozen_app_root_can_resolve_tauri_up_resource_dir(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            release_dir = Path(tmp) / "target" / "release"
            resource_dir = release_dir / "_up_"
            resource_dir.mkdir(parents=True)
            backend_exe = release_dir / "patchops-backend.exe"
            backend_exe.write_text("", encoding="utf-8")
            (resource_dir / "presets.json").write_text("{}", encoding="utf-8")

            with patch.dict(os.environ, {}, clear=True):
                with patch.object(api.sys, "frozen", True, create=True):
                    with patch.object(api.sys, "executable", str(backend_exe)):
                        self.assertEqual(api._resolve_app_root(), resource_dir.resolve())


class ParentWatchdogTests(unittest.TestCase):
    def tearDown(self) -> None:
        api._parent_watchdog_started = False

    def test_parent_process_alive_rejects_invalid_pid(self) -> None:
        self.assertFalse(api._parent_process_alive(0))
        self.assertFalse(api._parent_process_alive(-1))

    def test_parent_process_alive_accepts_current_pid(self) -> None:
        self.assertTrue(api._parent_process_alive(os.getpid()))

    def test_parent_watchdog_ignores_missing_or_invalid_env(self) -> None:
        with patch.dict(os.environ, {}, clear=True):
            api._start_parent_watchdog()
        self.assertFalse(api._parent_watchdog_started)

        with patch.dict(os.environ, {"PATCHOPSIII_PARENT_PID": "not-a-pid"}):
            with patch.object(api, "write_log") as write_log:
                api._start_parent_watchdog()
        self.assertFalse(api._parent_watchdog_started)
        self.assertTrue(write_log.called)


if __name__ == "__main__":
    unittest.main()
