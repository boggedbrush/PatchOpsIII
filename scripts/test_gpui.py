"""Lifecycle checks use harmless stand-ins; no game installation is modified."""
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("gpui_launcher", Path(__file__).with_name("gpui.py"))
launcher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(launcher)


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


if __name__ == "__main__":
    unittest.main()
