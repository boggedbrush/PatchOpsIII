"""Lifecycle and packaging checks use stand-ins; no game installation is modified."""
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
from zipfile import ZipFile

spec = importlib.util.spec_from_file_location("gpui_launcher", Path(__file__).with_name("gpui.py"))
launcher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(launcher)

package_spec = importlib.util.spec_from_file_location("gpui_packager", Path(__file__).with_name("package_gpui.py"))
packager = importlib.util.module_from_spec(package_spec)
package_spec.loader.exec_module(packager)


class LifecycleTests(unittest.TestCase):
    def test_relative_target_directory_is_consistent_for_build_and_run(self):
        with patch.dict(os.environ, {"CARGO_TARGET_DIR": "build/custom-gpui"}):
            self.assertEqual(launcher.target_directory(), launcher.ROOT / "build/custom-gpui")

    def test_stop_reaps_child(self):
        process = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])
        launcher.stop(process)
        self.assertIsNotNone(process.poll())
        launcher.stop(process)  # idempotent

    def test_startup_failure_cleans_service(self):
        process = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])
        try:
            with patch.object(launcher.subprocess, "Popen", return_value=process), patch.object(launcher, "wait_ready", side_effect=RuntimeError("unhealthy")):
                with self.assertRaisesRegex(RuntimeError, "unhealthy"):
                    launcher.run(Path(sys.executable))
            self.assertIsNotNone(process.poll())
        finally:
            launcher.stop(process)

    def test_early_service_exit_is_reported(self):
        process = subprocess.Popen([sys.executable, "-c", "raise SystemExit(7)"])
        process.wait()
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(RuntimeError, "code 7"):
                launcher.wait_ready(process, Path(directory) / "ready.json")

    def test_missing_executable_does_not_start_service(self):
        with patch.object(launcher.subprocess, "Popen") as popen:
            with self.assertRaisesRegex(RuntimeError, "not found"):
                launcher.run(Path("/missing/patchopsiii-gpui"))
            popen.assert_not_called()

    def test_ui_failure_stops_owned_service(self):
        service = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])
        app = subprocess.Popen([sys.executable, "-c", "raise SystemExit(9)"])
        app.wait()
        try:
            with patch.object(launcher.subprocess, "Popen", side_effect=[service, app]), patch.object(launcher, "wait_ready", return_value="http://127.0.0.1:12345"):
                self.assertEqual(launcher.run(Path(sys.executable)), 9)
            self.assertIsNotNone(service.poll())
        finally:
            launcher.stop(service)


class PackagingTests(unittest.TestCase):
    def test_archive_is_self_contained_and_matches_native_lookup_layout(self):
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory)
            executable = temporary / ("patchopsiii-gpui.exe" if os.name == "nt" else "patchopsiii-gpui")
            backend = temporary / ("patchops-backend.exe" if os.name == "nt" else "patchops-backend")
            executable.write_bytes(b"native")
            backend.write_bytes(b"backend")
            output = temporary / "PatchOpsIII-native.zip"

            packager.create_archive(executable, backend, output)

            with ZipFile(output) as archive:
                names = set(archive.namelist())
                self.assertIn(f"PatchOpsIII/{executable.name}", names)
                self.assertIn(f"PatchOpsIII/resources/backend-bin/{backend.name}", names)
                self.assertIn("PatchOpsIII/resources/presets.json", names)
                self.assertIn("PatchOpsIII/resources/package.json", names)
                self.assertIn("PatchOpsIII/resources/PatchOpsIII.ico", names)
                self.assertIn("PatchOpsIII/resources/icon-512.png", names)
                self.assertIn("PatchOpsIII/THIRD-PARTY-NOTICES/Lucide.txt", names)
                self.assertFalse(any(name.endswith(".py") for name in names))
                self.assertEqual(
                    archive.read(f"PatchOpsIII/resources/backend-bin/{backend.name}"),
                    b"backend",
                )

    def test_archive_requires_both_product_executables(self):
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory)
            executable = temporary / "patchopsiii-gpui"
            executable.write_bytes(b"native")
            with self.assertRaisesRegex(FileNotFoundError, "patchops-backend"):
                packager.archive_manifest(executable, temporary / "patchops-backend")


if __name__ == "__main__":
    unittest.main()
