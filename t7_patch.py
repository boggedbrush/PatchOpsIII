#!/usr/bin/env python
import os, sys, ctypes, subprocess, zipfile, shutil, requests, hashlib, re
from utils import (
    write_log,
    patchops_backup_path,
    existing_backup_path,
    PATCHOPS_BACKUP_SUFFIX,
    LEGACY_BACKUP_SUFFIX,
)

DEFAULT_STEAM_EXE_SHA256 = "9ba98dba41e18ef47de6c63937340f8eae7cb251f8fbc2e78d70047b64aa15b5"
T7_INSTALL_MARKERS = (
    "t7patchloader.dll",
    "t7patch.dll",
)
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
            write_log(f"Cannot modify {conf_path}. Run as administrator.", "Error", log_widget)
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


def is_t7_patch_installed(game_dir):
    if not game_dir or not os.path.isdir(game_dir):
        return False
    return any(os.path.exists(os.path.join(game_dir, marker)) for marker in T7_INSTALL_MARKERS)
