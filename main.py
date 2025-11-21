#!/usr/bin/env python
import sys
import os
import re
import errno
import shutil
import time
import tempfile
import vdf
import platform
import argparse
import json
from functools import lru_cache
from typing import Optional
from PySide6.QtWidgets import (
    QApplication, QMainWindow, QWidget, QVBoxLayout, QHBoxLayout, QLineEdit,
    QPushButton, QLabel, QFileDialog, QTextEdit, QTabWidget, QSizePolicy,
    QGroupBox, QRadioButton, QButtonGroup, QCheckBox, QGridLayout,
    QMessageBox
)
from PySide6.QtGui import QIcon, QDesktopServices
from PySide6.QtCore import Qt, QUrl, QThread, Signal, QTimer

from t7_patch import T7PatchWidget, is_admin
from dxvk_manager import DXVKWidget
from config import GraphicsSettingsWidget, AdvancedSettingsWidget
from updater import ReleaseInfo, WindowsUpdater, prompt_linux_update
from utils import write_log, apply_launch_options, find_steam_user_id, steam_userdata_path, app_id, launch_game_via_steam
from version import APP_VERSION


NUITKA_ENVIRONMENT_KEYS = (
    "NUITKA_ONEFILE_PARENT",
    "NUITKA_EXE_PATH",
    "NUITKA_PACKAGE_HOME",
)

_NUITKA_DETECTION_KEYS = NUITKA_ENVIRONMENT_KEYS + ("NUITKA_ONEFILE_TEMP",)


def _normalize_dir(path):
    """Return a valid directory path derived from the given value."""
    if not path:
        return None

    candidate = os.path.abspath(path)
    if os.path.isdir(candidate):
        return candidate

    if os.path.isfile(candidate):
        parent = os.path.dirname(candidate)
        if os.path.isdir(parent):
            return parent

    parent = os.path.dirname(candidate)
    if parent and os.path.isdir(parent):
        return parent

    return None


def _is_frozen_environment():
    """Determine if the application is running from a frozen executable."""
    if getattr(sys, "frozen", False):
        return True

    if getattr(sys, "_MEIPASS", None):
        return True

    for key in _NUITKA_DETECTION_KEYS:
        if os.environ.get(key):
            return True

    return False


def _get_true_executable_path():
    """Get the absolute path to the running executable using OS-specific methods.

    This function uses platform-specific APIs to reliably determine the actual
    location of the executable, bypassing limitations of sys.executable and
    sys.argv which may point to temporary extraction directories in frozen builds.

    Returns:
        str: Absolute directory path of the executable, or None if detection fails
    """
    system = platform.system()

    if system == "Windows":
        try:
            import ctypes
            # Use kernel32.GetModuleFileNameW for reliable path on Windows
            buffer = ctypes.create_unicode_buffer(32768)
            get_module_filename = ctypes.windll.kernel32.GetModuleFileNameW
            get_module_filename(None, buffer, len(buffer))
            exe_path = buffer.value
            if exe_path and os.path.exists(exe_path):
                write_log(f"Resolved Windows executable path: {exe_path}", "Info", None)
                return os.path.dirname(exe_path)
        except Exception as e:
            write_log(f"Windows path resolution failed: {e}", "Warning", None)

    elif system == "Linux":
        try:
            # Read /proc/self/exe symlink for true executable path
            exe_path = os.path.realpath('/proc/self/exe')
            if exe_path and os.path.exists(exe_path):
                write_log(f"Resolved Linux executable path: {exe_path}", "Info", None)
                return os.path.dirname(exe_path)
        except Exception as e:
            write_log(f"Linux path resolution failed: {e}", "Warning", None)

    elif system == "Darwin":
        # macOS support (optional - only if needed in future)
        try:
            import ctypes
            from ctypes.util import find_library

            libc = ctypes.CDLL(find_library('c'))
            buffer = ctypes.create_string_buffer(1024)
            size = ctypes.c_uint32(len(buffer))
            if libc._NSGetExecutablePath(buffer, ctypes.byref(size)) == 0:
                exe_path = os.path.realpath(buffer.value.decode())
                if exe_path and os.path.exists(exe_path):
                    write_log(f"Resolved macOS executable path: {exe_path}", "Info", None)
                    return os.path.dirname(exe_path)
        except Exception as e:
            write_log(f"macOS path resolution failed: {e}", "Warning", None)

    write_log(f"Platform-specific path resolution not available for {system}", "Warning", None)
    return None


@lru_cache(maxsize=1)
def _frozen_base_directory():
    """Resolve the persistent location of a frozen executable.

    Priority order:
    1. Platform-specific OS API (most reliable)
    2. Nuitka environment variables
    3. sys.argv[0] (if not in temp directory)
    4. sys.executable (if not in temp directory)

    Returns:
        str: Absolute directory path, or None if unable to resolve
    """
    if not _is_frozen_environment():
        return None

    temp_dir = os.path.normcase(os.path.abspath(tempfile.gettempdir()))

    # Priority 1: Use platform-specific API
    true_path = _get_true_executable_path()
    if true_path:
        normalized_true = os.path.normcase(true_path)
        if not normalized_true.startswith(temp_dir):
            write_log(f"Using platform-specific path: {true_path}", "Info", None)
            return true_path

    # Priority 2: Check Nuitka environment variables
    for key in NUITKA_ENVIRONMENT_KEYS:
        env_path = os.environ.get(key)
        if env_path:
            normalized = _normalize_dir(env_path)
            if normalized:
                normalized_case = os.path.normcase(normalized)
                if not normalized_case.startswith(temp_dir):
                    write_log(f"Using Nuitka environment variable {key}: {normalized}", "Info", None)
                    return normalized

    # Priority 3: Try sys.argv[0]
    if sys.argv and sys.argv[0]:
        argv_path = os.path.abspath(sys.argv[0])
        if os.path.isfile(argv_path):
            argv_case = os.path.normcase(argv_path)
            if not argv_case.startswith(temp_dir):
                argv_dir = os.path.dirname(argv_path)
                write_log(f"Using sys.argv[0]: {argv_dir}", "Info", None)
                return argv_dir

    # Priority 4: Last resort - sys.executable (but avoid temp directories)
    exe_path = os.path.abspath(sys.executable)
    exe_case = os.path.normcase(exe_path)
    if not exe_case.startswith(temp_dir):
        exe_dir = os.path.dirname(exe_path)
        write_log(f"Using sys.executable: {exe_dir}", "Warning", None)
        return exe_dir

    write_log("Could not resolve frozen executable path outside temp directory", "Error", None)
    return None


def resource_path(relative_path):
    """Resolve bundled assets in Nuitka and AppImage builds."""
    candidates = []

    meipass = getattr(sys, "_MEIPASS", None)
    if meipass:
        candidates.append(os.path.join(meipass, relative_path))

    appdir = os.environ.get("APPDIR")
    if appdir:
        candidates.extend([
            os.path.join(appdir, relative_path),
            os.path.join(appdir, "usr", relative_path),
            os.path.join(appdir, "usr", "share", relative_path),
            os.path.join(appdir, "usr", "share", "icons", "hicolor", "256x256", "apps", relative_path),
        ])

    if _is_frozen_environment():
        frozen_base = _frozen_base_directory()
        if frozen_base:
            candidates.append(os.path.join(frozen_base, relative_path))
        exe_dir = os.path.dirname(os.path.abspath(sys.executable))
        candidates.append(os.path.join(exe_dir, relative_path))
        nuitka_temp = os.environ.get("NUITKA_ONEFILE_TEMP")
        if nuitka_temp:
            candidates.append(os.path.join(nuitka_temp, relative_path))

    candidates.append(os.path.join(os.path.dirname(os.path.abspath(__file__)), relative_path))
    candidates.append(os.path.join(os.path.abspath("."), relative_path))

    seen = set()
    for candidate in candidates:
        normalized = os.path.normpath(candidate)
        if normalized in seen:
            continue
        seen.add(normalized)
        if os.path.exists(candidate):
            return candidate

    return None


def load_application_icon():
    """Return a QIcon and description for the application icon."""
    theme_name = "patchopsiii"
    if QIcon.hasThemeIcon(theme_name):
        icon = QIcon.fromTheme(theme_name)
        if not icon.isNull():
            return icon, f"theme:{theme_name}"

    icon_candidates = [
        "PatchOpsIII.ico",
        "patchopsiii.png",
        os.path.join("icons", "patchopsiii.png"),
        os.path.join("packaging", "appimage", "icons", "patchopsiii.png"),
    ]

    for candidate in icon_candidates:
        resolved = resource_path(candidate)
        if not resolved or not os.path.exists(resolved):
            continue
        icon = QIcon(resolved)
        if not icon.isNull():
            return icon, resolved

    return QIcon(), None

@lru_cache(maxsize=1)
def get_application_path():
    """Get the real application path for both script and frozen executables.

    This function determines where the application is actually installed,
    ensuring mod files and settings are stored in the correct location.

    Returns:
        str: Absolute path to the application directory

    Raises:
        SystemExit: If unable to determine a valid application path
    """
    base_dir = _frozen_base_directory()
    if base_dir and os.path.exists(base_dir):
        write_log(f"Application path (frozen): {base_dir}", "Info", None)
        return base_dir

    # Development mode or fallback
    base_dir = os.path.dirname(os.path.abspath(__file__))

    if _is_frozen_environment():
        error_msg = (
            "Could not determine application installation directory. "
            "Please ensure the executable is in a writable location (not Program Files or temp directories). "
            f"Attempted path: {base_dir or 'unknown'}"
        )
        _show_install_location_error(error_msg)

    write_log(f"Application path (development): {base_dir}", "Info", None)
    return base_dir

GAME_EXECUTABLE_NAMES = ("BlackOpsIII.exe", "BlackOps3.exe")


def _settings_file_path():
    base_path = STORAGE_PATH if 'STORAGE_PATH' in globals() and STORAGE_PATH else get_application_path()
    return os.path.join(base_path, "PatchOpsIII_settings.json")


def _has_game_executable(directory):
    if not directory or not os.path.isdir(directory):
        return False
    for executable in GAME_EXECUTABLE_NAMES:
        if os.path.exists(os.path.join(directory, executable)):
            return True
    return False


def _load_saved_game_directory():
    settings_path = _settings_file_path()
    if not os.path.exists(settings_path):
        return None

    try:
        with open(settings_path, "r", encoding="utf-8") as handle:
            data = json.load(handle)
    except (OSError, json.JSONDecodeError):
        return None

    saved_dir = data.get("game_directory")
    if saved_dir and _has_game_executable(saved_dir):
        return saved_dir
    return None


def save_game_directory(directory):
    settings_path = _settings_file_path()
    data = {}

    if os.path.exists(settings_path):
        try:
            with open(settings_path, "r", encoding="utf-8") as handle:
                data = json.load(handle)
        except (OSError, json.JSONDecodeError):
            data = {}

    data["game_directory"] = directory

    try:
        with open(settings_path, "w", encoding="utf-8") as handle:
            json.dump(data, handle, indent=4)
        return True
    except OSError as exc:
        write_log(f"Failed to save game directory: {exc}", "Error")
        return False


def _migrate_settings_if_needed(current_path):
    """Migrate settings from old location if they exist.

    This handles cases where the application previously detected the wrong path
    (e.g., in AppData temp directories) and needs to migrate settings to the
    correct location.

    Args:
        current_path: The newly detected correct application path
    """
    current_settings = os.path.join(current_path, "PatchOpsIII_settings.json")

    # If settings already exist in current location, no migration needed
    if os.path.exists(current_settings):
        return

    # Check potential old locations
    temp_dir = tempfile.gettempdir()

    # Search for settings files in temp directory tree
    old_settings_found = []
    try:
        for root, dirs, files in os.walk(temp_dir):
            if "PatchOpsIII_settings.json" in files:
                old_path = os.path.join(root, "PatchOpsIII_settings.json")
                # Only consider files modified in last 30 days
                if os.path.getmtime(old_path) > time.time() - (30 * 24 * 60 * 60):
                    old_settings_found.append(old_path)
    except (OSError, PermissionError):
        pass

    if old_settings_found:
        # Use most recently modified settings file
        old_settings = max(old_settings_found, key=os.path.getmtime)
        try:
            import shutil
            shutil.copy2(old_settings, current_settings)
            write_log(f"Migrated settings from {old_settings} to {current_settings}", "Success", None)
        except Exception as e:
            write_log(f"Failed to migrate settings: {e}", "Warning", None)


def find_game_executable(directory):
    for executable in GAME_EXECUTABLE_NAMES:
        candidate = os.path.join(directory, executable)
        if os.path.exists(candidate):
            return candidate
    return None


def get_game_directory():
    saved_dir = _load_saved_game_directory()
    if saved_dir:
        return saved_dir

    # First check if the game executable is in the same directory as the application
    app_dir = get_application_path()
    if find_game_executable(app_dir):
        return app_dir

    # Fall back to Steam default path
    if platform.system() == "Linux":
        return os.path.expanduser("~/.local/share/Steam/steamapps/common/Call of Duty Black Ops III")
    return r"C:\Program Files (x86)\Steam\steamapps\common\Call of Duty Black Ops III"

def parse_cli_arguments():
    parser = argparse.ArgumentParser(add_help=False, allow_abbrev=False)
    parser.add_argument("--install-t7", action="store_true")
    parser.add_argument("--game-dir", type=str)
    args, _ = parser.parse_known_args(sys.argv[1:])
    return args

def _show_install_location_error(message):
    if sys.platform.startswith("win"):
        try:
            import ctypes
            ctypes.windll.user32.MessageBoxW(None, message, "PatchOpsIII", 0x10 | 0x1000)
        except Exception:
            pass
    else:
        print(message, file=sys.stderr)
    raise SystemExit(message)


def _assert_directory_is_writable(path, error_message):
    test_file = os.path.join(path, ".patchops_write_test")
    try:
        with open(test_file, "w", encoding="utf-8") as handle:
            handle.write("patchops-permission-check")
    except OSError as exc:
        if exc.errno in (errno.EACCES, errno.EPERM, errno.EROFS):
            _show_install_location_error(error_message)
        else:
            raise
    finally:
        try:
            if os.path.exists(test_file):
                os.remove(test_file)
        except OSError:
            pass


def _ensure_install_location_writable(app_dir, mod_files_dir):
    error_message = (
        "Error: It seems this directory is not owned by the current user. "
        "Please put the program in a user directory or in the same directory as BlackOps3.exe"
    )
    try:
        os.makedirs(app_dir, exist_ok=True)
    except OSError as exc:
        if exc.errno in (errno.EACCES, errno.EPERM, errno.EROFS):
            _show_install_location_error(error_message)
        else:
            raise
    try:
        os.makedirs(mod_files_dir, exist_ok=True)
    except OSError as exc:
        if exc.errno in (errno.EACCES, errno.EPERM, errno.EROFS):
            _show_install_location_error(error_message)
        else:
            raise
    _assert_directory_is_writable(app_dir, error_message)
    _assert_directory_is_writable(mod_files_dir, error_message)


def _is_directory_writable(path):
    test_file = os.path.join(path, ".patchops_write_test")
    try:
        os.makedirs(path, exist_ok=True)
        with open(test_file, "w", encoding="utf-8") as handle:
            handle.write("ok")
        return True
    except OSError as exc:
        if exc.errno in (errno.EACCES, errno.EPERM, errno.EROFS):
            return False
        raise
    finally:
        try:
            if os.path.exists(test_file):
                os.remove(test_file)
        except OSError:
            pass


def _default_storage_directory():
    home = os.path.expanduser("~")
    if platform.system() == "Windows":
        roaming = os.environ.get("APPDATA")
        if not roaming:
            roaming = os.path.join(home, "AppData", "Roaming")
        return os.path.join(roaming, "PatchOpsIII")
    xdg_data = os.environ.get("XDG_DATA_HOME")
    if not xdg_data:
        xdg_data = os.path.join(home, ".local", "share")
    return os.path.join(xdg_data, "PatchOpsIII")


APPLICATION_PATH = get_application_path()
preferred_storage = _default_storage_directory()
if _is_directory_writable(preferred_storage):
    STORAGE_PATH = preferred_storage
    write_log(f"Using storage directory {STORAGE_PATH}", "Info", None)
else:
    STORAGE_PATH = APPLICATION_PATH
    if _is_directory_writable(STORAGE_PATH):
        write_log(
            f"Preferred storage directory {preferred_storage} is not writable; using application directory {STORAGE_PATH}",
            "Warning",
            None,
        )
    else:
        write_log(
            f"Neither preferred storage directory {preferred_storage} nor application directory {APPLICATION_PATH} are writable.",
            "Error",
            None,
        )

DEFAULT_GAME_DIR = get_game_directory()

MOD_FILES_DIR = os.path.join(STORAGE_PATH, "BO3 Mod Files")

# Migrate settings from old location if needed
_migrate_settings_if_needed(STORAGE_PATH)

_ensure_install_location_writable(STORAGE_PATH, MOD_FILES_DIR)

class ApplyLaunchOptionsWorker(QThread):
    finished = Signal()
    error = Signal(str)
    log_message = Signal(str, str) # New signal for logging

    def __init__(self, launch_option):
        super().__init__()
        self.launch_option = launch_option

    def run(self):
        try:
            # Pass None to log_widget in helper functions, as logging to GUI is done via signal
            log_widget_for_file = None

            self.log_message.emit("Applying launch options...", "Info")
            apply_launch_options(self.launch_option, log_widget_for_file)
            self.log_message.emit("Launch options applied successfully!", "Success")
            self.finished.emit()
        except Exception as e:
            self.log_message.emit(f"Error applying launch options: {e}", "Error")
            self.error.emit(str(e))

class QualityOfLifeWidget(QWidget):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.game_dir = None
        self.log_widget = None

        # --- Launch Options group box ---
        self.launch_group = QGroupBox("Launch Options")
        self.launch_group.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)
        launch_grid = QGridLayout(self.launch_group)
        launch_grid.setContentsMargins(5, 5, 5, 5)
        launch_grid.setSpacing(5)
        launch_grid.setAlignment(Qt.AlignTop)

        self.radio_group = QButtonGroup(self)
        self.radio_none = QRadioButton("Default (None)")
        self.radio_all_around = QRadioButton("All-around Enhancement Lite")
        self.radio_ultimate = QRadioButton("Ultimate Experience Mod")
        self.radio_offline = QRadioButton("Play Offline")
        self.radio_none.setChecked(True)

        # Block signals during initialization
        for rb in [self.radio_none, self.radio_all_around, self.radio_ultimate, self.radio_offline]:
            rb.blockSignals(True)
            self.radio_group.addButton(rb)
            rb.blockSignals(False)

        # Create help buttons with links
        all_around_help = QPushButton("?")
        ultimate_help = QPushButton("?")
        all_around_help.setFixedSize(20, 20)
        ultimate_help.setFixedSize(20, 20)
        
        # Connect buttons to open URLs
        all_around_help.clicked.connect(
            lambda: QDesktopServices.openUrl(QUrl("steam://openurl/https://steamcommunity.com/sharedfiles/filedetails/?id=2994481309"))
        )
        ultimate_help.clicked.connect(
            lambda: QDesktopServices.openUrl(QUrl("steam://openurl/https://steamcommunity.com/sharedfiles/filedetails/?id=2942053577"))
        )

        # Put the radio buttons and help buttons in rows
        launch_grid.addWidget(self.radio_none, 0, 0)
        launch_grid.addWidget(self.radio_offline, 1, 0)
        
        all_around_widget = QWidget()
        all_around_layout = QHBoxLayout(all_around_widget)
        all_around_layout.setContentsMargins(0, 0, 0, 0)
        all_around_layout.addWidget(self.radio_all_around)
        all_around_layout.addWidget(all_around_help)
        all_around_layout.addStretch()
        launch_grid.addWidget(all_around_widget, 2, 0)
        
        ultimate_widget = QWidget()
        ultimate_layout = QHBoxLayout(ultimate_widget)
        ultimate_layout.setContentsMargins(0, 0, 0, 0)
        ultimate_layout.addWidget(self.radio_ultimate)
        ultimate_layout.addWidget(ultimate_help)
        ultimate_layout.addStretch()
        launch_grid.addWidget(ultimate_widget, 3, 0)

        for rb in [self.radio_none, self.radio_all_around, self.radio_ultimate, self.radio_offline]:
            self.radio_group.addButton(rb)

        # Center the Apply button in row 4
        self.apply_button = QPushButton("Apply")
        self.apply_button.clicked.connect(self.on_apply_launch_options)
        apply_hbox = QHBoxLayout()
        apply_hbox.addStretch()
        apply_hbox.addWidget(self.apply_button)
        apply_hbox.addStretch()
        apply_container = QWidget()
        apply_container.setLayout(apply_hbox)
        launch_grid.addWidget(apply_container, 4, 0, 1, 1)

        # --- Quality of Life group box ---
        self.checkbox_group = QGroupBox("Quality of Life")
        self.checkbox_group.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)
        checkbox_layout = QVBoxLayout(self.checkbox_group)
        checkbox_layout.setContentsMargins(5, 5, 5, 5)
        checkbox_layout.setSpacing(2)

        self.reduce_stutter_cb = QCheckBox("Use latest d3dcompiler (d3dcompiler_46.dll)")
        self.skip_intro_cb = QCheckBox("Skip Intro (BO3_Global_Logo_LogoSequence.mkv)")
        self.skip_all_intro_cb = QCheckBox("Skip All Intros (Campaign, Zombies, etc.)")

        checkbox_layout.addWidget(self.reduce_stutter_cb)
        checkbox_layout.addWidget(self.skip_intro_cb)
        checkbox_layout.addWidget(self.skip_all_intro_cb)

        # Connect signals
        self.reduce_stutter_cb.toggled.connect(self.reduce_stutter_changed)
        self.skip_intro_cb.toggled.connect(self.skip_intro_changed)
        self.skip_all_intro_cb.toggled.connect(self.skip_all_intros_changed)

    def init_ui(self):
        # Remove the original init_ui implementation since we moved the initialization to __init__
        pass

    def set_game_directory(self, game_dir):
        self.game_dir = game_dir
        if self.game_dir:
            video_dir = os.path.join(self.game_dir, "video")
            intro_bak = os.path.join(video_dir, "BO3_Global_Logo_LogoSequence.mkv.bak")
            self.skip_intro_cb.setChecked(os.path.exists(intro_bak))
            
            if os.path.exists(video_dir):
                mkv_files = [f for f in os.listdir(video_dir) if f.endswith('.mkv')]
                bak_files = [f for f in os.listdir(video_dir) if f.endswith('.mkv.bak')]
                self.skip_all_intro_cb.setChecked(len(bak_files) > 0 and len(mkv_files) == 0)

            dll_file = os.path.join(self.game_dir, "d3dcompiler_46.dll")
            dll_bak = dll_file + ".bak"
            self.reduce_stutter_cb.setChecked(os.path.exists(dll_bak))

    def set_log_widget(self, log_widget):
        self.log_widget = log_widget

    def skip_intro_changed(self):
        if not self.game_dir:
            return
        video_dir = os.path.join(self.game_dir, "video")
        intro_file = os.path.join(video_dir, "BO3_Global_Logo_LogoSequence.mkv")
        intro_file_bak = intro_file + ".bak"

        if not os.path.exists(video_dir):
            write_log("Video directory not found.", "Warning", self.log_widget)
            return

        if self.skip_intro_cb.isChecked():
            # If backup exists, assume the intro is already skipped
            if os.path.exists(intro_file_bak):
                write_log("Intro video already skipped.", "Success", self.log_widget)
            else:
                # If the original file exists, rename it
                if os.path.exists(intro_file):
                    try:
                        os.rename(intro_file, intro_file_bak)
                        write_log("Intro video skipped.", "Success", self.log_widget)
                    except Exception as e:
                        write_log(f"Failed to rename intro video file: {e}", "Error", self.log_widget)
                else:
                    # Neither original nor backup exist, but the user wants intros skipped
                    write_log("Intro video skipped.", "Success", self.log_widget)
        else:
            if os.path.exists(intro_file_bak):
                try:
                    os.rename(intro_file_bak, intro_file)
                    write_log("Intro video restored.", "Success", self.log_widget)
                except Exception as e:
                    write_log(f"Failed to restore intro video file: {e}", "Error", self.log_widget)
            else:
                write_log("Backup intro video file not found.", "Warning", self.log_widget)

    def skip_all_intros_changed(self):
        if not self.game_dir:
            return
        video_dir = os.path.join(self.game_dir, "video")
        if not os.path.exists(video_dir):
            write_log("Video directory not found.", "Warning", self.log_widget)
            return

        if self.skip_all_intro_cb.isChecked():
            # Ensure main intro is also skipped
            self.skip_intro_cb.setChecked(True)

            mkv_files = [f for f in os.listdir(video_dir) if f.endswith('.mkv')]
            for mkv_file in mkv_files:
                file_path = os.path.join(video_dir, mkv_file)
                bak_path = file_path + '.bak'
                try:
                    if not os.path.exists(bak_path):
                        os.rename(file_path, bak_path)
                except Exception as e:
                    write_log(f"Failed to rename {mkv_file}: {e}", "Error", self.log_widget)
            write_log("All intro videos skipped.", "Success", self.log_widget)
        else:
            main_intro = "BO3_Global_Logo_LogoSequence.mkv"
            bak_files = [f for f in os.listdir(video_dir) if f.endswith('.mkv.bak')]
            for bak_file in bak_files:
                # If user still wants main intro skipped, don't restore that one
                if bak_file == main_intro + '.bak' and self.skip_intro_cb.isChecked():
                    continue

                bak_path = os.path.join(video_dir, bak_file)
                file_path = bak_path[:-4]
                try:
                    if not os.path.exists(file_path):
                        os.rename(bak_path, file_path)
                except Exception as e:
                    write_log(f"Failed to restore {bak_file}: {e}", "Error", self.log_widget)
            write_log("Other intro videos restored.", "Success", self.log_widget)

    def reduce_stutter_changed(self):
        if not self.game_dir:
            return
        dll_file = os.path.join(self.game_dir, "d3dcompiler_46.dll")
        dll_bak = dll_file + ".bak"
        if self.reduce_stutter_cb.isChecked():
            if os.path.exists(dll_file):
                try:
                    os.rename(dll_file, dll_bak)
                    write_log("Renamed d3dcompiler_46.dll to reduce stuttering.", "Success", self.log_widget)
                except Exception:
                    write_log("Failed to rename d3dcompiler_46.dll.", "Error", self.log_widget)
            elif os.path.exists(dll_bak):
                write_log("Already using latest d3dcompiler.", "Success", self.log_widget)
            else:
                write_log("d3dcompiler_46.dll not found.", "Warning", self.log_widget)
        else:
            if os.path.exists(dll_bak):
                try:
                    os.rename(dll_bak, dll_file)
                    write_log("Restored d3dcompiler_46.dll.", "Success", self.log_widget)
                except Exception:
                    write_log("Failed to restore d3dcompiler_46.dll.", "Error", self.log_widget)
            else:
                write_log("Backup not found to restore.", "Warning", self.log_widget)

    def on_apply_launch_options(self):
        # Figure out which radio is checked
        if self.radio_none.isChecked():
            option = ""
        elif self.radio_all_around.isChecked():
            option = "+set fs_game 2994481309"
        elif self.radio_ultimate.isChecked():
            option = "+set fs_game 2942053577"
        elif self.radio_offline.isChecked():
            option = "+set fs_game offlinemp"

        # Get current launch options to preserve T7Patch settings
        user_id = find_steam_user_id()
        if user_id:
            config_path = os.path.join(steam_userdata_path, user_id, "config", "localconfig.vdf")
            if os.path.exists(config_path):
                try:
                    with open(config_path, "r", encoding="utf-8") as file:
                        data = vdf.load(file)
                    current_options = data.get("UserLocalConfigStore", {}).get("Software", {}).get("Valve", {}).get("Steam", {}).get("apps", {}).get(app_id, {}).get("LaunchOptions", "")
                    if 'WINEDLLOVERRIDES="dsound=n,b"' in current_options:
                        if option:
                            option = f'WINEDLLOVERRIDES="dsound=n,b" %command% {option}'
                        else:
                            option = 'WINEDLLOVERRIDES="dsound=n,b" %command%'
                except Exception as e:
                    write_log(f"Error reading current launch options: {e}", "Error", self.log_widget)

        self.apply_button.setEnabled(False)
        self.worker = ApplyLaunchOptionsWorker(option)
        self.worker.log_message.connect(self.log_message_received) # Connect to the new log_message signal
        self.worker.finished.connect(self.on_apply_finished)
        self.worker.error.connect(self.on_apply_error)
        self.worker.start()

    def log_message_received(self, message, category):
        write_log(message, category, self.log_widget)

    def on_apply_finished(self):
        self.apply_button.setEnabled(True)

    def on_apply_error(self, error_message):
        self.apply_button.setEnabled(True)
        write_log(f"Error applying launch options: {error_message}", "Error", self.log_widget)

class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("PatchOpsIII")
        self._system = platform.system()
        self._t7_just_uninstalled = False
        self.updater: Optional[WindowsUpdater] = None
        self._staged_release: Optional[ReleaseInfo] = None
        self._staged_script_path: Optional[str] = None
        self._default_update_button_text = "Check for Updates"
        self._linux_update_worker = None

        icon, icon_source = load_application_icon()
        if icon.isNull():
            if icon_source:
                write_log(f"Icon not found or invalid: {icon_source}", "Warning")
            else:
                write_log("Icon not found in packaged resources", "Warning")
        self.setWindowIcon(icon)
        self.init_ui()

        # Load saved launch options state without applying
        self.load_launch_options_state()
        
        if os.path.exists(os.path.join(DEFAULT_GAME_DIR, "BlackOps3.exe")):
            if DEFAULT_GAME_DIR == get_application_path():
                write_log("Using Black Ops III from PatchOpsIII directory", "Info", self.log_text)

        self.t7_patch_widget.patch_uninstalled.connect(self.on_t7_patch_uninstalled)

        if os.path.exists(os.path.join(get_application_path(), "BlackOps3.exe")):
            write_log("Black Ops III found in the same directory as PatchOpsIII", "Info", self.log_text)

        # Connect tab changed signal
        self.tabs.currentChanged.connect(self.on_tab_changed)

        if self._system == "Windows":
            self._initialize_windows_updater()
        elif self._system == "Linux":
            QTimer.singleShot(2500, self._auto_check_for_linux_updates)

    def load_launch_options_state(self):
        # Get the Steam user ID and read current launch options
        user_id = find_steam_user_id()
        if not user_id:
            return

        config_path = os.path.join(steam_userdata_path, user_id, "config", "localconfig.vdf")
        if not os.path.exists(config_path):
            return

        try:
            with open(config_path, "r", encoding="utf-8") as file:
                data = vdf.load(file)
            
            current_options = data.get("UserLocalConfigStore", {}).get("Software", {}).get("Valve", {}).get("Steam", {}).get("apps", {}).get(app_id, {}).get("LaunchOptions", "")
            
            # Set radio button state based on current options without applying
            if "+set fs_game 2994481309" in current_options:
                self.qol_widget.radio_all_around.setChecked(True)
            elif "+set fs_game 2942053577" in current_options:
                self.qol_widget.radio_ultimate.setChecked(True)
            elif "+set fs_game offlinemp" in current_options:
                self.qol_widget.radio_offline.setChecked(True)
            else:
                self.qol_widget.radio_none.setChecked(True)
            
        except Exception as e:
            write_log(f"Error loading launch options state: {e}", "Error", self.log_text)

    def init_ui(self):
        central = QWidget()
        main_layout = QVBoxLayout(central)

        # Game Directory Section
        game_dir_widget = QWidget()
        gd_layout = QVBoxLayout(game_dir_widget)
        gd_layout.setContentsMargins(0, 0, 0, 0)
        gd_layout.setSpacing(6)

        directory_row = QHBoxLayout()
        directory_row.setSpacing(10)
        self.game_dir_edit = QLineEdit(DEFAULT_GAME_DIR)
        self.game_dir_edit.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
        label_text = (
            "Current Directory:" if DEFAULT_GAME_DIR == get_application_path()
            else "Game Directory:"
        )
        directory_row.addWidget(QLabel(label_text))
        directory_row.addWidget(self.game_dir_edit, 1)

        buttons_container = QWidget()
        buttons_layout = QHBoxLayout(buttons_container)
        buttons_layout.setContentsMargins(0, 0, 0, 0)
        buttons_layout.setSpacing(10)
        buttons_container.setSizePolicy(QSizePolicy.Fixed, QSizePolicy.Fixed)

        browse_btn = QPushButton("Browse")
        browse_btn.clicked.connect(self.browse_game_dir)
        buttons_layout.addWidget(browse_btn)

        launch_game_btn = QPushButton("Launch Game")
        launch_game_btn.clicked.connect(self.launch_game)
        buttons_layout.addWidget(launch_game_btn)

        directory_row.addWidget(buttons_container)
        gd_layout.addLayout(directory_row)

        self.update_button = None
        if self._system in ("Windows", "Linux"):
            self.update_button = QPushButton(self._default_update_button_text)
            self.update_button.clicked.connect(self.on_update_button_clicked)
            self.update_button.setSizePolicy(QSizePolicy.Fixed, QSizePolicy.Fixed)

            def _sync_update_button_width():
                total_width = (
                    browse_btn.sizeHint().width()
                    + launch_game_btn.sizeHint().width()
                    + buttons_layout.spacing()
                )
                self.update_button.setFixedWidth(total_width)

            _sync_update_button_width()
            QTimer.singleShot(0, _sync_update_button_width)

            update_row = QHBoxLayout()
            update_row.addStretch(1)
            update_row.addWidget(self.update_button)
            gd_layout.addLayout(update_row)

        main_layout.addWidget(game_dir_widget)

        # Individual widgets
        self.t7_patch_widget = T7PatchWidget(MOD_FILES_DIR)
        self.dxvk_widget = DXVKWidget(MOD_FILES_DIR)
        self.qol_widget = QualityOfLifeWidget()
        self.graphics_widget = GraphicsSettingsWidget(dxvk_widget=self.dxvk_widget)
        self.advanced_widget = AdvancedSettingsWidget()

        # Tabs
        self.tabs = QTabWidget()
        self.tabs.currentChanged.connect(self.on_tab_changed)

        # Mods Tab with Grid Layout
        mods_tab = QWidget()
        mods_grid = QGridLayout(mods_tab)
        mods_grid.setContentsMargins(5, 5, 5, 5)
        mods_grid.setSpacing(10)

        # Configure grid spacing
        mods_grid.setHorizontalSpacing(20)
        mods_grid.setVerticalSpacing(10)

        # Row 0: T7 Patch and Launch Options - without alignment to fill cells
        mods_grid.addWidget(self.t7_patch_widget.groupbox, 0, 0)
        mods_grid.addWidget(self.qol_widget.launch_group, 0, 1)

        # Row 1: DXVK and Options
        mods_grid.addWidget(self.dxvk_widget.groupbox, 1, 0)
        mods_grid.addWidget(self.qol_widget.checkbox_group, 1, 1)

        # Set equal column and row stretches
        mods_grid.setColumnStretch(0, 1)
        mods_grid.setColumnStretch(1, 1)
        mods_grid.setRowStretch(0, 1)
        mods_grid.setRowStretch(1, 1)

        self.tabs.addTab(mods_tab, "Mods")

        # Graphics Tab
        graphics_tab = QWidget()
        graphics_layout = QVBoxLayout(graphics_tab)
        graphics_layout.addWidget(self.graphics_widget)
        self.tabs.addTab(graphics_tab, "Graphics")

        # Advanced Tab
        advanced_tab = QWidget()
        advanced_layout = QVBoxLayout(advanced_tab)
        advanced_layout.addWidget(self.advanced_widget)
        self.tabs.addTab(advanced_tab, "Advanced")

        main_layout.addWidget(self.tabs)

        # Log Window
        self.log_text = QTextEdit()
        self.log_text.setReadOnly(True)
        self.log_text.setStyleSheet("background-color: black; color: white; font-family: Consolas;")
        main_layout.addWidget(self.log_text)

        self.setCentralWidget(central)
        self.adjustSize()

        # Initialize with the default directory
        game_dir = self.game_dir_edit.text().strip()
        self._apply_game_directory(game_dir)
        self.t7_patch_widget.set_log_widget(self.log_text)
        self.dxvk_widget.set_log_widget(self.log_text)
        self.graphics_widget.set_log_widget(self.log_text)
        self.advanced_widget.set_log_widget(self.log_text)
        self.qol_widget.set_log_widget(self.log_text)

    def on_tab_changed(self, index):
        tab_widget = self.tabs.widget(index)
        if tab_widget:
            # Refresh advanced settings if on that tab
            if index == 2:
                self.advanced_widget.refresh_settings()
            tab_widget.layout().update()

    def on_update_button_clicked(self):
        if self._system == "Linux":
            self._manual_check_for_linux_updates()
            return
        if not self.updater:
            return
        if self._staged_script_path:
            self._prompt_install_staged_update()
            return
        if self._staged_release:
            self.updater.download_update(self._staged_release)
            return
        self.updater.check_for_updates(force=True)

    def _initialize_windows_updater(self):
        self.updater = WindowsUpdater(
            current_version=APP_VERSION,
            install_dir=get_application_path(),
            executable_path=sys.executable,
            is_frozen=_is_frozen_environment(),
            log_widget=self.log_text,
        )
        self.updater.check_started.connect(self._on_update_check_started)
        self.updater.check_failed.connect(self._on_update_check_failed)
        self.updater.no_update_available.connect(self._on_no_update_available)
        self.updater.update_available.connect(self._on_update_available)
        self.updater.download_started.connect(self._on_download_started)
        self.updater.download_progress.connect(self._on_download_progress)
        self.updater.download_failed.connect(self._on_download_failed)
        self.updater.update_staged.connect(self._on_update_staged)
        QTimer.singleShot(2500, self._auto_check_for_updates)

    def _reset_update_button(self):
        if not getattr(self, "update_button", None):
            return
        self.update_button.setText(self._default_update_button_text)
        self.update_button.setEnabled(True)
        self._staged_release = None
        self._staged_script_path = None

    def _on_update_check_started(self):
        if self.update_button:
            self.update_button.setEnabled(False)

    def _on_update_check_failed(self, message):
        self._reset_update_button()
        QMessageBox.warning(self, "Update Check Failed", message)

    def _on_no_update_available(self):
        self._reset_update_button()

    def _on_update_available(self, release: ReleaseInfo):
        if self.update_button:
            self.update_button.setEnabled(True)
        self._staged_release = release
        release_page = release.page_url or "https://github.com/boggedbrush/PatchOpsIII/releases/latest"
        message = (
            f"<p>PatchOpsIII {release.version} is available for download.</p>"
            f'<p><a href="{release_page}">Open the latest release on GitHub</a></p>'
            "<p>Would you like to download the update now?</p>"
        )
        dialog = QMessageBox(self)
        dialog.setWindowTitle("Update Available")
        dialog.setTextFormat(Qt.RichText)
        dialog.setTextInteractionFlags(Qt.TextBrowserInteraction)
        dialog.setText(message)
        dialog.setStandardButtons(QMessageBox.Yes | QMessageBox.No)
        dialog.setDefaultButton(QMessageBox.Yes)
        if dialog.exec() == QMessageBox.Yes:
            self.updater.download_update(release)
        else:
            if self.update_button:
                self.update_button.setText("Download Update")

    def _on_download_started(self, release: ReleaseInfo):
        if self.update_button:
            self.update_button.setEnabled(False)

    def _on_download_progress(self, percent: int):
        pass

    def _on_download_failed(self, message: str):
        self._reset_update_button()
        QMessageBox.critical(self, "Update Failed", message)

    def _on_update_staged(self, release: ReleaseInfo, script_path: str):
        self._staged_release = release
        self._staged_script_path = script_path
        if self.update_button:
            self.update_button.setText("Install Update")
            self.update_button.setEnabled(True)
        prompt = (
            f"PatchOpsIII {release.version} has been downloaded and is ready to install.\n\n"
            "Would you like to close the application and apply the update now?"
        )
        if QMessageBox.question(self, "Install Update", prompt, QMessageBox.Yes | QMessageBox.No) == QMessageBox.Yes:
            self._prompt_install_staged_update()

    def _auto_check_for_updates(self):
        if self.updater and not self._staged_script_path:
            self.updater.check_for_updates(force=False)

    def _auto_check_for_linux_updates(self):
        self._check_for_linux_updates()

    def _manual_check_for_linux_updates(self):
        self._check_for_linux_updates()

    def _check_for_linux_updates(self):
        prompt_linux_update(
            self,
            APP_VERSION,
            log_widget=self.log_text,
        )

    def _prompt_install_staged_update(self):
        if not self.updater or not self._staged_release:
            return
        confirm = QMessageBox.question(
            self,
            "Install Update",
            (
                f"Install PatchOpsIII {self._staged_release.version} now?\n\n"
                "The application will close while the update is applied."
            ),
            QMessageBox.Yes | QMessageBox.No,
        )
        if confirm == QMessageBox.Yes:
            self._apply_staged_update()

    def _apply_staged_update(self):
        if not self.updater:
            return
        try:
            self.updater.apply_staged_update()
        except Exception as exc:  # noqa: BLE001
            write_log(f"Failed to launch update installer: {exc}", "Error", self.log_text)
            QMessageBox.critical(self, "Update Error", str(exc))
            self._reset_update_button()
            return
        QApplication.instance().quit()

    def browse_game_dir(self):
        directory = QFileDialog.getExistingDirectory(
            self, "Select Black Ops III Game Directory", self.game_dir_edit.text()
        )
        if not directory:
            return

        if not find_game_executable(directory):
            message = (
                "BlackOpsIII.exe not found in the selected directory. "
                "Please point the program to the folder containing BlackOpsIII.exe."
            )
            QMessageBox.warning(self, "Invalid Game Directory", message)
            write_log(message, "Error", self.log_text)
            return

        self._apply_game_directory(directory, save=True)
        write_log(f"Game directory set to {directory}", "Success", self.log_text)

    def on_t7_patch_uninstalled(self):
        self._t7_just_uninstalled = True

    def launch_game(self):
        game_dir = self.game_dir_edit.text().strip()
        game_exe_path = find_game_executable(game_dir)

        if not game_exe_path:
            write_log(
                "Error: BlackOpsIII.exe not found. Please point the program to the folder containing BlackOpsIII.exe.",
                "Error",
                self.log_text,
            )
            return

        launch_game_via_steam(app_id, self.log_text)

    def _apply_game_directory(self, directory, save=False):
        if not directory:
            return

        self.game_dir_edit.setText(directory)

        if save:
            if not save_game_directory(directory):
                write_log(
                    "Failed to remember the selected game directory.",
                    "Warning",
                    self.log_text,
                )

        if not self._t7_just_uninstalled:
            self.t7_patch_widget.set_game_directory(directory)
        self._t7_just_uninstalled = False
        self.dxvk_widget.set_game_directory(directory)
        self.graphics_widget.set_game_directory(directory)
        self.advanced_widget.set_game_directory(directory)
        self.qol_widget.set_game_directory(directory)


def main() -> int:
    """Launch the PatchOpsIII GUI and return its exit code."""
    cli_args = parse_cli_arguments()
    write_log(f"Process PID {os.getpid()} elevated={is_admin()}", "Info")
    app = QApplication(sys.argv)
    global_icon, icon_source = load_application_icon()
    if global_icon.isNull():
        if icon_source:
            write_log(f"Global icon not found or invalid: {icon_source}", "Warning")
        else:
            write_log("Global icon not found in packaged resources", "Warning")
    app.setWindowIcon(global_icon)
    window = MainWindow()

    if getattr(cli_args, "game_dir", None):
        directory = cli_args.game_dir
        if find_game_executable(directory):
            window._apply_game_directory(directory, save=True)
            write_log(
                f"Game directory set via CLI to {directory}",
                "Success",
                window.log_text,
            )
        else:
            message = (
                "BlackOpsIII.exe not found in the CLI-provided directory. "
                "Please point the program to the folder containing BlackOpsIII.exe."
            )
            write_log(message, "Error", window.log_text)

    window.show()

    if getattr(cli_args, "install_t7", False):
        if is_admin():
            QTimer.singleShot(0, window.t7_patch_widget.install_t7_patch)
        else:
            write_log(
                "Elevation flag detected but process is not running with administrator rights. "
                "Skipping automatic T7 Patch install.",
                "Warning",
                window.log_text,
            )

    return app.exec()


if __name__ == "__main__":
    sys.exit(main())
