#!/usr/bin/env python
import importlib.util
import os
import re
import shutil
import sys
import tarfile
import zipfile

import requests
from urllib.parse import urlsplit
from PySide6.QtWidgets import (
    QMessageBox,
    QWidget,
    QGroupBox,
    QHBoxLayout,
    QPushButton,
    QLabel,
    QVBoxLayout,
    QSizePolicy,
    QCheckBox,
    QComboBox,
    QSpinBox,
    QSlider,
    QGridLayout,
    QFrame,
)
from PySide6.QtCore import Qt
from utils import write_log

# ---------- DXVK Helper Functions (unchanged) ----------

DXVK_ASYNC_FILES = ["dxgi.dll", "d3d11.dll"]


def _supports_gpl_async_cache(release):
    """dxvk.gplAsyncCache was removed in gplasync 2.7+."""
    tag = (release or {}).get("tag_name") or (release or {}).get("name") or ""
    match = re.search(r"(\d+)\.(\d+)", tag)
    if not match:
        return True
    major = int(match.group(1))
    minor = int(match.group(2))
    return (major, minor) < (2, 7)


def _preset_settings(preset):
    if preset == "none":
        return {
            "enable_async": True,
            "gpl_async_cache": False,
            "num_compiler_threads": 0,
            "max_frame_rate": 0,
            "max_frame_latency": 0,
            "tear_free": "Auto",
            "hud_enabled": False,
        }
    return {
        "enable_async": True,
        "gpl_async_cache": True,
        "num_compiler_threads": 0,
        "max_frame_rate": 0,
        "max_frame_latency": 1,
        "tear_free": "True",
        "hud_enabled": False,
    }


def _build_dxvk_conf(settings, include_gpl_async_cache=True):
    lines = []
    lines.append(f"dxvk.enableAsync={'true' if settings.get('enable_async', True) else 'false'}")
    if include_gpl_async_cache and settings.get("gpl_async_cache", False):
        lines.append("dxvk.gplAsyncCache=true")
    lines.append(f"dxvk.numCompilerThreads={settings.get('num_compiler_threads', 0)}")
    lines.append(f"dxgi.maxFrameRate={settings.get('max_frame_rate', 0)}")
    lines.append(f"dxgi.maxFrameLatency={settings.get('max_frame_latency', 0)}")
    lines.append(f"dxvk.tearFree={settings.get('tear_free', 'Auto')}")
    if settings.get("hud_enabled", False):
        lines.append("dxvk.hud=fps,frametimes,gpuload")
    return "\n".join(lines) + "\n"


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

def manage_dxvk_async(game_dir, action, log_widget, mod_files_dir, dxvk_settings=None):
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
                    include_gpl_async_cache = _supports_gpl_async_cache(release)
                    selected_settings = dxvk_settings or _preset_settings("recommended")
                    conf_contents = _build_dxvk_conf(
                        selected_settings,
                        include_gpl_async_cache=include_gpl_async_cache,
                    )
                    with open(conf_path, "w") as conf_file:
                        conf_file.write(conf_contents)
                    if selected_settings.get("gpl_async_cache", False) and not include_gpl_async_cache:
                        write_log("dxvk.gplAsyncCache was requested but skipped because gplasync v2.7+ no longer supports it.", "Info", log_widget)
                    write_log("Created dxvk.conf from DXVK tab settings.", "Success", log_widget)
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
        self.group = QGroupBox("")
        self.group.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)
        self.init_ui()

    @property
    def groupbox(self):
        return self.group

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

    def init_ui(self):
        self.group = QGroupBox("")
        self.group.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)
        layout = QGridLayout(self.group)
        layout.setContentsMargins(14, 14, 14, 14)
        layout.setHorizontalSpacing(10)
        layout.setVerticalSpacing(0)
        row = 0

        # Action buttons
        btn_row = QWidget()
        btn_layout = QHBoxLayout(btn_row)
        btn_layout.setContentsMargins(0, 0, 0, 12)
        btn_layout.setSpacing(10)

        self.install_btn = QPushButton("Install DXVK-GPLAsync")
        self.install_btn.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
        self.install_btn.clicked.connect(lambda: self.manage_dxvk("Install"))
        btn_layout.addWidget(self.install_btn, 1)

        self.uninstall_btn = QPushButton("Uninstall DXVK-GPLAsync")
        self.uninstall_btn.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
        self.uninstall_btn.clicked.connect(lambda: self.manage_dxvk("Uninstall"))
        btn_layout.addWidget(self.uninstall_btn, 1)

        layout.addWidget(btn_row, row, 0, 1, 4)
        row += 1

        # Status row
        self._add_separator(layout, row); row += 1

        status_name = QLabel("Status")
        status_name.setObjectName("DashboardStatusName")
        status_name.setContentsMargins(0, 8, 0, 8)

        self.status_label = QLabel(self._status_html("Unknown"))
        self.status_label.setObjectName("DashboardStatusValue")
        self.status_label.setAlignment(Qt.AlignRight | Qt.AlignVCenter)
        self.status_label.setTextFormat(Qt.RichText)
        self.status_label.setContentsMargins(0, 8, 0, 8)

        layout.addWidget(status_name, row, 0)
        layout.addWidget(self.status_label, row, 1, 1, 3)
        row += 1

        # Preset row
        self._add_separator(layout, row); row += 1

        preset_name = QLabel("Preset")
        preset_name.setObjectName("DashboardStatusName")
        preset_name.setContentsMargins(0, 8, 0, 8)

        self.preset_combo = QComboBox()
        self.preset_combo.addItems(["Recommended", "None"])

        self.apply_preset_btn = QPushButton("Apply")
        self.apply_preset_btn.clicked.connect(self.apply_selected_preset)

        layout.addWidget(preset_name, row, 0)
        layout.addWidget(self.preset_combo, row, 1, 1, 2)
        layout.addWidget(self.apply_preset_btn, row, 3)
        row += 1

        # Checkboxes row
        self._add_separator(layout, row); row += 1

        cb_widget = QWidget()
        cb_layout = QHBoxLayout(cb_widget)
        cb_layout.setContentsMargins(0, 8, 0, 8)
        cb_layout.setSpacing(16)

        self.enable_async_checkbox = QCheckBox("Async shader compilation")
        self.gpl_async_cache_checkbox = QCheckBox("GPL async cache")
        self.hud_checkbox = QCheckBox("FPS/GPU HUD")
        cb_layout.addWidget(self.enable_async_checkbox)
        cb_layout.addWidget(self.gpl_async_cache_checkbox)
        cb_layout.addWidget(self.hud_checkbox)
        cb_layout.addStretch()

        layout.addWidget(cb_widget, row, 0, 1, 4)
        row += 1

        # Compiler Threads
        self._add_separator(layout, row); row += 1

        ct_name = QLabel("Compiler Threads")
        ct_name.setObjectName("DashboardStatusName")
        ct_name.setContentsMargins(0, 8, 0, 8)

        self.compiler_threads_spin = QSpinBox()
        self.compiler_threads_spin.setRange(0, 64)

        layout.addWidget(ct_name, row, 0)
        layout.addWidget(self.compiler_threads_spin, row, 1, 1, 2)
        row += 1

        # Frame Rate Cap
        self._add_separator(layout, row); row += 1

        frc_name = QLabel("Frame Rate Cap")
        frc_name.setObjectName("DashboardStatusName")
        frc_name.setContentsMargins(0, 8, 0, 8)

        fps_container = QWidget()
        fps_layout = QHBoxLayout(fps_container)
        fps_layout.setContentsMargins(0, 0, 0, 0)
        fps_layout.setSpacing(8)

        self.max_fps_slider = QSlider(Qt.Orientation.Horizontal)
        self.max_fps_slider.setRange(0, 360)
        self.max_fps_slider.setSingleStep(1)
        self.max_fps_slider.setPageStep(5)
        fps_layout.addWidget(self.max_fps_slider)

        self.max_fps_label = QLabel("0")
        self.max_fps_label.setObjectName("DashboardStatusValue")
        self.max_fps_label.setFixedWidth(36)
        fps_layout.addWidget(self.max_fps_label)

        layout.addWidget(frc_name, row, 0)
        layout.addWidget(fps_container, row, 1, 1, 3)
        row += 1

        # Frame Latency
        self._add_separator(layout, row); row += 1

        fl_name = QLabel("Frame Latency")
        fl_name.setObjectName("DashboardStatusName")
        fl_name.setContentsMargins(0, 8, 0, 8)

        self.frame_latency_spin = QSpinBox()
        self.frame_latency_spin.setRange(0, 16)

        layout.addWidget(fl_name, row, 0)
        layout.addWidget(self.frame_latency_spin, row, 1, 1, 2)
        row += 1

        # Tear Free
        self._add_separator(layout, row); row += 1

        tf_name = QLabel("Tear Free")
        tf_name.setObjectName("DashboardStatusName")
        tf_name.setContentsMargins(0, 8, 0, 8)

        self.tear_free_combo = QComboBox()
        self.tear_free_combo.addItems(["Auto", "True", "False"])

        layout.addWidget(tf_name, row, 0)
        layout.addWidget(self.tear_free_combo, row, 1, 1, 2)
        row += 1

        # Notes
        self._add_separator(layout, row); row += 1

        self.notes_label = QLabel("Note: dxvk.gplAsyncCache is skipped automatically on gplasync v2.7+.")
        self.notes_label.setObjectName("DashboardStatusName")
        self.notes_label.setWordWrap(True)
        self.notes_label.setContentsMargins(0, 8, 0, 8)
        layout.addWidget(self.notes_label, row, 0, 1, 4)
        row += 1

        layout.setColumnStretch(0, 1)
        layout.setColumnStretch(1, 1)
        layout.setColumnStretch(2, 2)
        layout.setColumnStretch(3, 0)
        layout.setRowStretch(row, 1)

        self.preset_combo.setCurrentText("Recommended")
        self._apply_preset("recommended")
        self.enable_async_checkbox.stateChanged.connect(self._update_conf_preview)
        self.gpl_async_cache_checkbox.stateChanged.connect(self._update_conf_preview)
        self.compiler_threads_spin.valueChanged.connect(self._update_conf_preview)
        self.max_fps_slider.valueChanged.connect(self._on_fps_changed)
        self.frame_latency_spin.valueChanged.connect(self._update_conf_preview)
        self.tear_free_combo.currentTextChanged.connect(self._update_conf_preview)
        self.hud_checkbox.stateChanged.connect(self._update_conf_preview)

        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(0, 0, 0, 0)
        main_layout.setSpacing(0)
        main_layout.addWidget(self.group)
        self.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)

    def apply_selected_preset(self):
        preset_text = self.preset_combo.currentText()
        self._apply_preset("recommended" if preset_text == "Recommended" else "none")

    def _apply_preset(self, preset):
        settings = _preset_settings(preset)
        self.enable_async_checkbox.setChecked(settings["enable_async"])
        self.gpl_async_cache_checkbox.setChecked(settings["gpl_async_cache"])
        self.compiler_threads_spin.setValue(settings["num_compiler_threads"])
        self.max_fps_slider.setValue(settings["max_frame_rate"])
        self.max_fps_label.setText(str(settings["max_frame_rate"]))
        self.frame_latency_spin.setValue(settings["max_frame_latency"])
        index = self.tear_free_combo.findText(settings["tear_free"])
        if index >= 0:
            self.tear_free_combo.setCurrentIndex(index)
        self.hud_checkbox.setChecked(settings["hud_enabled"])
        self._update_conf_preview()

    def _on_fps_changed(self, value):
        self.max_fps_label.setText(str(value))
        self._update_conf_preview()

    def _current_settings(self):
        return {
            "enable_async": self.enable_async_checkbox.isChecked(),
            "gpl_async_cache": self.gpl_async_cache_checkbox.isChecked(),
            "num_compiler_threads": self.compiler_threads_spin.value(),
            "max_frame_rate": self.max_fps_slider.value(),
            "max_frame_latency": self.frame_latency_spin.value(),
            "tear_free": self.tear_free_combo.currentText(),
            "hud_enabled": self.hud_checkbox.isChecked(),
        }

    def _update_conf_preview(self):
        return

    def set_game_directory(self, game_dir):
        self.game_dir = game_dir
        self.update_status()

    def set_log_widget(self, log_widget):
        self.log_widget = log_widget

    def update_status(self):
        if self.game_dir and os.path.exists(self.game_dir):
            if is_dxvk_async_installed(self.game_dir):
                self.status_label.setText(self._status_html("Installed", "good"))
            else:
                self.status_label.setText(self._status_html("Not Installed", "bad"))
        else:
            self.status_label.setText(self._status_html("Game directory not set", "neutral"))

    def manage_dxvk(self, action):
        if not self.game_dir or not os.path.exists(self.game_dir):
            write_log("Game directory does not exist.", "Error", self.log_widget)
            return
        manage_dxvk_async(
            self.game_dir,
            action,
            self.log_widget,
            self.mod_files_dir,
            dxvk_settings=self._current_settings(),
        )
        self.update_status()
