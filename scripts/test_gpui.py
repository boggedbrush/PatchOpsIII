"""Native launcher and archive checks; no game installation is modified."""
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
    def test_default_target_is_workspace_target(self):
        with patch.dict(os.environ, {}, clear=True):
            self.assertEqual(launcher.target_directory(), launcher.ROOT / "native" / "target")

    def test_relative_target_directory_is_consistent_for_build_and_run(self):
        with patch.dict(os.environ, {"CARGO_TARGET_DIR": "build/custom-gpui"}):
            self.assertEqual(launcher.target_directory(), launcher.ROOT / "build/custom-gpui")

    def test_stop_reaps_child(self):
        process = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])
        launcher.stop(process)
        self.assertIsNotNone(process.poll())
        launcher.stop(process)

    def test_missing_executable_starts_no_process(self):
        with patch.object(launcher.subprocess, "Popen") as popen:
            with self.assertRaisesRegex(RuntimeError, "not found"):
                launcher.run(Path("/missing/patchopsiii-gpui"))
            popen.assert_not_called()

    def test_runs_only_native_executable_and_preserves_exit_status(self):
        process = subprocess.Popen([sys.executable, "-c", "raise SystemExit(9)"])
        process.wait()
        with patch.object(launcher.subprocess, "Popen", return_value=process) as popen:
            self.assertEqual(launcher.run(Path(sys.executable)), 9)
            popen.assert_called_once_with([sys.executable], cwd=launcher.ROOT)

    def test_interrupted_wait_reaps_native_child(self):
        process = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])
        try:
            with patch.object(launcher.subprocess, "Popen", return_value=process), patch.object(process, "wait", side_effect=[KeyboardInterrupt, None]):
                with self.assertRaises(KeyboardInterrupt):
                    launcher.run(Path(sys.executable))
            process.wait(timeout=5)
            self.assertIsNotNone(process.poll())
        finally:
            launcher.stop(process)


class PackagingTests(unittest.TestCase):
    def test_archive_contains_one_rust_executable_and_no_python_backend(self):
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory)
            executable = temporary / ("patchopsiii-gpui.exe" if os.name == "nt" else "patchopsiii-gpui")
            executable.write_bytes(b"native")
            output = temporary / "PatchOpsIII-native.zip"
            packager.create_archive(executable, output)
            with ZipFile(output) as archive:
                names = set(archive.namelist())
                self.assertIn(f"PatchOpsIII/{executable.name}", names)
                self.assertIn("PatchOpsIII/resources/presets.json", names)
                self.assertIn("PatchOpsIII/resources/package.json", names)
                self.assertIn("PatchOpsIII/resources/PatchOpsIII.ico", names)
                self.assertIn("PatchOpsIII/resources/icon-512.png", names)
                self.assertIn("PatchOpsIII/THIRD-PARTY-NOTICES/Lucide.txt", names)
                self.assertFalse(any("backend-bin" in name or name.endswith(".py") or "patchops-backend" in name for name in names))
                self.assertEqual(archive.read(f"PatchOpsIII/{executable.name}"), b"native")

    def test_archive_requires_native_executable(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(FileNotFoundError, "patchopsiii-gpui"):
                packager.archive_manifest(Path(directory) / "patchopsiii-gpui")


if __name__ == "__main__":
    unittest.main()
