#!/usr/bin/env python
import os, sys, ctypes, subprocess, zipfile, shutil, requests, json, hashlib, re
from PySide6.QtWidgets import (
    QMessageBox, QWidget, QGroupBox, QGridLayout, QLineEdit, QPushButton,
    QLabel, QHBoxLayout, QVBoxLayout, QRadioButton, QButtonGroup, QCheckBox, QSizePolicy, QFrame
)
from PySide6.QtCore import Signal, QThread, Qt, QSize
from PySide6.QtGui import QIcon
from bo3_enhanced import detect_enhanced_install
from utils import (
    write_log,
    apply_launch_options,
    patchops_backup_path,
    existing_backup_path,
    PATCHOPS_BACKUP_SUFFIX,
    LEGACY_BACKUP_SUFFIX,
    read_exe_variant,
    file_sha256,
)

DEFAULT_STEAM_EXE_SHA256 = "9ba98dba41e18ef47de6c63937340f8eae7cb251f8fbc2e78d70047b64aa15b5"
REFORGED_TRUSTED_SHA256 = {
    "66b95eb4667bd5b3b3d230e7bed1d29ccd261d48ca2699f01216c863be24ff44",
}
T7PATCH_RELEASE_TAG_API = "https://api.github.com/repos/shiversoftdev/t7patch/releases/tags/Current"

# Trusted hashes for the pinned T7Patch "Current" assets.
# If upstream starts publishing digests in release metadata, those are preferred automatically.
TRUSTED_T7PATCH_ASSET_SHA256 = {
    "Linux.Steamdeck.and.Manual.Windows.Install.zip": {
        "388491c01643b0abd51f13290d0c36dec9737fcfbb0ed5e2f5ef6804e1b73dcb",
    },
    "LPC.1.zip": {
        "c94855841a233c9dcdea2799c12693fed8554d0e59fe68257ae66ffbdf2fa58b",
    },
}

_t7patch_release_digests_cache = None

def _load_icon(name: str) -> QIcon:
    path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "icons", f"{name}.svg")
    return QIcon(path) if os.path.exists(path) else QIcon()

# Add module-level flag
defender_warning_logged = False

# === Core T7 Patch functions (unchanged) ===

def is_admin():
    try:
        return bool(ctypes.windll.shell32.IsUserAnAdmin())
    except Exception:
        return False

def run_as_admin(extra_args=None):
    if not sys.platform.startswith("win"):
        return False

    if extra_args is None:
        extra_args = []
    elif isinstance(extra_args, str):
        extra_args = [extra_args]
    else:
        extra_args = list(extra_args)

    script = os.path.abspath(sys.argv[0])
    if getattr(sys, "frozen", False):
        executable = script
        params_list = extra_args
        working_dir = os.path.dirname(executable)
    else:
        executable = sys.executable
        params_list = [script] + extra_args
        working_dir = os.path.dirname(script)

    params = subprocess.list2cmdline(params_list)

    try:
        write_log(f"Attempting elevation via UAC: exe='{executable}' params='{params}'", "Info", None)
        result = ctypes.windll.shell32.ShellExecuteW(None, "runas", executable, params, working_dir or None, 1)
        if result <= 32:
            raise PermissionError(f"ShellExecuteW failed with code {result}")
        write_log(f"UAC elevation request dispatched successfully (code {result}).", "Info", None)
    except Exception as e:
        write_log(f"Failed to elevate privileges: {e}", "Error", None)
        QMessageBox.critical(None, "Elevation Failed",
                             "Unable to acquire administrator rights via UAC. Please run PatchOpsIII as administrator and try again.")
        return False

    sys.exit(0)

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
    """Create backups of original LPC files by renaming them with .patchops.bak extension."""
    lpc_dir = os.path.join(game_dir, "LPC")
    if not os.path.exists(lpc_dir):
        os.makedirs(lpc_dir)
        write_log("Created LPC directory.", "Info", log_widget)
        return True
    
    try:
        backed_up = 0
        for file in os.listdir(lpc_dir):
            if file.endswith(".ff"):
                src = os.path.join(lpc_dir, file)
                dst = patchops_backup_path(src)
                if not existing_backup_path(src):
                    try:
                        # Keep the first backup as the rollback target.
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
    """Restore original LPC files from PatchOps and legacy backups."""
    lpc_dir = os.path.join(game_dir, "LPC")
    if not os.path.exists(lpc_dir):
        return
    
    try:
        restored = 0
        selected_backups = {}
        for file in os.listdir(lpc_dir):
            if file.endswith(PATCHOPS_BACKUP_SUFFIX):
                base = file[:-len(PATCHOPS_BACKUP_SUFFIX)]
                selected_backups[base] = os.path.join(lpc_dir, file)
            elif file.endswith(LEGACY_BACKUP_SUFFIX):
                base = file[:-len(LEGACY_BACKUP_SUFFIX)]
                selected_backups.setdefault(base, os.path.join(lpc_dir, file))

        for base_name, src in selected_backups.items():
            dst = os.path.join(lpc_dir, base_name)
            if os.path.exists(dst):
                os.remove(dst)
            os.rename(src, dst)
            restored += 1
        
        if restored > 0:
            write_log(f"Restored {restored} LPC backup files", "Success", log_widget)
    except Exception as e:
        write_log(f"Error restoring LPC backups: {e}", "Error", log_widget)


def _fetch_t7patch_release_digests():
    global _t7patch_release_digests_cache
    if _t7patch_release_digests_cache is not None:
        return _t7patch_release_digests_cache

    digests = {}
    try:
        response = requests.get(T7PATCH_RELEASE_TAG_API, timeout=30)
        response.raise_for_status()
        data = response.json()
        for asset in data.get("assets") or []:
            name = str(asset.get("name") or "").strip()
            digest_value = str(asset.get("digest") or "").strip()
            if not name or not digest_value.lower().startswith("sha256:"):
                continue
            candidate = digest_value.split(":", 1)[1].strip().lower()
            if re.fullmatch(r"[a-f0-9]{64}", candidate):
                digests[name] = candidate
    except Exception:
        # Network/API failures should not break installs if a trusted pinned hash exists.
        digests = {}

    _t7patch_release_digests_cache = digests
    return digests


def _expected_asset_sha256(asset_name, log_widget):
    api_digest = _fetch_t7patch_release_digests().get(asset_name)
    if api_digest:
        return {api_digest.lower()}

    trusted = {value.lower() for value in TRUSTED_T7PATCH_ASSET_SHA256.get(asset_name, set()) if value}
    if trusted:
        write_log(
            f"Release metadata digest unavailable for {asset_name}; using pinned trusted hash.",
            "Warning",
            log_widget,
        )
    return trusted


def download_file(url, filename, log_widget, expected_sha256=None):
    write_log(f"Downloading from {url}", "Info", log_widget)
    expected_set = {value.lower() for value in (expected_sha256 or set()) if value}
    digest = hashlib.sha256()
    with requests.get(url, stream=True, timeout=60) as r:
        r.raise_for_status()
        with open(filename, "wb") as f:
            for chunk in r.iter_content(chunk_size=8192):
                if chunk:
                    f.write(chunk)
                    digest.update(chunk)

    if expected_set:
        file_hash = digest.hexdigest().lower()
        if file_hash not in expected_set:
            try:
                os.remove(filename)
            except OSError:
                pass
            raise RuntimeError(
                f"Downloaded file failed integrity verification (SHA-256 mismatch): {os.path.basename(filename)}"
            )
    write_log(f"Downloaded file saved as: {filename}", "Success", log_widget)

def install_lpc_files(game_dir, mod_files_dir, log_widget):
    """Download and install LPC files"""
    zip_url = "https://github.com/shiversoftdev/t7patch/releases/download/Current/LPC.1.zip"
    zip_dest = os.path.join(mod_files_dir, "LPC.zip")
    temp_dir = os.path.join(mod_files_dir, "LPC_temp")
    lpc_dir = os.path.join(game_dir, "LPC")
    
    # Create game's LPC directory if it doesn't exist
    os.makedirs(lpc_dir, exist_ok=True)
    
    # Clean up temporary extraction directory if it exists
    if os.path.exists(temp_dir):
        shutil.rmtree(temp_dir)
    
    try:
        # Download LPC.zip
        expected_hashes = _expected_asset_sha256("LPC.1.zip", log_widget)
        if not expected_hashes:
            write_log(
                "No trusted SHA-256 available for LPC.1.zip; aborting download.",
                "Error",
                log_widget,
            )
            return False
        download_file(zip_url, zip_dest, log_widget, expected_sha256=expected_hashes)
        
        # Create backups of existing LPC files
        if not backup_lpc_files(game_dir, log_widget):
            return False
        
        # Extract files
        with zipfile.ZipFile(zip_dest, "r") as zf:
            os.makedirs(temp_dir, exist_ok=True)
            zf.extractall(temp_dir)
            
            # Copy files from extracted LPC folder to game's LPC folder
            src_lpc = os.path.join(temp_dir, "LPC")
            if not os.path.exists(src_lpc):
                # Try without LPC subfolder
                src_lpc = temp_dir
            
            # Copy new files while preserving existing backup files.
            for file in os.listdir(src_lpc):
                if file.endswith(".ff"):
                    src_file = os.path.join(src_lpc, file)
                    dst_file = os.path.join(lpc_dir, file)
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
        except Exception as e:
            write_log(f"Warning: Could not clean up temporary files: {e}", "Warning", log_widget)

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


def _t7_json_path(game_dir):
    return os.path.join(game_dir, "players", "T7.json")


def _find_bo3_executable(game_dir):
    for name in ("BlackOps3.exe", "BlackOpsIII.exe"):
        candidate = os.path.join(game_dir, name)
        if os.path.exists(candidate):
            return candidate
    return None


def read_reforged_t7_options(game_dir):
    result = {
        "network_pass": "",
        "force_ranked": False,
        "steam_achievements": False,
    }
    path = _t7_json_path(game_dir)
    if not os.path.exists(path):
        return result
    try:
        with open(path, "r", encoding="utf-8") as handle:
            data = json.load(handle)
        result["network_pass"] = str(data.get("network_pass", ""))
        result["force_ranked"] = bool(data.get("force_ranked", False))
        result["steam_achievements"] = bool(data.get("steam_achievements", False))
    except Exception:
        return result
    return result


def read_reforged_t7_password(game_dir):
    return read_reforged_t7_options(game_dir).get("network_pass", "")


def update_reforged_t7_options(
    game_dir,
    *,
    password=None,
    force_ranked=None,
    steam_achievements=None,
    log_widget=None,
):
    path = _t7_json_path(game_dir)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    data = {}
    if os.path.exists(path):
        try:
            with open(path, "r", encoding="utf-8") as handle:
                data = json.load(handle)
        except Exception:
            data = {}

    if password is not None:
        if password:
            data["network_pass"] = password
        else:
            data.pop("network_pass", None)
    if force_ranked is not None:
        data["force_ranked"] = bool(force_ranked)
    if steam_achievements is not None:
        data["steam_achievements"] = bool(steam_achievements)

    try:
        with open(path, "w", encoding="utf-8") as handle:
            json.dump(data, handle, indent=4)
        if password is not None:
            if password:
                write_log("Updated network password in players/T7.json.", "Success", log_widget)
            else:
                write_log("Cleared network password in players/T7.json.", "Success", log_widget)
        if force_ranked is not None or steam_achievements is not None:
            write_log("Updated Reforged options in players/T7.json.", "Success", log_widget)
    except Exception as exc:
        write_log(f"Failed to update players/T7.json: {exc}", "Error", log_widget)


def update_reforged_t7_password(game_dir, password, log_widget=None):
    update_reforged_t7_options(game_dir, password=password, log_widget=log_widget)

class _WorkerLogForwarder:
    def __init__(self, signal):
        self._signal = signal

    def handle_write_log(self, *, full_message, category, html_message, plain_message):
        self._signal.emit(plain_message, category, html_message)


class InstallT7PatchWorker(QThread):
    finished = Signal()
    error = Signal(str)
    log_message = Signal(str, str, str)  # Signal to send log messages to the GUI
    patch_installed = Signal()

    def __init__(self, game_dir, mod_files_dir):
        super().__init__()
        self.game_dir = game_dir
        self.mod_files_dir = mod_files_dir

    def run(self):
        log_forwarder = _WorkerLogForwarder(self.log_message)
        try:
            if sys.platform.startswith("win"):
                add_defender_exclusion(self.mod_files_dir, log_forwarder)
                add_defender_exclusion(self.game_dir, log_forwarder)
            else:
                write_log("Linux detected. Skipping antivirus exclusion. Please add an exclusion in your antivirus settings if needed.", "Warning", log_forwarder)

            write_log("Downloading T7 Patch...", "Info", log_forwarder)
            zip_url = "https://github.com/shiversoftdev/t7patch/releases/download/Current/Linux.Steamdeck.and.Manual.Windows.Install.zip"
            zip_dest = os.path.join(self.mod_files_dir, "T7Patch.zip")
            source_dir = os.path.join(self.mod_files_dir, "linux")

            if os.path.exists(zip_dest):
                os.remove(zip_dest)
            if os.path.exists(source_dir):
                shutil.rmtree(source_dir)

            expected_hashes = _expected_asset_sha256("Linux.Steamdeck.and.Manual.Windows.Install.zip", log_forwarder)
            if not expected_hashes:
                raise Exception("No trusted SHA-256 available for T7 Patch archive.")

            download_file(zip_url, zip_dest, log_forwarder, expected_sha256=expected_hashes)
            write_log("Downloaded T7 Patch successfully.", "Success", log_forwarder)

            with zipfile.ZipFile(zip_dest, "r") as zf:
                zf.extractall(self.mod_files_dir)
            write_log("Extracted T7 Patch successfully.", "Success", log_forwarder)

            if os.path.exists(source_dir):
                for root, dirs, files in os.walk(source_dir):
                    rel_path = os.path.relpath(root, source_dir)
                    dest = os.path.join(self.game_dir, rel_path)
                    os.makedirs(dest, exist_ok=True)
                    for file in files:
                        if file.lower() == "t7patch.conf" and os.path.exists(os.path.join(dest, file)):
                            continue
                        shutil.copy2(os.path.join(root, file), dest)

                try:
                    os.remove(zip_dest)
                    shutil.rmtree(source_dir)
                except Exception:
                    pass

                write_log("Installing LPC files...", "Info", log_forwarder)
                if not install_lpc_files(self.game_dir, self.mod_files_dir, log_forwarder):
                    raise Exception("Failed to install LPC files.")
                write_log("Installed LPC files successfully.", "Success", log_forwarder)

                self.patch_installed.emit()

                if sys.platform == "linux":
                    write_log("Applying Linux launch options...", "Info", log_forwarder)
                    apply_launch_options('WINEDLLOVERRIDES="dsound=n,b" %command%', log_forwarder, preserve_fs_game=True)
                    write_log("Linux launch options applied.", "Success", log_forwarder)
            else:
                raise Exception("Could not find extracted files.")

            write_log("T7 Patch installation complete.", "Success", log_forwarder)
            self.finished.emit()
        except Exception as e:
            write_log(f"Error during T7 Patch installation: {e}", "Error", log_forwarder)
            self.error.emit(str(e))


def uninstall_t7_patch(game_dir, mod_files_dir, log_widget):
    warning = ("WARNING: It is HIGHLY recommended to keep the T7 Patch installed.\n\n"
              "You should only uninstall it if the game is crashing on startup.\n\n"
              "Do you want to proceed with uninstallation?")
    reply = QMessageBox.warning(None, "Uninstall T7 Patch", warning,
                              QMessageBox.Yes | QMessageBox.No, QMessageBox.No)
    if reply == QMessageBox.Yes:
        try:
            # Files to remove from game directory
            game_files = ['t7patch.dll', 't7patch.conf', 'discord_game_sdk.dll', 'dsound.dll', 't7patchloader.dll', 'zbr2.dll']

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
        self._stored_password = ""
        self.init_ui()

    @property
    def groupbox(self):
        return self.group

    _GAMERTAG_COLOR_MAP = {
        "":   None,        # Default (inherit)
        "^1": "#ff4d4d",   # Red
        "^2": "#4dff4d",   # Green
        "^3": "#ffff4d",   # Yellow
        "^4": "#6666ff",   # Blue
        "^5": "#4dffff",   # Cyan
        "^6": "#ff4dff",   # Pink
        "^7": "#ffffff",   # White
        "^8": "#6699ff",   # Mid Blue
        "^9": "#e34234",   # Cinnabar
        "^0": "#888888",   # Black (lightened for dark bg)
    }

    @classmethod
    def _gamertag_html(cls, plain_name: str, color_code: str) -> str:
        hex_color = cls._GAMERTAG_COLOR_MAP.get(color_code)
        if hex_color:
            return f'<span style="color:{hex_color};">{plain_name}</span>'
        return plain_name

    @staticmethod
    def _status_html(text: str, state: str = "neutral") -> str:
        colors = {
            "good": "#4ade80",
            "bad": "#f87171",
            "info": "#60a5fa",
            "neutral": "#9ca3af",
        }
        color = colors.get(state, colors["neutral"])
        return f'<span style="color:{color};">&#9679; {text}</span>'

    def _add_separator(self, layout, row):
        sep = QFrame()
        sep.setObjectName("DashboardDivider")
        sep.setFrameShape(QFrame.HLine)
        sep.setFixedHeight(1)
        layout.addWidget(sep, row, 0, 1, 4)

    def _set_current_password(self, password: str):
        self._stored_password = password or ""
        if self._stored_password:
            text = self._stored_password if self._pw_display_eye.isChecked() else "••••••"
        else:
            text = "None"
        self.current_pw_label.setText(text)

    def _toggle_pw_display(self, checked: bool):
        self._pw_display_eye.setIcon(_load_icon("eye" if checked else "eye-off"))
        if self._stored_password:
            self.current_pw_label.setText(self._stored_password if checked else "••••••")

    def _toggle_pw_edit(self, checked: bool):
        self._pw_edit_eye.setIcon(_load_icon("eye" if checked else "eye-off"))
        self.password_edit.setEchoMode(QLineEdit.Normal if checked else QLineEdit.Password)

    def init_ui(self):
        self.group = QGroupBox("T7 Patch Management", self)
        layout = QGridLayout(self.group)
        layout.setContentsMargins(14, 14, 14, 14)
        layout.setHorizontalSpacing(10)
        layout.setVerticalSpacing(0)

        row = 0

        # Action buttons + Friends Only
        btn_row = QWidget()
        btn_layout = QHBoxLayout(btn_row)
        btn_layout.setContentsMargins(0, 0, 0, 12)
        btn_layout.setSpacing(10)

        self.patch_btn = QPushButton("Install / Update T7 Patch")
        self.patch_btn.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
        self.patch_btn.clicked.connect(self.install_t7_patch)
        btn_layout.addWidget(self.patch_btn, 1)

        self.uninstall_btn = QPushButton("Uninstall T7 Patch")
        self.uninstall_btn.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
        self.uninstall_btn.clicked.connect(self.uninstall_t7_patch)
        btn_layout.addWidget(self.uninstall_btn, 1)

        self.friends_only_cb = QCheckBox("Friends Only Mode")
        self.friends_only_cb.setEnabled(False)
        self.friends_only_cb.stateChanged.connect(self.friends_only_changed)
        btn_layout.addWidget(self.friends_only_cb)

        layout.addWidget(btn_row, row, 0, 1, 4)
        row += 1

        # Gamertag row
        self._add_separator(layout, row); row += 1

        gt_name = QLabel("Gamertag")
        gt_name.setObjectName("DashboardStatusName")
        gt_name.setContentsMargins(0, 8, 0, 8)

        self.current_gt_label = QLabel("None")
        self.current_gt_label.setObjectName("DashboardStatusValue")
        self.current_gt_label.setAlignment(Qt.AlignRight | Qt.AlignVCenter)
        self.current_gt_label.setTextFormat(Qt.RichText)
        self.current_gt_label.setContentsMargins(0, 8, 0, 8)

        self.gamertag_edit = QLineEdit()
        self.gamertag_edit.setEnabled(False)
        self.gamertag_edit.setPlaceholderText("Enter gamertag…")

        self.update_gamertag_btn = QPushButton("Update")
        self.update_gamertag_btn.setEnabled(False)
        self.update_gamertag_btn.clicked.connect(self.update_gamertag)

        layout.addWidget(gt_name, row, 0)
        layout.addWidget(self.current_gt_label, row, 1)
        layout.addWidget(self.gamertag_edit, row, 2)
        layout.addWidget(self.update_gamertag_btn, row, 3)
        row += 1

        # Password row
        self._add_separator(layout, row); row += 1

        pw_name = QLabel("Network Password")
        pw_name.setObjectName("DashboardStatusName")
        pw_name.setContentsMargins(0, 8, 0, 8)

        # Display label + eye toggle
        pw_display_container = QWidget()
        pw_display_layout = QHBoxLayout(pw_display_container)
        pw_display_layout.setContentsMargins(0, 0, 0, 0)
        pw_display_layout.setSpacing(4)

        self.current_pw_label = QLabel("None")
        self.current_pw_label.setObjectName("DashboardStatusValue")
        self.current_pw_label.setAlignment(Qt.AlignRight | Qt.AlignVCenter)
        self.current_pw_label.setContentsMargins(0, 8, 0, 8)
        pw_display_layout.addWidget(self.current_pw_label, 1)

        self._pw_display_eye = QPushButton()
        self._pw_display_eye.setIcon(_load_icon("eye-off"))
        self._pw_display_eye.setIconSize(QSize(14, 14))
        self._pw_display_eye.setFixedSize(22, 22)
        self._pw_display_eye.setFlat(True)
        self._pw_display_eye.setCheckable(True)
        self._pw_display_eye.setToolTip("Show / hide password")
        self._pw_display_eye.toggled.connect(self._toggle_pw_display)
        pw_display_layout.addWidget(self._pw_display_eye, 0)

        # Edit field + eye toggle
        pw_edit_container = QWidget()
        pw_edit_layout = QHBoxLayout(pw_edit_container)
        pw_edit_layout.setContentsMargins(0, 0, 0, 0)
        pw_edit_layout.setSpacing(4)

        self.password_edit = QLineEdit()
        self.password_edit.setEnabled(False)
        self.password_edit.setPlaceholderText("Enter network password…")
        self.password_edit.setEchoMode(QLineEdit.Password)
        pw_edit_layout.addWidget(self.password_edit, 1)

        self._pw_edit_eye = QPushButton()
        self._pw_edit_eye.setIcon(_load_icon("eye-off"))
        self._pw_edit_eye.setIconSize(QSize(14, 14))
        self._pw_edit_eye.setFixedSize(22, 22)
        self._pw_edit_eye.setFlat(True)
        self._pw_edit_eye.setCheckable(True)
        self._pw_edit_eye.setToolTip("Show / hide password")
        self._pw_edit_eye.toggled.connect(self._toggle_pw_edit)
        pw_edit_layout.addWidget(self._pw_edit_eye, 0)

        self.update_password_btn = QPushButton("Update")
        self.update_password_btn.setEnabled(False)
        self.update_password_btn.clicked.connect(self.update_password)

        layout.addWidget(pw_name, row, 0)
        layout.addWidget(pw_display_container, row, 1)
        layout.addWidget(pw_edit_container, row, 2)
        layout.addWidget(self.update_password_btn, row, 3)
        row += 1

        # Reforged-only options row
        self._add_separator(layout, row); row += 1

        reforged_options_name = QLabel("Reforged Options")
        reforged_options_name.setObjectName("DashboardStatusName")
        reforged_options_name.setContentsMargins(0, 8, 0, 8)

        reforged_options_container = QWidget()
        reforged_options_layout = QHBoxLayout(reforged_options_container)
        reforged_options_layout.setContentsMargins(0, 0, 0, 0)
        reforged_options_layout.setSpacing(16)

        self.reforged_force_ranked_cb = QCheckBox("Force Ranked Mode")
        self.reforged_force_ranked_cb.setEnabled(False)
        self.reforged_steam_achievements_cb = QCheckBox("Steam Achievements")
        self.reforged_steam_achievements_cb.setEnabled(False)
        reforged_options_layout.addWidget(self.reforged_force_ranked_cb)
        reforged_options_layout.addWidget(self.reforged_steam_achievements_cb)
        reforged_options_layout.addStretch(1)

        self.update_reforged_options_btn = QPushButton("Update")
        self.update_reforged_options_btn.setEnabled(False)
        self.update_reforged_options_btn.clicked.connect(self.update_reforged_options)

        layout.addWidget(reforged_options_name, row, 0)
        layout.addWidget(reforged_options_container, row, 1, 1, 2)
        layout.addWidget(self.update_reforged_options_btn, row, 3)
        row += 1

        # Gamertag Color
        self._add_separator(layout, row); row += 1

        self.color_group_box = QGroupBox("Gamertag Color")
        color_layout = QHBoxLayout(self.color_group_box)
        color_layout.setContentsMargins(8, 6, 8, 6)
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
            {"Code": "^8", "Label": "Mid Blue"},
            {"Code": "^9", "Label": "Cinnabar"},
            {"Code": "^0", "Label": "Black"},
        ]
        first_button = None
        for color in GAMERTAG_COLORS:
            rb = QRadioButton(color["Label"])
            rb.setProperty("code", color["Code"])
            rb.toggled.connect(self.on_color_selected)
            self.color_buttons.addButton(rb)
            color_layout.addWidget(rb)
            if color["Code"] == "":
                first_button = rb
        if first_button:
            first_button.setChecked(True)
            self.selected_gamertag_prefix = ""
            self.selected_gamertag_color = "Default"

        layout.addWidget(self.color_group_box, row, 0, 1, 4)
        row += 1

        # Bottom: reforged note + T7 mode indicator
        self._add_separator(layout, row); row += 1

        indicators_row = QWidget()
        indicators_layout = QHBoxLayout(indicators_row)
        indicators_layout.setContentsMargins(0, 8, 0, 0)
        indicators_layout.setSpacing(12)

        self.reforged_support_label = QLabel(
            "Reforged: Network Password / Force Ranked / Steam Achievements sync to players/T7.json."
        )
        self.reforged_support_label.setObjectName("DashboardStatusName")
        indicators_layout.addWidget(self.reforged_support_label, 1)

        self.t7_mode_label = QLabel(self._status_html("Unknown"))
        self.t7_mode_label.setObjectName("DashboardStatusValue")
        self.t7_mode_label.setAlignment(Qt.AlignRight | Qt.AlignVCenter)
        self.t7_mode_label.setTextFormat(Qt.RichText)
        indicators_layout.addWidget(self.t7_mode_label, 0, Qt.AlignRight)

        layout.addWidget(indicators_row, row, 0, 1, 4)
        row += 1

        layout.setColumnStretch(0, 1)
        layout.setColumnStretch(1, 1)
        layout.setColumnStretch(2, 2)
        layout.setColumnStretch(3, 0)
        layout.setRowStretch(row, 1)

        main_layout = QVBoxLayout(self)
        main_layout.addWidget(self.group)
        self.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Preferred)

    def set_game_directory(self, game_dir, skip_status_check=False):
        self.game_dir = game_dir
        self.refresh_t7_mode_indicator()
        if not skip_status_check and self.game_dir and os.path.exists(self.game_dir):
            status = check_t7_patch_status(self.game_dir)
            reforged_options = read_reforged_t7_options(self.game_dir)
            reforged_password = reforged_options.get("network_pass", "")
            self.reforged_force_ranked_cb.setChecked(reforged_options.get("force_ranked", False))
            self.reforged_steam_achievements_cb.setChecked(reforged_options.get("steam_achievements", False))
            if status["gamertag"]:
                # Update display format to show plain name and color
                if "plain_name" in status and "color_code" in status:
                    self.current_gt_label.setText(
                        self._gamertag_html(status["plain_name"], status["color_code"])
                    )
                else:
                    self.current_gt_label.setText(status["gamertag"])

                # Update password display and input field
                current_password = status["password"] or reforged_password
                self._set_current_password(current_password)
                self.password_edit.setText(current_password)  # Set current password in input field
                
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
                self.current_gt_label.setText("None")
                self._set_current_password(reforged_password)
                self.gamertag_edit.setEnabled(False)
                self.update_gamertag_btn.setEnabled(False)
                # Reforged compatibility: allow password edits even when T7 Patch is not installed.
                self.password_edit.setEnabled(True)
                self.update_password_btn.setEnabled(True)
                self.password_edit.setText(reforged_password)
                self.friends_only_cb.setEnabled(False)
                self.friends_only_cb.setChecked(False)

    def _refresh_reforged_option_controls(self):
        reforged_active = self._is_reforged_active()
        self.reforged_force_ranked_cb.setEnabled(reforged_active)
        self.reforged_steam_achievements_cb.setEnabled(reforged_active)
        self.update_reforged_options_btn.setEnabled(reforged_active)

    def _is_reforged_active(self):
        if not self.game_dir or not os.path.isdir(self.game_dir):
            return False

        exe_path = _find_bo3_executable(self.game_dir)
        exe_hash = file_sha256(exe_path) if exe_path else None
        if exe_hash and exe_hash.lower() in REFORGED_TRUSTED_SHA256:
            return True

        # Fall back to persisted variant only when hash could not be read.
        if read_exe_variant(self.game_dir) == "reforged" and exe_hash is None and exe_path and os.path.exists(exe_path):
            return True

        return False

    def refresh_t7_mode_indicator(self):
        try:
            if not self.game_dir or not os.path.exists(self.game_dir):
                self.t7_mode_label.setText(self._status_html("Unknown"))
                return

            if detect_enhanced_install(self.game_dir):
                self.t7_mode_label.setText(self._status_html("Enhanced", "good"))
                return

            if self._is_reforged_active():
                self.t7_mode_label.setText(self._status_html("Reforged", "info"))
                return

            variant = read_exe_variant(self.game_dir)
            exe_path = _find_bo3_executable(self.game_dir)
            exe_hash = file_sha256(exe_path) if exe_path else None

            if exe_hash and exe_hash == DEFAULT_STEAM_EXE_SHA256:
                self.t7_mode_label.setText(self._status_html("Default", "neutral"))
                return

            if variant == "default":
                self.t7_mode_label.setText(self._status_html("Default", "neutral"))
                return

            self.t7_mode_label.setText(self._status_html("Custom", "info"))
        finally:
            self._refresh_reforged_option_controls()

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
        self.current_gt_label.setText(self._gamertag_html(plain_name, selected_prefix))
        write_log(f"Updated gamertag to: {plain_name} (Color: {selected_color_name})", "Success", self.log_widget)

    def update_password(self):
        if not self.game_dir:
            return
        new_password = self.password_edit.text().strip()
        conf_path = os.path.join(self.game_dir, "t7patch.conf")
        if os.path.exists(conf_path):
            update_t7patch_conf(self.game_dir, new_password=new_password, log_widget=self.log_widget)
        update_reforged_t7_password(self.game_dir, new_password, log_widget=self.log_widget)
        status = check_t7_patch_status(self.game_dir)  # Refresh status after update
        effective_password = status["password"] or read_reforged_t7_password(self.game_dir)
        self._set_current_password(effective_password)

    def update_reforged_options(self):
        if not self.game_dir:
            return
        update_reforged_t7_options(
            self.game_dir,
            force_ranked=self.reforged_force_ranked_cb.isChecked(),
            steam_achievements=self.reforged_steam_achievements_cb.isChecked(),
            log_widget=self.log_widget,
        )

    def friends_only_changed(self):
        if not self.game_dir:
            return
        is_enabled = self.friends_only_cb.isChecked()
        update_t7patch_conf(self.game_dir, friends_only=is_enabled, log_widget=self.log_widget)

    def install_t7_patch(self):
        if not self.game_dir or not os.path.exists(self.game_dir):
            write_log("Game directory does not exist.", "Error", self.log_widget)
            return
        
        if sys.platform.startswith("win") and not is_admin():
            write_log("Requesting elevation via UAC for T7 Patch installation...", "Info", self.log_widget)
            if run_as_admin(["--install-t7", "--game-dir", self.game_dir]) is False:
                write_log("Elevation request was cancelled or failed.", "Warning", self.log_widget)
            return

        self.patch_btn.setEnabled(False)
        self.worker = InstallT7PatchWorker(self.game_dir, self.mod_files_dir)
        self.worker.log_message.connect(self.log_message_received) # Connect to the new log_message signal
        self.worker.finished.connect(self.on_install_finished)
        self.worker.error.connect(self.on_install_error)
        self.worker.patch_installed.connect(self.on_patch_installed)
        self.worker.start()

    def log_message_received(self, message, category, html_message=None):
        if not self.log_widget:
            return
        if html_message:
            self.log_widget.append(html_message)
        else:
            self.log_widget.append(message)

    def on_install_finished(self):
        self.patch_btn.setEnabled(True)
        self.set_game_directory(self.game_dir)

    def on_install_error(self, error_message):
        self.patch_btn.setEnabled(True)
        write_log(f"Error installing T7 Patch: {error_message}", "Error", self.log_widget)

    def on_patch_installed(self):
        self.gamertag_edit.setEnabled(True)
        self.update_gamertag_btn.setEnabled(True)

    def uninstall_t7_patch(self):
        if not self.game_dir or not os.path.exists(self.game_dir):
            write_log("Game directory does not exist.", "Error", self.log_widget)
            return
        uninstall_t7_patch(self.game_dir, self.mod_files_dir, self.log_widget)
        reforged_options = read_reforged_t7_options(self.game_dir)
        reforged_password = reforged_options.get("network_pass", "")
        # Reset UI state without checking status
        self.current_gt_label.setText("None")
        self._set_current_password(reforged_password)
        self.gamertag_edit.setEnabled(False)
        self.update_gamertag_btn.setEnabled(False)
        self.password_edit.setEnabled(True)
        self.update_password_btn.setEnabled(True)
        self.password_edit.setText(reforged_password)
        self.friends_only_cb.setEnabled(False)
        self.friends_only_cb.setChecked(False)
        self.reforged_force_ranked_cb.setChecked(reforged_options.get("force_ranked", False))
        self.reforged_steam_achievements_cb.setChecked(reforged_options.get("steam_achievements", False))
        self._refresh_reforged_option_controls()
        for btn in self.color_buttons.buttons():
            btn.setChecked(False)
        self.patch_uninstalled.emit()  # Notify parent of uninstall
