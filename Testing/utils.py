import datetime
import os
import re
import shutil
import subprocess
import time
import vdf
import platform

def write_log(message, category="Info", log_widget=None, log_file="PatchOpsIII.log"):
    timestamp = datetime.datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    full_message = f"{timestamp} - {category}: {message}"
    if log_widget:
        # Define colors based on category
        if category == "Info":
            color = "white"
        elif category == "Error":
            color = "red"
        elif category == "Warning":
            color = "yellow"
        elif category == "Success":
            color = "green"
        else:
            color = "blue"
        html_message = f'<span style="color:{color};">{full_message}</span>'
        log_widget.append(html_message)
    
    with open(log_file, "a") as f:
        f.write(full_message + "\n")

def _find_windows_steam_root():
    candidates = []
    try:
        import winreg
        registry_targets = [
            (winreg.HKEY_CURRENT_USER, r"Software\\Valve\\Steam", "SteamPath"),
            (winreg.HKEY_LOCAL_MACHINE, r"SOFTWARE\\WOW6432Node\\Valve\\Steam", "InstallPath"),
            (winreg.HKEY_LOCAL_MACHINE, r"SOFTWARE\\Valve\\Steam", "InstallPath"),
        ]
        for hive, subkey, value in registry_targets:
            try:
                with winreg.OpenKey(hive, subkey) as key:
                    location, _ = winreg.QueryValueEx(key, value)
                    if location:
                        candidates.append(location)
            except (FileNotFoundError, OSError):
                continue
    except ImportError:
        pass

    possible_program_files = [
        os.environ.get("PROGRAMFILES(X86)"),
        os.environ.get("PROGRAMFILES"),
    ]
    for root in possible_program_files:
        if root:
            candidates.append(os.path.join(root, "Steam"))

    candidates.extend([
        r"C:\\Program Files (x86)\\Steam",
        r"C:\\Program Files\\Steam",
    ])

    for candidate in candidates:
        if not candidate:
            continue
        normalized = os.path.normpath(candidate)
        exe_path = os.path.join(normalized, "steam.exe")
        if os.path.exists(exe_path):
            return normalized
    return None
def get_steam_paths():
    system = platform.system()
    if system == "Windows":
        steam_root = _find_windows_steam_root()
        if steam_root:
            return {
                'userdata': os.path.join(steam_root, "userdata"),
                'steam_exe': os.path.join(steam_root, "steam.exe"),
            }
        default_base = os.environ.get("PROGRAMFILES(X86)") or os.environ.get("PROGRAMFILES") or r"C:\\Program Files (x86)"
        default_root = os.path.join(default_base, "Steam")
        return {
            'userdata': os.path.join(default_root, "userdata"),
            'steam_exe': os.path.join(default_root, "steam.exe"),
        }
    if system == "Linux":
        home = os.path.expanduser("~")
        return {
            'userdata': os.path.join(home, ".steam/steam/userdata"),
            'steam_exe': "steam",
        }
    if system == "Darwin":
        home = os.path.expanduser("~")
        return {
            'userdata': os.path.join(home, "Library/Application Support/Steam/userdata"),
            'steam_exe': "open",
        }
    return None
steam_paths = get_steam_paths()
steam_userdata_path = None
steam_exe_path = None
if steam_paths:
    steam_userdata_path = steam_paths.get('userdata')
    steam_exe_path = steam_paths.get('steam_exe')
else:
    write_log("Unsupported operating system", "Error", None)

if platform.system() == "Windows" and steam_exe_path and not os.path.exists(steam_exe_path):
    write_log(f"Steam executable not found at {steam_exe_path}. Update your configuration or reinstall Steam.", "Warning", None)

app_id = "311210"  # Black Ops III AppID

def find_steam_user_id():
    if not steam_userdata_path or not os.path.exists(steam_userdata_path):
        write_log("Steam userdata path not found!", "Warning", None)
        return None
    user_ids = [f for f in os.listdir(steam_userdata_path) if f.isdigit()]
    if not user_ids:
        write_log("No Steam user ID found!", "Warning", None)
        return None
    return user_ids[0]

def backup_config_file(config_path, log_widget):
    backup_dir = os.path.join(os.path.dirname(os.path.abspath(__file__)), "backups")
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
    backup_file_path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "backups", "localconfig_backup.vdf")
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
            subprocess.run(["taskkill", "/F", "/IM", "steam.exe"], check=True, timeout=10)
        elif system == "Linux":
            subprocess.run(["pkill", "steam"], check=True, timeout=10)
        time.sleep(5)
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as e:
        write_log(f"Failed to close Steam: {e}", "Error", log_widget)

def open_steam(log_widget):
    system = platform.system()
    try:
        if system == "Windows":
            write_log("Opening Steam...", "Info", log_widget)
            if steam_exe_path and os.path.exists(steam_exe_path):
                subprocess.Popen([steam_exe_path, "-silent"],
                                 stdout=subprocess.DEVNULL,
                                 stderr=subprocess.DEVNULL)
            else:
                write_log("Steam executable path not found; attempting to use system URI handler.", "Warning", log_widget)
                try:
                    os.startfile("steam://open/main")  # type: ignore[attr-defined]
                except AttributeError:
                    subprocess.Popen(["cmd", "/c", "start", "", "steam://"])
        else:
            # Check if Steam is already running
            was_running = (subprocess.call(["pgrep", "-x", "steam"],
                                           stdout=subprocess.DEVNULL, timeout=5) == 0)
            if was_running:
                write_log("Closing Steam...", "Info", log_widget)
                subprocess.call(["pkill", "-x", "steam"],
                                stdout=subprocess.DEVNULL,
                                stderr=subprocess.DEVNULL, timeout=5)
                time.sleep(2)  # Allow time for Steam to shut down

            write_log("Setting launch options...", "Info", log_widget)
            # (Place here any code that sets your launch options.)
            
            write_log("Opening Steam...", "Info", log_widget)
            subprocess.Popen(["xdg-open", "steam://"],
                             stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL)
            # Fallback: if xdg-open fails and 'steam' exists, launch it directly.
            if subprocess.call(["which", "steam"],
                               stdout=subprocess.DEVNULL, timeout=5) == 0:
                subprocess.Popen(["steam", "-silent"],
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
                               stdout=subprocess.DEVNULL, timeout=5) == 0:
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
                                                            stdout=subprocess.DEVNULL, timeout=5) == 0:
            write_log("Steam launched successfully", "Success", log_widget)
        else:
            write_log("Steam did not launch successfully", "Error", log_widget)
    except Exception as e:
        write_log(f"Failed to open Steam: {e}", "Error", log_widget)
        write_log("Please start Steam manually", "Info", log_widget)
        
def launch_game_via_steam(app_id, log_widget=None):
    uri = f"steam://rungameid/{app_id}"
    system = platform.system()
    try:
        if system == "Windows":
            if steam_exe_path and os.path.exists(steam_exe_path):
                subprocess.Popen([steam_exe_path, "-applaunch", app_id])
            else:
                write_log("Steam executable path not found; attempting to use system URI handler.", "Warning", log_widget)
                try:
                    os.startfile(uri)  # type: ignore[attr-defined]
                except AttributeError:
                    subprocess.Popen(["cmd", "/c", "start", "", uri])
        elif system == "Linux":
            if shutil.which("xdg-open"):
                subprocess.Popen(["xdg-open", uri])
            else:
                subprocess.Popen([steam_exe_path or "steam", uri])
        elif system == "Darwin":
            subprocess.Popen(["open", uri])
        else:
            subprocess.Popen([steam_exe_path or "steam", uri])
        write_log(f"Launched Black Ops III via Steam (AppID: {app_id})", "Success", log_widget)
    except FileNotFoundError:
        write_log("Steam client not found. Please verify your Steam installation path.", "Error", log_widget)
    except Exception as exc:
        write_log(f"Error launching game via Steam: {exc}", "Error", log_widget)


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
        
        # If the new options contain fs_game, remove any existing fs_game parameters
        if '+set fs_game' in launch_options:
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

