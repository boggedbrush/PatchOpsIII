#!/usr/bin/env python
import importlib.util
import os
import shutil
import sys
import tarfile
import zipfile

import requests
from urllib.parse import urlsplit
from PySide6.QtWidgets import QMessageBox, QWidget, QGroupBox, QHBoxLayout, QPushButton, QLabel, QVBoxLayout, QSizePolicy
from PySide6.QtCore import QEvent
from utils import write_log

# ---------- DXVK Helper Functions (unchanged) ----------

DXVK_ASYNC_FILES = ["dxgi.dll", "d3d11.dll"]

def get_latest_release():
    api_url = "https://gitlab.com/api/v4/projects/Ph42oN%2Fdxvk-gplasync/releases"
    r = requests.get(api_url)
    r.raise_for_status()
    releases = r.json()
    if not releases:
        raise RuntimeError("No releases returned from DXVK-GPLAsync API")
    return releases[0]  # Assumes releases are sorted latest first

def get_download_url(release):
    assets = release.get("assets", {})
    links = assets.get("links", [])
    if links:
        # Prefer archives we can extract natively before falling back to anything else
        preferred_order = (".zip", ".tar.xz", ".tar.gz", ".tar.bz2", ".tar.zst", ".tzst")
        for suffix in preferred_order:
            for link in links:
                url = link.get("url", "")
                if url.lower().endswith(suffix):
                    return url
        return links[0]["url"]
    sources = assets.get("sources", [])
    if sources:
        for source in sources:
            if source.get("format") == "zip":
                return source.get("url")
        return sources[0].get("url")
    raise RuntimeError("No downloadable asset found in DXVK-GPLAsync release metadata")


def _load_zstandard():
    if "zstandard" in sys.modules:
        return sys.modules["zstandard"]

    spec = importlib.util.find_spec("zstandard")
    if spec is None:
        raise ModuleNotFoundError(
            "The 'zstandard' package is required to unpack .tar.zst archives."
        )

    module = importlib.util.module_from_spec(spec)
    loader = spec.loader
    if loader is None:
        raise ImportError("Unable to load the 'zstandard' module")
    loader.exec_module(module)
    sys.modules["zstandard"] = module
    return module


def extract_archive(archive_path, extract_dir):
    lower_name = archive_path.lower()
    if lower_name.endswith(".zip"):
        with zipfile.ZipFile(archive_path, "r") as zip_ref:
            zip_ref.extractall(extract_dir)
        return

    if lower_name.endswith((".tar", ".tar.gz", ".tgz", ".tar.bz2", ".tbz2", ".tar.xz", ".txz")):
        with tarfile.open(archive_path, "r:*") as tar:
            tar.extractall(path=extract_dir)
        return

    if lower_name.endswith((".tar.zst", ".tzst")):
        zstandard = _load_zstandard()
        with open(archive_path, "rb") as compressed:
            dctx = zstandard.ZstdDecompressor()
            with dctx.stream_reader(compressed) as reader:
                with tarfile.open(fileobj=reader, mode="r|") as tar:
                    tar.extractall(path=extract_dir)
        return

    # Let shutil attempt to handle any other known formats
    shutil.unpack_archive(archive_path, extract_dir)

def download_file(url, filename):
    print(f"Downloading from {url}")
    with requests.get(url, stream=True) as r:
        r.raise_for_status()
        # Extract filename from URL
        parsed_url = urlsplit(url)
        original_filename = os.path.basename(parsed_url.path)
        
        # Use the original filename if available, otherwise use the provided filename
        if original_filename:
            final_filename = os.path.join(os.path.dirname(filename), original_filename)
        else:
            final_filename = filename

        with open(final_filename, "wb") as f:
            for chunk in r.iter_content(chunk_size=8192):
                if chunk:
                    f.write(chunk)
    print(f"Downloaded file saved as: {final_filename}")
    return final_filename  # Return the modified filename

def is_dxvk_async_installed(game_dir):
    return all(os.path.exists(os.path.join(game_dir, f)) for f in DXVK_ASYNC_FILES)

def manage_dxvk_async(game_dir, action, log_widget, mod_files_dir):
    if action == "Uninstall":
        dxvk_installed = all(os.path.exists(os.path.join(game_dir, f)) for f in DXVK_ASYNC_FILES)
        if dxvk_installed:
            write_log("DXVK-GPLAsync is detected. Uninstalling...", "Info", log_widget)
            for f in DXVK_ASYNC_FILES:
                path = os.path.join(game_dir, f)
                try:
                    if os.path.exists(path):
                        os.remove(path)
                        write_log(f"Removed '{f}'.", "Success", log_widget)
                except Exception as e:
                    write_log(f"Failed to remove '{f}': {str(e)}", "Error", log_widget)
            # Remove dxvk.conf if it exists
            conf_path = os.path.join(game_dir, "dxvk.conf")
            if os.path.exists(conf_path):
                try:
                    os.remove(conf_path)
                    write_log("Removed dxvk.conf.", "Success", log_widget)
                except Exception as e:
                    write_log(f"Failed to remove dxvk.conf: {str(e)}", "Error", log_widget)
            write_log("DXVK-GPLAsync has been uninstalled.", "Success", log_widget)
        else:
            write_log("DXVK-GPLAsync is not installed.", "Info", log_widget)
    elif action == "Install":
        dxvk_installed = all(os.path.exists(os.path.join(game_dir, f)) for f in DXVK_ASYNC_FILES)
        if dxvk_installed:
            write_log("DXVK-GPLAsync is already installed.", "Info", log_widget)
            return
        write_log("DXVK-GPLAsync can reduce stuttering by using async shader compilation.", "Info", log_widget)
        if QMessageBox.question(None, "Install DXVK-GPLAsync", "Do you want to install DXVK-GPLAsync?",
                              QMessageBox.Yes | QMessageBox.No) == QMessageBox.Yes:
            try:
                x64_dir = None
                release = get_latest_release()
                write_log("Latest release: " + release.get("name", release.get("tag_name", "Unknown")), "Info", log_widget)
                dxvk_url = get_download_url(release)
                dxvk_archive = download_file(dxvk_url, os.path.join(mod_files_dir, "dxvk-gplasync"))
                write_log("Downloaded DXVK-GPLAsync successfully.", "Success", log_widget)

                extract_dir = os.path.join(mod_files_dir, "dxvk_extracted")
                if os.path.exists(extract_dir):
                    shutil.rmtree(extract_dir)
                os.makedirs(extract_dir, exist_ok=True)

                # Extract files based on archive type
                try:
                    extract_archive(dxvk_archive, extract_dir)
                    write_log("Extracted DXVK-GPLAsync successfully.", "Success", log_widget)
                except Exception as e:
                    write_log(f"Failed to extract DXVK-GPLAsync: {str(e)}", "Error", log_widget)
                    return

                # Look for the directory containing DXVK files recursively
                x64_dir = None
                for root, dirs, files in os.walk(extract_dir):
                    if all(f in files for f in DXVK_ASYNC_FILES):
                        x64_dir = root
                        break

                if not x64_dir:
                    write_log("Required DXVK files (dxgi.dll, d3d11.dll) not found in extracted DXVK-GPLAsync directory.", "Error", log_widget)
                    return

                # Install DXVK files
                for file in DXVK_ASYNC_FILES:
                    src = os.path.join(x64_dir, file)
                    dst = os.path.join(game_dir, file)
                    try:
                        if os.path.exists(src):
                            shutil.copy2(src, dst)
                            write_log(f"Installed {file}.", "Success", log_widget)
                        else:
                            write_log(f"Source file {file} not found.", "Error", log_widget)
                            return
                    except Exception as e:
                        write_log(f"Failed to install {file}: {str(e)}", "Error", log_widget)
                        return

                # Write dxvk.conf
                try:
                    conf_path = os.path.join(game_dir, "dxvk.conf")
                    with open(conf_path, "w") as conf_file:
                        conf_file.write("dxvk.enableAsync=true\n")
                        conf_file.write("dxvk.gplAsyncCache=true\n")
                    write_log("Created dxvk.conf with async settings.", "Success", log_widget)
                except Exception as e:
                    write_log(f"Failed to create dxvk.conf: {str(e)}", "Error", log_widget)
                    return

                write_log("DXVK-GPLAsync installed successfully.", "Success", log_widget)

            except Exception as e:
                write_log(f"Error during DXVK-GPLAsync installation: {str(e)}", "Error", log_widget)
            finally:
                # Clean up temporary files
                try:
                    if os.path.exists(dxvk_archive):
                        os.remove(dxvk_archive)
                    if os.path.exists(extract_dir):
                        shutil.rmtree(extract_dir)
                except Exception as e:
                    write_log(f"Warning: Could not clean up temporary files: {str(e)}", "Warning", log_widget)
        else:
            write_log("DXVK-GPLAsync installation canceled by user.", "Info", log_widget)

# ---------- DXVK GUI Widget ----------

class DXVKWidget(QWidget):
    def __init__(self, mod_files_dir, parent=None):
        super().__init__(parent)
        self.mod_files_dir = mod_files_dir
        self.game_dir = None
        self.log_widget = None
        self.group = QGroupBox("DXVK-GPLAsync Management")
        self.group.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)
        self.init_ui()
        self.update_theme()

    @property
    def groupbox(self):
        return self.group

    def init_ui(self):
        self.group = QGroupBox("DXVK-GPLAsync Management")
        layout = QHBoxLayout(self.group)
        # Match T7 Patch margins/spacing:
        layout.setContentsMargins(5, 5, 5, 5)
        layout.setSpacing(5)
        self.install_btn = QPushButton("Install DXVK-GPLAsync")
        self.install_btn.clicked.connect(lambda: self.manage_dxvk("Install"))
        self.uninstall_btn = QPushButton("Uninstall DXVK-GPLAsync")
        self.uninstall_btn.clicked.connect(lambda: self.manage_dxvk("Uninstall"))
        self.status_label = QLabel("")
        layout.addWidget(self.install_btn)
        layout.addWidget(self.uninstall_btn)
        layout.addWidget(self.status_label)
        main_layout = QVBoxLayout(self)
        main_layout.addWidget(self.group)

    def update_theme(self):
        is_dark = self.palette().window().color().lightness() < 128
        if (is_dark):
            control_color = "#2D2D30"
            fore_color = "#FFFFFF"
        else:
            control_color = "#F0F0F0"
            fore_color = "#000000"

        # Update button styles
        for btn in [self.install_btn, self.uninstall_btn]:
            btn.setStyleSheet(f"background-color: {control_color}; color: {fore_color};")

        # Update label style
        self.status_label.setStyleSheet(f"color: {fore_color};")

    def changeEvent(self, event):
        if event.type() == QEvent.Type.PaletteChange:
            self.update_theme()
        super().changeEvent(event)

    def set_game_directory(self, game_dir):
        self.game_dir = game_dir
        self.update_status()

    def set_log_widget(self, log_widget):
        self.log_widget = log_widget

    def update_status(self):
        if self.game_dir and os.path.exists(self.game_dir):
            if is_dxvk_async_installed(self.game_dir):
                self.status_label.setText("DXVK-GPLAsync: Installed")
            else:
                self.status_label.setText("DXVK-GPLAsync: Not Installed")
        else:
            self.status_label.setText("Game directory not set")

    def manage_dxvk(self, action):
        if not self.game_dir or not os.path.exists(self.game_dir):
            write_log("Game directory does not exist.", "Error", self.log_widget)
            return
        manage_dxvk_async(self.game_dir, action, self.log_widget, self.mod_files_dir)
        self.update_status()

