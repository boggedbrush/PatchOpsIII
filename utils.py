import datetime
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import time
import vdf
import platform

DEFAULT_LOG_FILENAME = "PatchOpsIII.log"
LAUNCH_COUNTER_FILENAME = "launch_counter.txt"
EXE_VARIANT_STATE_FILENAME = ".patchops_exe_variant.json"
PATCHOPS_BACKUP_SUFFIX = ".patchops.bak"
LEGACY_BACKUP_SUFFIX = ".bak"


def patchops_backup_path(path):
    return f"{path}{PATCHOPS_BACKUP_SUFFIX}"


def legacy_backup_path(path):
    return f"{path}{LEGACY_BACKUP_SUFFIX}"


def existing_backup_path(path):
    for candidate in (patchops_backup_path(path), legacy_backup_path(path)):
        if os.path.exists(candidate):
            return candidate
    return None


def _exe_variant_state_path(game_dir):
    return os.path.join(game_dir, EXE_VARIANT_STATE_FILENAME)


def write_exe_variant(game_dir, variant):
    if not game_dir:
        return
    try:
        os.makedirs(game_dir, exist_ok=True)
        with open(_exe_variant_state_path(game_dir), "w", encoding="utf-8") as handle:
            json.dump({"variant": variant}, handle, indent=2)
    except Exception:
        pass


def read_exe_variant(game_dir):
    if not game_dir:
        return None
    path = _exe_variant_state_path(game_dir)
    if not os.path.exists(path):
        return None
    try:
        with open(path, "r", encoding="utf-8") as handle:
            data = json.load(handle)
        value = data.get("variant")
        return value if isinstance(value, str) else None
    except Exception:
        return None


def file_sha256(path):
    if not path or not os.path.exists(path):
        return None
    digest = hashlib.sha256()
    try:
        with open(path, "rb") as handle:
            for chunk in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(chunk)
        return digest.hexdigest()
    except Exception:
        return None

def get_app_data_dir():
    system = platform.system()
    home = os.path.expanduser("~")

    if system == "Windows":
        base = os.environ.get("APPDATA") or os.path.join(home, "AppData", "Roaming")
    elif system == "Darwin":
        base = os.path.join(home, "Library", "Application Support")
    else:
        base = os.environ.get("XDG_DATA_HOME") or os.path.join(home, ".local", "share")

    return os.path.join(base, "PatchOpsIII")

def _resolve_log_path(log_file):
    if not log_file:
        log_file = DEFAULT_LOG_FILENAME

    if os.path.isabs(log_file):
        return log_file

    filename = os.path.basename(log_file) or DEFAULT_LOG_FILENAME
    return os.path.join(get_app_data_dir(), filename)

def write_log(message, category="Info", log_widget=None, log_file=DEFAULT_LOG_FILENAME):
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
        handler = getattr(log_widget, "handle_write_log", None)
        if callable(handler):
            handler(full_message=full_message, category=category, html_message=html_message, plain_message=message)
        else:
            log_widget.append(html_message)

    log_path = _resolve_log_path(log_file)
    try:
        directory = os.path.dirname(log_path)
        if directory:
            os.makedirs(directory, exist_ok=True)
        with open(log_path, "a", encoding="utf-8") as f:
            f.write(full_message + "\n")
    except Exception as exc:
        try:
            print(f"Failed to write log entry to {log_path}: {exc}", file=sys.stderr)
        except Exception:
            pass


def get_log_file_path(log_file=DEFAULT_LOG_FILENAME):
    """Return the resolved path for the given log file."""
    return _resolve_log_path(log_file)


def _launch_counter_path():
    return os.path.join(get_app_data_dir(), LAUNCH_COUNTER_FILENAME)


def _read_launch_count():
    try:
        with open(_launch_counter_path(), "r", encoding="utf-8") as f:
            value = f.read().strip()
            return int(value) if value else 0
    except (FileNotFoundError, ValueError):
        return 0
    except Exception:
        return 0


def _write_launch_count(count):
    try:
        os.makedirs(get_app_data_dir(), exist_ok=True)
        with open(_launch_counter_path(), "w", encoding="utf-8") as f:
            f.write(str(count))
    except Exception:
        pass


def clear_log_file(log_file=DEFAULT_LOG_FILENAME):
    """Truncate the specified log file."""
    path = _resolve_log_path(log_file)
    try:
        directory = os.path.dirname(path)
        if directory:
            os.makedirs(directory, exist_ok=True)
        with open(path, "w", encoding="utf-8"):
            pass
        return True
    except Exception as exc:
        try:
            print(f"Failed to clear log file at {path}: {exc}", file=sys.stderr)
        except Exception:
            pass
        return False


def manage_log_retention_on_launch(threshold=3):
    """Increment launch counter and clear logs every `threshold` launches."""
    if threshold is None or threshold <= 0:
        return False

    count = _read_launch_count() + 1
    cleared = False

    if count >= threshold:
        cleared = clear_log_file()
        count = 0

    _write_launch_count(count)
    return cleared

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


def _dedupe_existing_dirs(paths):
    unique = []
    seen = set()
    for path in paths:
        if not path:
            continue
        normalized = os.path.normcase(os.path.normpath(os.path.abspath(path)))
        if normalized in seen:
            continue
        seen.add(normalized)
        if os.path.isdir(path):
            unique.append(os.path.normpath(path))
    return unique


def _candidate_steam_roots():
    roots = []

    if steam_exe_path and os.path.isabs(steam_exe_path):
        roots.append(os.path.dirname(steam_exe_path))

    if steam_userdata_path:
        roots.append(os.path.dirname(steam_userdata_path))

    system = platform.system()
    home = os.path.expanduser("~")
    if system == "Linux":
        roots.extend([
            os.path.join(home, ".steam", "steam"),
            os.path.join(home, ".local", "share", "Steam"),
        ])
    elif system == "Windows":
        detected_root = _find_windows_steam_root()
        if detected_root:
            roots.append(detected_root)
    elif system == "Darwin":
        roots.append(os.path.join(home, "Library", "Application Support", "Steam"))

    return _dedupe_existing_dirs(roots)


def _extract_library_paths_from_vdf(library_vdf_path):
    if not library_vdf_path or not os.path.exists(library_vdf_path):
        return []
    try:
        with open(library_vdf_path, "r", encoding="utf-8") as handle:
            data = vdf.load(handle)
    except Exception:
        return []

    libraryfolders = data.get("libraryfolders")
    if not isinstance(libraryfolders, dict):
        return []

    libraries = []
    for key, value in libraryfolders.items():
        if key == "contentstatsid":
            continue
        if isinstance(value, str):
            libraries.append(value)
            continue
        if isinstance(value, dict):
            path_value = value.get("path")
            if isinstance(path_value, str):
                libraries.append(path_value)

    return _dedupe_existing_dirs(libraries)


def get_steam_library_paths():
    libraries = []
    for root in _candidate_steam_roots():
        libraries.append(root)
        library_vdf = os.path.join(root, "steamapps", "libraryfolders.vdf")
        libraries.extend(_extract_library_paths_from_vdf(library_vdf))
    return _dedupe_existing_dirs(libraries)


def _workshop_item_installed_in_library(steam_library, game_app_id, workshop_id):
    install_dir = os.path.join(
        steam_library,
        "steamapps",
        "workshop",
        "content",
        str(game_app_id),
        str(workshop_id),
    )
    if not os.path.isdir(install_dir):
        return False, None
    try:
        has_files = any(os.scandir(install_dir))
    except OSError:
        has_files = False
    return has_files, install_dir


def _contains_workshop_id(value, workshop_id):
    if isinstance(value, dict):
        if str(workshop_id) in value:
            return True
        return any(_contains_workshop_id(item, workshop_id) for item in value.values())
    if isinstance(value, list):
        return any(_contains_workshop_id(item, workshop_id) for item in value)
    return str(value) == str(workshop_id)


def _workshop_item_subscribed_in_library(steam_library, game_app_id, workshop_id):
    acf_path = os.path.join(
        steam_library,
        "steamapps",
        "workshop",
        f"appworkshop_{game_app_id}.acf",
    )
    if not os.path.exists(acf_path):
        return False
    try:
        with open(acf_path, "r", encoding="utf-8") as handle:
            data = vdf.load(handle)
    except Exception:
        return False
    workshop_data = data.get("AppWorkshop", {})
    if not isinstance(workshop_data, dict):
        return False
    return _contains_workshop_id(workshop_data, workshop_id)


def get_workshop_item_state(game_app_id, workshop_id):
    """Return the Steam Workshop state for a BO3 workshop item."""
    workshop_id = str(workshop_id)
    game_app_id = str(game_app_id)

    subscribed = False
    for steam_library in get_steam_library_paths():
        installed, install_dir = _workshop_item_installed_in_library(steam_library, game_app_id, workshop_id)
        if installed:
            return {
                "state": "Installed",
                "installed": True,
                "subscribed": True,
                "path": install_dir,
            }
        if _workshop_item_subscribed_in_library(steam_library, game_app_id, workshop_id):
            subscribed = True

    if subscribed:
        return {
            "state": "Subscribed (not installed yet)",
            "installed": False,
            "subscribed": True,
            "path": None,
        }

    return {
        "state": "Not Subscribed",
        "installed": False,
        "subscribed": False,
        "path": None,
    }

def get_backup_locations():
    user_backup_dir = os.path.join(get_app_data_dir(), "backups")
    module_backup_dir = os.path.join(os.path.dirname(os.path.abspath(__file__)), "backups")
    locations = [user_backup_dir]
    if os.path.normpath(module_backup_dir) != os.path.normpath(user_backup_dir):
        locations.append(module_backup_dir)
    return locations

def backup_config_file(config_path, log_widget):
    backup_dirs = get_backup_locations()
    primary_dir = backup_dirs[0]
    try:
        os.makedirs(primary_dir, exist_ok=True)
    except Exception as e:
        write_log(f"Failed to prepare backup directory '{primary_dir}': {e}", "Error", log_widget)
        return False

    backup_file_path = os.path.join(primary_dir, "localconfig_backup.vdf")

    try:
        shutil.copy2(config_path, backup_file_path)  # copy2 preserves metadata
        write_log(f"Config backup created at {backup_file_path}", "Success", log_widget)
        return True
    except Exception as e:
        write_log(f"Failed to create backup: {e}", "Error", log_widget)
        return False

def restore_config_file(config_path, log_widget):
    for directory in get_backup_locations():
        backup_file_path = os.path.join(directory, "localconfig_backup.vdf")
        if not os.path.exists(backup_file_path):
            continue
        try:
            shutil.copy2(backup_file_path, config_path)
            write_log("Config restored from backup", "Success", log_widget)
            return True
        except Exception as e:
            write_log(f"Failed to restore backup from {backup_file_path}: {e}", "Error", log_widget)
            return False
    write_log("No backup file found", "Warning", log_widget)
    return False

def is_steam_running():
    system = platform.system()
    try:
        if system == "Windows":
            result = subprocess.run(
                ["tasklist", "/FI", "IMAGENAME eq steam.exe"],
                capture_output=True,
                text=True,
                timeout=5
            )
            output = (result.stdout or "").lower()
            return "steam.exe" in output
        return subprocess.call(
            ["pgrep", "-x", "steam"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=5
        ) == 0
    except (subprocess.SubprocessError, OSError):
        return False

def close_steam(log_widget):
    system = platform.system()
    try:
        if system == "Windows":
            result = subprocess.run(
                ["taskkill", "/F", "/IM", "steam.exe"],
                check=False,
                timeout=10,
                capture_output=True,
                text=True
            )
            if result.returncode != 0:
                if not is_steam_running():
                    write_log("Steam was not running.", "Info", log_widget)
                else:
                    write_log(f"Failed to close Steam: {result.stderr.strip() or result.stdout.strip()}", "Error", log_widget)
                    return
        elif system == "Linux":
            result = subprocess.run(
                ["pkill", "steam"],
                check=False,
                timeout=10,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL
            )
            if result.returncode == 1:
                write_log("Steam was not running.", "Info", log_widget)
            elif result.returncode != 0:
                write_log(f"Failed to close Steam (return code {result.returncode}).", "Error", log_widget)
                return
        time.sleep(5)
    except subprocess.TimeoutExpired:
        write_log("Timed out while attempting to close Steam.", "Warning", log_widget)
    except FileNotFoundError as e:
        write_log(f"Steam control command not found: {e}", "Error", log_widget)
    except subprocess.SubprocessError as e:
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

        def steam_running():
            if system == "Windows":
                try:
                    result = subprocess.run(["tasklist", "/FI", "IMAGENAME eq steam.exe"],
                                            capture_output=True,
                                            text=True,
                                            timeout=5)
                except (subprocess.SubprocessError, OSError):
                    return False
                output = (result.stdout or "").lower()
                return "steam.exe" in output
            return subprocess.call(["pgrep", "-x", "steam"],
                                   stdout=subprocess.DEVNULL, timeout=5) == 0

        max_wait = 15      # maximum total wait time (seconds)
        poll_interval = 0.5  # poll every 0.5 seconds
        stable_duration = 2  # require Steam to be present for 2 consecutive seconds
        elapsed = 0
        stable_time = 0

        while elapsed < max_wait:
            if steam_running():
                stable_time += poll_interval
                if stable_time >= stable_duration:
                    break
            else:
                if system != "Windows" and stable_time > 0:
                    # Try launching again if it vanished after being detected.
                    subprocess.Popen(["xdg-open", "steam://"],
                                     stdout=subprocess.DEVNULL,
                                     stderr=subprocess.DEVNULL)
                stable_time = 0
            time.sleep(poll_interval)
            elapsed += poll_interval

        if stable_time >= stable_duration and steam_running():
            write_log("Steam launched successfully", "Success", log_widget)
        else:
            level = "Warning" if system == "Windows" else "Error"
            write_log("Steam did not launch successfully", level, log_widget)
    except Exception as e:
        write_log(f"Failed to open Steam: {e}", "Error", log_widget)
        write_log("Please start Steam manually", "Info", log_widget)


def launch_game_via_steam(app_id, log_widget=None):
    uri = f"steam://rungameid/{app_id}"
    system = platform.system()
    last_error = None

    try:
        if system == "Windows":
            if steam_exe_path and os.path.exists(steam_exe_path):
                subprocess.Popen(
                    [steam_exe_path, "-applaunch", str(app_id)],
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL
                )
            else:
                write_log("Steam executable path not found; attempting to use system URI handler.", "Warning", log_widget)
                try:
                    os.startfile(uri)  # type: ignore[attr-defined]
                except AttributeError:
                    subprocess.Popen(["cmd", "/c", "start", "", uri])
        elif system == "Linux":
            commands = []

            steam_cmd = None
            if steam_exe_path:
                if os.path.isabs(steam_exe_path) and os.path.exists(steam_exe_path):
                    steam_cmd = steam_exe_path
                else:
                    steam_cmd = shutil.which(steam_exe_path)
            if not steam_cmd:
                steam_cmd = shutil.which("steam")

            if steam_cmd:
                commands.append(
                    {
                        "cmd": [steam_cmd, "-applaunch", str(app_id)],
                        "description": f"{os.path.basename(steam_cmd)} -applaunch"
                    }
                )

            xdg = shutil.which("xdg-open")
            if xdg:
                commands.append(
                    {
                        "cmd": [xdg, uri],
                        "description": "xdg-open steam URI",
                        "check": True
                    }
                )

            # Fallback if nothing else is available
            if not commands:
                commands.append(
                    {
                        "cmd": [steam_exe_path or "steam", uri],
                        "description": "steam URI fallback"
                    }
                )

            for entry in commands:
                try:
                    if entry.get("check"):
                        result = subprocess.run(
                            entry["cmd"],
                            check=False,
                            stdout=subprocess.DEVNULL,
                            stderr=subprocess.DEVNULL
                        )
                        if result.returncode != 0:
                            last_error = f"{entry['cmd'][0]} exited with code {result.returncode}"
                            continue
                    else:
                        subprocess.Popen(
                            entry["cmd"],
                            stdout=subprocess.DEVNULL,
                            stderr=subprocess.DEVNULL
                        )
                    write_log(
                        f"Launched Black Ops III via Steam (AppID: {app_id}) using {entry['description']}",
                        "Success",
                        log_widget
                    )
                    return
                except FileNotFoundError:
                    last_error = f"{entry['cmd'][0]} not found"
                    continue
                except Exception as exc:
                    last_error = str(exc)
                    continue
            raise RuntimeError(last_error or "No suitable launcher command found")
        elif system == "Darwin":
            subprocess.Popen(["open", uri])
        else:
            subprocess.Popen([steam_exe_path or "steam", uri])
        write_log(f"Launched Black Ops III via Steam (AppID: {app_id})", "Success", log_widget)
    except FileNotFoundError:
        write_log("Steam client not found. Please verify your Steam installation path.", "Error", log_widget)
    except Exception as exc:
        write_log(f"Error launching game via Steam: {exc}", "Error", log_widget)


def set_launch_options(user_id, app_id, launch_options, log_widget, preserve_fs_game=False):
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
        steam_config = data.setdefault("UserLocalConfigStore", {})
        software = steam_config.setdefault("Software", {})
        valve = software.setdefault("Valve", {})
        steam = valve.setdefault("Steam", {})
        apps = steam.setdefault("apps", {})

        app_entry = apps.setdefault(app_id, {})
        current_options = app_entry.get("LaunchOptions", "")
        requested_options = launch_options or ""

        # Parse existing options
        wine_override = 'WINEDLLOVERRIDES="dsound=n,b"'
        command_marker = "%command%"
        fs_game_pattern = r'\+set\s+fs_game\s+\S+'

        def strip_token(option_str, token):
            if not option_str or not token:
                return option_str
            return re.sub(rf'(?<!\S){re.escape(token)}(?!\S)', '', option_str)

        def normalize(option_str):
            if not option_str:
                return ''
            return re.sub(r'\s+', ' ', option_str).strip()

        existing_fs_games = re.findall(fs_game_pattern, current_options) if current_options else []
        new_fs_games = re.findall(fs_game_pattern, requested_options)

        # Keep existing fs_game entries when requested and no new ones are provided
        fs_game_entries = new_fs_games or (existing_fs_games if preserve_fs_game else [])

        # Always strip fs_game from the strings we merge, then append the chosen entries
        cleaned_current = re.sub(fs_game_pattern, '', current_options).strip()
        cleaned_launch = re.sub(fs_game_pattern, '', requested_options).strip()

        has_current_wine = wine_override in cleaned_current
        has_current_command = command_marker in cleaned_current
        has_new_wine = wine_override in requested_options
        has_new_command = command_marker in requested_options

        cleaned_current = strip_token(strip_token(cleaned_current, wine_override), command_marker)
        cleaned_launch = strip_token(strip_token(cleaned_launch, wine_override), command_marker)

        include_wine = has_new_wine or has_current_wine
        include_command = has_new_command or has_current_command
        if include_wine and not include_command:
            include_command = True

        segments = []
        if include_wine:
            segments.append(wine_override)
        if include_command:
            segments.append(command_marker)
        if cleaned_current:
            segments.append(normalize(cleaned_current))
        if cleaned_launch:
            segments.append(normalize(cleaned_launch))
        if fs_game_entries:
            unique_fs_games = []
            for entry in fs_game_entries:
                if entry not in unique_fs_games:
                    unique_fs_games.append(entry)
            segments.append(normalize(' '.join(unique_fs_games)))

        final_options = normalize(' '.join(segments))
        app_entry["LaunchOptions"] = final_options

        write_log(f"Setting launch options to: {final_options}", "Info", log_widget)

        with open(config_path, "w", encoding="utf-8") as file:
            vdf.dump(data, file, pretty=True)
        return True
    except Exception as e:
        write_log(f"Error updating launch options: {e}", "Error", log_widget)
        restore_config_file(config_path, log_widget)
        return False


def apply_launch_options(launch_option, log_widget, preserve_fs_game=False):
    user_id = find_steam_user_id()
    if not user_id:
        raise Exception("Steam user ID not found!")
    close_steam(log_widget)
    set_launch_options(user_id, app_id, launch_option, log_widget, preserve_fs_game=preserve_fs_game)
    open_steam(log_widget)
