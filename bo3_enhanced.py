"""Proof-of-concept BO3 Enhanced integration utilities for PatchOpsIII."""
from __future__ import annotations

import hashlib
import io
import json
import os
import time
import zipfile
from dataclasses import dataclass
from typing import Callable, Dict, Iterable, Optional, Tuple

import requests


from utils import (
    write_log,
    patchops_backup_path,
    existing_backup_path,
)
import shutil
import re

# Upstream resources
GITHUB_LATEST_ENHANCED_API = "https://api.github.com/repos/shiversoftdev/BO3Enhanced/releases/latest"
GITHUB_LATEST_ENHANCED_PAGE = "https://github.com/shiversoftdev/BO3Enhanced/releases/latest"


# Local file names
STATE_FILENAME = "bo3_enhanced_state.json"
ENHANCED_ARCHIVE_NAME = "BO3Enhanced_latest.zip"
DUMP_ARCHIVE_NAME = "DUMP.zip"
CHECKSUMS_FILENAME = "bo3_enhanced_checksums.json"

# Expected contents for basic validation
EXPECTED_ENHANCED_FILES = {
    "T7WSBootstrapper.dll",
    "T7InternalWS.dll",
    "steam_api65.dll",
    "WindowsCodecs.dll",
}
EXPECTED_DUMP_FILES = {
    "BlackOps3.exe",
    "MicrosoftGame.config",
}


# Hardcoded whitelist for security
UWP_DUMP_WHITELIST = {
    "BlackOps3.exe",
    "MicrosoftGame.config",
    "Party.dll",
    "PartyXboxLive.dll",
    "PlayFabMultiplayerGDK.dll",
    "libScePad.dll",
    "XCurl.dll",
}

STEAM_CORE_FILES = {
    "BlackOps3.exe",
    "cod.bmp",
    "codlogo.bmp",
    "controller.vdf",
    "CrashUploader.exe",
    "d3dcompiler_46.dll",
    "installscript_311210.vdf",
    "localization.txt",
    "steam_api64.dll",
}



_SESSION = requests.Session()





def _is_within_root(root_path: str, candidate_path: str) -> bool:
    """Return True if candidate_path is inside root_path."""
    try:
        root_abs = os.path.abspath(root_path)
        candidate_abs = os.path.abspath(candidate_path)
        return os.path.commonpath([root_abs, candidate_abs]) == root_abs
    except (ValueError, OSError):
        return False


def _state_path(storage_dir: str) -> str:
    return os.path.join(storage_dir, STATE_FILENAME)


def _checksums_path(storage_dir: str) -> str:
    return os.path.join(storage_dir, CHECKSUMS_FILENAME)


def load_state(storage_dir: str) -> Dict[str, str]:
    try:
        with open(_state_path(storage_dir), "r", encoding="utf-8") as handle:
            return json.load(handle)
    except Exception:
        return {}


def save_state(storage_dir: str, state: Dict[str, str]) -> None:
    try:
        os.makedirs(storage_dir, exist_ok=True)
        with open(_state_path(storage_dir), "w", encoding="utf-8") as handle:
            json.dump(state, handle, indent=2)
    except Exception as exc:
        write_log(f"Failed to persist BO3 Enhanced state: {exc}", "Warning", None)


def mark_enhanced_detected(storage_dir: str) -> None:
    state = load_state(storage_dir)
    state["installed"] = True
    state["detected_at"] = state.get("detected_at") or time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    save_state(storage_dir, state)


def set_acknowledged(storage_dir: str) -> None:
    state = load_state(storage_dir)
    state["acknowledged_at"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    save_state(storage_dir, state)


def _normalize_version(value: str) -> Tuple[int, ...]:
    cleaned = value.strip()
    if cleaned.startswith("v"):
        cleaned = cleaned[1:]
    if not cleaned:
        return (0,)
    parts = []
    for segment in cleaned.replace("-", ".").split("."):
        try:
            parts.append(int(segment))
        except ValueError:
            break
    return tuple(parts) if parts else (0,)


@dataclass
class EnhancedRelease:
    version: str
    name: str
    body: str
    asset_url: str
    asset_name: str
    page_url: str
    asset_sha256: Optional[str] = None


def fetch_latest_release() -> Optional[EnhancedRelease]:
    try:
        response = requests.get(GITHUB_LATEST_ENHANCED_API, timeout=30)
        response.raise_for_status()
        data = response.json()
    except requests.RequestException as exc:
        write_log(f"Failed to fetch BO3 Enhanced release metadata: {exc}", "Warning", None)
        return None

    assets = data.get("assets") or []
    selected = None
    for asset in assets:
        name = asset.get("name", "").lower()
        if name.endswith(".zip"):
            selected = asset
            break
        if name.endswith(".7z") and selected is None:
            selected = asset

    if not selected:
        write_log("No downloadable asset found for BO3 Enhanced latest release", "Warning", None)
        return None

    digest_value = str(selected.get("digest") or "").strip()
    asset_sha256 = None
    if digest_value.lower().startswith("sha256:"):
        candidate = digest_value.split(":", 1)[1].strip().lower()
        if re.fullmatch(r"[a-f0-9]{64}", candidate):
            asset_sha256 = candidate

    return EnhancedRelease(
        version=data.get("tag_name") or data.get("name") or "0.0.0",
        name=data.get("name") or "BO3 Enhanced",
        body=data.get("body") or "",
        asset_url=selected.get("browser_download_url") or "",
        asset_name=selected.get("name") or ENHANCED_ARCHIVE_NAME,
        page_url=data.get("html_url") or GITHUB_LATEST_ENHANCED_PAGE,
        asset_sha256=asset_sha256,
    )


def _download_file(
    url: str,
    dest_path: str,
    *,
    progress: Optional[Callable[[int], None]] = None,
    timeout: int = 30,
) -> Optional[str]:
    try:
        with _SESSION.get(url, stream=True, timeout=timeout, allow_redirects=True) as response:
            response.raise_for_status()
            content_type = response.headers.get("Content-Type", "").lower()
            if "text/html" in content_type:
                write_log(f"Received HTML content from {url}; expected binary download.", "Error", None)
                return None
            total = int(response.headers.get("Content-Length") or 0)
            downloaded = 0
            os.makedirs(os.path.dirname(dest_path) or ".", exist_ok=True)
            with open(dest_path, "wb") as handle:
                for chunk in response.iter_content(chunk_size=1024 * 512):
                    if not chunk:
                        continue
                    handle.write(chunk)
                    downloaded += len(chunk)
                    if progress and total:
                        percent = int(downloaded * 100 / total)
                        progress(min(percent, 100))
        if progress:
            progress(100)
        return dest_path
    except requests.RequestException as exc:
        write_log(f"Download failed for {url}: {exc}", "Warning", None)
    except Exception as exc:  # noqa: BLE001
        write_log(f"Unexpected error downloading {url}: {exc}", "Error", None)
    return None


def _compute_sha256(path: str) -> str:
    sha = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            sha.update(chunk)
    return sha.hexdigest()


def _is_probably_zip(data: bytes) -> bool:
    return zipfile.is_zipfile(io.BytesIO(data))


def _download_bytes(url: str, *, timeout: int = 60) -> Optional[bytes]:
    try:
        resp = _SESSION.get(url, timeout=timeout, allow_redirects=True)
        resp.raise_for_status()
        content_type = resp.headers.get("Content-Type", "").lower()
        if "text/html" in content_type:
            raise RuntimeError(f"Received HTML content from {url}; expected binary download.")
        return resp.content
    except Exception as exc:  # noqa: BLE001
        write_log(f"Download failed for {url}: {exc}", "Error", None)
        return None


def _load_checksums(storage_dir: str) -> Dict[str, str]:
    try:
        with open(_checksums_path(storage_dir), "r", encoding="utf-8") as handle:
            return json.load(handle)
    except Exception:
        return {}


def _save_checksums(storage_dir: str, checksums: Dict[str, str]) -> None:
    try:
        os.makedirs(storage_dir, exist_ok=True)
        with open(_checksums_path(storage_dir), "w", encoding="utf-8") as handle:
            json.dump(checksums, handle, indent=2)
    except Exception as exc:
        write_log(f"Failed to save checksum cache: {exc}", "Warning", None)


def validate_enhanced_archive(path: str) -> bool:
    try:
        with zipfile.ZipFile(path, "r") as archive:
            names = set(info.filename.split("/")[-1] for info in archive.infolist())
            missing = EXPECTED_ENHANCED_FILES.difference(names)
            if missing:
                write_log(f"Enhanced archive missing expected files: {', '.join(sorted(missing))}", "Error", None)
                return False
    except zipfile.BadZipFile:
        write_log("Enhanced archive is not a valid zip", "Error", None)
        return False
    except Exception as exc:
        write_log(f"Failed to validate Enhanced archive: {exc}", "Error", None)
        return False
    return True


def validate_dump_source(source_path: str) -> bool:
    """Validate a dump zip archive or directory."""
    if not os.path.exists(source_path):
        write_log(f"Dump source not found: {source_path}", "Error", None)
        return False

    is_dir = os.path.isdir(source_path)
    
    try:
        found_files = set()
        
        if is_dir:
            # Check for files in root or DUMP/ subdir
            for root, _, files in os.walk(source_path):
                for f in files:
                    # Normalize path relative to source_path
                    rel = os.path.relpath(os.path.join(root, f), source_path).replace("\\", "/")
                    # If inside DUMP/, strip prefix
                    if rel.startswith("DUMP/"):
                        found_files.add(rel[5:])
                    else:
                        found_files.add(rel)
        else:
            if not zipfile.is_zipfile(source_path):
                write_log("Dump source is not a valid zip file or directory", "Error", None)
                return False
                
            with zipfile.ZipFile(source_path, "r") as archive:
                for info in archive.infolist():
                    name = info.filename
                    if name.startswith("DUMP/"):
                        found_files.add(name[5:])
                    else:
                        found_files.add(name)

        # Check required files
        missing = [f for f in EXPECTED_DUMP_FILES if f not in found_files]
        if missing:
            write_log(f"Dump source missing required files: {', '.join(missing)}", "Error", None)
            return False
            
    except Exception as exc:
        write_log(f"Failed to validate dump source: {exc}", "Error", None)
        return False
        
    return True


def download_latest_enhanced(mod_files_dir: str, storage_dir: str, progress: Optional[Callable[[int], None]] = None) -> Optional[str]:
    release = fetch_latest_release()
    if not release or not release.asset_url:
        return None
    if not release.asset_sha256:
        write_log(
            "Latest BO3 Enhanced release metadata did not include a SHA-256 digest; refusing unverified download.",
            "Error",
            None,
        )
        return None
    dest = os.path.join(mod_files_dir, ENHANCED_ARCHIVE_NAME)
    path = _download_file(release.asset_url, dest, progress=progress)
    if not path:
        return None
    checksum = _compute_sha256(path)
    if checksum.lower() != release.asset_sha256.lower():
        write_log(
            "BO3 Enhanced download failed integrity verification (SHA-256 mismatch).",
            "Error",
            None,
        )
        try:
            os.remove(path)
        except Exception:
            pass
        return None
    if not validate_enhanced_archive(path):
        return None
    checksums = _load_checksums(storage_dir)
    checksums[os.path.basename(path)] = checksum
    _save_checksums(storage_dir, checksums)
    write_log(f"Downloaded BO3 Enhanced {release.version} to {path}", "Success", None)
    return path





def detect_enhanced_install(game_dir: str) -> bool:
    if not game_dir:
        return False
    present = []
    for filename in EXPECTED_ENHANCED_FILES:
        candidate = os.path.join(game_dir, filename)
        if os.path.exists(candidate):
            present.append(filename)
    if len(present) == len(EXPECTED_ENHANCED_FILES):
        return True
    return False


def clear_enhanced_state(storage_dir: str) -> None:
    state = load_state(storage_dir)
    state["installed"] = False
    state["installed_files"] = []
    save_state(storage_dir, state)


def _safe_extract_member(archive: zipfile.ZipFile, member: zipfile.ZipInfo, dest_root: str) -> None:
    # Prevent directory traversal
    target = os.path.normpath(os.path.join(dest_root, os.path.basename(member.filename)))
    if not target.startswith(os.path.normpath(dest_root)):
        raise ValueError(f"Unsafe path detected in archive: {member.filename}")
    with archive.open(member, "r") as src, open(target, "wb") as dst:
        shutil.copyfileobj(src, dst)


def _backup_with_bak(target_path: str) -> Optional[str]:
    if not os.path.exists(target_path):
        return None
    bak_path = patchops_backup_path(target_path)
    # Keep the first backup as the original rollback point.
    existing_backup = existing_backup_path(target_path)
    if existing_backup:
        write_log(f"Existing backup found for {os.path.basename(target_path)}; preserving original backup.", "Info", None)
        return existing_backup
    os.rename(target_path, bak_path)
    return bak_path


def _should_copy_dump_member(rel_path: str) -> bool:
    """
    Decide whether a file from DUMP.zip should be copied into the game dir.
    rel_path is the path inside the DUMP/ folder (e.g. 'BlackOps3.exe').
    """
    name = os.path.basename(rel_path)
    lower_name = name.lower()
    _, ext = os.path.splitext(lower_name)

    # Skip files Enhanced already provides
    if name in EXPECTED_ENHANCED_FILES:
        return False

    # Strict Whitelist Check
    if name in UWP_DUMP_WHITELIST:
        return True

    return False


def install_enhanced_files(game_dir: str, mod_files_dir: str, storage_dir: str, dump_source: str, log_widget=None) -> bool:
    """Install BO3 Enhanced files + dump (from zip or folder) into the game directory with backups."""
    if not game_dir or not os.path.isdir(game_dir):
        write_log("Invalid game directory for Enhanced install.", "Error", log_widget)
        return False

    enhanced_zip = os.path.join(mod_files_dir, ENHANCED_ARCHIVE_NAME)
    
    if not os.path.exists(enhanced_zip):
        write_log(f"Enhanced archive not found at {enhanced_zip}", "Error", log_widget)
        return False
    if not os.path.exists(dump_source):
        write_log(f"Dump source not found at {dump_source}", "Error", log_widget)
        return False

    if not validate_enhanced_archive(enhanced_zip):
        write_log("Enhanced archive failed validation; aborting install.", "Error", log_widget)
        return False
    if not validate_dump_source(dump_source):
        write_log("Dump source failed validation; aborting install.", "Error", log_widget)
        return False

    try:
        installed_rel_paths = []

        # 1) Install all dump contents
        if os.path.isdir(dump_source):
            # Install from directory
            for root, _, files in os.walk(dump_source):
                for f in files:
                    src_path = os.path.join(root, f)
                    rel_path = os.path.relpath(src_path, dump_source).replace("\\", "/")
                    
                    # Handle DUMP/ prefix if it exists in folder structure
                    if rel_path.startswith("DUMP/"):
                        final_rel_path = rel_path[5:]
                    else:
                        final_rel_path = rel_path
                        
                    if not final_rel_path:
                        continue
                        
                    if not _should_copy_dump_member(final_rel_path):
                        continue
                        
                    target = os.path.normpath(os.path.join(game_dir, final_rel_path))
                    if not _is_within_root(game_dir, target):
                        continue
                        
                    _backup_with_bak(target)
                    os.makedirs(os.path.dirname(target), exist_ok=True)
                    shutil.copy2(src_path, target)
                    installed_rel_paths.append(final_rel_path)
        else:
            # Install from ZIP
            with zipfile.ZipFile(dump_source, "r") as archive:
                for info in archive.infolist():
                    rel_path = info.filename
                    
                    if rel_path.startswith("DUMP/"):
                        final_rel_path = rel_path[5:]
                    else:
                        final_rel_path = rel_path
                        
                    if not final_rel_path or final_rel_path.endswith("/"):
                        continue
                        
                    if not _should_copy_dump_member(final_rel_path):
                        continue
                    
                    target = os.path.normpath(os.path.join(game_dir, final_rel_path))
                    if not _is_within_root(game_dir, target):
                        raise ValueError(f"Unsafe path detected in dump archive: {info.filename}")

                    _backup_with_bak(target)
                    os.makedirs(os.path.dirname(target), exist_ok=True)
                    with archive.open(info, "r") as src, open(target, "wb") as dst:
                        shutil.copyfileobj(src, dst)

                    installed_rel_paths.append(final_rel_path)

        # 2) Install BO3 Enhanced DLLs on top
        with zipfile.ZipFile(enhanced_zip, "r") as archive:
            for info in archive.infolist():
                name = os.path.basename(info.filename)
                if name in EXPECTED_ENHANCED_FILES:
                    dest_path = os.path.join(game_dir, name)
                    _backup_with_bak(dest_path)
                    _safe_extract_member(archive, info, game_dir)
                    installed_rel_paths.append(name)

        write_log("Installed BO3 Enhanced files and dump into game directory.", "Success", log_widget)
        state = load_state(storage_dir)
        state["installed"] = True
        state["detected_at"] = state.get("detected_at") or time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        state["installed_files"] = sorted(set(installed_rel_paths))
        if not state["installed_files"]:
            state["installed"] = False
        save_state(storage_dir, state)
        return True
    except Exception as exc:  # noqa: BLE001
        write_log(f"Failed to install BO3 Enhanced: {exc}", "Error", log_widget)
        return False


def install_dump_only(game_dir: str, mod_files_dir: str, storage_dir: str, dump_source: str, log_widget=None) -> bool:
    """Install only the dump contents (testing helper)."""
    if not game_dir or not os.path.isdir(game_dir):
        write_log("Invalid game directory for dump install.", "Error", log_widget)
        return False

    if not os.path.exists(dump_source):
        write_log(f"Dump source not found at {dump_source}", "Error", log_widget)
        return False
    if not validate_dump_source(dump_source):
        write_log("Dump source failed validation; aborting install.", "Error", log_widget)
        return False

    try:
        installed_rel_paths = []
        if os.path.isdir(dump_source):
             for root, _, files in os.walk(dump_source):
                for f in files:
                    src_path = os.path.join(root, f)
                    rel_path = os.path.relpath(src_path, dump_source).replace("\\", "/")
                    if rel_path.startswith("DUMP/"):
                        final_rel_path = rel_path[5:]
                    else:
                        final_rel_path = rel_path
                    if not final_rel_path: continue
                    if not _should_copy_dump_member(final_rel_path): continue
                    
                    target = os.path.normpath(os.path.join(game_dir, final_rel_path))
                    if not _is_within_root(game_dir, target):
                        continue
                    _backup_with_bak(target)
                    os.makedirs(os.path.dirname(target), exist_ok=True)
                    shutil.copy2(src_path, target)
                    installed_rel_paths.append(final_rel_path)
        else:
            with zipfile.ZipFile(dump_source, "r") as archive:
                for info in archive.infolist():
                    rel_path = info.filename
                    if rel_path.startswith("DUMP/"):
                        final_rel_path = rel_path[5:]
                    else:
                        final_rel_path = rel_path
                    
                    if not final_rel_path or final_rel_path.endswith("/"): continue
                    if not _should_copy_dump_member(final_rel_path): continue
                    target = os.path.normpath(os.path.join(game_dir, final_rel_path))
                    if not _is_within_root(game_dir, target):
                        raise ValueError(f"Unsafe path detected in dump archive: {info.filename}")
                    _backup_with_bak(target)
                    os.makedirs(os.path.dirname(target), exist_ok=True)
                    with archive.open(info, "r") as src, open(target, "wb") as dst:
                        shutil.copyfileobj(src, dst)
                    installed_rel_paths.append(final_rel_path)

        write_log("Dump-only install completed.", "Success", log_widget)
        if installed_rel_paths:
            state = load_state(storage_dir)
            state["dump_only_files"] = sorted(set(installed_rel_paths))
            save_state(storage_dir, state)
        return True
    except Exception as exc:  # noqa: BLE001
        write_log(f"Failed to install dump: {exc}", "Error", log_widget)
        return False


def uninstall_dump_only(game_dir: str, mod_files_dir: str, storage_dir: str, log_widget=None) -> bool:
    """Uninstall dump-only changes by restoring backups or removing added files."""
    if not game_dir or not os.path.isdir(game_dir):
        write_log("Invalid game directory for dump uninstall.", "Error", log_widget)
        return False

    state = load_state(storage_dir)
    tracked = state.get("dump_only_files", [])

    dump_zip = os.path.join(mod_files_dir, DUMP_ARCHIVE_NAME)
    restored = 0
    removed = 0
    try:
        if not tracked:
            if not os.path.exists(dump_zip):
                # Nothing was ever installed; nothing to uninstall.
                return True
            with zipfile.ZipFile(dump_zip, "r") as archive:
                for info in archive.infolist():
                    if not info.filename.startswith("DUMP/"):
                        continue
                    rel_path = info.filename.split("/", 1)[-1]
                    if not rel_path or rel_path.endswith("/"):
                        continue
                    if not _should_copy_dump_member(rel_path):
                        continue
                    tracked.append(rel_path)

        for rel_path in tracked:
            target = os.path.normpath(os.path.join(game_dir, rel_path))
            if not _is_within_root(game_dir, target):
                continue
            bak_path = existing_backup_path(target)
            if bak_path and os.path.exists(bak_path):
                try:
                    if os.path.exists(target):
                        os.remove(target)
                    os.rename(bak_path, target)
                    restored += 1
                    continue
                except Exception as exc:  # noqa: BLE001
                    write_log(f"Failed to restore {rel_path}: {exc}", "Warning", log_widget)
            # If no backup exists, remove only if it's safe to do so.
            # CRITICAL: Do NOT delete the game executable if backup is missing!
            if os.path.exists(target):
                # Protect core files from deletion if backup is missing
                filename = os.path.basename(target)
                is_protected = (filename.lower() == "blackops3.exe")
                
                if is_protected:
                    write_log(f"Backup missing for {rel_path}; skipping deletion to preserve game executable.", "Warning", None)
                else:
                    try:
                        os.remove(target)
                        removed += 1
                    except Exception as exc:  # noqa: BLE001
                        write_log(f"Failed to remove {rel_path}: {exc}", "Warning", log_widget)
        if restored or removed:
            write_log(
                f"Dump-only uninstall completed. Restored {restored} files; removed {removed} files.",
                "Success",
                log_widget,
            )
            state["dump_only_files"] = []
            save_state(storage_dir, state)
            return True
        write_log("No dump files were restored or removed.", "Warning", log_widget)
        return False
    except Exception as exc:  # noqa: BLE001
        write_log(f"Failed to uninstall dump: {exc}", "Error", log_widget)
        return False


def uninstall_enhanced_files(game_dir: str, mod_files_dir: str, storage_dir: str, log_widget=None) -> bool:
    if not game_dir or not os.path.isdir(game_dir):
        write_log("Invalid game directory for Enhanced uninstall.", "Error", log_widget)
        return False

    try:
        if uninstall_dump_only(game_dir, mod_files_dir, storage_dir, log_widget):
            write_log("Dump files uninstalled.", "Success", log_widget)
    except Exception as exc:  # noqa: BLE001
        write_log(f"Failed to uninstall dump files: {exc}", "Warning", log_widget)

    # Reload state after dump uninstall, since uninstall_dump_only may mutate persisted keys.
    state = load_state(storage_dir)
    installed_files = state.get("installed_files", [])

    restored = 0
    removed = 0
    attempted = bool(installed_files)

    for rel_path in installed_files:
        target = os.path.join(game_dir, rel_path)
        backup_path = existing_backup_path(target)
        if backup_path and os.path.exists(backup_path):
            try:
                os.makedirs(os.path.dirname(target), exist_ok=True)
                if os.path.exists(target):
                    os.remove(target)
                os.rename(backup_path, target)
                restored += 1
                continue
            except Exception as exc:  # noqa: BLE001
                write_log(f"Failed to restore backup for {rel_path}: {exc}", "Warning", log_widget)
        
        # Backup missing. Remove tracked files, but protect the executable.
        # A normal first-time install has no backups for newly added dump files.
        if os.path.exists(target):
            filename = os.path.basename(rel_path)
            is_protected_executable = filename.lower() == "blackops3.exe"

            if not is_protected_executable:
                try:
                    os.remove(target)
                    removed += 1
                except Exception as exc:  # noqa: BLE001
                    write_log(f"Failed to remove {rel_path}: {exc}", "Warning", log_widget)
            else:
                write_log(f"Backup missing for {rel_path}; skipping deletion to preserve game file.", "Warning", None)

    # Fallback: if nothing was tracked, attempt a best-effort cleanup
    if not attempted and not restored and not removed:
        attempted = True
        for filename in EXPECTED_ENHANCED_FILES:
            target = os.path.join(game_dir, filename)
            backup_path = existing_backup_path(target)
            if backup_path and os.path.exists(backup_path):
                try:
                    if os.path.exists(target):
                        os.remove(target)
                    os.rename(backup_path, target)
                    restored += 1
                    continue
                except Exception as exc:  # noqa: BLE001
                    write_log(f"Failed to restore backup for {filename}: {exc}", "Warning", log_widget)
            if os.path.exists(target):
                try:
                    os.remove(target)
                    removed += 1
                except Exception as exc:  # noqa: BLE001
                    write_log(f"Failed to remove {filename}: {exc}", "Warning", log_widget)

    state["installed"] = False
    state["installed_files"] = []
    save_state(storage_dir, state)

    if restored or removed:
        write_log(
            f"Uninstall completed. Restored {restored} files from backup; removed {removed} files.",
            "Success",
            log_widget,
        )
        return True

    write_log("No BO3 Enhanced files were removed or restored (none tracked).", "Warning", log_widget)
    return attempted


def status_summary(game_dir: str, storage_dir: str) -> Dict[str, Optional[str]]:
    state = load_state(storage_dir)
    detected = detect_enhanced_install(game_dir)
    if not detected and not state.get("installed_files"):
        # If nothing is detected or tracked, clear the installed flag
        if state.get("installed"):
            clear_enhanced_state(storage_dir)
        detected = False
    else:
        detected = detected or bool(state.get("installed"))
    return {
        "installed": detected,
        "detected_at": state.get("detected_at"),
        "acknowledged_at": state.get("acknowledged_at"),
    }
