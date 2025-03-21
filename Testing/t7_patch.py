#!/usr/bin/env python
import os, sys, ctypes, subprocess, zipfile, tarfile, shutil, requests
from PySide6.QtWidgets import (
    QMessageBox, QWidget, QGroupBox, QGridLayout, QLineEdit, QPushButton,
    QLabel, QHBoxLayout, QVBoxLayout, QRadioButton, QButtonGroup, QCheckBox, QSizePolicy
)
from PySide6.QtCore import Signal, Qt, QEvent
from utils import write_log

# Add module-level flag
defender_warning_logged = False

# === Core T7 Patch functions (unchanged) ===

def is_admin():
    try:
        return ctypes.windll.shell32.isUserAnAdmin()
    except Exception:
        return False

def run_as_admin(extra_args=""):
    script = sys.argv[0]
    params = f'"{script}" {extra_args}'
    try:
        if sys.platform.startswith("win"):
            ctypes.windll.shell32.ShellExecuteW(None, "runas", sys.executable, params, None, 1)
    except Exception as e:
        write_log(f"Failed to elevate privileges: {e}", "Error", None)
    sys.exit(0)  # Exit immediately after requesting elevation

def update_t7patch_conf(game_dir, new_name=None, new_password=None, friends_only=None, log_widget=None):
    conf_path = os.path.join(game_dir, "t7patch.conf")
    if os.path.exists(conf_path):
        try:
            with open(conf_path, "r") as f:
                lines = f.readlines()
            
            name_found = False
            password_found = False
            friends_found = False
            new_lines = []
            
            # Keep lines that don't match what we're updating
            for line in lines:
                if new_name is not None and line.startswith("playername="):
                    new_lines.append(f"playername={new_name}\n")
                    name_found = True
                elif new_password is not None and line.startswith("networkpassword="):
                    new_lines.append(f"networkpassword={new_password}\n")
                    password_found = True
                elif friends_only is not None and line.startswith("isfriendsonly="):
                    new_lines.append(f"isfriendsonly={1 if friends_only else 0}\n")
                    friends_found = True
                else:
                    new_lines.append(line)

            # Add new entries if they weren't found
            if new_name is not None and not name_found:
                new_lines.insert(0, f"playername={new_name}\n")
            if new_password is not None and not password_found:
                new_lines.insert(0, f"networkpassword={new_password}\n")
            if friends_only is not None and not friends_found:
                new_lines.insert(0, f"isfriendsonly={1 if friends_only else 0}\n")

            # Write changes back to file
            with open(conf_path, "w") as f:
                f.writelines(new_lines)

            if new_name is not None:
                write_log(f"Updated 'playername' in t7patch.conf to '{new_name}'.", "Success", log_widget)
            if new_password is not None:
                if new_password:
                    write_log("Updated network password in t7patch.conf.", "Success", log_widget)
                else:
                    write_log("Cleared network password in t7patch.conf.", "Success", log_widget)
            if friends_only is not None:
                write_log(f"Updated 'isfriendsonly' in t7patch.conf to {'On' if friends_only else 'Off'}.", "Success", log_widget)

        except PermissionError:
            QMessageBox.critical(None, "Permission Error",
                               f"Cannot modify {conf_path}.\nRun as administrator.")
        except Exception as e:
            write_log(f"Error updating config: {e}", "Error", log_widget)
    else:
        write_log(f"t7patch.conf not found in {game_dir}.", "Warning", log_widget)

def backup_lpc_files(game_dir, log_widget):
    """Create backups of original LPC files by renaming them with .bak extension"""
    lpc_dir = os.path.join(game_dir, "LPC")
    if not os.path.exists(lpc_dir):
        os.makedirs(lpc_dir)
        write_log("Created LPC directory.", "Info", log_widget)
        return True
    
    try:
        backed_up = 0
        for file in os.listdir(lpc_dir):
            if file.endswith(".ff") and not file.endswith(".bak"):
                src = os.path.join(lpc_dir, file)
                dst = src + ".bak"
                if not os.path.exists(dst):
                    try:
                        # Just rename the file to .bak
                        os.rename(src, dst)
                        backed_up += 1
                    except Exception as e:
                        write_log(f"Failed to backup {file}: {e}", "Error", log_widget)
                        return False
        
        if backed_up > 0:
            write_log(f"Created backups for {backed_up} LPC files", "Success", log_widget)
        return True
    except Exception as e:
        write_log(f"Error during LPC backup process: {e}", "Error", log_widget)
        return False

def restore_lpc_backups(game_dir, log_widget):
    """Restore original LPC files from .bak backups"""
    lpc_dir = os.path.join(game_dir, "LPC")
    if not os.path.exists(lpc_dir):
        return
    
    try:
        restored = 0
        for file in os.listdir(lpc_dir):
            if file.endswith(".bak"):
                src = os.path.join(lpc_dir, file)
                dst = src[:-4]  # Remove .bak extension
                if os.path.exists(dst):
                    os.remove(dst)
                os.rename(src, dst)
                restored += 1
        
        if restored > 0:
            write_log(f"Restored {restored} LPC backup files", "Success", log_widget)
    except Exception as e:
        write_log(f"Error restoring LPC backups: {e}", "Error", log_widget)

def download_file(url, filename, log_widget):
    write_log(f"Downloading from {url}", "Info", log_widget)
    with requests.get(url, stream=True) as r:
        r.raise_for_status()
        with open(filename, "wb") as f:
            for chunk in r.iter_content(chunk_size=8192):
                if chunk:
                    f.write(chunk)
    write_log(f"Downloaded file saved as: {filename}", "Success", log_widget)

def install_lpc_files(game_dir, mod_files_dir, log_widget):
    """Download and install LPC files"""
    zip_url = "https://github.com/shiversoftdev/t7patch/releases/download/Current/LPC.1.zip"
    zip_dest = os.path.join(mod_files_dir, "LPC.zip")
    temp_dir = os.path.join(mod_files_dir, "LPC_temp")
    
    # Clean up temporary extraction directory if it exists
    if os.path.exists(temp_dir):
        shutil.rmtree(temp_dir)
    
    try:
        # Download LPC.zip using the new download_file function
        download_file(zip_url, zip_dest, log_widget)
        
        # Create backups of existing LPC files
        if not backup_lpc_files(game_dir, log_widget):
            return False
        
        # Extract files
        with zipfile.ZipFile(zip_dest, "r") as zf:
            os.makedirs(temp_dir, exist_ok=True)
            zf.extractall(temp_dir)
            
            # Copy files from extracted LPC folder to game's LPC folder
            src_lpc = os.path.join(temp_dir, "LPC")
            dst_lpc = os.path.join(game_dir, "LPC")
            
            # Create LPC directory if it doesn't exist
            os.makedirs(dst_lpc, exist_ok=True)
            
            # Copy new files, preserving existing .bak files
            for file in os.listdir(src_lpc):
                src_file = os.path.join(src_lpc, file)
                dst_file = os.path.join(dst_lpc, file)
                if os.path.isfile(src_file):
                    shutil.copy2(src_file, dst_file)
            
        write_log("Installed LPC files successfully.", "Success", log_widget)
        return True
        
    except Exception as e:
        write_log(f"Error installing LPC files: {e}", "Error", log_widget)
        return False
        
    finally:
        # Clean up temporary files
        try:
            if os.path.exists(zip_dest):
                os.remove(zip_dest)
            if os.path.exists(temp_dir):
                shutil.rmtree(temp_dir)
        except Exception:
            pass

def check_defender_available():
    """Check if Windows Defender is available and active"""
    try:
        result = subprocess.run(
            ["powershell", "-Command", "Get-MpComputerStatus"],
            capture_output=True,
            text=True,
            check=True
        )
        return "AntivirusEnabled" in result.stdout and "True" in result.stdout
    except subprocess.CalledProcessError:
        return False

def add_defender_exclusion(path, log_widget):
    """Safely add a Windows Defender exclusion with proper error handling"""
    global defender_warning_logged
    if not check_defender_available():
        if not defender_warning_logged:
            write_log("Windows Defender is not active or accessible.", "Warning", log_widget)
            write_log("If you are using another anti-virus, then you will have to exclude the game folder manually.", "Warning", log_widget)
            defender_warning_logged = True
        return False
    
    try:
        subprocess.run(
            ["powershell", "-Command", f"Add-MpPreference -ExclusionPath '{path}'"],
            check=True,
            capture_output=True,
            text=True
        )
        write_log(f"Added Windows Defender exclusion to {path}.", "Success", log_widget)
        return True
    except subprocess.CalledProcessError as e:
        write_log(f"Could not add Windows Defender exclusion for {path}. This is normal if using a different antivirus.", "Warning", log_widget)
        return False

def install_t7_patch(game_dir, txt_gamertag, btn_update_gamertag, log_widget, mod_files_dir):
    # Check if we're running with admin rights and this is the elevated process
    if "--install-t7" in sys.argv:
        try:
            if sys.platform.startswith("win"):
                # Add Windows Defender exclusions on Windows with proper error handling
                add_defender_exclusion(mod_files_dir, log_widget)
                add_defender_exclusion(game_dir, log_widget)
            else:
                write_log("Linux detected. Skipping antivirus exclusion. Please add an exclusion in your antivirus settings if needed.", "Warning", log_widget)
            
            # Download and install T7 Patch
            write_log("Downloading T7 Patch...", "Info", log_widget)
            zip_url = "https://github.com/shiversoftdev/t7patch/releases/download/Current/Linux.Steamdeck.and.Manual.Windows.Install.zip"
            zip_dest = os.path.join(mod_files_dir, "T7Patch.zip")
            source_dir = os.path.join(mod_files_dir, "linux")
            
            # Clean up existing files
            if os.path.exists(zip_dest):
                os.remove(zip_dest)
            if os.path.exists(source_dir):
                shutil.rmtree(source_dir)
            
            # Use the new download_file function for streaming download
            download_file(zip_url, zip_dest, log_widget)
            
            with zipfile.ZipFile(zip_dest, "r") as zf:
                zf.extractall(mod_files_dir)
            
            # Copy files to game directory
            if os.path.exists(source_dir):
                for root, dirs, files in os.walk(source_dir):
                    rel_path = os.path.relpath(root, source_dir)
                    dest = os.path.join(game_dir, rel_path)
                    os.makedirs(dest, exist_ok=True)
                    for file in files:
                        if file.lower() == "t7patch.conf" and os.path.exists(os.path.join(dest, file)):
                            continue
                        shutil.copy2(os.path.join(root, file), dest)
                
                # Clean up
                try:
                    os.remove(zip_dest)
                    shutil.rmtree(source_dir)
                except Exception:
                    pass
                
                # Install LPC files
                write_log("Installing LPC files...", "Info", log_widget)
                if not install_lpc_files(game_dir, mod_files_dir, log_widget):
                    write_log("Failed to install LPC files.", "Error", log_widget)
                    return
                
                write_log("T7 Patch installed successfully.", "Success", log_widget)
                txt_gamertag.setEnabled(True)
                btn_update_gamertag.setEnabled(True)

                # Add Linux-specific launch options
                import platform
                if platform.system() == "Linux":
                    from main import apply_launch_options  # import the function from main.py
                    default_launch = 'WINEDLLOVERRIDES="dsound=n,b" %command%'
                    apply_launch_options(default_launch, log_widget)
            else:
                write_log("Error: Could not find extracted files.", "Error", log_widget)
                
        except Exception as e:
            write_log(f"Error during installation: {e}", "Error", log_widget)
        return

    # Normal entry point
    prompt_text = ("Do you want to install the T7 Patch now?\n\n"
                   "On Windows, this will attempt to add Windows Defender exclusions for the required folders.\n"
                   "On Linux, the antivirus exclusion step will be skipped. Please ensure your antivirus has an exclusion for the mod files if needed.")
    reply = QMessageBox.question(None, "Install T7 Patch", prompt_text,
                                 QMessageBox.Yes | QMessageBox.No)
    if reply == QMessageBox.Yes:
        if sys.platform.startswith("win") and not is_admin():
            QMessageBox.information(None, "Elevation Required",
                                    "This action requires administrator rights. The application will now restart with elevated privileges.")
            run_as_admin("--install-t7")
            return
        else:
            if sys.platform.startswith("win"):
                add_defender_exclusion(mod_files_dir, log_widget)
                add_defender_exclusion(game_dir, log_widget)
            else:
                write_log("Linux detected. Skipping antivirus exclusion. Please add an exclusion in your antivirus settings if needed.", "Warning", log_widget)
            
            try:
                # Download and install using same code as elevated process
                write_log("Downloading T7 Patch...", "Info", log_widget)
                zip_url = "https://github.com/shiversoftdev/t7patch/releases/download/Current/Linux.Steamdeck.and.Manual.Windows.Install.zip"
                zip_dest = os.path.join(mod_files_dir, "T7Patch.zip")
                source_dir = os.path.join(mod_files_dir, "linux")
                
                # Clean up existing files
                if os.path.exists(zip_dest):
                    os.remove(zip_dest)
                if os.path.exists(source_dir):
                    shutil.rmtree(source_dir)
                
                download_file(zip_url, zip_dest, log_widget)
                
                with zipfile.ZipFile(zip_dest, "r") as zf:
                    zf.extractall(mod_files_dir)
                
                # Copy files to game directory
                if os.path.exists(source_dir):
                    for root, dirs, files in os.walk(source_dir):
                        rel_path = os.path.relpath(root, source_dir)
                        dest = os.path.join(game_dir, rel_path)
                        os.makedirs(dest, exist_ok=True)
                        for file in files:
                            if file.lower() == "t7patch.conf" and os.path.exists(os.path.join(dest, file)):
                                continue
                            shutil.copy2(os.path.join(root, file), dest)
                    
                    # Clean up
                    try:
                        os.remove(zip_dest)
                        shutil.rmtree(source_dir)
                    except Exception:
                        pass
                    
                    # Install LPC files
                    write_log("Installing LPC files...", "Info", log_widget)
                    if not install_lpc_files(game_dir, mod_files_dir, log_widget):
                        write_log("Failed to install LPC files.", "Error", log_widget)
                        return
                    
                    write_log("T7 Patch installed successfully.", "Success", log_widget)
                    txt_gamertag.setEnabled(True)
                    btn_update_gamertag.setEnabled(True)

                    # Add Linux-specific launch options
                    import platform
                    if platform.system() == "Linux":
                        from main import apply_launch_options  # import the function from main.py
                        default_launch = 'WINEDLLOVERRIDES="dsound=n,b" %command%'
                        apply_launch_options(default_launch, log_widget)
                else:
                    write_log("Error: Could not find extracted files.", "Error", log_widget)
                    
            except Exception as e:
                write_log(f"Error during installation: {e}", "Error", log_widget)
    else:
        write_log("Installation cancelled by user.", "Info", log_widget)

def check_t7_patch_status(game_dir):
    conf_path = os.path.join(game_dir, "t7patch.conf")
    result = {"gamertag": "", "password": "", "friends_only": False}
    if os.path.exists(conf_path):
        with open(conf_path, "r") as f:
            for line in f:
                if line.startswith("playername="):
                    result["gamertag"] = line.strip().split("=", 1)[1]
                    # Extract actual name without color code
                    if result["gamertag"].startswith("^") and len(result["gamertag"]) > 2:
                        result["color_code"] = result["gamertag"][:2]
                        result["plain_name"] = result["gamertag"][2:]
                    else:
                        result["color_code"] = ""
                        result["plain_name"] = result["gamertag"]
                elif line.startswith("networkpassword="):
                    result["password"] = line.strip().split("=", 1)[1]
                elif line.startswith("isfriendsonly="):
                    result["friends_only"] = line.strip().split("=", 1)[1] == "1"
    return result

def uninstall_t7_patch(game_dir, mod_files_dir, log_widget):
    warning = ("WARNING: It is HIGHLY recommended to keep the T7 Patch installed.\n\n"
              "You should only uninstall it if the game is crashing on startup.\n\n"
              "Do you want to proceed with uninstallation?")
    reply = QMessageBox.warning(None, "Uninstall T7 Patch", warning,
                              QMessageBox.Yes | QMessageBox.No, QMessageBox.No)
    if reply == QMessageBox.Yes:
        try:
            # Files to remove from game directory
            game_files = ['t7patch.dll', 't7patch.conf', 'discord_game_sdk.dll', 
                         'dsound.dll', 't7patchloader.dll', 'zbr2.dll']
            
            # Remove files from game directory
            removed_game = False
            for file in game_files:
                file_path = os.path.join(game_dir, file)
                if os.path.exists(file_path):
                    os.remove(file_path)
                    removed_game = True
            if removed_game:
                write_log("Uninstalled t7patch", "Success", log_widget)

            # Remove files from mod files directory
            linux_dir = os.path.join(mod_files_dir, "linux")
            removed_mod = False
            if os.path.exists(linux_dir):
                for file in game_files:
                    file_path = os.path.join(linux_dir, file)
                    if os.path.exists(file_path):
                        os.remove(file_path)
                        removed_mod = True
                
                # Remove linux directory if empty
                if not os.listdir(linux_dir):
                    os.rmdir(linux_dir)

                # Remove T7Patch.zip if it exists
                zip_path = os.path.join(mod_files_dir, "T7Patch.zip")
                if os.path.exists(zip_path):
                    os.remove(zip_path)
                    removed_mod = True

            if removed_mod:
                write_log("Removed t7patch files from BO3 Mod files directory", "Success", log_widget)

            write_log("T7 Patch has been completely uninstalled.", "Success", log_widget)
            
            restore_lpc_backups(game_dir, log_widget)
            
        except Exception as e:
            write_log(f"Error uninstalling T7 Patch: {e}", "Error", log_widget)

# === GUI Class for T7 Patch Management ===

class T7PatchWidget(QWidget):
    patch_uninstalled = Signal()  # Add signal for uninstall notification
    
    def __init__(self, mod_files_dir, parent=None):
        super().__init__(parent)
        self.mod_files_dir = mod_files_dir
        self.game_dir = None
        self.log_widget = None
        self.group = QGroupBox("T7 Patch Management")
        self.group.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)
        self.selected_gamertag_prefix = ""
        self.selected_gamertag_color = ""
        self.init_ui()
        self.update_theme()

    @property
    def groupbox(self):
        return self.group

    def init_ui(self):
        DARK_CONTROL_COLOR = "#2D2D30"
        LIGHT_FORE_COLOR = "#FFFFFF"
        self.group = QGroupBox("T7 Patch Management", self)
        layout = QGridLayout(self.group)
        layout.setContentsMargins(5, 5, 5, 5)  # Add smaller margins
        layout.setSpacing(5)  # Add consistent spacing

        # Row 0: Install button, Uninstall button, and Friends Only checkbox
        install_widget = QWidget()
        install_layout = QHBoxLayout(install_widget)
        install_layout.setContentsMargins(0, 0, 0, 0)
        install_layout.setSpacing(10)  # Add some spacing between elements
        
        # Remove min-width and let the buttons fill the space naturally
        self.patch_btn = QPushButton("Install/Update T7 Patch")
        self.patch_btn.setStyleSheet(f"background-color: {DARK_CONTROL_COLOR}; color: {LIGHT_FORE_COLOR};")
        self.patch_btn.clicked.connect(self.install_t7_patch)
        install_layout.addWidget(self.patch_btn, 1)  # Add stretch factor of 1

        self.uninstall_btn = QPushButton("Uninstall T7 Patch")
        self.uninstall_btn.setStyleSheet(f"background-color: {DARK_CONTROL_COLOR}; color: {LIGHT_FORE_COLOR};")
        self.uninstall_btn.clicked.connect(self.uninstall_t7_patch)
        install_layout.addWidget(self.uninstall_btn, 1)  # Add stretch factor of 1

        install_layout.addStretch(0.5)  # Reduced stretch factor before checkbox

        self.friends_only_cb = QCheckBox("Friends Only Mode")
        self.friends_only_cb.setEnabled(False)
        self.friends_only_cb.setStyleSheet("color: " + LIGHT_FORE_COLOR + ";")
        self.friends_only_cb.stateChanged.connect(self.friends_only_changed)
        install_layout.addWidget(self.friends_only_cb)
        
        layout.addWidget(install_widget, 0, 0, 1, 5)

        # Row 1: Gamertag
        self.current_gt_label = QLabel("Current Gamertag: None")
        self.current_gt_label.setStyleSheet("color: " + LIGHT_FORE_COLOR + ";")
        layout.addWidget(self.current_gt_label, 1, 0, 1, 2)

        layout.addWidget(QLabel("Enter Gamertag:"), 1, 2)
        self.gamertag_edit = QLineEdit()
        self.gamertag_edit.setEnabled(False)
        self.gamertag_edit.setStyleSheet(f"background-color: {DARK_CONTROL_COLOR}; color: {LIGHT_FORE_COLOR};")
        layout.addWidget(self.gamertag_edit, 1, 3)

        self.update_gamertag_btn = QPushButton("Update Gamertag")
        self.update_gamertag_btn.setEnabled(False)
        self.update_gamertag_btn.setStyleSheet(f"background-color: {DARK_CONTROL_COLOR}; color: {LIGHT_FORE_COLOR};")
        self.update_gamertag_btn.clicked.connect(self.update_gamertag)
        layout.addWidget(self.update_gamertag_btn, 1, 4)

        # Row 2: Network Password
        self.current_pw_label = QLabel("Current Network Password: None")
        self.current_pw_label.setStyleSheet("color: " + LIGHT_FORE_COLOR + ";")
        layout.addWidget(self.current_pw_label, 2, 0, 1, 2)

        layout.addWidget(QLabel("Network Password:"), 2, 2)
        self.password_edit = QLineEdit()
        self.password_edit.setEnabled(False)
        self.password_edit.setStyleSheet(f"background-color: {DARK_CONTROL_COLOR}; color: {LIGHT_FORE_COLOR};")
        layout.addWidget(self.password_edit, 2, 3)

        self.update_password_btn = QPushButton("Update Password")
        self.update_password_btn.setEnabled(False)
        self.update_password_btn.setStyleSheet(f"background-color: {DARK_CONTROL_COLOR}; color: {LIGHT_FORE_COLOR};")
        self.update_password_btn.clicked.connect(self.update_password)
        layout.addWidget(self.update_password_btn, 2, 4)

        # Row 3: Color Options (now moved up one row since we removed the separate Friends Only row)
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
        first_button = None
        for color in GAMERTAG_COLORS:
            rb = QRadioButton(color["Label"])
            rb.setStyleSheet("color: " + LIGHT_FORE_COLOR + ";")
            rb.setProperty("code", color["Code"])
            rb.toggled.connect(self.on_color_selected)
            self.color_buttons.addButton(rb)
            color_layout.addWidget(rb)
            if color["Code"] == "":  # Default color
                first_button = rb
        
        # Set default color button as checked
        if first_button:
            first_button.setChecked(True)
            self.selected_gamertag_prefix = ""
            self.selected_gamertag_color = "Default"

        layout.addWidget(self.color_group_box, 3, 0, 1, 5)

        main_layout = QVBoxLayout(self)
        main_layout.addWidget(self.group)
        
        # Set size policy to allow widget to expand horizontally but maintain vertical size
        self.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Preferred)

    def set_game_directory(self, game_dir, skip_status_check=False):
        self.game_dir = game_dir
        if not skip_status_check and self.game_dir and os.path.exists(self.game_dir):
            status = check_t7_patch_status(self.game_dir)
            if status["gamertag"]:
                # Update display format to show plain name and color
                if "plain_name" in status and "color_code" in status:
                    color_name = "Default"
                    for btn in self.color_buttons.buttons():
                        if btn.property("code") == status["color_code"]:
                            color_name = btn.text()
                            break
                    self.current_gt_label.setText(f"Current Gamertag: {status['plain_name']} (Color: {color_name})")
                else:
                    self.current_gt_label.setText(f"Current Gamertag: {status['gamertag']}")
                
                # Update password display and input field
                self.current_pw_label.setText(f"Current Network Password: {status['password']}")
                self.password_edit.setText(status['password'])  # Set current password in input field
                
                # Enable all controls
                self.gamertag_edit.setEnabled(True)
                self.update_gamertag_btn.setEnabled(True)
                self.password_edit.setEnabled(True)
                self.update_password_btn.setEnabled(True)
                self.friends_only_cb.setEnabled(True)
                self.friends_only_cb.setChecked(status["friends_only"])
                
                # Set the color button based on detected color code
                color_found = False
                if "color_code" in status:
                    for btn in self.color_buttons.buttons():
                        if btn.property("code") == status["color_code"]:
                            btn.setChecked(True)
                            color_found = True
                            break
                
                # If no color code found or matched, select default
                if not color_found:
                    for btn in self.color_buttons.buttons():
                        if btn.property("code") == "":
                            btn.setChecked(True)
                            break
                
                # Set the plain name in the edit field
                if "plain_name" in status:
                    self.gamertag_edit.setText(status["plain_name"])
            else:
                self.current_gt_label.setText("Current Gamertag: None")
                self.current_pw_label.setText("Current Network Password: None")
                self.gamertag_edit.setEnabled(False)
                self.update_gamertag_btn.setEnabled(False)
                self.password_edit.setEnabled(False)
                self.update_password_btn.setEnabled(False)
                self.friends_only_cb.setEnabled(False)
                self.friends_only_cb.setChecked(False)

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
            
        # Get the selected color prefix from the checked button
        selected_prefix = ""
        selected_color_name = "Default"
        for btn in self.color_buttons.buttons():
            if btn.isChecked():
                selected_prefix = btn.property("code")
                selected_color_name = btn.text()
                break
                
        new_name = selected_prefix + plain_name
        update_t7patch_conf(self.game_dir, new_name=new_name, log_widget=self.log_widget)
        self.current_gt_label.setText(f"Current Gamertag: {new_name}")
        write_log(f"Updated gamertag to: {plain_name} (Color: {selected_color_name})", "Success", self.log_widget)

    def update_password(self):
        if not self.game_dir:
            return
        new_password = self.password_edit.text().strip()
        update_t7patch_conf(self.game_dir, new_password=new_password, log_widget=self.log_widget)
        status = check_t7_patch_status(self.game_dir)  # Refresh status after update
        self.current_pw_label.setText(f"Current Network Password: {status['password']}")

    def friends_only_changed(self):
        if not self.game_dir:
            return
        is_enabled = self.friends_only_cb.isChecked()
        update_t7patch_conf(self.game_dir, friends_only=is_enabled, log_widget=self.log_widget)

    def install_t7_patch(self):
        if not self.game_dir or not os.path.exists(self.game_dir):
            write_log("Game directory does not exist.", "Error", self.log_widget)
            return
        install_t7_patch(self.game_dir, self.gamertag_edit, self.update_gamertag_btn, self.log_widget, self.mod_files_dir)
        self.set_game_directory(self.game_dir)

    def uninstall_t7_patch(self):
        if not self.game_dir or not os.path.exists(self.game_dir):
            write_log("Game directory does not exist.", "Error", self.log_widget)
            return
        uninstall_t7_patch(self.game_dir, self.mod_files_dir, self.log_widget)
        # Reset UI state without checking status
        self.current_gt_label.setText("Current Gamertag: None")
        self.current_pw_label.setText("Current Network Password: None")
        self.gamertag_edit.setEnabled(False)
        self.update_gamertag_btn.setEnabled(False)
        self.password_edit.setEnabled(False)
        self.update_password_btn.setEnabled(False)
        self.friends_only_cb.setEnabled(False)
        self.friends_only_cb.setChecked(False)
        for btn in self.color_buttons.buttons():
            btn.setChecked(False)
        self.patch_uninstalled.emit()  # Notify parent of uninstall

    def update_theme(self):
        is_dark = self.palette().window().color().lightness() < 128
        if is_dark:
            control_color = "#2D2D30"
            fore_color = "#FFFFFF"
        else:
            control_color = "#F0F0F0"
            fore_color = "#000000"

        # Update all controls with the new theme
        for btn in [self.patch_btn, self.uninstall_btn, self.update_gamertag_btn, 
                   self.update_password_btn]:
            btn.setStyleSheet(f"background-color: {control_color}; color: {fore_color};")

        for edit in [self.gamertag_edit, self.password_edit]:
            edit.setStyleSheet(f"background-color: {control_color}; color: {fore_color};")

        for label in [self.current_gt_label, self.current_pw_label]:
            label.setStyleSheet(f"color: {fore_color};")

        self.friends_only_cb.setStyleSheet(f"color: {fore_color};")
        self.color_group_box.setStyleSheet(f"color: {fore_color};")

        # Update all radio buttons in the color group
        for btn in self.color_buttons.buttons():
            btn.setStyleSheet(f"color: {fore_color};")

    def changeEvent(self, event):
        if event.type() == QEvent.Type.PaletteChange:
            self.update_theme()
        super().changeEvent(event)
