#!/usr/bin/env python
import os, sys, ctypes, subprocess, zipfile, tarfile, shutil, requests
from PySide6.QtWidgets import (
    QMessageBox, QWidget, QGroupBox, QGridLayout, QLineEdit, QPushButton,
    QLabel, QHBoxLayout, QVBoxLayout, QRadioButton, QButtonGroup
)
from utils import write_log

# === Core T7 Patch functions (unchanged) ===

def is_admin():
    try:
        return ctypes.windll.shell32.IsUserAnAdmin()
    except Exception:
        return False

def run_as_admin(extra_args=""):
    script = sys.argv[0]
    params = f'"{script}" {extra_args}'
    ctypes.windll.shell32.ShellExecuteW(None, "runas", sys.executable, params, None, 1)
    sys.exit()

def update_t7patch_conf(game_dir, new_name, log_widget):
    conf_path = os.path.join(game_dir, "t7patch.conf")
    if os.path.exists(conf_path):
        try:
            with open(conf_path, "r") as f:
                lines = f.readlines()
            found = False
            for idx, line in enumerate(lines):
                if line.startswith("playername="):
                    current = line.strip().split("=", 1)[1]
                    if current and current.lower() != "unknown soldier":
                        write_log("Existing gamertag detected; not overwriting.", "Info", log_widget)
                        return
                    lines[idx] = f"playername={new_name}\n"
                    found = True
                    break
            if not found:
                lines.insert(0, f"playername={new_name}\n")
            with open(conf_path, "w") as f:
                f.writelines(lines)
            write_log(f"Updated 'playername' in t7patch.conf to '{new_name}'.", "Success", log_widget)
        except PermissionError:
            QMessageBox.critical(None, "Permission Error",
                                 f"Cannot modify {conf_path}.\nRun as administrator.")
        except Exception as e:
            write_log(f"Error updating config: {e}", "Error", log_widget)
    else:
        write_log(f"t7patch.conf not found in {game_dir}.", "Warning", log_widget)

def install_t7_patch(game_dir, txt_gamertag, btn_update_gamertag, log_widget, mod_files_dir):
    prompt_text = ("Do you want to install the T7 Patch now?\n\n"
                   "Warning: This will temporarily disable Windows Defender within the BO3 Mod Files folder and require admin rights.")
    reply = QMessageBox.question(None, "Install T7 Patch", prompt_text,
                                 QMessageBox.Yes | QMessageBox.No)
    if reply == QMessageBox.Yes:
        if sys.platform.startswith("win") and not is_admin():
            QMessageBox.information(None, "Elevation Required",
                                    "This action requires administrator rights. The application will now restart with elevated privileges.")
            run_as_admin("--install-t7")
            return
        try:
            subprocess.run(["powershell", "-Command", f"Add-MpPreference -ExclusionPath '{mod_files_dir}'"], check=True)
            write_log(f"Added Windows Defender exclusion to {mod_files_dir}.", "Success", log_widget)
        except Exception as e:
            write_log(f"Failed to add exclusion for mod files: {e}", "Error", log_widget)
        try:
            subprocess.run(["powershell", "-Command", f"Add-MpPreference -ExclusionPath '{game_dir}'"], check=True)
            write_log(f"Added Windows Defender exclusion to {game_dir}.", "Success", log_widget)
        except Exception as e:
            write_log(f"Failed to add exclusion for game directory: {e}", "Error", log_widget)
        write_log("Proceeding with T7 Patch installation.", "Info", log_widget)
        zip_url = "https://github.com/shiversoftdev/t7patch/releases/download/Current/Linux.Steamdeck.and.Manual.Windows.Install.zip"
        zip_dest = os.path.join(mod_files_dir, "T7Patch.zip")
        try:
            r = requests.get(zip_url, stream=True)
            with open(zip_dest, "wb") as f:
                f.write(r.content)
            write_log("Downloaded T7 Patch successfully.", "Success", log_widget)
        except Exception:
            write_log("Failed to download T7 Patch. Check your internet connection.", "Error", log_widget)
            return
        try:
            with zipfile.ZipFile(zip_dest, "r") as zf:
                zf.extractall(mod_files_dir)
            write_log("Unzipped T7 Patch successfully.", "Success", log_widget)
        except Exception:
            write_log("Failed to unzip T7 Patch. The zip file may be corrupted.", "Error", log_widget)
            return
        source_dir = os.path.join(mod_files_dir, "linux")
        if not os.path.exists(source_dir):
            write_log("'linux' folder not found after extracting T7 Patch.", "Error", log_widget)
            return
        copy_errors = False
        try:
            for root, dirs, files in os.walk(source_dir):
                rel_path = os.path.relpath(root, source_dir)
                dest = os.path.join(game_dir, rel_path)
                if not os.path.exists(dest):
                    os.makedirs(dest)
                for file in files:
                    if file.lower() == "t7patch.conf" and os.path.exists(os.path.join(dest, file)):
                        continue
                    try:
                        shutil.copy(os.path.join(root, file), dest)
                    except Exception as e:
                        write_log(f"Error copying {file}: {e}", "Warning", log_widget)
                        copy_errors = True
        except Exception as e:
            write_log(f"Error during file copy: {e}", "Error", log_widget)
            return

        conf_path = os.path.join(game_dir, "t7patch.conf")
        if os.path.exists(conf_path):
            if copy_errors:
                write_log("T7 Patch installed with some copy errors; t7patch.conf exists.", "Warning", log_widget)
            else:
                write_log("T7 Patch installed successfully.", "Success", log_widget)
            txt_gamertag.setEnabled(True)
            btn_update_gamertag.setEnabled(True)
            txt_gamertag.clear()
            write_log("You can now enter your Gamertag and click 'Update Gamertag'.", "Info", log_widget)
        else:
            write_log("Error installing T7 Patch. Critical files are missing.", "Error", log_widget)
    else:
        write_log("T7 Patch installation canceled by user.", "Info", log_widget)

def check_t7_patch_status(game_dir):
    conf_path = os.path.join(game_dir, "t7patch.conf")
    if os.path.exists(conf_path):
        with open(conf_path, "r") as f:
            for line in f:
                if line.startswith("playername="):
                    return line.strip().split("=", 1)[1]
    return ""

# === GUI Class for T7 Patch Management ===

class T7PatchWidget(QWidget):
    def __init__(self, mod_files_dir, parent=None):
        super().__init__(parent)
        self.mod_files_dir = mod_files_dir
        self.game_dir = None
        self.log_widget = None
        self.selected_gamertag_prefix = ""
        self.selected_gamertag_color = ""
        self.init_ui()

    def init_ui(self):
        DARK_CONTROL_COLOR = "#2D2D30"
        LIGHT_FORE_COLOR = "#FFFFFF"
        self.group = QGroupBox("T7 Patch Management", self)
        layout = QGridLayout(self.group)

        self.patch_btn = QPushButton("Install/Update T7 Patch")
        self.patch_btn.setStyleSheet(f"background-color: {DARK_CONTROL_COLOR}; color: {LIGHT_FORE_COLOR};")
        self.patch_btn.clicked.connect(self.install_t7_patch)
        layout.addWidget(self.patch_btn, 0, 0, 1, 2)

        layout.addWidget(QLabel("Enter Gamertag:"), 0, 2)
        self.gamertag_edit = QLineEdit()
        self.gamertag_edit.setEnabled(False)
        self.gamertag_edit.setStyleSheet(f"background-color: {DARK_CONTROL_COLOR}; color: {LIGHT_FORE_COLOR};")
        layout.addWidget(self.gamertag_edit, 0, 3)

        self.update_gamertag_btn = QPushButton("Update Gamertag")
        self.update_gamertag_btn.setEnabled(False)
        self.update_gamertag_btn.setStyleSheet(f"background-color: {DARK_CONTROL_COLOR}; color: {LIGHT_FORE_COLOR};")
        self.update_gamertag_btn.clicked.connect(self.update_gamertag)
        layout.addWidget(self.update_gamertag_btn, 0, 4)

        self.current_gt_label = QLabel("")
        self.current_gt_label.setStyleSheet("color: " + LIGHT_FORE_COLOR + ";")
        layout.addWidget(self.current_gt_label, 1, 0, 1, 5)

        # Gamertag Color selection:
        self.color_group_box = QGroupBox("Gamertag Color")
        color_layout = QHBoxLayout(self.color_group_box)
        self.color_buttons = QButtonGroup(self)
        GAMERTAG_COLORS = [
            {"Code": "", "Label": "Default"},
            {"Code": "^1", "Label": "Red"},
            {"Code": "^2", "Label": "Green"},
            {"Code": "^3", "Label": "Yellow"},
            {"Code": "^4", "Label": "Blue"},
            {"Code": "^5", "Label": "Cyan"},
            {"Code": "^6", "Label": "Pink"},
            {"Code": "^7", "Label": "White"},
            {"Code": "^8", "Label": "Middle Blue"},
            {"Code": "^9", "Label": "Cinnabar Red"},
            {"Code": "^0", "Label": "Black"}
        ]
        for color in GAMERTAG_COLORS:
            rb = QRadioButton(color["Label"])
            rb.setStyleSheet("color: " + LIGHT_FORE_COLOR + ";")
            rb.setProperty("code", color["Code"])
            rb.toggled.connect(self.on_color_selected)
            self.color_buttons.addButton(rb)
            color_layout.addWidget(rb)
        layout.addWidget(self.color_group_box, 2, 0, 1, 5)

        main_layout = QVBoxLayout(self)
        main_layout.addWidget(self.group)

    def set_game_directory(self, game_dir):
        self.game_dir = game_dir
        if self.game_dir and os.path.exists(self.game_dir):
            current_gt = check_t7_patch_status(self.game_dir)
            if current_gt:
                self.current_gt_label.setText(f"Current Gamertag: {current_gt}")
                self.gamertag_edit.setEnabled(True)
                self.update_gamertag_btn.setEnabled(True)
                for btn in self.color_buttons.buttons():
                    code = btn.property("code")
                    if code and current_gt.startswith(code):
                        btn.setChecked(True)
                        break
            else:
                self.current_gt_label.setText("T7 Patch not installed")
                self.gamertag_edit.setEnabled(False)
                self.update_gamertag_btn.setEnabled(False)

    def set_log_widget(self, log_widget):
        self.log_widget = log_widget

    def on_color_selected(self):
        sender = self.sender()
        if sender.isChecked():
            self.selected_gamertag_prefix = sender.property("code")
            self.selected_gamertag_color = sender.text()

    def update_gamertag(self):
        if not self.game_dir:
            return
        plain_name = self.gamertag_edit.text().strip()
        if not plain_name:
            write_log("Gamertag cannot be empty.", "Warning", self.log_widget)
            return
        if len(plain_name) > 20:
            write_log("Gamertag cannot exceed 20 characters.", "Warning", self.log_widget)
            return
        new_name = self.selected_gamertag_prefix + plain_name
        update_t7patch_conf(self.game_dir, new_name, self.log_widget)
        write_log(f"Updated gamertag: {plain_name} (Color: {self.selected_gamertag_color})", "Success", self.log_widget)
        self.current_gt_label.setText(f"Current Gamertag: {new_name} (Color: {self.selected_gamertag_color})")

    def install_t7_patch(self):
        if not self.game_dir or not os.path.exists(self.game_dir):
            write_log("Game directory does not exist.", "Error", self.log_widget)
            return
        install_t7_patch(self.game_dir, self.gamertag_edit, self.update_gamertag_btn, self.log_widget, self.mod_files_dir)
        self.set_game_directory(self.game_dir)
