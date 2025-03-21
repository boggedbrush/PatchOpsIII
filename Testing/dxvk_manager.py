#!/usr/bin/env python
import os, shutil, tarfile, requests, sys
from urllib.parse import urlsplit
from PySide6.QtWidgets import QMessageBox, QWidget, QGroupBox, QHBoxLayout, QPushButton, QLabel, QVBoxLayout, QSizePolicy
from PySide6.QtCore import Qt, QEvent
from utils import write_log

# ---------- DXVK Helper Functions (unchanged) ----------

DXVK_ASYNC_FILES = ["dxgi.dll", "d3d11.dll"]

def get_latest_release():
    api_url = "https://gitlab.com/api/v4/projects/Ph42oN%2Fdxvk-gplasync/releases"
    r = requests.get(api_url)
    r.raise_for_status()
    releases = r.json()
    if not releases:
        sys.exit("No releases found!")
    return releases[0]  # Assumes releases are sorted latest first

def get_download_url(release):
    assets = release.get("assets", {})
    links = assets.get("links", [])
    if links:
        return links[0]["url"]
    sources = assets.get("sources", [])
    if sources:
        for source in sources:
            if source.get("format") == "zip":
                return source.get("url")
        return sources[0].get("url")
    sys.exit("No downloadable asset found!")

def download_file(url, filename):
    print(f"Downloading from {url}")
    with requests.get(url, stream=True) as r:
        r.raise_for_status()
        # Check content type header to determine file type
        content_type = r.headers.get('content-type', '')
        if 'zip' in content_type:
            filename = filename + '.zip'
        elif 'gzip' in content_type:
            filename = filename + '.tar.gz'
        
        with open(filename, "wb") as f:
            for chunk in r.iter_content(chunk_size=8192):
                if chunk:
                    f.write(chunk)
    print(f"Downloaded file saved as: {filename}")
    return filename  # Return the modified filename

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
                    os.remove(path)
                    write_log(f"Removed '{f}'.", "Success", log_widget)
                except Exception:
                    write_log(f"Failed to remove '{f}'.", "Error", log_widget)
            # Remove dxvk.conf if it exists
            conf_path = os.path.join(game_dir, "dxvk.conf")
            if os.path.exists(conf_path):
                try:
                    os.remove(conf_path)
                    write_log("Removed dxvk.conf.", "Success", log_widget)
                except Exception:
                    write_log("Failed to remove dxvk.conf.", "Error", log_widget)
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
                release = get_latest_release()
                write_log("Latest release: " + release.get("name", release.get("tag_name", "Unknown")), "Info", log_widget)
                dxvk_url = get_download_url(release)
            except Exception as e:
                write_log("Failed to fetch latest DXVK-GPLAsync release info.", "Error", log_widget)
                return
            try:
                dxvk_archive = download_file(dxvk_url, os.path.join(mod_files_dir, "dxvk-gplasync"))
                write_log("Downloaded DXVK-GPLAsync successfully.", "Success", log_widget)
            except Exception as e:
                write_log(f"Failed to download DXVK-GPLAsync: {str(e)}", "Error", log_widget)
                return

            extract_dir = os.path.join(mod_files_dir, "dxvk_extracted")
            if os.path.exists(extract_dir):
                try:
                    shutil.rmtree(extract_dir)
                except Exception:
                    write_log("Failed to remove existing extraction directory.", "Error", log_widget)
                    return

            os.makedirs(extract_dir, exist_ok=True)
            
            try:
                if dxvk_archive.endswith('.tar.gz'):
                    with tarfile.open(dxvk_archive, "r:gz") as tar:
                        tar.extractall(path=extract_dir)
                elif dxvk_archive.endswith('.zip'):
                    import zipfile
                    with zipfile.ZipFile(dxvk_archive, 'r') as zip_ref:
                        zip_ref.extractall(extract_dir)
                else:
                    write_log(f"Unsupported archive format: {dxvk_archive}", "Error", log_widget)
                    return
                    
                write_log("Extracted DXVK-GPLAsync successfully.", "Success", log_widget)
            except Exception as e:
                write_log(f"Failed to extract DXVK-GPLAsync: {str(e)}", "Error", log_widget)
                return

            # Look for x64 directory recursively
            x64_dir = None
            for root, dirs, files in os.walk(extract_dir):
                if 'x64' in dirs:
                    x64_dir = os.path.join(root, 'x64')
                    break

            if not x64_dir:
                write_log("'x64' folder not found in extracted DXVK-GPLAsync directory.", "Error", log_widget)
                return
            try:
                shutil.copy(os.path.join(x64_dir, "dxgi.dll"), game_dir)
                shutil.copy(os.path.join(x64_dir, "d3d11.dll"), game_dir)
                # Write dxvk.conf with async and GPL async cache enabled
                conf_path = os.path.join(game_dir, "dxvk.conf")
                with open(conf_path, "w") as conf_file:
                    conf_file.write("dxvk.enableAsync=true\n")
                    conf_file.write("dxvk.gplAsyncCache=true\n")
                write_log("DXVK-GPLAsync installed successfully with dxvk.conf configured.", "Success", log_widget)
            except Exception:
                write_log("Error installing DXVK-GPLAsync. Antivirus or permissions may be blocking files.", "Error", log_widget)
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

