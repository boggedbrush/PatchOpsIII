#!/usr/bin/env python
import sys
import os
import re
import shutil
import subprocess
import time
import vdf
import platform
from PySide6.QtWidgets import (
    QApplication, QMainWindow, QWidget, QVBoxLayout, QHBoxLayout, QLineEdit,
    QPushButton, QLabel, QFileDialog, QTextEdit, QTabWidget, QSizePolicy,
    QGroupBox, QRadioButton, QButtonGroup, QCheckBox, QGridLayout
)
from PySide6.QtGui import QIcon, QDesktopServices
from PySide6.QtCore import Signal, Qt, QUrl

from t7_patch import T7PatchWidget
from dxvk_manager import DXVKWidget
from config import GraphicsSettingsWidget, AdvancedSettingsWidget
from utils import write_log

def resource_path(relative_path):
    base_path = getattr(sys, '_MEIPASS', os.path.abspath("."))
    candidate = os.path.join(base_path, relative_path)
    if os.path.exists(candidate):
        return candidate
    return os.path.join(os.path.abspath("."), relative_path)

def get_application_path():
    """Get the actual application path whether running as script or frozen exe"""
    if getattr(sys, 'frozen', False):
        return os.path.dirname(sys.executable)
    return os.path.dirname(os.path.abspath(__file__))

def get_game_directory():
    # First check if BlackOps3.exe is in the same directory as the application
    app_dir = get_application_path()
    local_exe = os.path.join(app_dir, "BlackOps3.exe")
    if os.path.exists(local_exe):
        return app_dir
    
    # Fall back to Steam default path
    return r"C:\Program Files (x86)\Steam\steamapps\common\Call of Duty Black Ops III"

DEFAULT_GAME_DIR = get_game_directory()

MOD_FILES_DIR = os.path.join(get_application_path(), "BO3 Mod Files")
if not os.path.exists(MOD_FILES_DIR):
    os.makedirs(MOD_FILES_DIR)

def get_steam_paths():
    system = platform.system()
    if system == "Windows":
        return {
            'userdata': r"C:\Program Files (x86)\Steam\userdata",
            'steam_exe': r"C:\Program Files (x86)\Steam\steam.exe"
        }
    elif system == "Linux":
        home = os.path.expanduser("~")
        return {
            'userdata': os.path.join(home, ".steam/steam/userdata"),
            'steam_exe': "steam"  # On Linux, we can just use the command 'steam'
        }
    else:
        return None

# Replace the existing steam path variables with dynamic ones
steam_paths = get_steam_paths()
if steam_paths:
    steam_userdata_path = steam_paths['userdata']
    steam_exe_path = steam_paths['steam_exe']
else:
    write_log("Unsupported operating system", "Error", None)

app_id = "311210"  # Black Ops III AppID

def find_steam_user_id():
    if not os.path.exists(steam_userdata_path):
        write_log("Steam userdata path not found!", "Warning", None)
        return None
    user_ids = [f for f in os.listdir(steam_userdata_path) if f.isdigit()]
    if not user_ids:
        write_log("No Steam user ID found!", "Warning", None)
        return None
    return user_ids[0]

def backup_config_file(config_path, log_widget):
    backup_dir = os.path.join(get_application_path(), "backups")
    os.makedirs(backup_dir, exist_ok=True)
    backup_file_path = os.path.join(backup_dir, "localconfig_backup.vdf")
    
    try:
        shutil.copy2(config_path, backup_file_path)  # copy2 preserves metadata
        write_log(f"Config backup created in program directory", "Success", log_widget)
        return True
    except Exception as e:
        write_log(f"Failed to create backup: {e}", "Error", log_widget)
        return False

def restore_config_file(config_path, log_widget):
    backup_file_path = os.path.join(get_application_path(), "backups", "localconfig_backup.vdf")
    if os.path.exists(backup_file_path):
        try:
            shutil.copy2(backup_file_path, config_path)
            write_log("Config restored from backup", "Success", log_widget)
            return True
        except Exception as e:
            write_log(f"Failed to restore backup: {e}", "Error", log_widget)
            return False
    else:
        write_log("No backup file found", "Warning", log_widget)
        return False

def close_steam(log_widget):
    system = platform.system()
    try:
        if system == "Windows":
            subprocess.run(["taskkill", "/F", "/IM", "steam.exe"], check=True)
        elif system == "Linux":
            subprocess.run(["pkill", "steam"], check=True)
        time.sleep(5)
    except subprocess.CalledProcessError as e:
        write_log(f"Failed to close Steam: {e}", "Error", log_widget)

def open_steam(log_widget):
    system = platform.system()
    try:
        if system == "Windows":
            write_log("Opening Steam...", "Info", log_widget)
            subprocess.Popen([steam_exe_path],
                             stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL)
        else:
            # Check if Steam is already running
            was_running = (subprocess.call(["pgrep", "-x", "steam"],
                                           stdout=subprocess.DEVNULL) == 0)
            if was_running:
                write_log("Closing Steam...", "Info", log_widget)
                subprocess.call(["pkill", "-x", "steam"],
                                stdout=subprocess.DEVNULL,
                                stderr=subprocess.DEVNULL)
                time.sleep(2)  # Allow time for Steam to shut down

            write_log("Setting launch options...", "Info", log_widget)
            # (Place here any code that sets your launch options.)
            
            write_log("Opening Steam...", "Info", log_widget)
            subprocess.Popen(["xdg-open", "steam://"],
                             stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL)
            # Fallback: if xdg-open fails and 'steam' exists, launch it directly.
            if subprocess.call(["which", "steam"],
                               stdout=subprocess.DEVNULL) == 0:
                subprocess.Popen(["steam"],
                                 stdout=subprocess.DEVNULL,
                                 stderr=subprocess.DEVNULL)
        
        # Poll for a stable Steam process
        max_wait = 15      # maximum total wait time (seconds)
        poll_interval = 0.5  # poll every 0.5 seconds
        stable_duration = 2  # require Steam to be present for 2 consecutive seconds
        elapsed = 0
        stable_time = 0

        while elapsed < max_wait:
            if subprocess.call(["pgrep", "-x", "steam"],
                               stdout=subprocess.DEVNULL) == 0:
                stable_time += poll_interval
                if stable_time >= stable_duration:
                    break
            else:
                if stable_time > 0:
                    # Try launching again if it vanished after being detected.
                    subprocess.Popen(["xdg-open", "steam://"],
                                     stdout=subprocess.DEVNULL,
                                     stderr=subprocess.DEVNULL)
                stable_time = 0
            time.sleep(poll_interval)
            elapsed += poll_interval

        # Final check: ensure Steam is running stably.
        if stable_time >= stable_duration and subprocess.call(["pgrep", "-x", "steam"],
                                                            stdout=subprocess.DEVNULL) == 0:
            write_log("Steam launched successfully", "Success", log_widget)
        else:
            write_log("Steam did not launch successfully", "Error", log_widget)
    except Exception as e:
        write_log(f"Failed to open Steam: {e}", "Error", log_widget)
        write_log("Please start Steam manually", "Info", log_widget)
        
def set_launch_options(user_id, app_id, launch_options, log_widget):
    config_path = os.path.join(steam_userdata_path, user_id, "config", "localconfig.vdf")
    if not os.path.exists(config_path):
        write_log("localconfig.vdf not found!", "Error", log_widget)
        return False
    if not backup_config_file(config_path, log_widget):
        write_log("Aborting due to backup failure", "Error", log_widget)
        return False
    try:
        with open(config_path, "r", encoding="utf-8") as file:
            data = vdf.load(file)
        # Navigate to the apps section
        steam_config = data.get("UserLocalConfigStore", {})
        software = steam_config.get("Software", {})
        valve = software.get("Valve", {})
        steam = valve.get("Steam", {})
        apps = steam.get("apps", {})

        if app_id not in apps:
            apps[app_id] = {}
        current_options = apps[app_id].get("LaunchOptions", "")
        
        # Parse existing options
        wine_override = 'WINEDLLOVERRIDES="dsound=n,b"'
        command_marker = "%command%"
        fs_game_pattern = r'\+set fs_game \S+'
        
        # Keep WINE override if it exists and we're not explicitly setting it
        if wine_override in current_options and wine_override not in launch_options:
            launch_options = f"{wine_override} {command_marker} {launch_options}"
        
        # If we're adding WINE override, remove any existing one
        elif wine_override in launch_options and wine_override in current_options:
            current_options = current_options.replace(wine_override, "").strip()
        
        # Remove any existing fs_game parameters
        current_options = re.sub(fs_game_pattern, '', current_options).strip()
        
        # Keep existing options that aren't fs_game or WINE related
        current_parts = [opt for opt in current_options.split() 
                        if opt != wine_override 
                        and opt != command_marker]
        
        # Combine options, ensuring no duplicates
        if current_parts:
            final_options = " ".join(filter(None, [
                wine_override if wine_override in launch_options else "",
                command_marker if command_marker in launch_options else "",
                " ".join(current_parts),
                launch_options.replace(wine_override, "").replace(command_marker, "").strip()
            ])).strip()
        else:
            final_options = launch_options

        apps[app_id]["LaunchOptions"] = final_options
        
        write_log(f"Setting launch options to: {final_options}", "Info", log_widget)

        with open(config_path, "w", encoding="utf-8") as file:
            vdf.dump(data, file, pretty=True)
        write_log("Launch options updated successfully", "Success", log_widget)
        return True
    except Exception as e:
        write_log(f"Error updating launch options: {e}", "Error", log_widget)
        restore_config_file(config_path, log_widget)
        return False

def apply_launch_options(launch_option, log_widget):
    user_id = find_steam_user_id()
    if not user_id:
        raise Exception("Steam user ID not found!")
    close_steam(log_widget)
    set_launch_options(user_id, app_id, launch_option, log_widget)
    open_steam(log_widget)

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

        try:
            write_log("Applying launch options...", "Info", self.log_widget)
            apply_launch_options(option, self.log_widget)
            write_log("Launch options set successfully!", "Success", self.log_widget)
        except Exception as e:
            write_log(f"Error applying launch options: {e}", "Error", self.log_widget)

class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("PatchOpsIII")

        icon_path = resource_path("PatchOpsIII.ico")
        icon = QIcon(icon_path)
        if icon.isNull():
            write_log(f"Icon not found or invalid: {icon_path}", "Warning")
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
            
        # Add Linux T7Patch launch options check
        game_dir = self.game_dir_edit.text().strip()
        t7_conf = os.path.join(game_dir, "t7patch.conf")
        if platform.system() == "Linux" and os.path.exists(t7_conf):
            launch_options = 'WINEDLLOVERRIDES="dsound=n,b" %command%'
            apply_launch_options(launch_options, self.log_text)

        # Connect tab changed signal
        self.tabs.currentChanged.connect(self.on_tab_changed)

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
        gd_layout = QHBoxLayout(game_dir_widget)
        self.game_dir_edit = QLineEdit(DEFAULT_GAME_DIR)
        browse_btn = QPushButton("Browse")
        browse_btn.clicked.connect(self.browse_game_dir)
        
        label_text = (
            "Current Directory:" if DEFAULT_GAME_DIR == get_application_path()
            else "Game Directory:"
        )
        gd_layout.addWidget(QLabel(label_text))
        gd_layout.addWidget(self.game_dir_edit)
        gd_layout.addWidget(browse_btn)
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
        self.t7_patch_widget.set_game_directory(game_dir)
        self.t7_patch_widget.set_log_widget(self.log_text)

        self.dxvk_widget.set_game_directory(game_dir)
        self.dxvk_widget.set_log_widget(self.log_text)

        self.graphics_widget.set_game_directory(game_dir)
        self.graphics_widget.set_log_widget(self.log_text)

        self.advanced_widget.set_game_directory(game_dir)
        self.advanced_widget.set_log_widget(self.log_text)

        self.qol_widget.set_game_directory(game_dir)
        self.qol_widget.set_log_widget(self.log_text)

    def on_tab_changed(self, index):
        tab_widget = self.tabs.widget(index)
        if tab_widget:
            # Refresh advanced settings if on that tab
            if index == 2:
                self.advanced_widget.refresh_settings()
            tab_widget.layout().update()

    def browse_game_dir(self):
        directory = QFileDialog.getExistingDirectory(
            self, "Select Black Ops III Game Directory", self.game_dir_edit.text()
        )
        if directory:
            self.game_dir_edit.setText(directory)
            if not getattr(self, '_t7_just_uninstalled', False):
                self.t7_patch_widget.set_game_directory(directory)
            self._t7_just_uninstalled = False
            self.dxvk_widget.set_game_directory(directory)
            self.graphics_widget.set_game_directory(directory)
            self.advanced_widget.set_game_directory(directory)
            self.qol_widget.set_game_directory(directory)

    def on_t7_patch_uninstalled(self):
        self._t7_just_uninstalled = True

if __name__ == "__main__":
    app = QApplication(sys.argv)
    global_icon_path = resource_path("PatchOpsIII.ico")
    global_icon = QIcon(global_icon_path)
    if global_icon.isNull():
        write_log(f"Global icon not found or invalid: {global_icon_path}", "Warning")
    app.setWindowIcon(global_icon)
    window = MainWindow()
    window.show()
    sys.exit(app.exec())
