"""Windows self-update utilities for PatchOpsIII."""
from __future__ import annotations

import hashlib
import os
import platform
import re
import shutil
import subprocess
import tempfile
import time
import zipfile
from dataclasses import dataclass
from typing import Optional, Tuple

import requests
from PySide6.QtCore import QObject, QThread, Signal, Qt, QUrl
from PySide6.QtGui import QDesktopServices
from PySide6.QtWidgets import QMessageBox

from utils import write_log

GITHUB_LATEST_RELEASE_URL = "https://api.github.com/repos/boggedbrush/PatchOpsIII/releases/latest"
GITHUB_RELEASE_PAGE_URL = "https://github.com/boggedbrush/PatchOpsIII/releases/latest"
CACHE_TTL_SECONDS = 15 * 60
GEAR_LEVER_URL = "https://flathub.org/en/apps/it.mijorus.gearlever"


@dataclass
class ReleaseInfo:
    """Metadata for a GitHub release asset relevant to Windows or Linux updates."""

    version: str
    name: str
    body: str
    asset_url: str
    asset_name: str
    asset_size: int
    asset_content_type: str
    page_url: str
    checksum_url: Optional[str] = None


def _normalize_version(value: str) -> Tuple[int, ...]:
    sanitized = value.strip()
    if sanitized.startswith("v"):
        sanitized = sanitized[1:]
    if not sanitized:
        return (0,)
    numeric_parts = re.split(r"[^0-9]+", sanitized)
    normalized = []
    for part in numeric_parts:
        if not part:
            continue
        try:
            normalized.append(int(part))
        except ValueError:
            break
    return tuple(normalized) if normalized else (0,)


def _download_checksum(url: str) -> Optional[str]:
    try:
        response = requests.get(url, timeout=15)
        response.raise_for_status()
    except requests.RequestException:
        return None
    content = response.text.strip()
    if not content:
        return None
    return content.split()[0]


def _select_windows_asset(release_data: dict) -> Optional[ReleaseInfo]:
    assets = release_data.get("assets", [])
    checksum_lookup = {}
    for asset in assets:
        name = asset.get("name", "")
        if not name:
            continue
        if name.lower().endswith((".sha256", ".sha512", ".digest")):
            checksum_lookup[name.rsplit(".", 1)[0]] = asset.get("browser_download_url")

    preferred_asset = None
    fallback_asset = None
    for asset in assets:
        name = asset.get("name", "")
        download_url = asset.get("browser_download_url")
        if not name or not download_url:
            continue
        lowered = name.lower()
        if lowered.endswith(".exe"):
            preferred_asset = asset
            break
        if lowered.endswith(".zip") and fallback_asset is None:
            fallback_asset = asset

    selected = preferred_asset or fallback_asset
    if not selected:
        return None

    asset_name = selected.get("name", "")
    checksum_url = checksum_lookup.get(asset_name)
    download_url = selected.get("browser_download_url")
    if not download_url:
        return None

    return ReleaseInfo(
        version=release_data.get("tag_name") or release_data.get("name") or "0.0.0",
        name=release_data.get("name") or "PatchOpsIII",
        body=release_data.get("body") or "",
        asset_url=download_url,
        asset_name=asset_name,
        asset_size=selected.get("size") or 0,
        asset_content_type=selected.get("content_type") or "application/octet-stream",
        page_url=release_data.get("html_url") or GITHUB_RELEASE_PAGE_URL,
        checksum_url=checksum_url,
    )


def _select_linux_asset(release_data: dict) -> Optional[ReleaseInfo]:
    """Return release metadata when an AppImage (or zsync) asset exists."""

    version = release_data.get("tag_name") or release_data.get("name") or "0.0.0"
    name = release_data.get("name") or "PatchOpsIII"
    body = release_data.get("body") or ""

    assets = release_data.get("assets", [])
    selected = None

    for asset in assets:
        asset_name = asset.get("name", "")
        if not asset_name:
            continue
        lowered = asset_name.lower()
        if lowered.endswith(".appimage"):
            selected = asset
            break
        if lowered.endswith(".appimage.zsync") and selected is None:
            selected = asset

    if not selected:
        return None

    download_url = selected.get("browser_download_url") or ""
    if not download_url:
        return None

    return ReleaseInfo(
        version=version,
        name=name,
        body=body,
        asset_url=download_url,
        asset_name=selected.get("name", "PatchOpsIII.AppImage"),
        asset_size=selected.get("size") or 0,
        asset_content_type=selected.get("content_type") or "application/octet-stream",
        page_url=release_data.get("html_url") or GITHUB_RELEASE_PAGE_URL,
    )


class UpdateCheckWorker(QThread):
    finished = Signal(object)
    failed = Signal(str)

    def __init__(
        self,
        current_version: str,
        api_url: str = GITHUB_LATEST_RELEASE_URL,
        *,
        asset_selector=_select_windows_asset,
    ):
        super().__init__()
        self.current_version = current_version
        self.api_url = api_url
        self.asset_selector = asset_selector

    def run(self) -> None:
        try:
            response = requests.get(self.api_url, timeout=30)
            response.raise_for_status()
            release_data = response.json()
            if release_data.get("draft") or release_data.get("prerelease"):
                self.finished.emit(None)
                return
            release = self.asset_selector(release_data)
            if not release or not release.asset_url:
                self.finished.emit(None)
                return
            if _normalize_version(release.version) <= _normalize_version(self.current_version):
                self.finished.emit(None)
                return
            self.finished.emit(release)
        except requests.RequestException as exc:
            self.failed.emit(str(exc))
        except ValueError as exc:
            self.failed.emit(f"Failed to parse release metadata: {exc}")


class UpdateDownloadWorker(QThread):
    progress = Signal(int)
    finished = Signal(str)
    failed = Signal(str)

    def __init__(self, release: ReleaseInfo):
        super().__init__()
        self.release = release
        self._checksum: Optional[str] = None

    def run(self) -> None:
        try:
            if self.release.checksum_url:
                self._checksum = _download_checksum(self.release.checksum_url)
            with requests.get(self.release.asset_url, stream=True, timeout=30) as response:
                response.raise_for_status()
                total_size = int(response.headers.get("Content-Length") or self.release.asset_size or 0)
                downloaded = 0
                fd, temp_path = tempfile.mkstemp(prefix="patchopsiii_update_", suffix=os.path.splitext(self.release.asset_name)[1])
                os.close(fd)
                sha256 = hashlib.sha256() if self._checksum else None
                with open(temp_path, "wb") as handle:
                    for chunk in response.iter_content(chunk_size=1024 * 512):
                        if not chunk:
                            continue
                        handle.write(chunk)
                        downloaded += len(chunk)
                        if sha256 is not None:
                            sha256.update(chunk)
                        if total_size:
                            percent = max(1, int(downloaded * 100 / total_size))
                            self.progress.emit(min(percent, 100))
                if sha256 is not None and self._checksum:
                    digest = sha256.hexdigest()
                    if digest.lower() != self._checksum.lower():
                        os.remove(temp_path)
                        raise ValueError("Checksum mismatch for downloaded update")
                self.progress.emit(100)
                self.finished.emit(temp_path)
        except Exception as exc:  # noqa: BLE001 - propagate all errors
            self.failed.emit(str(exc))


class WindowsUpdater(QObject):
    """Manage update discovery and installation for Windows builds."""

    check_started = Signal()
    check_failed = Signal(str)
    no_update_available = Signal()
    update_available = Signal(object)
    download_started = Signal(object)
    download_progress = Signal(int)
    download_failed = Signal(str)
    update_staged = Signal(object, str)

    def __init__(
        self,
        current_version: str,
        install_dir: str,
        executable_path: str,
        *,
        is_frozen: bool,
        api_url: str = GITHUB_LATEST_RELEASE_URL,
        log_widget=None,
    ) -> None:
        super().__init__()
        self.current_version = current_version
        self.install_dir = os.path.abspath(install_dir) if install_dir else ""
        self.executable_path = os.path.abspath(executable_path)
        self.is_frozen = is_frozen
        self.api_url = api_url
        self._log_widget = log_widget
        self._cached_result: Optional[Tuple[str, Optional[ReleaseInfo]]] = None
        self._last_check = 0.0
        self._check_worker: Optional[UpdateCheckWorker] = None
        self._download_worker: Optional[UpdateDownloadWorker] = None
        self._staged_script: Optional[str] = None
        self._staged_release: Optional[ReleaseInfo] = None

    def set_log_widget(self, widget) -> None:
        self._log_widget = widget

    def _log(self, message: str, category: str = "Info") -> None:
        write_log(message, category, self._log_widget)

    def check_for_updates(self, *, force: bool = False) -> None:
        if platform.system() != "Windows":
            self.check_failed.emit("Windows updater is only available on Windows.")
            return
        if self._check_worker is not None:
            return
        if not force and self._cached_result and (time.time() - self._last_check) < CACHE_TTL_SECONDS:
            state, release = self._cached_result
            if state == "available" and release:
                self.update_available.emit(release)
            else:
                self.no_update_available.emit()
            return
        self._log("Checking for updates...")
        self.check_started.emit()
        self._check_worker = UpdateCheckWorker(
            self.current_version,
            self.api_url,
            asset_selector=_select_windows_asset,
        )
        self._check_worker.finished.connect(self._on_check_finished)
        self._check_worker.failed.connect(self._on_check_failed)
        self._check_worker.start()

    def _on_check_finished(self, release: Optional[ReleaseInfo]) -> None:
        self._last_check = time.time()
        if release:
            self._cached_result = ("available", release)
            self._log(f"Update available: {release.version}", "Success")
            self.update_available.emit(release)
        else:
            self._cached_result = ("none", None)
            self._log("No updates available.", "Info")
            self.no_update_available.emit()
        self._check_worker = None

    def _on_check_failed(self, message: str) -> None:
        self._last_check = time.time()
        self._cached_result = None
        self._log(f"Update check failed: {message}", "Error")
        self.check_failed.emit(message)
        self._check_worker = None

    def download_update(self, release: ReleaseInfo) -> None:
        if self._download_worker is not None:
            return
        self._log(f"Starting download for PatchOpsIII {release.version}...")
        self.download_started.emit(release)
        self._download_worker = UpdateDownloadWorker(release)
        self._download_worker.progress.connect(self.download_progress.emit)
        self._download_worker.finished.connect(lambda path: self._on_download_finished(release, path))
        self._download_worker.failed.connect(self._on_download_failed)
        self._download_worker.start()

    def _on_download_finished(self, release: ReleaseInfo, path: str) -> None:
        self._log("Download completed. Preparing installation...", "Success")
        try:
            script = self._stage_update(release, path)
        except Exception as exc:  # noqa: BLE001
            self._log(f"Failed to stage update: {exc}", "Error")
            self.download_failed.emit(str(exc))
            if os.path.exists(path):
                os.remove(path)
            self._download_worker = None
            return
        self._staged_release = release
        self._staged_script = script
        self.update_staged.emit(release, script)
        self._download_worker = None

    def _on_download_failed(self, message: str) -> None:
        self._log(f"Update download failed: {message}", "Error")
        self.download_failed.emit(message)
        self._download_worker = None

    def _stage_update(self, release: ReleaseInfo, downloaded_path: str) -> str:
        if not self.is_frozen:
            raise RuntimeError("Automatic updates are only supported in packaged builds.")
        if not os.path.isdir(self.install_dir):
            raise RuntimeError("Unable to locate installation directory.")
        os.makedirs(self.install_dir, exist_ok=True)
        target_suffix = os.path.splitext(release.asset_name)[1]
        staged_path = os.path.join(self.install_dir, f"PatchOpsIII_update{target_suffix}")
        if os.path.exists(staged_path):
            if os.path.isdir(staged_path):
                shutil.rmtree(staged_path)
            else:
                os.remove(staged_path)
        if release.asset_name.lower().endswith(".zip"):
            extract_dir = staged_path
            os.makedirs(extract_dir, exist_ok=True)
            with zipfile.ZipFile(downloaded_path, "r") as archive:
                archive.extractall(extract_dir)
            os.remove(downloaded_path)
            return self._write_zip_swap_script(extract_dir)
        shutil.move(downloaded_path, staged_path)
        return self._write_exe_swap_script(staged_path)

    def _write_exe_swap_script(self, staged_executable: str) -> str:
        script_path = os.path.join(self.install_dir, "apply_patchopsiii_update.bat")
        backup_path = self.executable_path + ".old"
        lines = [
            "@echo off",
            "setlocal enableextensions",
            f"set PID={os.getpid()}",
            f"set TARGET={self.executable_path}",
            f"set UPDATED={staged_executable}",
            f"set BACKUP={backup_path}",
            ":wait_loop",
            "timeout /t 1 /nobreak >nul",
            "tasklist /FI \"PID eq %PID%\" | findstr /I \"%PID%\" >nul",
            "if %ERRORLEVEL%==0 goto wait_loop",
            "if exist \"%BACKUP%\" del /f /q \"%BACKUP%\"",
            "if exist \"%TARGET%\" move /y \"%TARGET%\" \"%BACKUP%\"",
            "move /y \"%UPDATED%\" \"%TARGET%\"",
            "start \"\" \"%TARGET%\"",
            "del /f /q \"%~f0\"",
        ]
        with open(script_path, "w", encoding="utf-8", newline="\r\n") as handle:
            handle.write("\n".join(lines))
        return script_path

    def _write_zip_swap_script(self, extract_dir: str) -> str:
        script_path = os.path.join(self.install_dir, "apply_patchopsiii_update.bat")
        lines = [
            "@echo off",
            "setlocal enableextensions",
            f"set PID={os.getpid()}",
            f"set SOURCE={extract_dir}",
            f"set TARGET={self.install_dir}",
            f"set EXECUTABLE={self.executable_path}",
            ":wait_loop",
            "timeout /t 1 /nobreak >nul",
            "tasklist /FI \"PID eq %PID%\" | findstr /I \"%PID%\" >nul",
            "if %ERRORLEVEL%==0 goto wait_loop",
            "xcopy \"%SOURCE%\" \"%TARGET%\" /E /H /K /Y /I",
            "rmdir /S /Q \"%SOURCE%\"",
            "start \"\" \"%EXECUTABLE%\"",
            "del /f /q \"%~f0\"",
        ]
        with open(script_path, "w", encoding="utf-8", newline="\r\n") as handle:
            handle.write("\n".join(lines))
        return script_path

    def apply_staged_update(self) -> None:
        if not self._staged_script or not os.path.exists(self._staged_script):
            raise RuntimeError("No staged update is available.")
        self._log("Launching update installer and exiting...")
        cmd = [self._staged_script]

        # On Windows, wrap the batch in a VBS shim to avoid showing a console window.
        if os.name == "nt":
            vbs_path = os.path.join(self.install_dir, "run_patchopsiii_update.vbs")
            try:
                with open(vbs_path, "w", encoding="utf-8") as vbs:
                    vbs.write(
                        'Set WshShell = CreateObject("WScript.Shell")\n'
                        f'WshShell.Run """{self._staged_script}""", 0, False\n'
                    )
                creationflags = subprocess.CREATE_NO_WINDOW if hasattr(subprocess, "CREATE_NO_WINDOW") else 0
                subprocess.Popen(
                    ["wscript.exe", vbs_path],
                    creationflags=creationflags,
                    startupinfo=self._hidden_startupinfo(),
                )
                return
            except Exception as exc:  # noqa: BLE001
                # Fallback to the normal batch invocation if VBS shim fails
                self._log(f"VBS wrapper failed, falling back to visible script: {exc}", "Warning")

        try:
            subprocess.Popen(
                cmd,
                creationflags=self._no_window_flags(),
                startupinfo=self._hidden_startupinfo(),
            )
        except OSError as exc:
            raise RuntimeError(f"Failed to launch update script: {exc}") from exc

    def _no_window_flags(self) -> int:
        if os.name != "nt":
            return 0
        return subprocess.CREATE_NO_WINDOW if hasattr(subprocess, "CREATE_NO_WINDOW") else 0

    def _hidden_startupinfo(self):
        if os.name != "nt" or not hasattr(subprocess, "STARTUPINFO"):
            return None
        startupinfo = subprocess.STARTUPINFO()
        if hasattr(subprocess, "STARTF_USESHOWWINDOW"):
            startupinfo.dwFlags |= subprocess.STARTF_USESHOWWINDOW
            startupinfo.wShowWindow = 0
        return startupinfo

    def reset(self) -> None:
        self._cached_result = None
        self._last_check = 0.0
        self._staged_script = None
        self._staged_release = None


def _launch_gear_lever(log_callback) -> bool:
    """Attempt to launch Gear Lever via Flatpak first, then native binary."""

    candidates = [
        ["flatpak", "run", "it.mijorus.gearlever"],
        ["gearlever"],
    ]

    for command in candidates:
        try:
            proc = subprocess.Popen(
                command,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            # If the process dies immediately with a non-zero exit code, treat as a failure
            time.sleep(1.0)
            if proc.poll() is not None and proc.returncode:
                raise RuntimeError(f"Exited early with code {proc.returncode}")
            log_callback(
                "Launching Gear Lever to manage the PatchOpsIII update.",
                "Success",
            )
            return True
        except FileNotFoundError:
            log_callback(
                f"Command not found while attempting to launch Gear Lever: {' '.join(command)}",
                "Warning",
            )
        except Exception as exc:  # noqa: BLE001 - log unexpected errors
            log_callback(
                f"Failed to launch {' '.join(command)}: {exc}",
                "Error",
            )

    return False


def _show_gear_lever_required(parent, log_callback) -> None:
    """Inform the user that Gear Lever must be installed for automatic updates."""

    log_callback(
        "Gear Lever is required for automatic updates on Linux.",
        "Warning",
    )

    message_box = QMessageBox(parent)
    message_box.setWindowTitle("Gear Lever Required")
    message_box.setIcon(QMessageBox.Warning)
    message_box.setText(
        "Automatic updates on Linux require Gear Lever to be installed."
    )
    message_box.setInformativeText(
        f"Install Gear Lever from Flathub to enable automatic updates.\n{GEAR_LEVER_URL}"
    )
    message_box.setStandardButtons(QMessageBox.Open | QMessageBox.Close)
    message_box.setDefaultButton(QMessageBox.Open)
    message_box.setTextFormat(Qt.PlainText)
    message_box.setTextInteractionFlags(Qt.TextSelectableByMouse | Qt.LinksAccessibleByMouse)

    result = message_box.exec()
    if result == QMessageBox.Open:
        QDesktopServices.openUrl(QUrl(GEAR_LEVER_URL))


def prompt_linux_update(
    parent,
    current_version: str,
    *,
    api_url: str = GITHUB_LATEST_RELEASE_URL,
    log_widget=None,
) -> None:
    """Check for Linux updates and prompt the user to launch Gear Lever if needed."""

    if platform.system() != "Linux":
        return

    def log(message: str, category: str = "Info") -> None:
        write_log(message, category, log_widget)

    if getattr(parent, "_linux_update_worker", None):
        return

    log("Checking for Linux updates...", "Info")

    worker = UpdateCheckWorker(
        current_version,
        api_url,
        asset_selector=_select_linux_asset,
    )
    parent._linux_update_worker = worker

    def _cleanup() -> None:
        if getattr(parent, "_linux_update_worker", None) is worker:
            parent._linux_update_worker = None
        worker.deleteLater()

    def _handle_finished(release: Optional[ReleaseInfo]) -> None:
        _cleanup()
        if not release:
            log("No updates available for the Linux build.")
            return

        message = (
            f"PatchOpsIII {release.version} is available for download.\n\n"
            "Would you like to launch Gear Lever to apply the update now?"
        )

        response = QMessageBox.question(
            parent,
            "Update Available",
            message,
            QMessageBox.Yes | QMessageBox.No,
        )
        if response == QMessageBox.Yes:
            if not _launch_gear_lever(log):
                _show_gear_lever_required(parent, log)
        else:
            log("User deferred the Linux update.")

    def _handle_failed(message: str) -> None:
        _cleanup()
        log(f"Linux update check failed: {message}", "Error")

    worker.finished.connect(_handle_finished)
    worker.failed.connect(_handle_failed)
    worker.start()
