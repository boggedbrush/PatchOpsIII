from __future__ import annotations

import hashlib
import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from backend import core_bridge


class CoreBridgeTests(unittest.TestCase):
    def test_core_available_false_when_binary_missing(self) -> None:
        with patch.object(core_bridge, "_resolve_core_binary", return_value=None):
            self.assertFalse(core_bridge.core_available())

    def test_core_call_raises_when_binary_missing(self) -> None:
        with patch.object(core_bridge, "_resolve_core_binary", return_value=None):
            with self.assertRaises(core_bridge.CoreUnavailableError):
                core_bridge.core_call("status", {"gameDir": "missing"})

    def test_core_call_raises_on_non_zero_exit(self) -> None:
        result = subprocess.CompletedProcess(["patchops-core"], 1, "", "boom")
        with patch.object(core_bridge, "_resolve_core_binary", return_value=Path("patchops-core")):
            with patch.object(core_bridge, "_run_core", return_value=result):
                with self.assertRaisesRegex(core_bridge.CoreBridgeError, "boom"):
                    core_bridge.core_call("status", {"gameDir": "x"})

    def test_core_call_raises_on_invalid_json(self) -> None:
        result = subprocess.CompletedProcess(["patchops-core"], 0, "not-json", "")
        with patch.object(core_bridge, "_resolve_core_binary", return_value=Path("patchops-core")):
            with patch.object(core_bridge, "_run_core", return_value=result):
                with self.assertRaisesRegex(core_bridge.CoreBridgeError, "invalid JSON"):
                    core_bridge.core_call("status", {"gameDir": "x"})

    def test_core_call_hash_uses_real_binary_when_available(self) -> None:
        if core_bridge._resolve_core_binary() is None:
            self.skipTest("patchops-core binary is not built")

        with tempfile.TemporaryDirectory() as tmp:
            sample = Path(tmp) / "sample.bin"
            content = b"patchops-core-bridge"
            sample.write_bytes(content)

            response = core_bridge.core_call("hash", {"path": str(sample)}, timeout=10)

        self.assertTrue(response["ok"])
        self.assertEqual(response["sha256"], hashlib.sha256(content).hexdigest())

    def test_app_root_can_come_from_resource_dir_env(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            with patch.dict(os.environ, {"PATCHOPSIII_RESOURCE_DIR": tmp}):
                self.assertEqual(core_bridge._resolve_app_root(), Path(tmp).resolve())

    def test_app_root_resource_env_can_point_at_tauri_parent(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            parent = Path(tmp)
            resource_dir = parent / "_up_"
            resource_dir.mkdir()
            (resource_dir / core_bridge._binary_name()).write_text("", encoding="utf-8")

            with patch.dict(os.environ, {"PATCHOPSIII_RESOURCE_DIR": str(parent)}):
                self.assertEqual(core_bridge._resolve_app_root(), resource_dir.resolve())

    def test_frozen_app_root_can_resolve_tauri_sidecar_parent(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            release_dir = Path(tmp) / "target" / "release"
            release_dir.mkdir(parents=True)
            backend_exe = release_dir / "patchops-backend.exe"
            backend_exe.write_text("", encoding="utf-8")
            (release_dir / core_bridge._binary_name()).write_text("", encoding="utf-8")

            with patch.dict(os.environ, {}, clear=True):
                with patch.object(core_bridge.sys, "frozen", True, create=True):
                    with patch.object(core_bridge.sys, "executable", str(backend_exe)):
                        self.assertEqual(core_bridge._resolve_app_root(), release_dir.resolve())


if __name__ == "__main__":
    unittest.main()
