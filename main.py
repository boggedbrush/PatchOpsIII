#!/usr/bin/env python
import sys
import os
import re
import errno
import shutil
import time
import tempfile
import urllib.request
import vdf
import platform
import argparse
import json
import inspect
from functools import lru_cache
from typing import Optional

# Silence noisy Qt portal logging on some Linux hosts before Qt loads
os.environ.setdefault("QT_LOGGING_RULES", "qt.qpa.*=false;qt.scenegraph.general=false")

from PySide6.QtWidgets import (
    QApplication, QMainWindow, QWidget, QVBoxLayout, QHBoxLayout, QLineEdit,
    QPushButton, QLabel, QFileDialog, QTextEdit, QSizePolicy,
    QGroupBox, QRadioButton, QButtonGroup, QCheckBox, QGridLayout,
    QMessageBox, QMenu, QListWidget, QListWidgetItem,
    QStackedWidget, QAbstractItemView, QFrame
)
from PySide6.QtGui import QIcon, QDesktopServices, QAction, QFont
from PySide6.QtCore import Qt, QUrl, QThread, Signal, QTimer, QSize

# QtModernRedux (Qt6 fork) imports are resolved dynamically to support both module names
try:
    import qtmodernredux6.styles as qt_styles
    import qtmodernredux6.windows as qt_windows
    QT_MODERN_AVAILABLE = True
except ModuleNotFoundError:
    try:
        import qtmodernredux.styles as qt_styles
        import qtmodernredux.windows as qt_windows
        QT_MODERN_AVAILABLE = True
    except ModuleNotFoundError:
        QT_MODERN_AVAILABLE = False

from t7_patch import T7PatchWidget, is_admin, check_t7_patch_status
from dxvk_manager import DXVKWidget, is_dxvk_async_installed
from config import GraphicsSettingsWidget, AdvancedSettingsWidget
from updater import ReleaseInfo, WindowsUpdater, prompt_linux_update
from utils import (
    write_log,
    apply_launch_options,
    find_steam_user_id,
    steam_userdata_path,
    app_id,
    launch_game_via_steam,
    manage_log_retention_on_launch,
    patchops_backup_path,
    existing_backup_path,
    PATCHOPS_BACKUP_SUFFIX,
    LEGACY_BACKUP_SUFFIX,
    read_exe_variant,
    write_exe_variant,
    file_sha256,
    get_workshop_item_state,
)
from bo3_enhanced import (
    download_latest_enhanced,
    detect_enhanced_install,
    GITHUB_LATEST_ENHANCED_PAGE,
    install_enhanced_files,
    install_dump_only,
    uninstall_dump_only,
    mark_enhanced_detected,
    set_acknowledged,
    status_summary,
    uninstall_enhanced_files,
    validate_dump_source,
)
from version import APP_VERSION

REFORGED_DOWNLOAD_URL = "https://downloads.bo3reforged.com/BlackOps3.exe"
REFORGED_WORKSHOP_URL = "https://steamcommunity.com/sharedfiles/filedetails/?id=3667377161"
REFORGED_WORKSHOP_STEAM_URL = f"steam://openurl/{REFORGED_WORKSHOP_URL}"
REFORGED_DOWNLOAD_HEADERS = {
    "User-Agent": (
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 "
        "(KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36"
    ),
    "Accept": "application/octet-stream,*/*;q=0.8",
    "Referer": "https://bo3reforged.com/",
}

WORKSHOP_PROFILES = {
    "all_around": {
        "name": "All-around Enhancement Lite",
        "workshop_id": "2994481309",
        "workshop_url": "https://steamcommunity.com/sharedfiles/filedetails/?id=2994481309",
        "launch_option": "+set fs_game 2994481309",
    },
    "ultimate": {
        "name": "Ultimate Experience Mod",
        "workshop_id": "2942053577",
        "workshop_url": "https://steamcommunity.com/sharedfiles/filedetails/?id=2942053577",
        "launch_option": "+set fs_game 2942053577",
    },
    "forged": {
        "name": "Reforged",
        "workshop_id": "3667377161",
        "workshop_url": "https://steamcommunity.com/sharedfiles/filedetails/?id=3667377161",
        "launch_option": "+set fs_game 3667377161",
    },
}

# Trusted SHA-256 hashes for known-good Reforged executables.
# Update when upstream publishes a new verified build.
REFORGED_TRUSTED_SHA256 = {
    "66b95eb4667bd5b3b3d230e7bed1d29ccd261d48ca2699f01216c863be24ff44",
}


NUITKA_ENVIRONMENT_KEYS = (
    "NUITKA_ONEFILE_PARENT",
    "NUITKA_EXE_PATH",
    "NUITKA_PACKAGE_HOME",
)

_NUITKA_DETECTION_KEYS = NUITKA_ENVIRONMENT_KEYS + ("NUITKA_ONEFILE_TEMP",)


def apply_modern_theme(app: QApplication) -> None:
    """Lightweight overlay styles to complement QtModernRedux."""
    app.setStyleSheet(
        """
        QLabel#HeadingTitle {
            font-size: 18px;
            font-weight: 600;
        }
        QLabel#HeadingVersion {
            font-size: 11px;
            color: rgb(150, 155, 165);
        }
        QPushButton#PrimaryButton {
            font-weight: 600;
            padding: 6px 14px;
        }
        QPushButton#SecondaryButton {
            padding: 6px 12px;
        }
        QTextEdit#LogView {
            font-family: "JetBrains Mono", "Fira Code", "Consolas", monospace;
            font-size: 11px;
        }
        QListWidget#SidebarTabs {
            border: 1px solid rgba(255, 255, 255, 0.06);
            border-radius: 10px;
            background: rgba(0, 0, 0, 0.12);
            padding: 6px;
        }
        QListWidget#SidebarTabs::item {
            padding: 8px 10px;
            border-radius: 8px;
        }
        QListWidget#SidebarTabs::item:hover {
            background: rgba(255, 255, 255, 0.06);
        }
        QListWidget#SidebarTabs::item:selected {
            background: rgba(255, 255, 255, 0.12);
        }
        QLabel#DashboardStatusName {
            color: rgb(170, 175, 185);
            font-size: 12px;
        }
        QLabel#DashboardStatusValue {
            font-size: 12px;
            font-weight: 600;
        }
        QLabel[workshopState="good"] {
            color: #4ade80;
            font-weight: 600;
        }
        QLabel[workshopState="info"] {
            color: #60a5fa;
            font-weight: 600;
        }
        QLabel[workshopState="bad"] {
            color: #f87171;
            font-weight: 600;
        }
        QFrame#DashboardDivider {
            color: rgba(255, 255, 255, 0.08);
        }
        """
    )


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


@lru_cache(maxsize=64)
def load_ui_icon(icon_name: str) -> QIcon:
    resolved = resource_path(os.path.join("icons", f"{icon_name}.svg"))
    if not resolved:
        return QIcon()
    return QIcon(resolved)


class SidebarTabWidget(QWidget):
    currentChanged = Signal(int)

    def __init__(self, parent: Optional[QWidget] = None):
        super().__init__(parent)
        self._nav = QListWidget()
        self._nav.setObjectName("SidebarTabs")
        self._nav.setSelectionMode(QAbstractItemView.SingleSelection)
        self._nav.setHorizontalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        self._nav.setIconSize(QSize(20, 20))
        self._nav.setSpacing(2)
        self._nav.setFocusPolicy(Qt.NoFocus)
        self._nav.setUniformItemSizes(True)
        self._nav.setSizePolicy(QSizePolicy.Fixed, QSizePolicy.Expanding)
        self._nav.setFixedWidth(160)

        self._stack = QStackedWidget()
        self._stack.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)

        layout = QHBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(12)
        layout.addWidget(self._nav)
        layout.addWidget(self._stack, 1)

        self._nav.currentRowChanged.connect(self._on_row_changed)

    def setDocumentMode(self, enabled: bool) -> None:  # noqa: ARG002
        # Kept for compatibility with QTabWidget callers.
        return

    def addTab(self, widget: QWidget, *args):
        if len(args) == 1:
            icon = QIcon()
            label = args[0]
        elif len(args) == 2:
            icon, label = args
        else:
            raise TypeError("addTab(widget, label) or addTab(widget, icon, label)")

        index = self._stack.addWidget(widget)
        item = QListWidgetItem(icon, label)
        item.setSizeHint(QSize(0, 40))
        self._nav.addItem(item)

        if self._nav.count() == 1:
            self._nav.setCurrentRow(0)

        return index

    def widget(self, index: int) -> Optional[QWidget]:
        return self._stack.widget(index)

    def currentIndex(self) -> int:
        return self._stack.currentIndex()

    def setCurrentIndex(self, index: int) -> None:
        self._nav.setCurrentRow(index)

    def count(self) -> int:
        return self._stack.count()

    def _on_row_changed(self, row: int) -> None:
        if row < 0 or row >= self._stack.count():
            return
        self._stack.setCurrentIndex(row)
        self.currentChanged.emit(row)

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


class EnhancedDownloadWorker(QThread):
    progress = Signal(str)
    download_complete = Signal()
    failed = Signal(str)

    def __init__(self, mod_files_dir: str, storage_dir: str):
        super().__init__()
        self.mod_files_dir = mod_files_dir
        self.storage_dir = storage_dir

    def run(self):
        try:
            self.progress.emit("Fetching latest BO3 Enhanced release...")
            enhanced_path = download_latest_enhanced(self.mod_files_dir, self.storage_dir)
            if not enhanced_path:
                raise RuntimeError("Failed to download BO3 Enhanced.")

            mark_enhanced_detected(self.storage_dir)
            self.progress.emit("BO3 Enhanced assets ready.")
            self.download_complete.emit()
        except Exception as exc:  # noqa: BLE001
            self.failed.emit(str(exc))


class ReforgedInstallWorker(QThread):
    progress = Signal(str)
    installed = Signal(str)
    failed = Signal(str)

    def __init__(self, game_dir: str):
        super().__init__()
        self.game_dir = game_dir

    def run(self):
        temp_path = None
        try:
            if not self.game_dir or not os.path.isdir(self.game_dir):
                raise RuntimeError("Invalid game directory.")

            target_exe = find_game_executable(self.game_dir) or os.path.join(self.game_dir, "BlackOps3.exe")
            os.makedirs(self.game_dir, exist_ok=True)

            self.progress.emit("Downloading Reforged executable...")
            fd, temp_path = tempfile.mkstemp(prefix="patchops_reforged_", suffix=".exe")
            os.close(fd)

            request = urllib.request.Request(REFORGED_DOWNLOAD_URL, headers=REFORGED_DOWNLOAD_HEADERS)
            with urllib.request.urlopen(request, timeout=120) as response, open(temp_path, "wb") as handle:
                while True:
                    chunk = response.read(1024 * 256)
                    if not chunk:
                        break
                    handle.write(chunk)

            if os.path.getsize(temp_path) <= 0:
                raise RuntimeError("Downloaded file is empty.")

            with open(temp_path, "rb") as handle:
                if handle.read(2) != b"MZ":
                    raise RuntimeError("Downloaded file is not a valid Windows executable.")
            downloaded_hash = file_sha256(temp_path)
            if not downloaded_hash:
                raise RuntimeError("Failed to compute SHA-256 for downloaded Reforged executable.")
            if downloaded_hash.lower() not in REFORGED_TRUSTED_SHA256:
                raise RuntimeError(
                    "Downloaded Reforged executable failed integrity verification (unknown SHA-256)."
                )

            if os.path.exists(target_exe):
                backup_path = patchops_backup_path(target_exe)
                if existing_backup_path(target_exe):
                    self.progress.emit("Existing executable backup found. Preserving original backup...")
                else:
                    self.progress.emit("Backing up current executable...")
                    shutil.copy2(target_exe, backup_path)

            shutil.move(temp_path, target_exe)
            temp_path = None
            self.installed.emit(target_exe)
        except Exception as exc:
            self.failed.emit(str(exc))
        finally:
            if temp_path and os.path.exists(temp_path):
                try:
                    os.remove(temp_path)
                except OSError:
                    pass


class QualityOfLifeWidget(QWidget):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.game_dir = None
        self.log_widget = None
        self.worker = None
        self.workshop_install_worker = None

        # --- Launch Options group box ---
        self.launch_group = QGroupBox("Launch Options")
        self.launch_group.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)
        launch_layout = QGridLayout(self.launch_group)
        launch_layout.setContentsMargins(14, 14, 14, 14)
        launch_layout.setHorizontalSpacing(10)
        launch_layout.setVerticalSpacing(0)

        self.radio_group = QButtonGroup(self)
        self.radio_none = QRadioButton("Default (None)")
        self.radio_all_around = QRadioButton(WORKSHOP_PROFILES["all_around"]["name"])
        self.radio_ultimate = QRadioButton(WORKSHOP_PROFILES["ultimate"]["name"])
        self.radio_forged = QRadioButton(WORKSHOP_PROFILES["forged"]["name"])
        self.radio_offline = QRadioButton("Play Offline")
        self.radio_none.setChecked(True)
        self.workshop_profile_radios = {}
        self.workshop_profile_status_labels = {}

        for rb in [self.radio_none, self.radio_all_around, self.radio_ultimate, self.radio_forged, self.radio_offline]:
            rb.blockSignals(True)
            self.radio_group.addButton(rb)
            rb.blockSignals(False)

        # Help buttons (flat, fixed-size, link to mod pages)
        all_around_help = QPushButton("?")
        ultimate_help = QPushButton("?")
        forged_help = QPushButton("?")
        for btn in [all_around_help, ultimate_help, forged_help]:
            btn.setFixedSize(22, 22)
            btn.setFlat(True)
        all_around_help.clicked.connect(
            lambda: QDesktopServices.openUrl(QUrl("steam://openurl/https://steamcommunity.com/sharedfiles/filedetails/?id=2994481309"))
        )
        ultimate_help.clicked.connect(
            lambda: QDesktopServices.openUrl(QUrl("steam://openurl/https://steamcommunity.com/sharedfiles/filedetails/?id=2942053577"))
        )
        forged_help.clicked.connect(
            lambda: QDesktopServices.openUrl(QUrl("https://bo3reforged.com/"))
        )

        def _launch_row(layout, row, radio, help_btn=None, status_label=None):
            container = QWidget()
            hbox = QHBoxLayout(container)
            hbox.setContentsMargins(0, 3, 0, 3)
            hbox.setSpacing(8)
            hbox.addWidget(radio, 0)
            if help_btn:
                hbox.addWidget(help_btn, 0)
            hbox.addStretch(1)
            if status_label:
                hbox.addWidget(status_label, 0, Qt.AlignRight)
            layout.addWidget(container, row, 0, 1, 2)

        def _launch_sep(layout, row):
            sep = QFrame()
            sep.setObjectName("DashboardDivider")
            sep.setFrameShape(QFrame.HLine)
            sep.setFixedHeight(1)
            layout.addWidget(sep, row, 0, 1, 2)

        def _workshop_status_label():
            label = QLabel("Not Subscribed")
            label.setObjectName("DashboardStatusValue")
            label.setAlignment(Qt.AlignRight | Qt.AlignVCenter)
            return label

        all_around_status = _workshop_status_label()
        ultimate_status = _workshop_status_label()
        forged_status = _workshop_status_label()
        self.workshop_profile_radios.update({
            "all_around": self.radio_all_around,
            "ultimate": self.radio_ultimate,
            "forged": self.radio_forged,
        })
        self.workshop_profile_status_labels.update({
            "all_around": all_around_status,
            "ultimate": ultimate_status,
            "forged": forged_status,
        })

        lrow = 0
        _launch_row(launch_layout, lrow, self.radio_none);          lrow += 1
        _launch_sep(launch_layout, lrow);                           lrow += 1
        _launch_row(launch_layout, lrow, self.radio_offline);       lrow += 1
        _launch_sep(launch_layout, lrow);                           lrow += 1
        _launch_row(launch_layout, lrow, self.radio_all_around, all_around_help, all_around_status); lrow += 1
        _launch_sep(launch_layout, lrow);                           lrow += 1
        _launch_row(launch_layout, lrow, self.radio_ultimate, ultimate_help, ultimate_status);     lrow += 1
        _launch_sep(launch_layout, lrow);                           lrow += 1
        _launch_row(launch_layout, lrow, self.radio_forged, forged_help, forged_status);         lrow += 1

        # Action buttons row
        _launch_sep(launch_layout, lrow);                           lrow += 1
        btn_row = QWidget()
        btn_layout = QHBoxLayout(btn_row)
        btn_layout.setContentsMargins(0, 4, 0, 0)
        btn_layout.setSpacing(10)

        self.install_workshop_button = QPushButton("Install Selected Mod")
        self.install_workshop_button.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
        self.install_workshop_button.clicked.connect(self.on_install_selected_workshop_mod)
        btn_layout.addWidget(self.install_workshop_button, 1)

        self.refresh_workshop_status_button = QPushButton("Refresh")
        self.refresh_workshop_status_button.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
        self.refresh_workshop_status_button.clicked.connect(self.refresh_workshop_status)
        btn_layout.addWidget(self.refresh_workshop_status_button, 1)

        self.apply_button = QPushButton("Apply")
        self.apply_button.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
        self.apply_button.clicked.connect(self.on_apply_launch_options)
        btn_layout.addWidget(self.apply_button, 1)

        launch_layout.addWidget(btn_row, lrow, 0, 1, 2)
        lrow += 1

        launch_layout.setColumnStretch(0, 1)
        launch_layout.setColumnStretch(1, 0)
        launch_layout.setRowStretch(lrow, 1)

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
            intro_path = os.path.join(video_dir, "BO3_Global_Logo_LogoSequence.mkv")
            self.skip_intro_cb.setChecked(existing_backup_path(intro_path) is not None)
            
            if os.path.exists(video_dir):
                mkv_files = [f for f in os.listdir(video_dir) if f.endswith('.mkv')]
                bak_files = [
                    f for f in os.listdir(video_dir)
                    if f.endswith(f".mkv{PATCHOPS_BACKUP_SUFFIX}") or f.endswith(f".mkv{LEGACY_BACKUP_SUFFIX}")
                ]
                self.skip_all_intro_cb.setChecked(len(bak_files) > 0 and len(mkv_files) == 0)

            dll_file = os.path.join(self.game_dir, "d3dcompiler_46.dll")
            self.reduce_stutter_cb.setChecked(existing_backup_path(dll_file) is not None)

    def set_log_widget(self, log_widget):
        self.log_widget = log_widget

    def _selected_launch_option(self):
        if self.radio_none.isChecked():
            return ""
        if self.radio_all_around.isChecked():
            return WORKSHOP_PROFILES["all_around"]["launch_option"]
        if self.radio_ultimate.isChecked():
            return WORKSHOP_PROFILES["ultimate"]["launch_option"]
        if self.radio_forged.isChecked():
            return WORKSHOP_PROFILES["forged"]["launch_option"]
        if self.radio_offline.isChecked():
            return "+set fs_game offlinemp"
        return ""

    def _selected_workshop_profile(self):
        if self.radio_all_around.isChecked():
            return WORKSHOP_PROFILES["all_around"]
        if self.radio_ultimate.isChecked():
            return WORKSHOP_PROFILES["ultimate"]
        if self.radio_forged.isChecked():
            return WORKSHOP_PROFILES["forged"]
        return None

    def _preserve_existing_wine_overrides(self, option):
        user_id = find_steam_user_id()
        if not user_id:
            return option
        config_path = os.path.join(steam_userdata_path, user_id, "config", "localconfig.vdf")
        if not os.path.exists(config_path):
            return option
        try:
            with open(config_path, "r", encoding="utf-8") as file:
                data = vdf.load(file)
            current_options = data.get("UserLocalConfigStore", {}).get("Software", {}).get("Valve", {}).get("Steam", {}).get("apps", {}).get(app_id, {}).get("LaunchOptions", "")
            if 'WINEDLLOVERRIDES="dsound=n,b"' in current_options:
                if option:
                    return f'WINEDLLOVERRIDES="dsound=n,b" %command% {option}'
                return 'WINEDLLOVERRIDES="dsound=n,b" %command%'
        except Exception as e:
            write_log(f"Error reading current launch options: {e}", "Error", self.log_widget)
        return option

    def refresh_workshop_status(self):
        for key, radio in self.workshop_profile_radios.items():
            profile = WORKSHOP_PROFILES[key]
            state = get_workshop_item_state(app_id, profile["workshop_id"])
            status_label_widget = self.workshop_profile_status_labels.get(key)
            if state.get("installed"):
                status_label = "Installed"
                status_state = "good"
            elif state.get("subscribed"):
                status_label = "Subscribed"
                status_state = "info"
            else:
                status_label = "Not Subscribed"
                status_state = "bad"

            radio.setText(profile["name"])
            if status_label_widget is not None:
                status_label_widget.setText(f"\u25cf {status_label}")
                status_label_widget.setProperty("workshopState", status_state)
                status_label_widget.style().unpolish(status_label_widget)
                status_label_widget.style().polish(status_label_widget)

    def skip_intro_changed(self):
        if not self.game_dir:
            return
        video_dir = os.path.join(self.game_dir, "video")
        intro_file = os.path.join(video_dir, "BO3_Global_Logo_LogoSequence.mkv")
        intro_file_bak = patchops_backup_path(intro_file)
        legacy_intro_bak = f"{intro_file}{LEGACY_BACKUP_SUFFIX}"

        if not os.path.exists(video_dir):
            write_log("Video directory not found.", "Warning", self.log_widget)
            return

        if self.skip_intro_cb.isChecked():
            # If backup exists, assume the intro is already skipped
            if os.path.exists(intro_file_bak) or os.path.exists(legacy_intro_bak):
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
            backup_path = existing_backup_path(intro_file)
            if backup_path:
                try:
                    os.rename(backup_path, intro_file)
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
                bak_path = patchops_backup_path(file_path)
                try:
                    if not os.path.exists(bak_path):
                        os.rename(file_path, bak_path)
                except Exception as e:
                    write_log(f"Failed to rename {mkv_file}: {e}", "Error", self.log_widget)
            write_log("All intro videos skipped.", "Success", self.log_widget)
        else:
            main_intro = "BO3_Global_Logo_LogoSequence.mkv"
            backup_candidates = [
                f for f in os.listdir(video_dir)
                if f.endswith(f".mkv{PATCHOPS_BACKUP_SUFFIX}") or f.endswith(f".mkv{LEGACY_BACKUP_SUFFIX}")
            ]
            for bak_file in backup_candidates:
                if bak_file.endswith(PATCHOPS_BACKUP_SUFFIX):
                    file_name = bak_file[:-len(PATCHOPS_BACKUP_SUFFIX)]
                else:
                    file_name = bak_file[:-len(LEGACY_BACKUP_SUFFIX)]

                # If user still wants main intro skipped, don't restore that one
                if file_name == main_intro and self.skip_intro_cb.isChecked():
                    continue

                bak_path = os.path.join(video_dir, bak_file)
                file_path = os.path.join(video_dir, file_name)
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
        dll_bak = patchops_backup_path(dll_file)
        legacy_dll_bak = f"{dll_file}{LEGACY_BACKUP_SUFFIX}"
        if self.reduce_stutter_cb.isChecked():
            if os.path.exists(dll_file):
                try:
                    os.rename(dll_file, dll_bak)
                    write_log("Renamed d3dcompiler_46.dll to reduce stuttering.", "Success", self.log_widget)
                except Exception:
                    write_log("Failed to rename d3dcompiler_46.dll.", "Error", self.log_widget)
            elif os.path.exists(dll_bak) or os.path.exists(legacy_dll_bak):
                write_log("Already using latest d3dcompiler.", "Success", self.log_widget)
            else:
                write_log("d3dcompiler_46.dll not found.", "Warning", self.log_widget)
        else:
            backup_path = existing_backup_path(dll_file)
            if backup_path:
                try:
                    os.rename(backup_path, dll_file)
                    write_log("Restored d3dcompiler_46.dll.", "Success", self.log_widget)
                except Exception:
                    write_log("Failed to restore d3dcompiler_46.dll.", "Error", self.log_widget)
            else:
                write_log("Backup not found to restore.", "Warning", self.log_widget)

    def on_apply_launch_options(self):
        option = self._preserve_existing_wine_overrides(self._selected_launch_option())
        if self.worker and self.worker.isRunning():
            return
        self.apply_button.setEnabled(False)
        self.worker = ApplyLaunchOptionsWorker(option)
        self.worker.log_message.connect(self.log_message_received) # Connect to the new log_message signal
        self.worker.finished.connect(self.on_apply_finished)
        self.worker.error.connect(self.on_apply_error)
        self.worker.start()

    def on_install_selected_workshop_mod(self):
        profile = self._selected_workshop_profile()
        if not profile:
            write_log(
                "Select a workshop launch option before using one-click install.",
                "Warning",
                self.log_widget,
            )
            return
        if self.workshop_install_worker and self.workshop_install_worker.isRunning():
            return

        option = self._preserve_existing_wine_overrides(self._selected_launch_option())
        workshop_url = f"steam://openurl/{profile['workshop_url']}"
        QDesktopServices.openUrl(QUrl(workshop_url))
        write_log(f"Opened {profile['name']} workshop page in Steam.", "Info", self.log_widget)

        self.apply_button.setEnabled(False)
        self.install_workshop_button.setEnabled(False)
        self.workshop_install_worker = ApplyLaunchOptionsWorker(option)
        self.workshop_install_worker.log_message.connect(self.log_message_received)
        self.workshop_install_worker.finished.connect(self.on_workshop_install_finished)
        self.workshop_install_worker.error.connect(self.on_workshop_install_error)
        self.workshop_install_worker.start()

    def log_message_received(self, message, category):
        write_log(message, category, self.log_widget)

    def on_apply_finished(self):
        self.apply_button.setEnabled(True)
        QTimer.singleShot(1500, self.refresh_workshop_status)

    def on_apply_error(self, error_message):
        self.apply_button.setEnabled(True)
        write_log(f"Error applying launch options: {error_message}", "Error", self.log_widget)

    def on_workshop_install_finished(self):
        self.apply_button.setEnabled(True)
        self.install_workshop_button.setEnabled(True)
        write_log(
            "Applied launch options for selected workshop mod. Steam may still need time to download content.",
            "Success",
            self.log_widget,
        )
        QTimer.singleShot(2000, self.refresh_workshop_status)

    def on_workshop_install_error(self, error_message):
        self.apply_button.setEnabled(True)
        self.install_workshop_button.setEnabled(True)
        write_log(f"Workshop install flow failed: {error_message}", "Error", self.log_widget)

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
        self._enhanced_warning_session_shown = False
        self._enhanced_active = False
        self._enhanced_worker: Optional[EnhancedDownloadWorker] = None
        self._reforged_worker: Optional[ReforgedInstallWorker] = None
        self._reforged_stored_password = ""
        self._enhanced_last_failed = False

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

        if self._system == "Windows":
            self._initialize_windows_updater()
        elif self._system == "Linux":
            QTimer.singleShot(2500, self._auto_check_for_linux_updates)

    def load_launch_options_state(self):
        current_options = self._get_applied_launch_options()
        if current_options is None:
            self.refresh_dashboard_status()
            return

        try:
            # Set radio button state based on current options without applying
            if "+set fs_game 2994481309" in current_options:
                self.qol_widget.radio_all_around.setChecked(True)
            elif "+set fs_game 2942053577" in current_options:
                self.qol_widget.radio_ultimate.setChecked(True)
            elif "+set fs_game 3667377161" in current_options:
                self.qol_widget.radio_forged.setChecked(True)
            elif "+set fs_game offlinemp" in current_options:
                self.qol_widget.radio_offline.setChecked(True)
            else:
                self.qol_widget.radio_none.setChecked(True)
            
        except Exception as e:
            write_log(f"Error loading launch options state: {e}", "Error", self.log_text)
        finally:
            self.qol_widget.refresh_workshop_status()
            self.refresh_dashboard_status()

    def init_ui(self):
        central = QWidget()
        main_layout = QVBoxLayout(central)
        main_layout.setContentsMargins(16, 16, 16, 16)
        main_layout.setSpacing(12)

        # Header bar with title, version, and quick actions
        header = QWidget()
        header_layout = QHBoxLayout(header)
        header_layout.setContentsMargins(0, 0, 0, 0)
        header_layout.setSpacing(10)

        title_label = QLabel("PatchOpsIII")
        title_label.setObjectName("HeadingTitle")
        version_label = QLabel(f"v{APP_VERSION}")
        version_label.setObjectName("HeadingVersion")

        header_layout.addWidget(title_label)
        header_layout.addWidget(version_label)
        header_layout.addStretch()

        self.update_button = None
        if self._system in ("Windows", "Linux"):
            self.update_button = QPushButton(self._default_update_button_text)
            self.update_button.setObjectName("SecondaryButton")
            self.update_button.clicked.connect(self.on_update_button_clicked)
            self.update_button.setSizePolicy(QSizePolicy.Fixed, QSizePolicy.Fixed)
            update_icon = load_ui_icon("update")
            if not update_icon.isNull():
                self.update_button.setIcon(update_icon)
                self.update_button.setIconSize(QSize(18, 18))
            header_layout.addWidget(self.update_button)

        launch_game_btn = QPushButton("Launch Game")
        launch_game_btn.setObjectName("PrimaryButton")
        launch_game_btn.clicked.connect(self.launch_game)
        launch_icon = load_ui_icon("launch")
        if not launch_icon.isNull():
            launch_game_btn.setIcon(launch_icon)
            launch_game_btn.setIconSize(QSize(18, 18))
        header_layout.addWidget(launch_game_btn)

        main_layout.addWidget(header)

        # Game Directory Section
        path_container = QWidget()
        path_container_layout = QVBoxLayout(path_container)
        path_container_layout.setContentsMargins(0, 0, 0, 0)
        path_container_layout.setSpacing(4)

        path_label = QLabel(
            "Current Directory:" if DEFAULT_GAME_DIR == get_application_path() else "Game Directory:"
        )
        path_label.setObjectName("HeadingVersion")
        path_container_layout.addWidget(path_label)

        directory_row = QHBoxLayout()
        directory_row.setContentsMargins(0, 0, 0, 0)
        directory_row.setSpacing(8)

        self.game_dir_edit = QLineEdit(DEFAULT_GAME_DIR)
        self.game_dir_edit.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
        self.game_dir_edit.setPlaceholderText("Black Ops III directory")
        directory_row.addWidget(self.game_dir_edit, 1)

        self.game_dir_edit.ensurePolished()
        directory_control_height = self.game_dir_edit.sizeHint().height()

        browse_btn = QPushButton("Browse…")
        browse_btn.setObjectName("SecondaryButton")
        browse_btn.setSizePolicy(QSizePolicy.Fixed, QSizePolicy.Fixed)
        browse_icon = load_ui_icon("browse")
        if not browse_icon.isNull():
            browse_btn.setIcon(browse_icon)
            browse_btn.setIconSize(QSize(18, 18))
        browse_btn.setFixedHeight(directory_control_height)
        browse_btn.clicked.connect(self.browse_game_dir)
        directory_row.addWidget(browse_btn)

        launch_game_btn.setFixedHeight(directory_control_height)
        if self.update_button is not None:
            self.update_button.setFixedHeight(directory_control_height)

        path_container_layout.addLayout(directory_row)
        main_layout.addWidget(path_container)

        # Individual widgets
        self.t7_patch_widget = T7PatchWidget(MOD_FILES_DIR)
        self.dxvk_widget = DXVKWidget(MOD_FILES_DIR)
        self.qol_widget = QualityOfLifeWidget()
        self.graphics_widget = GraphicsSettingsWidget(dxvk_widget=self.dxvk_widget)
        self.advanced_widget = AdvancedSettingsWidget()
        self.enhanced_group = self._build_enhanced_group()
        self.dashboard_status_group = self._build_dashboard_status_group()

        # Tabs
        self.tabs = SidebarTabWidget()
        self.tabs.currentChanged.connect(self.on_tab_changed)
        self.tabs.setDocumentMode(True)

        # Dashboard Tab with Grid Layout
        dashboard_tab = QWidget()
        dashboard_grid = QGridLayout(dashboard_tab)
        dashboard_grid.setContentsMargins(5, 5, 5, 5)
        dashboard_grid.setHorizontalSpacing(14)
        dashboard_grid.setVerticalSpacing(10)

        # Row 0: full-width status overview
        dashboard_grid.addWidget(self.dashboard_status_group, 0, 0, 1, 2)

        # Row 1: quality-of-life (left) and launch options (right)
        dashboard_grid.addWidget(self.qol_widget.checkbox_group, 1, 0)
        dashboard_grid.addWidget(self.qol_widget.launch_group, 1, 1)

        # Set column and row stretches
        dashboard_grid.setColumnStretch(0, 1)
        dashboard_grid.setColumnStretch(1, 1)
        dashboard_grid.setRowStretch(0, 0)
        dashboard_grid.setRowStretch(1, 1)

        self.tabs.addTab(dashboard_tab, load_ui_icon("mods"), "Dashboard")

        # T7 Patch Tab
        t7_tab = QWidget()
        t7_layout = QVBoxLayout(t7_tab)
        t7_layout.addWidget(self.t7_patch_widget.groupbox)
        t7_layout.addStretch(1)
        self.tabs.addTab(t7_tab, load_ui_icon("t7patch"), "T7 Patch")

        # Enhanced Tab
        enhanced_tab = QWidget()
        enhanced_layout = QVBoxLayout(enhanced_tab)
        enhanced_layout.addWidget(self.enhanced_group)
        enhanced_layout.addStretch(1)
        self.enhanced_tab_index = self.tabs.addTab(enhanced_tab, load_ui_icon("enhanced"), "Enhanced")

        # Reforged Tab
        reforged_tab = QWidget()
        reforged_layout = QVBoxLayout(reforged_tab)
        reforged_layout.addWidget(self._build_reforged_group())
        reforged_layout.addStretch(1)
        self.tabs.addTab(reforged_tab, load_ui_icon("reforged"), "Reforged")

        # Graphics Tab
        graphics_tab = QWidget()
        graphics_layout = QVBoxLayout(graphics_tab)
        graphics_layout.addWidget(self.graphics_widget)
        self.tabs.addTab(graphics_tab, load_ui_icon("graphics"), "Graphics")

        # Advanced Tab
        advanced_tab = QWidget()
        advanced_layout = QVBoxLayout(advanced_tab)
        advanced_layout.addWidget(self.advanced_widget)
        self.advanced_tab_index = self.tabs.addTab(advanced_tab, load_ui_icon("advanced"), "Advanced")

        main_layout.addWidget(self.tabs)

        # Log Window
        log_group = QGroupBox("Activity Log")
        log_layout = QVBoxLayout(log_group)
        log_layout.setContentsMargins(8, 8, 8, 8)
        self.log_text = QTextEdit()
        self.log_text.setObjectName("LogView")
        self.log_text.setReadOnly(True)
        self.log_text.setMaximumHeight(130)
        log_layout.addWidget(self.log_text)
        main_layout.addWidget(log_group)

        self.setCentralWidget(central)
        self.setMinimumSize(820, 560)
        self.resize(1020, 680)

        # Initialize with the default directory
        game_dir = self.game_dir_edit.text().strip()
        self._apply_game_directory(game_dir)
        self.t7_patch_widget.set_log_widget(self.log_text)
        self.dxvk_widget.set_log_widget(self.log_text)
        self.graphics_widget.set_log_widget(self.log_text)
        self.advanced_widget.set_log_widget(self.log_text)
        self.advanced_widget.set_mod_files_dir(MOD_FILES_DIR)
        self.qol_widget.set_log_widget(self.log_text)
        self.qol_widget.refresh_workshop_status()
        self.refresh_enhanced_status(show_warning=True)
        self.refresh_dashboard_status()

        for control in (
            self.qol_widget.radio_none,
            self.qol_widget.radio_all_around,
            self.qol_widget.radio_ultimate,
            self.qol_widget.radio_forged,
            self.qol_widget.radio_offline,
            self.qol_widget.reduce_stutter_cb,
            self.qol_widget.skip_intro_cb,
            self.qol_widget.skip_all_intro_cb,
        ):
            signal = getattr(control, "toggled", None)
            if signal is not None:
                signal.connect(self._on_dashboard_state_changed)

    def _build_enhanced_group(self):
        group = QGroupBox("BO3 Enhanced")
        layout = QGridLayout(group)
        layout.setContentsMargins(14, 14, 14, 14)
        layout.setHorizontalSpacing(10)
        layout.setVerticalSpacing(0)

        row = 0

        def _add_sep(r):
            sep = QFrame()
            sep.setObjectName("DashboardDivider")
            sep.setFrameShape(QFrame.HLine)
            sep.setFixedHeight(1)
            layout.addWidget(sep, r, 0, 1, 4)

        # Status row
        _add_sep(row); row += 1

        status_name = QLabel("Status")
        status_name.setObjectName("DashboardStatusName")
        status_name.setContentsMargins(0, 8, 0, 8)

        self.enhanced_status_label = QLabel(self._status_html("Not installed"))
        self.enhanced_status_label.setObjectName("DashboardStatusValue")
        self.enhanced_status_label.setAlignment(Qt.AlignRight | Qt.AlignVCenter)
        self.enhanced_status_label.setTextFormat(Qt.RichText)
        self.enhanced_status_label.setContentsMargins(0, 8, 0, 8)

        layout.addWidget(status_name, row, 0)
        layout.addWidget(self.enhanced_status_label, row, 1, 1, 3)
        row += 1

        # Game Dump row
        _add_sep(row); row += 1

        dump_name = QLabel("Game Dump")
        dump_name.setObjectName("DashboardStatusName")
        dump_name.setContentsMargins(0, 8, 0, 8)

        self.enhanced_dump_edit = QLineEdit()
        self.enhanced_dump_edit.setPlaceholderText("Select DUMP.zip or BlackOps3.exe from dump folder…")

        browse_btn = QPushButton("Browse")
        browse_btn.clicked.connect(self._enhanced_smart_browse)

        layout.addWidget(dump_name, row, 0)
        layout.addWidget(self.enhanced_dump_edit, row, 1, 1, 2)
        layout.addWidget(browse_btn, row, 3)
        row += 1

        # Action buttons
        _add_sep(row); row += 1

        btn_row = QWidget()
        btn_layout = QHBoxLayout(btn_row)
        btn_layout.setContentsMargins(0, 8, 0, 12)
        btn_layout.setSpacing(10)

        self.enhanced_install_btn = QPushButton("Install / Update Enhanced")
        self.enhanced_install_btn.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
        self.enhanced_install_btn.clicked.connect(self.on_enhanced_install_clicked)
        btn_layout.addWidget(self.enhanced_install_btn, 1)

        self.enhanced_uninstall_btn = QPushButton("Uninstall Enhanced")
        self.enhanced_uninstall_btn.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
        self.enhanced_uninstall_btn.clicked.connect(self.on_enhanced_uninstall_clicked)
        btn_layout.addWidget(self.enhanced_uninstall_btn, 1)

        layout.addWidget(btn_row, row, 0, 1, 4)
        row += 1

        # Info / guide
        _add_sep(row); row += 1

        info = QLabel(
            "PatchOpsIII cannot download game files automatically due to legal restrictions. "
            "A UWP game dump must be provided manually."
        )
        info.setObjectName("DashboardStatusName")
        info.setWordWrap(True)
        info.setContentsMargins(0, 8, 0, 4)
        layout.addWidget(info, row, 0, 1, 4)
        row += 1

        guide_link = QLabel('<a href="https://youtu.be/rBZZTcSJ9_s?si=41p0r_Enten3h5AQ">Watch the dump guide on YouTube, and read the video description →</a>')
        guide_link.setOpenExternalLinks(True)
        guide_link.setContentsMargins(0, 0, 0, 8)
        layout.addWidget(guide_link, row, 0, 1, 4)
        row += 1

        layout.setColumnStretch(0, 1)
        layout.setColumnStretch(1, 1)
        layout.setColumnStretch(2, 2)
        layout.setColumnStretch(3, 0)
        layout.setRowStretch(row, 1)

        return group

    def _build_dashboard_status_group(self):
        group = QGroupBox("Status Overview")
        layout = QGridLayout(group)
        layout.setContentsMargins(14, 14, 14, 14)
        layout.setHorizontalSpacing(0)
        layout.setVerticalSpacing(0)

        status_rows = [
            ("T7 Patch", "dashboard_t7_status"),
            ("DXVK-GPLAsync", "dashboard_dxvk_status"),
            ("BO3 Enhanced", "dashboard_enhanced_status"),
            ("BO3 Reforged", "dashboard_reforged_status"),
            ("Launch Option", "dashboard_launch_status"),
            ("Quality of Life", "dashboard_qol_status"),
        ]

        for i, (name_text, attr) in enumerate(status_rows):
            # Thin separator line between rows (except first)
            if i > 0:
                sep = QFrame()
                sep.setObjectName("DashboardDivider")
                sep.setFrameShape(QFrame.HLine)
                sep.setFixedHeight(1)
                layout.addWidget(sep, i * 2 - 1, 0, 1, 2)

            name_lbl = QLabel(name_text)
            name_lbl.setObjectName("DashboardStatusName")
            name_lbl.setContentsMargins(0, 5, 0, 5)

            val_lbl = QLabel(self._status_html("—"))
            val_lbl.setObjectName("DashboardStatusValue")
            val_lbl.setAlignment(Qt.AlignRight | Qt.AlignVCenter)
            val_lbl.setTextFormat(Qt.RichText)
            val_lbl.setContentsMargins(0, 5, 0, 5)

            setattr(self, attr, val_lbl)

            layout.addWidget(name_lbl, i * 2, 0)
            layout.addWidget(val_lbl, i * 2, 1)

        layout.setColumnStretch(0, 1)
        layout.setColumnStretch(1, 2)
        layout.setRowStretch(len(status_rows) * 2, 1)
        return group

    def _on_dashboard_state_changed(self, *_):
        self.refresh_dashboard_status()

    @staticmethod
    def _status_html(text: str, state: str = "neutral") -> str:
        """Return an HTML-colored status string for dashboard labels.

        state values: 'good' (green), 'bad' (muted red), 'info' (blue), 'neutral' (gray)
        """
        colors = {
            "good": "#4ade80",
            "bad": "#f87171",
            "info": "#60a5fa",
            "neutral": "#9ca3af",
        }
        color = colors.get(state, colors["neutral"])
        return f'<span style="color:{color};">&#9679; {text}</span>'

    def _get_applied_launch_options(self):
        user_id = find_steam_user_id()
        if not user_id:
            return None
        config_path = os.path.join(steam_userdata_path, user_id, "config", "localconfig.vdf")
        if not os.path.exists(config_path):
            return None
        try:
            with open(config_path, "r", encoding="utf-8") as file:
                data = vdf.load(file)
            return data.get("UserLocalConfigStore", {}).get("Software", {}).get("Valve", {}).get("Steam", {}).get("apps", {}).get(app_id, {}).get("LaunchOptions", "")
        except Exception:
            return None

    @staticmethod
    def _launch_option_name_from_string(launch_options: str):
        options = launch_options or ""
        if "+set fs_game 2994481309" in options:
            return "All-around Enhancement Lite"
        if "+set fs_game 2942053577" in options:
            return "Ultimate Experience Mod"
        if "+set fs_game 3667377161" in options:
            return "Forged"
        if "+set fs_game offlinemp" in options:
            return "Play Offline"
        return "Default (None)"

    def _set_reforged_composite_status(self, *, exe_installed: bool, launch_active: bool):
        exe_text = "Installed" if exe_installed else "Not Installed"
        launch_text = "Active" if launch_active else "Inactive"
        status_text = f"{exe_text} | {launch_text}"

        if exe_installed and launch_active:
            state = "good"
        elif exe_installed:
            state = "info"
        elif launch_active:
            state = "info"
        else:
            state = "neutral"

        self.dashboard_reforged_status.setText(self._status_html(status_text, state))

        # Keep the Reforged tab status aligned with dashboard state unless a worker
        # is currently reporting install/uninstall progress.
        worker_running = bool(self._reforged_worker and self._reforged_worker.isRunning())
        if not worker_running and hasattr(self, "reforged_status_label"):
            self.reforged_status_label.setText(self._status_html(status_text, state))

    def refresh_dashboard_status(self):
        game_dir = self.game_dir_edit.text().strip()
        if not os.path.isdir(game_dir):
            _na = self._status_html("Not configured", "neutral")
            self.dashboard_t7_status.setText(_na)
            self.dashboard_dxvk_status.setText(_na)
            self.dashboard_enhanced_status.setText(_na)
            self.dashboard_reforged_status.setText(_na)
            self.dashboard_launch_status.setText(_na)
            self.dashboard_qol_status.setText(_na)
            return

        t7_status = check_t7_patch_status(game_dir)
        t7_installed = os.path.exists(os.path.join(game_dir, "t7patch.conf"))
        t7_label = "Installed" if t7_installed else "Not Installed"
        if t7_installed and t7_status.get("gamertag"):
            t7_label = f"Installed ({t7_status.get('plain_name', t7_status['gamertag'])})"
        self.dashboard_t7_status.setText(
            self._status_html(t7_label, "good" if t7_installed else "neutral")
        )

        dxvk_installed = is_dxvk_async_installed(game_dir)
        self.dashboard_dxvk_status.setText(
            self._status_html("Installed" if dxvk_installed else "Not Installed",
                              "good" if dxvk_installed else "neutral")
        )

        enhanced_summary = status_summary(game_dir, STORAGE_PATH)
        enhanced_active = bool(enhanced_summary.get("installed"))
        self.dashboard_enhanced_status.setText(
            self._status_html("Active" if enhanced_active else "Not Installed",
                              "good" if enhanced_active else "neutral")
        )

        applied_launch_options = self._get_applied_launch_options() or ""
        launch_name = self._launch_option_name_from_string(applied_launch_options)

        exe_variant = read_exe_variant(game_dir)
        reforged_exe_installed = exe_variant == "reforged"
        reforged_launch_active = "+set fs_game 3667377161" in applied_launch_options
        self._set_reforged_composite_status(
            exe_installed=reforged_exe_installed,
            launch_active=reforged_launch_active,
        )

        launch_state = "neutral" if launch_name == "Default (None)" else "info"
        self.dashboard_launch_status.setText(self._status_html(launch_name, launch_state))

        active_qol = []
        if self.qol_widget.reduce_stutter_cb.isChecked():
            active_qol.append("Latest d3dcompiler")
        if self.qol_widget.skip_intro_cb.isChecked() and not self.qol_widget.skip_all_intro_cb.isChecked():
            active_qol.append("Skip Intro")
        if self.qol_widget.skip_all_intro_cb.isChecked():
            active_qol.append("Skip All Intros")
        qol_text = ", ".join(active_qol) if active_qol else "None enabled"
        self.dashboard_qol_status.setText(
            self._status_html(qol_text, "good" if active_qol else "neutral")
        )

    def _set_reforged_password(self, password: str):
        self._reforged_stored_password = password or ""
        if self._reforged_stored_password:
            text = self._reforged_stored_password if self._reforged_pw_display_eye.isChecked() else "••••••"
        else:
            text = "None"
        self.reforged_current_pw_label.setText(text)

    def _toggle_reforged_pw_display(self, checked: bool):
        self._reforged_pw_display_eye.setIcon(load_ui_icon("eye" if checked else "eye-off"))
        if self._reforged_stored_password:
            self.reforged_current_pw_label.setText(self._reforged_stored_password if checked else "••••••")

    def _toggle_reforged_pw_edit(self, checked: bool):
        self._reforged_pw_edit_eye.setIcon(load_ui_icon("eye" if checked else "eye-off"))
        self.reforged_password_edit.setEchoMode(QLineEdit.Normal if checked else QLineEdit.Password)

    def _build_reforged_group(self):
        group = QGroupBox("BO3 Reforged")
        layout = QGridLayout(group)
        layout.setContentsMargins(14, 14, 14, 14)
        layout.setHorizontalSpacing(10)
        layout.setVerticalSpacing(0)

        row = 0

        # Action buttons
        btn_row = QWidget()
        btn_layout = QHBoxLayout(btn_row)
        btn_layout.setContentsMargins(0, 0, 0, 12)
        btn_layout.setSpacing(10)

        self.reforged_install_btn = QPushButton("Install Reforged")
        self.reforged_install_btn.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
        self.reforged_install_btn.clicked.connect(self.install_reforged)
        btn_layout.addWidget(self.reforged_install_btn, 1)

        self.reforged_uninstall_btn = QPushButton("Uninstall Reforged")
        self.reforged_uninstall_btn.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
        self.reforged_uninstall_btn.clicked.connect(self.uninstall_reforged)
        btn_layout.addWidget(self.reforged_uninstall_btn, 1)

        refresh_btn = QPushButton("Refresh T7")
        refresh_btn.clicked.connect(lambda: self.load_reforged_t7_options(user_initiated=True))
        btn_layout.addWidget(refresh_btn)

        layout.addWidget(btn_row, row, 0, 1, 4)
        row += 1

        def _add_sep(r):
            sep = QFrame()
            sep.setObjectName("DashboardDivider")
            sep.setFrameShape(QFrame.HLine)
            sep.setFixedHeight(1)
            layout.addWidget(sep, r, 0, 1, 4)

        # Status row
        _add_sep(row); row += 1

        status_name = QLabel("Status")
        status_name.setObjectName("DashboardStatusName")
        status_name.setContentsMargins(0, 8, 0, 8)

        self.reforged_status_label = QLabel(self._status_html("Not installed"))
        self.reforged_status_label.setObjectName("DashboardStatusValue")
        self.reforged_status_label.setAlignment(Qt.AlignRight | Qt.AlignVCenter)
        self.reforged_status_label.setTextFormat(Qt.RichText)
        self.reforged_status_label.setContentsMargins(0, 8, 0, 8)

        layout.addWidget(status_name, row, 0)
        layout.addWidget(self.reforged_status_label, row, 1, 1, 3)
        row += 1

        # Network Password row
        _add_sep(row); row += 1

        pw_name = QLabel("Network Password")
        pw_name.setObjectName("DashboardStatusName")
        pw_name.setContentsMargins(0, 8, 0, 8)

        pw_display_container = QWidget()
        pw_display_layout = QHBoxLayout(pw_display_container)
        pw_display_layout.setContentsMargins(0, 0, 0, 0)
        pw_display_layout.setSpacing(4)

        self.reforged_current_pw_label = QLabel("None")
        self.reforged_current_pw_label.setObjectName("DashboardStatusValue")
        self.reforged_current_pw_label.setAlignment(Qt.AlignRight | Qt.AlignVCenter)
        self.reforged_current_pw_label.setContentsMargins(0, 8, 0, 8)
        pw_display_layout.addWidget(self.reforged_current_pw_label, 1)

        self._reforged_pw_display_eye = QPushButton()
        self._reforged_pw_display_eye.setIcon(load_ui_icon("eye-off"))
        self._reforged_pw_display_eye.setIconSize(QSize(14, 14))
        self._reforged_pw_display_eye.setFixedSize(22, 22)
        self._reforged_pw_display_eye.setFlat(True)
        self._reforged_pw_display_eye.setCheckable(True)
        self._reforged_pw_display_eye.setToolTip("Show / hide password")
        self._reforged_pw_display_eye.toggled.connect(self._toggle_reforged_pw_display)
        pw_display_layout.addWidget(self._reforged_pw_display_eye, 0)

        pw_edit_container = QWidget()
        pw_edit_layout = QHBoxLayout(pw_edit_container)
        pw_edit_layout.setContentsMargins(0, 0, 0, 0)
        pw_edit_layout.setSpacing(4)

        self.reforged_password_edit = QLineEdit()
        self.reforged_password_edit.setPlaceholderText("Enter network password…")
        self.reforged_password_edit.setEchoMode(QLineEdit.Password)
        pw_edit_layout.addWidget(self.reforged_password_edit, 1)

        self._reforged_pw_edit_eye = QPushButton()
        self._reforged_pw_edit_eye.setIcon(load_ui_icon("eye-off"))
        self._reforged_pw_edit_eye.setIconSize(QSize(14, 14))
        self._reforged_pw_edit_eye.setFixedSize(22, 22)
        self._reforged_pw_edit_eye.setFlat(True)
        self._reforged_pw_edit_eye.setCheckable(True)
        self._reforged_pw_edit_eye.setToolTip("Show / hide password")
        self._reforged_pw_edit_eye.toggled.connect(self._toggle_reforged_pw_edit)
        pw_edit_layout.addWidget(self._reforged_pw_edit_eye, 0)

        update_btn = QPushButton("Update")
        update_btn.clicked.connect(self.apply_reforged_t7_options)

        layout.addWidget(pw_name, row, 0)
        layout.addWidget(pw_display_container, row, 1)
        layout.addWidget(pw_edit_container, row, 2)
        layout.addWidget(update_btn, row, 3)
        row += 1

        # Checkboxes row
        _add_sep(row); row += 1

        cb_row = QWidget()
        cb_layout = QHBoxLayout(cb_row)
        cb_layout.setContentsMargins(0, 8, 0, 8)
        cb_layout.setSpacing(16)

        self.reforged_force_ranked_cb = QCheckBox("Force Ranked Mode")
        self.reforged_steam_achievements_cb = QCheckBox("Steam Achievements")
        cb_layout.addWidget(self.reforged_force_ranked_cb)
        cb_layout.addWidget(self.reforged_steam_achievements_cb)
        cb_layout.addStretch(1)

        layout.addWidget(cb_row, row, 0, 1, 4)
        row += 1

        # Notes
        _add_sep(row); row += 1

        workshop_note = QLabel(
            f'Installing Reforged also installs the <a href="{REFORGED_WORKSHOP_STEAM_URL}">BO3 Reforged Workshop mod</a>.'
        )
        workshop_note.setObjectName("DashboardStatusName")
        workshop_note.setOpenExternalLinks(True)
        workshop_note.setContentsMargins(0, 8, 0, 4)
        layout.addWidget(workshop_note, row, 0, 1, 4)
        row += 1

        compat_note = QLabel(
            "T7Patch compatible: Reforged can be used with or without T7Patch. "
            "If both players use Reforged, matching passwords are cross-compatible."
        )
        compat_note.setObjectName("DashboardStatusName")
        compat_note.setWordWrap(True)
        compat_note.setContentsMargins(0, 0, 0, 8)
        layout.addWidget(compat_note, row, 0, 1, 4)
        row += 1

        layout.setColumnStretch(0, 1)
        layout.setColumnStretch(1, 1)
        layout.setColumnStretch(2, 2)
        layout.setColumnStretch(3, 0)
        layout.setRowStretch(row, 1)

        return group

    def install_reforged(self):
        game_dir = self.game_dir_edit.text().strip()
        if not os.path.isdir(game_dir):
            write_log("Set a valid game directory before installing Reforged.", "Error", self.log_text)
            return
        if self._reforged_worker and self._reforged_worker.isRunning():
            return

        self.reforged_install_btn.setEnabled(False)
        self.reforged_uninstall_btn.setEnabled(False)
        self.reforged_status_label.setText(self._status_html("Installing Reforged…", "info"))

        self._reforged_worker = ReforgedInstallWorker(game_dir)
        self._reforged_worker.progress.connect(self._on_reforged_progress)
        self._reforged_worker.installed.connect(self._on_reforged_installed)
        self._reforged_worker.failed.connect(self._on_reforged_failed)
        self._reforged_worker.start()

    def _on_reforged_progress(self, message: str):
        self.reforged_status_label.setText(self._status_html(message, "info"))
        write_log(message, "Info", self.log_text)

    def _on_reforged_installed(self, target_path: str):
        self.reforged_install_btn.setEnabled(True)
        self.reforged_uninstall_btn.setEnabled(True)
        self.reforged_status_label.setText(self._status_html("Installed", "good"))
        write_log(f"Reforged installed to {target_path}", "Success", self.log_text)
        write_exe_variant(self.game_dir_edit.text().strip(), "reforged")
        self.refresh_dashboard_status()
        self.t7_patch_widget.refresh_t7_mode_indicator()
        QDesktopServices.openUrl(QUrl(REFORGED_WORKSHOP_STEAM_URL))
        write_log("Opened Reforged Workshop page in Steam.", "Info", self.log_text)

    def _on_reforged_failed(self, reason: str):
        self.reforged_install_btn.setEnabled(True)
        self.reforged_uninstall_btn.setEnabled(True)
        self.reforged_status_label.setText(self._status_html(f"Install Failed — {reason}", "bad"))
        write_log(f"Reforged install failed: {reason}", "Error", self.log_text)

    def uninstall_reforged(self):
        game_dir = self.game_dir_edit.text().strip()
        if not os.path.isdir(game_dir):
            write_log("Set a valid game directory before uninstalling Reforged.", "Error", self.log_text)
            return

        target_exe = find_game_executable(game_dir) or os.path.join(game_dir, "BlackOps3.exe")
        backup_path = existing_backup_path(target_exe)

        if not backup_path or not os.path.exists(backup_path):
            write_log("No backup executable found — cannot restore original.", "Error", self.log_text)
            self.reforged_status_label.setText(self._status_html("Uninstall Failed — no backup found", "bad"))
            return

        self.reforged_install_btn.setEnabled(False)
        self.reforged_uninstall_btn.setEnabled(False)
        self.reforged_status_label.setText(self._status_html("Uninstalling…", "info"))

        try:
            if os.path.exists(target_exe):
                os.remove(target_exe)
            os.rename(backup_path, target_exe)
            write_exe_variant(game_dir, "default")
            write_log("Reforged uninstalled. Original executable restored.", "Success", self.log_text)
            self.reforged_status_label.setText(self._status_html("Uninstalled", "neutral"))
            self.refresh_dashboard_status()
            self.t7_patch_widget.refresh_t7_mode_indicator()
        except Exception as exc:
            write_log(f"Reforged uninstall failed: {exc}", "Error", self.log_text)
            self.reforged_status_label.setText(self._status_html(f"Uninstall Failed — {exc}", "bad"))
        finally:
            self.reforged_install_btn.setEnabled(True)
            self.reforged_uninstall_btn.setEnabled(True)

    def _t7_json_path(self):
        game_dir = self.game_dir_edit.text().strip()
        if not game_dir:
            return None
        return os.path.join(game_dir, "players", "T7.json")

    def load_reforged_t7_options(self, user_initiated: bool = False):
        path = self._t7_json_path()
        if not path:
            return

        if not os.path.exists(path):
            self.reforged_password_edit.setText("")
            self.reforged_force_ranked_cb.setChecked(False)
            self.reforged_steam_achievements_cb.setChecked(False)
            self._set_reforged_password("")
            if user_initiated:
                write_log("T7.json not found. Default values loaded.", "Info", self.log_text)
            return

        try:
            with open(path, "r", encoding="utf-8") as handle:
                data = json.load(handle)
            network_pass = str(data.get("network_pass", ""))
            self.reforged_password_edit.setText(network_pass)
            self.reforged_force_ranked_cb.setChecked(bool(data.get("force_ranked", False)))
            self.reforged_steam_achievements_cb.setChecked(bool(data.get("steam_achievements", False)))
            self._set_reforged_password(network_pass)
            if user_initiated:
                write_log(f"Loaded T7 options from {path}", "Success", self.log_text)
        except Exception as exc:
            write_log(f"Failed to read T7.json: {exc}", "Error", self.log_text)

    def apply_reforged_t7_options(self):
        path = self._t7_json_path()
        if not path:
            return

        data = {}
        if os.path.exists(path):
            try:
                with open(path, "r", encoding="utf-8") as handle:
                    data = json.load(handle)
            except Exception as exc:
                write_log(f"Failed to parse existing T7.json, creating a fresh file: {exc}", "Warning", self.log_text)
                data = {}

        os.makedirs(os.path.dirname(path), exist_ok=True)

        password = self.reforged_password_edit.text().strip()
        if password:
            data["network_pass"] = password
        elif "network_pass" in data:
            data.pop("network_pass", None)

        data["force_ranked"] = self.reforged_force_ranked_cb.isChecked()
        data["steam_achievements"] = self.reforged_steam_achievements_cb.isChecked()

        try:
            with open(path, "w", encoding="utf-8") as handle:
                json.dump(data, handle, indent=4)
            write_log(f"Updated T7 settings at {path}", "Success", self.log_text)
            self._set_reforged_password(password)
        except Exception as exc:
            write_log(f"Failed to write T7.json: {exc}", "Error", self.log_text)

    def on_tab_changed(self, index):
        tab_widget = self.tabs.widget(index)
        if tab_widget:
            # Refresh advanced settings if on that tab
            if index == getattr(self, "advanced_tab_index", -1):
                self.advanced_widget.refresh_settings()
            tab_widget.layout().update()
        if index == 0 or index == getattr(self, "enhanced_tab_index", -1):
            self.refresh_enhanced_status(show_warning=False)
        if index == 0:
            self.qol_widget.refresh_workshop_status()
        self.refresh_dashboard_status()

    def refresh_enhanced_status(self, *, show_warning: bool):
        summary = status_summary(self.game_dir_edit.text().strip(), STORAGE_PATH)
        active = bool(summary.get("installed"))
        self._enhanced_active = active
        self.enhanced_status_label.setText(
            self._status_html("Enhanced Mode Active", "good") if active
            else self._status_html("Not installed", "neutral")
        )
        self._toggle_launch_options_enabled(not active)
        self.refresh_dashboard_status()
        if active and show_warning and not self._enhanced_warning_session_shown and not summary.get("acknowledged_at"):
            self._show_enhanced_warning()

    def _toggle_launch_options_enabled(self, enabled: bool):
        self.qol_widget.launch_group.setEnabled(enabled)
        if enabled:
            self.qol_widget.launch_group.setToolTip("")
        else:
            self.qol_widget.launch_group.setToolTip(
                "Launch options are disabled while BO3 Enhanced is active."
            )

    def _show_enhanced_warning(self):
        message = (
            "Launch options are disabled when BO3 Enhanced is active. "
            "Most third-party mods will not be compatible."
        )
        QMessageBox.warning(self, "BO3 Enhanced Warning", message)
        set_acknowledged(STORAGE_PATH)
        self._enhanced_warning_session_shown = True

    def _enhanced_smart_browse(self):
        filter_str = "Dump Sources (DUMP.zip BlackOps3.exe *.zip);;All Files (*)"
        path, _ = QFileDialog.getOpenFileName(self, "Select Dump Source", "", filter_str)
        if not path:
            return
        if path.lower().endswith(".zip"):
            final_path = path
        elif os.path.basename(path).lower() == "blackops3.exe":
            final_path = os.path.dirname(path)
        else:
            final_path = os.path.dirname(path) if os.path.isfile(path) else path
        self.enhanced_dump_edit.setText(final_path)

    def on_enhanced_install_clicked(self):
        path = self.enhanced_dump_edit.text().strip()
        if not path:
            QMessageBox.warning(self, "Dump Required", "Please select a game dump source before installing.")
            return
        if not os.path.exists(path):
            QMessageBox.critical(self, "Invalid Path", "The selected path does not exist.")
            return
        if not validate_dump_source(path):
            QMessageBox.critical(
                self,
                "Invalid Dump",
                "The selected source is missing required files (BlackOps3.exe, MicrosoftGame.config).",
            )
            return

        self._pending_dump_source = path

        if self._enhanced_worker and self._enhanced_worker.isRunning():
            return

        self.enhanced_install_btn.setEnabled(False)
        self.enhanced_uninstall_btn.setEnabled(False)
        self.enhanced_status_label.setText(self._status_html("Downloading Enhanced files…", "info"))

        self._enhanced_worker = EnhancedDownloadWorker(MOD_FILES_DIR, STORAGE_PATH)
        self._enhanced_worker.progress.connect(self._on_enhanced_progress)
        self._enhanced_worker.download_complete.connect(self._on_enhanced_download_finished)
        self._enhanced_worker.failed.connect(self._on_enhanced_download_failed)
        self._enhanced_last_failed = False
        self._enhanced_worker.start()

    def _on_enhanced_progress(self, message: str):
        self.enhanced_status_label.setText(self._status_html(message, "info"))
        write_log(message, "Info", self.log_text)

    def _on_enhanced_download_finished(self):
        # Disconnect to prevent double-execution
        try:
            if self._enhanced_worker:
                self._enhanced_worker.download_complete.disconnect(self._on_enhanced_download_finished)
        except Exception:
            pass

        if self._enhanced_last_failed:
            self._reset_enhanced_buttons()
            return
            
        self.enhanced_status_label.setText(self._status_html("Installing files…", "info"))
        write_log("Download complete. Installing BO3 Enhanced files...", "Success", self.log_text)

        # Proceed immediately to install using the pending dump source
        game_dir = self.game_dir_edit.text().strip()
        dump_source = getattr(self, "_pending_dump_source", None)

        if not dump_source or not os.path.exists(dump_source):
            self.enhanced_status_label.setText(self._status_html("Installation Failed — Dump missing", "bad"))
            write_log("Pending dump source missing.", "Error", self.log_text)
            self._reset_enhanced_buttons()
            return

        if install_enhanced_files(game_dir, MOD_FILES_DIR, STORAGE_PATH, dump_source, log_widget=self.log_text):
            self.enhanced_status_label.setText(self._status_html("Installed Successfully", "good"))
            write_exe_variant(game_dir, "enhanced")
            self.refresh_enhanced_status(show_warning=True)
            self.t7_patch_widget.refresh_t7_mode_indicator()
        else:
            self.enhanced_status_label.setText(self._status_html("Installation Failed", "bad"))
            
        self._reset_enhanced_buttons()

    def _on_enhanced_download_failed(self, reason: str):
        self._enhanced_last_failed = True
        self.enhanced_status_label.setText(self._status_html(f"Download Failed — {reason}", "bad"))
        write_log(f"BO3 Enhanced download failed: {reason}", "Error", self.log_text)
        self._reset_enhanced_buttons()

    def _reset_enhanced_buttons(self):
        self.enhanced_install_btn.setEnabled(True)
        self.enhanced_uninstall_btn.setEnabled(True)





    def on_enhanced_uninstall_clicked(self):
        self.enhanced_install_btn.setEnabled(False)
        self.enhanced_uninstall_btn.setEnabled(False)
        self.enhanced_status_label.setText(self._status_html("Uninstalling…", "info"))

        game_dir = self.game_dir_edit.text().strip()
        # Use QTimer to allow UI update before blocking operation
        QTimer.singleShot(100, lambda: self._perform_uninstall(game_dir))

    def _perform_uninstall(self, game_dir):
        if uninstall_enhanced_files(game_dir, MOD_FILES_DIR, STORAGE_PATH, log_widget=self.log_text):
            self.enhanced_status_label.setText(self._status_html("Uninstalled", "neutral"))
            write_exe_variant(game_dir, "default")
            self.refresh_enhanced_status(show_warning=False)
            self.t7_patch_widget.refresh_t7_mode_indicator()
        else:
            self.enhanced_status_label.setText(self._status_html("Uninstall Failed", "bad"))

        self.enhanced_install_btn.setEnabled(True)
        self.enhanced_uninstall_btn.setEnabled(True)

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
            install_dir=os.path.dirname(os.path.abspath(sys.argv[0])),
            executable_path=os.path.abspath(sys.argv[0]),
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
            f"Install PatchOpsIII {release.version} now?\n\n"
            "The application will close while the update is applied."
        )
        if QMessageBox.question(self, "Install Update", prompt, QMessageBox.Yes | QMessageBox.No) == QMessageBox.Yes:
            self._apply_staged_update()

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
        # If we've already staged the update, apply it immediately without another prompt.
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
        self.refresh_dashboard_status()

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
        self.qol_widget.refresh_workshop_status()
        self.refresh_enhanced_status(show_warning=False)
        self.refresh_dashboard_status()
        self.load_reforged_t7_options()


def main() -> int:
    """Launch the PatchOpsIII GUI and return its exit code."""
    cli_args = parse_cli_arguments()
    cleared_logs = manage_log_retention_on_launch()
    if cleared_logs:
        write_log("Cleared logs after three application launches.", "Info")
    write_log(f"Process PID {os.getpid()} elevated={is_admin()}", "Info")
    app = QApplication(sys.argv)
    base_font = QFont("Inter", 10)
    app.setFont(base_font)
    if QT_MODERN_AVAILABLE:
        try:
            qt_styles.dark(app)
        except Exception:
            pass
    apply_modern_theme(app)
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

    target_window = window
    if QT_MODERN_AVAILABLE:
        try:
            modern_kwargs = {}
            if "use_native_titlebar" in inspect.signature(qt_windows.ModernWindow.__init__).parameters:
                modern_kwargs["use_native_titlebar"] = False
            modern_window = qt_windows.ModernWindow(window, **modern_kwargs)
            if not global_icon.isNull():
                modern_window.setWindowIcon(global_icon)
            target_window = modern_window
        except Exception as exc:
            write_log(f"QtModernRedux wrapper failed: {exc}", "Warning")

    target_window.show()

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
