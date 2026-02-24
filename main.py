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
    QMessageBox, QDialog, QStyle, QMenu, QListWidget, QListWidgetItem,
    QStackedWidget, QAbstractItemView
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

from t7_patch import T7PatchWidget, is_admin
from dxvk_manager import DXVKWidget
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
    write_exe_variant,
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

class DumpSelectionDialog(QDialog):
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("BO3 Enhanced - Dump Required")
        self.setFixedWidth(600)
        self.result_path = None

        layout = QVBoxLayout(self)
        layout.setSpacing(15)
        layout.setContentsMargins(20, 20, 20, 20)

        # Header
        header = QLabel("<h3>Step 1: Obtain Game Dump</h3>")
        header.setTextFormat(Qt.RichText)
        layout.addWidget(header)

        info = QLabel(
            "To avoid legal risks, PatchOpsIII cannot download game files automatically.\n"
            "You must provide a UWP dump file manually."
        )
        info.setWordWrap(True)
        layout.addWidget(info)

        # Guide Link
        guide_layout = QHBoxLayout()
        guide_icon = QLabel()
        guide_icon.setPixmap(self.style().standardIcon(QStyle.SP_MessageBoxInformation).pixmap(32, 32))
        guide_layout.addWidget(guide_icon)
        
        link_label = QLabel('<a href="https://youtu.be/rBZZTcSJ9_s?si=41p0r_Enten3h5AQ">Click here to watch the guide on how to get the dump</a>')
        link_label.setOpenExternalLinks(True)
        guide_layout.addWidget(link_label)
        guide_layout.addStretch()
        layout.addLayout(guide_layout)

        # Divider
        line = QLabel()
        line.setFrameShape(QLabel.HLine)
        line.setFrameShadow(QLabel.Sunken)
        layout.addWidget(line)

        # Selection
        step2 = QLabel("<h3>Step 2: Install</h3>")
        step2.setTextFormat(Qt.RichText)
        layout.addWidget(step2)

        # Browse Bar
        browse_layout = QHBoxLayout()
        browse_layout.setSpacing(10)

        self.path_edit = QLineEdit()
        self.path_edit.setPlaceholderText("Select DUMP.zip or BlackOps3.exe from dump folder...")
        browse_layout.addWidget(self.path_edit)

        # Smart Browse Button
        browse_btn = QPushButton("Browse")
        browse_btn.clicked.connect(self.smart_browse)
        browse_layout.addWidget(browse_btn)
        
        layout.addLayout(browse_layout)

        # Install Button
        action_layout = QHBoxLayout()
        action_layout.addStretch()
        
        self.install_btn = QPushButton("Install Enhanced")
        self.install_btn.clicked.connect(self.validate_and_install)
        self.install_btn.setEnabled(False) # Disabled until path set
        self.install_btn.setMinimumWidth(120)
        
        action_layout.addWidget(self.install_btn)
        layout.addLayout(action_layout)

        # Enable install button when text changes
        self.path_edit.textChanged.connect(self.on_text_changed)

    def on_text_changed(self, text):
        self.install_btn.setEnabled(bool(text.strip()))

    def smart_browse(self):
        # We allow selecting ZIPs or specific marker files to identify a folder
        filter_str = "Dump Sources (DUMP.zip BlackOps3.exe *.zip);;All Files (*)"
        path, _ = QFileDialog.getOpenFileName(self, "Select Dump Source", "", filter_str)
        
        if not path:
            return

        # Smart inference logic
        if path.lower().endswith(".zip"):
            final_path = path
        elif os.path.basename(path).lower() == "blackops3.exe":
            # User selected the exe inside the dump folder; infer the folder
            final_path = os.path.dirname(path)
        else:
            # Fallback: just use what they picked, validation will catch if wrong
            # If it's a file but not a zip, maybe they picked another file in the folder?
            # Let's try to infer folder if it's unlikely to be the archive
            if os.path.isfile(path):
                 final_path = os.path.dirname(path)
            else:
                 final_path = path

        self.path_edit.setText(final_path)

    def validate_and_install(self):
        path = self.path_edit.text().strip()
        if not path:
            return
            
        if not os.path.exists(path):
            QMessageBox.critical(self, "Invalid Path", "The selected path does not exist.")
            return

        # Basic validation before closing
        # validate_dump_source expects a zip path or a folder path
        if not validate_dump_source(path):
             QMessageBox.critical(self, "Invalid Dump", "The selected source is missing required files (appxmanifest.xml, BlackOps3.exe, MicrosoftGame.config).")
             return

        self.result_path = path
        self.accept()


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

            with urllib.request.urlopen(REFORGED_DOWNLOAD_URL, timeout=120) as response, open(temp_path, "wb") as handle:
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
        self._enhanced_warning_session_shown = False
        self._enhanced_active = False
        self._enhanced_worker: Optional[EnhancedDownloadWorker] = None
        self._reforged_worker: Optional[ReforgedInstallWorker] = None
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

        # Tabs
        self.tabs = SidebarTabWidget()
        self.tabs.currentChanged.connect(self.on_tab_changed)
        self.tabs.setDocumentMode(True)

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
        mods_grid.setRowStretch(0, 3)
        mods_grid.setRowStretch(1, 1)

        self.tabs.addTab(mods_tab, load_ui_icon("mods"), "Mods")

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
        log_layout.addWidget(self.log_text)
        main_layout.addWidget(log_group)

        self.setCentralWidget(central)
        self.adjustSize()

        # Initialize with the default directory
        game_dir = self.game_dir_edit.text().strip()
        self._apply_game_directory(game_dir)
        self.t7_patch_widget.set_log_widget(self.log_text)
        self.dxvk_widget.set_log_widget(self.log_text)
        self.graphics_widget.set_log_widget(self.log_text)
        self.advanced_widget.set_log_widget(self.log_text)
        self.advanced_widget.set_mod_files_dir(MOD_FILES_DIR)
        self.qol_widget.set_log_widget(self.log_text)
        self.refresh_enhanced_status(show_warning=True)

    def _build_enhanced_group(self):
        group = QGroupBox("BO3 Enhanced (Preview)")
        layout = QGridLayout(group)
        layout.setContentsMargins(8, 8, 8, 8)
        layout.setHorizontalSpacing(10)
        layout.setVerticalSpacing(8)

        info = QLabel(
            "BO3 Enhanced improves the game with community fixes, including higher frame rates and faster load times.\n"
            "Click Install to automatically download and apply the latest version."
        )
        info.setWordWrap(True)
        layout.addWidget(info, 0, 0, 1, 2)

        # Primary install/uninstall actions
        install_row = QHBoxLayout()
        install_row.setSpacing(10)
        self.enhanced_install_btn = QPushButton("Install / Update Enhanced")
        self.enhanced_install_btn.clicked.connect(self.on_enhanced_install_clicked)
        self.enhanced_uninstall_btn = QPushButton("Uninstall Enhanced")
        self.enhanced_uninstall_btn.clicked.connect(self.on_enhanced_uninstall_clicked)
        
        for btn in (self.enhanced_install_btn, self.enhanced_uninstall_btn):
            btn.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
            
        install_row.addWidget(self.enhanced_install_btn)
        install_row.addWidget(self.enhanced_uninstall_btn)
        
        layout.addLayout(install_row, 1, 0, 1, 2)

        # Status line
        self.enhanced_status_label = QLabel("Status: Enhanced not installed")
        layout.addWidget(self.enhanced_status_label, 2, 0, 1, 2)
        
        # Add a stretch to push everything up
        layout.setRowStretch(3, 1)
        
        return group

    def _build_reforged_group(self):
        group = QGroupBox("BO3 Reforged")
        layout = QVBoxLayout(group)
        layout.setContentsMargins(8, 8, 8, 8)
        layout.setSpacing(8)

        info = QLabel(
            "One-click installer: downloads and installs the Reforged executable into your game directory."
        )
        info.setWordWrap(True)
        layout.addWidget(info)

        self.reforged_install_btn = QPushButton("Install Reforged")
        self.reforged_install_btn.clicked.connect(self.install_reforged)
        layout.addWidget(self.reforged_install_btn)

        self.reforged_status_label = QLabel("Status: Reforged Not installed")
        layout.addWidget(self.reforged_status_label)

        workshop_note_row = QHBoxLayout()
        workshop_note_row.setContentsMargins(0, 0, 0, 0)
        workshop_note_row.setSpacing(8)
        workshop_note = QLabel("Installing Reforged also installs the BO3 Reforged Workshop mod.")
        workshop_note.setWordWrap(False)
        workshop_note_row.addWidget(workshop_note)
        workshop_help_btn = QPushButton("?")
        workshop_help_btn.setFixedSize(20, 20)
        workshop_help_btn.clicked.connect(
            lambda: QDesktopServices.openUrl(QUrl(REFORGED_WORKSHOP_STEAM_URL))
        )
        workshop_note_row.addWidget(workshop_help_btn)
        workshop_note_row.addStretch(1)
        layout.addLayout(workshop_note_row)

        compat_note = QLabel(
            "T7Patch compatibility: Reforged can be used with or without T7Patch. "
            "If both players use Reforged, matching passwords across T7Patch/T7.json are compatible."
        )
        compat_note.setWordWrap(True)
        layout.addWidget(compat_note)

        t7_group = QGroupBox("Reforged T7 Management (players/T7.json)")
        t7_layout = QGridLayout(t7_group)
        t7_layout.setContentsMargins(8, 8, 8, 8)
        t7_layout.setHorizontalSpacing(10)
        t7_layout.setVerticalSpacing(8)

        self.reforged_current_pw_label = QLabel("Current Network Password: None")
        t7_layout.addWidget(self.reforged_current_pw_label, 0, 0, 1, 2)

        password_label = QLabel("Network Password:")
        self.reforged_password_edit = QLineEdit()
        self.reforged_password_edit.setPlaceholderText("Enter Network Password")
        t7_layout.addWidget(password_label, 1, 0)
        t7_layout.addWidget(self.reforged_password_edit, 1, 1)

        self.reforged_force_ranked_cb = QCheckBox("Force Ranked Mode")
        self.reforged_steam_achievements_cb = QCheckBox("Steam Achievements")
        t7_layout.addWidget(self.reforged_force_ranked_cb, 2, 0, 1, 2)
        t7_layout.addWidget(self.reforged_steam_achievements_cb, 3, 0, 1, 2)

        t7_btn_row = QHBoxLayout()
        self.reforged_t7_load_btn = QPushButton("Refresh Reforged T7")
        self.reforged_t7_apply_btn = QPushButton("Update Reforged T7")
        self.reforged_t7_load_btn.clicked.connect(self.load_reforged_t7_options)
        self.reforged_t7_apply_btn.clicked.connect(self.apply_reforged_t7_options)
        t7_btn_row.addWidget(self.reforged_t7_load_btn)
        t7_btn_row.addWidget(self.reforged_t7_apply_btn)
        t7_layout.addLayout(t7_btn_row, 4, 0, 1, 2)

        layout.addWidget(t7_group)

        return group

    def install_reforged(self):
        game_dir = self.game_dir_edit.text().strip()
        if not os.path.isdir(game_dir):
            write_log("Set a valid game directory before installing Reforged.", "Error", self.log_text)
            return
        if self._reforged_worker and self._reforged_worker.isRunning():
            return

        self.reforged_install_btn.setEnabled(False)
        self.reforged_status_label.setText("Status: Installing Reforged...")

        self._reforged_worker = ReforgedInstallWorker(game_dir)
        self._reforged_worker.progress.connect(self._on_reforged_progress)
        self._reforged_worker.installed.connect(self._on_reforged_installed)
        self._reforged_worker.failed.connect(self._on_reforged_failed)
        self._reforged_worker.start()

    def _on_reforged_progress(self, message: str):
        self.reforged_status_label.setText(f"Status: {message}")
        write_log(message, "Info", self.log_text)

    def _on_reforged_installed(self, target_path: str):
        self.reforged_install_btn.setEnabled(True)
        self.reforged_status_label.setText("Status: Reforged installed")
        write_log(f"Reforged installed to {target_path}", "Success", self.log_text)
        write_exe_variant(self.game_dir_edit.text().strip(), "reforged")
        self.t7_patch_widget.refresh_t7_mode_indicator()
        QDesktopServices.openUrl(QUrl(REFORGED_WORKSHOP_STEAM_URL))
        write_log("Opened Reforged Workshop page in Steam.", "Info", self.log_text)

    def _on_reforged_failed(self, reason: str):
        self.reforged_install_btn.setEnabled(True)
        self.reforged_status_label.setText("Status: Reforged install failed")
        write_log(f"Reforged install failed: {reason}", "Error", self.log_text)

    def _t7_json_path(self):
        game_dir = self.game_dir_edit.text().strip()
        if not game_dir:
            return None
        return os.path.join(game_dir, "players", "T7.json")

    def load_reforged_t7_options(self):
        path = self._t7_json_path()
        if not path:
            return

        if not os.path.exists(path):
            self.reforged_password_edit.setText("")
            self.reforged_force_ranked_cb.setChecked(False)
            self.reforged_steam_achievements_cb.setChecked(False)
            self.reforged_current_pw_label.setText("Current Network Password: None")
            write_log("T7.json not found. Default values loaded.", "Info", self.log_text)
            return

        try:
            with open(path, "r", encoding="utf-8") as handle:
                data = json.load(handle)
            network_pass = str(data.get("network_pass", ""))
            self.reforged_password_edit.setText(network_pass)
            self.reforged_force_ranked_cb.setChecked(bool(data.get("force_ranked", False)))
            self.reforged_steam_achievements_cb.setChecked(bool(data.get("steam_achievements", False)))
            self.reforged_current_pw_label.setText(
                f"Current Network Password: {network_pass if network_pass else 'None'}"
            )
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
            self.reforged_current_pw_label.setText(
                f"Current Network Password: {password if password else 'None'}"
            )
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

    def refresh_enhanced_status(self, *, show_warning: bool):
        summary = status_summary(self.game_dir_edit.text().strip(), STORAGE_PATH)
        active = bool(summary.get("installed"))
        self._enhanced_active = active
        self.enhanced_status_label.setText(
            "Status: Enhanced Mode Active" if active else "Status: Enhanced not installed"
        )
        self._toggle_launch_options_enabled(not active)
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

    def on_enhanced_install_clicked(self):
        # Manual dump selection flow
        dialog = DumpSelectionDialog(self)
        if dialog.exec() != QDialog.Accepted or not dialog.result_path:
            return
            
        self._pending_dump_source = dialog.result_path

        if self._enhanced_worker and self._enhanced_worker.isRunning():
            return
            
        self.enhanced_install_btn.setEnabled(False)
        self.enhanced_uninstall_btn.setEnabled(False)
        self.enhanced_status_label.setText("Status: Downloading Enhanced files...")
        
        self._enhanced_worker = EnhancedDownloadWorker(MOD_FILES_DIR, STORAGE_PATH)
        self._enhanced_worker.progress.connect(self._on_enhanced_progress)
        self._enhanced_worker.download_complete.connect(self._on_enhanced_download_finished)
        self._enhanced_worker.failed.connect(self._on_enhanced_download_failed)
        self._enhanced_last_failed = False
        self._enhanced_worker.start()

    def _on_enhanced_progress(self, message: str):
        self.enhanced_status_label.setText(f"Status: {message}")
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
            
        self.enhanced_status_label.setText("Status: Installing files...")
        write_log("Download complete. Installing BO3 Enhanced files...", "Success", self.log_text)
        
        # Proceed immediately to install using the pending dump source
        game_dir = self.game_dir_edit.text().strip()
        dump_source = getattr(self, "_pending_dump_source", None)
        
        if not dump_source or not os.path.exists(dump_source):
             self.enhanced_status_label.setText("Status: Installation Failed (Dump missing)")
             write_log("Pending dump source missing.", "Error", self.log_text)
             self._reset_enhanced_buttons()
             return

        if install_enhanced_files(game_dir, MOD_FILES_DIR, STORAGE_PATH, dump_source, log_widget=self.log_text):
            self.enhanced_status_label.setText("Status: Enhanced Installed Successfully")
            write_exe_variant(game_dir, "enhanced")
            self.refresh_enhanced_status(show_warning=True)
            self.t7_patch_widget.refresh_t7_mode_indicator()
        else:
            self.enhanced_status_label.setText("Status: Installation Failed")
            
        self._reset_enhanced_buttons()

    def _on_enhanced_download_failed(self, reason: str):
        self._enhanced_last_failed = True
        self.enhanced_status_label.setText(f"Status: Failed - {reason}")
        write_log(f"BO3 Enhanced download failed: {reason}", "Error", self.log_text)
        self._reset_enhanced_buttons()

    def _reset_enhanced_buttons(self):
        self.enhanced_install_btn.setEnabled(True)
        self.enhanced_uninstall_btn.setEnabled(True)





    def on_enhanced_uninstall_clicked(self):
        self.enhanced_install_btn.setEnabled(False)
        self.enhanced_uninstall_btn.setEnabled(False)
        self.enhanced_status_label.setText("Status: Uninstalling...")
        
        game_dir = self.game_dir_edit.text().strip()
        # Use QTimer to allow UI update before blocking operation
        QTimer.singleShot(100, lambda: self._perform_uninstall(game_dir))

    def _perform_uninstall(self, game_dir):
        if uninstall_enhanced_files(game_dir, MOD_FILES_DIR, STORAGE_PATH, log_widget=self.log_text):
            self.enhanced_status_label.setText("Status: Enhanced Uninstalled")
            write_exe_variant(game_dir, "default")
            self.refresh_enhanced_status(show_warning=False)
            self.t7_patch_widget.refresh_t7_mode_indicator()
        else:
            self.enhanced_status_label.setText("Status: Uninstall execution failed")
            
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
        self.refresh_enhanced_status(show_warning=False)
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
