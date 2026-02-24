#!/usr/bin/env python
import os
import re
import json
import stat
import platform
import shutil
from typing import Optional
from PySide6.QtWidgets import (
    QMessageBox, QWidget, QGroupBox, QVBoxLayout, QHBoxLayout, QLabel,
    QComboBox, QPushButton, QFormLayout, QCheckBox, QSpinBox, QLineEdit, QSlider,
    QSizePolicy, QTabWidget, QGridLayout, QFrame
)
from PySide6.QtGui import QIntValidator, QGuiApplication
from PySide6.QtCore import Qt, QTimer, Signal
from version import APP_VERSION
from utils import (
    write_log,
    get_log_file_path,
    clear_log_file,
    patchops_backup_path,
    existing_backup_path,
)

def set_config_value(game_dir, key, value, comment, log_widget):
    config_path = os.path.join(game_dir, "players", "config.ini")
    pattern = rf'^\s*{re.escape(key)}\s*='
    replacement = f'{key} = "{value}" // {comment}'
    try:
        update_config_values(
            config_path,
            {pattern: replacement},
            f"Set {key} to {value}.",
            log_widget
        )
    except Exception as e:
        write_log(f"Error setting {key}: {e}", "Error", log_widget)

def update_config_values(config_path, changes, success_message, log_widget, suppress_output=False):
    if os.path.exists(config_path):
        try:
            with open(config_path, "r") as f:
                lines = f.readlines()
            new_lines = []
            for line in lines:
                replaced = False
                for pattern, replacement in changes.items():
                    if re.search(pattern, line):
                        new_lines.append(replacement + "\n")
                        replaced = True
                        break
                if not replaced:
                    new_lines.append(line)
            with open(config_path, "w") as f:
                f.writelines(new_lines)
            if not suppress_output:
                write_log(success_message, "Success", log_widget)
        except PermissionError:
            QMessageBox.critical(
                None, "Permission Error",
                f"Cannot write to {config_path}.\nPlease run as administrator."
            )
        except Exception as e:
            write_log(f"Error updating config: {e}", "Error", log_widget)
    else:
        write_log(f"config.ini not found at {config_path}.", "Error", log_widget)

def toggle_stuttering_setting(game_dir, reduce_stutter, log_widget):
    dll_file = os.path.join(game_dir, "d3dcompiler_46.dll")
    dll_bak = patchops_backup_path(dll_file)
    if reduce_stutter:
        if os.path.exists(dll_file):
            try:
                os.rename(dll_file, dll_bak)
                write_log("Renamed d3dcompiler_46.dll to reduce stuttering.", "Success", log_widget)
            except Exception:
                write_log("Failed to rename d3dcompiler_46.dll.", "Error", log_widget)
        elif existing_backup_path(dll_file):
            write_log("Stutter reduction already enabled.", "Success", log_widget)
        else:
            write_log("d3dcompiler_46.dll not found.", "Warning", log_widget)
    else:
        backup_path = existing_backup_path(dll_file)
        if backup_path:
            try:
                os.rename(backup_path, dll_file)
                write_log("Restored d3dcompiler_46.dll.", "Success", log_widget)
            except Exception:
                write_log("Failed to restore d3dcompiler_46.dll.", "Error", log_widget)
        else:
            write_log("Backup not found to restore.", "Warning", log_widget)

def set_config_readonly(game_dir, read_only, log_widget):
    config_path = os.path.join(game_dir, "players", "config.ini")
    if os.path.exists(config_path):
        try:
            if read_only:
                os.chmod(config_path, stat.S_IREAD)
                write_log("config.ini set to read-only.", "Success", log_widget)
            else:
                os.chmod(config_path, stat.S_IWRITE | stat.S_IREAD)
                write_log("config.ini set to writable.", "Success", log_widget)
        except Exception as e:
            write_log(f"Failed to change config.ini permissions: {e}", "Error", log_widget)

def load_presets_from_json(json_path):
    if not os.path.exists(json_path):
        return None
    try:
        with open(json_path, "r") as f:
            return json.load(f)
    except Exception:
        return None

def apply_preset(game_dir, preset_name, log_widget, presets_dict):
    config_path = os.path.join(game_dir, "players", "config.ini")
    preset = presets_dict.get(preset_name)
    if preset is None:
        write_log(f"Preset {preset_name} not found.", "Error", log_widget)
        return
    changes = {}
    for setting, (value, comment) in preset.items():
        if setting == "ReduceStutter":
            toggle_stuttering_setting(game_dir, True, log_widget)
        else:
            pattern = r'^\s*' + re.escape(setting) + r'\s*='
            replacement = f'{setting} = "{value}" // {comment}'
            changes[pattern] = replacement
    if "BackbufferCount" in preset and preset["BackbufferCount"][0] == "3":
        pattern = r'^\s*Vsync\s*='
        changes[pattern] = 'Vsync = "1" // Enabled with triple-buffered V-sync'
    update_config_values(
        config_path,
        changes,
        f"Applied preset '{preset_name}'.",
        log_widget
    )

def check_essential_status(game_dir):
    status = {}
    config_path = os.path.join(game_dir, "players", "config.ini")
    if os.path.exists(config_path):
        with open(config_path, "r") as f:
            content = f.read()
        match = re.search(r'MaxFPS\s*=\s*"([^"]+)"', content)
        status["max_fps"] = int(match.group(1)) if match else 165

        match = re.search(r'FOV\s*=\s*"([^"]+)"', content)
        status["fov"] = int(match.group(1)) if match else 80

        match = re.search(r'FullScreenMode\s*=\s*"([^"]+)"', content)
        status["display_mode"] = int(match.group(1)) if match else 1

        match = re.search(r'WindowSize\s*=\s*"([^"]+)"', content)
        status["resolution"] = match.group(1) if match else "2560x1440"

        match = re.search(r'RefreshRate\s*=\s*"([^"]+)"', content)
        status["refresh_rate"] = float(match.group(1)) if match else 165

        match = re.search(r'Vsync\s*=\s*"([^"]+)"', content)
        status["vsync"] = (match.group(1) == "1") if match else True

        match = re.search(r'DrawFPS\s*=\s*"([^"]+)"', content)
        status["draw_fps"] = (match.group(1) == "1") if match else False

        status["all_settings"] = bool(re.search(r'RestrictGraphicsOptions\s*=\s*"0"', content))
        status["smooth"] = bool(re.search(r'SmoothFramerate\s*=\s*"1"', content))
        
        # Update VRAM status check logic
        video_memory_match = re.search(r'VideoMemory\s*=\s*"([^"]+)"', content)
        stream_resident_match = re.search(r'StreamMinResident\s*=\s*"([^"]+)"', content)
        
        has_full_vram = (video_memory_match and video_memory_match.group(1) == "1" and
                        stream_resident_match and stream_resident_match.group(1) == "0")
        
        status["vram"] = not has_full_vram
        if video_memory_match and not has_full_vram:
            status["vram_value"] = float(video_memory_match.group(1))
        else:
            status["vram_value"] = 0.75  # Default value when not set

        match = re.search(r'MaxFrameLatency\s*=\s*"(\d)"', content)
        status["latency"] = int(match.group(1)) if match else 1

        status["reduce_cpu"] = bool(re.search(r'SerializeRender\s*=\s*"2"', content))

        video_dir = os.path.join(game_dir, "video")
        intro_file = os.path.join(video_dir, "BO3_Global_Logo_LogoSequence.mkv")
        status["skip_intro"] = existing_backup_path(intro_file) is not None
    else:
        status = {
            "max_fps": 60,
            "fov": 80,
            "display_mode": 1,
            "resolution": "1920x1080",
            "refresh_rate": 60,
            "vsync": True,
            "draw_fps": False,
            "all_settings": False,
            "smooth": False,
            "vram": False,
            "vram_value": 0.75,
            "latency": 1,
            "reduce_cpu": False,
            "skip_intro": False,
        }
    return status

class GraphicsSettingsWidget(QWidget):
    def __init__(self, dxvk_widget=None, parent=None):
        super().__init__(parent)
        self.dxvk_widget = dxvk_widget  # Reference to the DXVK widget
        self.game_dir = None
        self.log_widget = None
        self.preset_dict = {}
        self.init_ui()

        self._pending_fov_value = None
        self._last_applied_fov = None
        self._fov_update_timer = QTimer(self)
        self._fov_update_timer.setSingleShot(True)
        self._fov_update_timer.setInterval(250)
        self._fov_update_timer.timeout.connect(self._commit_pending_fov_value)

    def _add_separator(self, layout, row):
        sep = QFrame()
        sep.setObjectName("DashboardDivider")
        sep.setFrameShape(QFrame.HLine)
        sep.setFixedHeight(1)
        layout.addWidget(sep, row, 0, 1, 4)

    def init_ui(self):
        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(0, 0, 0, 0)
        main_layout.setSpacing(0)

        graphics_group = QGroupBox("")
        layout = QGridLayout(graphics_group)
        layout.setContentsMargins(14, 14, 14, 14)
        layout.setHorizontalSpacing(10)
        layout.setVerticalSpacing(0)
        row = 0

        # Preset row
        preset_lbl = QLabel("Preset")
        preset_lbl.setObjectName("DashboardStatusName")
        preset_lbl.setContentsMargins(0, 8, 0, 8)

        self.preset_combo = QComboBox()
        self.preset_combo.addItems(["Quality", "Balanced", "Performance", "Ultra Performance", "Custom"])

        self.apply_preset_btn = QPushButton("Apply")
        self.apply_preset_btn.clicked.connect(self.apply_preset_clicked)

        layout.addWidget(preset_lbl, row, 0)
        layout.addWidget(self.preset_combo, row, 1, 1, 2)
        layout.addWidget(self.apply_preset_btn, row, 3)
        row += 1

        # Display Mode
        self._add_separator(layout, row); row += 1

        dm_lbl = QLabel("Display Mode")
        dm_lbl.setObjectName("DashboardStatusName")
        dm_lbl.setContentsMargins(0, 8, 0, 8)

        self.display_mode_combo = QComboBox()
        self.display_mode_combo.addItems(["Windowed", "Fullscreen", "Fullscreen Windowed"])
        self.display_mode_combo.currentIndexChanged.connect(self.display_mode_changed)

        layout.addWidget(dm_lbl, row, 0)
        layout.addWidget(self.display_mode_combo, row, 1, 1, 3)
        row += 1

        # Resolution
        self._add_separator(layout, row); row += 1

        res_lbl = QLabel("Resolution")
        res_lbl.setObjectName("DashboardStatusName")
        res_lbl.setContentsMargins(0, 8, 0, 8)

        self.resolution_edit = QLineEdit("2560x1440")
        self.resolution_edit.editingFinished.connect(self.resolution_changed)

        layout.addWidget(res_lbl, row, 0)
        layout.addWidget(self.resolution_edit, row, 1, 1, 3)
        row += 1

        # Refresh Rate
        self._add_separator(layout, row); row += 1

        rr_lbl = QLabel("Refresh Rate")
        rr_lbl.setObjectName("DashboardStatusName")
        rr_lbl.setContentsMargins(0, 8, 0, 8)

        self.refresh_rate_spin = QSpinBox()
        self.refresh_rate_spin.setRange(1, 240)
        self.refresh_rate_spin.setValue(165)
        self.refresh_rate_spin.valueChanged.connect(self.refresh_rate_changed)

        layout.addWidget(rr_lbl, row, 0)
        layout.addWidget(self.refresh_rate_spin, row, 1, 1, 2)
        row += 1

        # FPS Limiter
        self._add_separator(layout, row); row += 1

        fps_lbl = QLabel("FPS Limiter")
        fps_lbl.setObjectName("DashboardStatusName")
        fps_lbl.setContentsMargins(0, 8, 0, 8)

        self.fps_limiter_spin = QSpinBox()
        self.fps_limiter_spin.setRange(0, 1000)
        self.fps_limiter_spin.setValue(165)
        self.fps_limiter_spin.valueChanged.connect(self.fps_limiter_changed)

        layout.addWidget(fps_lbl, row, 0)
        layout.addWidget(self.fps_limiter_spin, row, 1, 1, 2)
        row += 1

        # FOV
        self._add_separator(layout, row); row += 1

        fov_lbl = QLabel("FOV")
        fov_lbl.setObjectName("DashboardStatusName")
        fov_lbl.setContentsMargins(0, 8, 0, 8)

        self.fov_slider = QSlider(Qt.Horizontal)
        self.fov_slider.setRange(65, 120)
        self.fov_slider.setTickInterval(5)
        self.fov_slider.setSingleStep(1)
        self.fov_slider.setValue(80)
        self.fov_slider.valueChanged.connect(self.on_fov_slider_changed)

        self.fov_input = QLineEdit("80")
        self.fov_input.setValidator(QIntValidator(65, 120, self.fov_input))
        self.fov_input.setFixedWidth(60)
        self.fov_input.editingFinished.connect(self.on_fov_input_edited)

        fov_container = QWidget()
        fov_layout = QHBoxLayout(fov_container)
        fov_layout.setContentsMargins(0, 0, 0, 0)
        fov_layout.setSpacing(8)
        fov_layout.addWidget(self.fov_slider)
        fov_layout.addWidget(self.fov_input)

        layout.addWidget(fov_lbl, row, 0)
        layout.addWidget(fov_container, row, 1, 1, 3)
        row += 1

        # Render Resolution %
        self._add_separator(layout, row); row += 1

        rres_lbl = QLabel("Render Resolution %")
        rres_lbl.setObjectName("DashboardStatusName")
        rres_lbl.setContentsMargins(0, 8, 0, 8)

        self.render_res_spin = QSpinBox()
        self.render_res_spin.setRange(50, 200)
        self.render_res_spin.setSingleStep(10)
        self.render_res_spin.setValue(100)
        self.render_res_spin.valueChanged.connect(self.render_res_percent_changed)

        layout.addWidget(rres_lbl, row, 0)
        layout.addWidget(self.render_res_spin, row, 1, 1, 2)
        row += 1

        # Checkboxes row
        self._add_separator(layout, row); row += 1

        cb_widget = QWidget()
        cb_layout = QHBoxLayout(cb_widget)
        cb_layout.setContentsMargins(0, 8, 0, 8)
        cb_layout.setSpacing(16)

        self.vsync_cb = QCheckBox("Enable V-Sync")
        self.vsync_cb.stateChanged.connect(self.vsync_changed)
        self.draw_fps_cb = QCheckBox("Show FPS Counter")
        self.draw_fps_cb.stateChanged.connect(self.draw_fps_changed)
        cb_layout.addWidget(self.vsync_cb)
        cb_layout.addWidget(self.draw_fps_cb)
        cb_layout.addStretch()

        layout.addWidget(cb_widget, row, 0, 1, 4)
        row += 1

        layout.setColumnStretch(0, 1)
        layout.setColumnStretch(1, 1)
        layout.setColumnStretch(2, 2)
        layout.setColumnStretch(3, 0)
        layout.setRowStretch(row, 1)

        if self.dxvk_widget:
            tabs = QTabWidget()
            tabs.setDocumentMode(True)

            graphics_tab = QWidget()
            graphics_tab_layout = QVBoxLayout(graphics_tab)
            graphics_tab_layout.setContentsMargins(0, 0, 0, 0)
            graphics_tab_layout.setSpacing(0)
            graphics_tab_layout.addWidget(graphics_group)

            dxvk_tab = QWidget()
            dxvk_tab_layout = QVBoxLayout(dxvk_tab)
            dxvk_tab_layout.setContentsMargins(0, 0, 0, 0)
            dxvk_tab_layout.setSpacing(0)
            self.dxvk_widget.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Expanding)
            dxvk_tab_layout.addWidget(self.dxvk_widget)

            tabs.addTab(graphics_tab, "Graphics")
            tabs.addTab(dxvk_tab, "DXVK")
            main_layout.addWidget(tabs)
        else:
            main_layout.addWidget(graphics_group)

    def set_game_directory(self, game_dir):
        self.game_dir = game_dir
        json_path = os.path.join(os.path.dirname(__file__), "presets.json")
        loaded = load_presets_from_json(json_path)
        self.preset_dict = loaded if loaded else {}

        self.preset_combo.clear()
        for preset_name in self.preset_dict.keys():
            self.preset_combo.addItem(preset_name)

        if not self.game_dir or not os.path.exists(self.game_dir):
            write_log(f"Game directory does not exist: {self.game_dir}", "Error", self.log_widget)
            return

        config_path = os.path.join(self.game_dir, "players", "config.ini")
        if os.path.exists(config_path) and not os.access(config_path, os.W_OK):
            set_config_readonly(self.game_dir, False, self.log_widget)

        self.initialize_status()

    def set_log_widget(self, log_widget):
        self.log_widget = log_widget

    def initialize_status(self):
        if not self.game_dir:
            return
        status = check_essential_status(self.game_dir)

        self.fps_limiter_spin.setValue(status.get("max_fps", 165))
        self._fov_update_timer.stop()
        fov_value = status.get("fov", 80)
        self.fov_slider.blockSignals(True)
        self.fov_slider.setValue(fov_value)
        self.fov_slider.blockSignals(False)
        self.fov_input.blockSignals(True)
        self.fov_input.setText(str(fov_value))
        self.fov_input.blockSignals(False)
        self._last_applied_fov = fov_value
        self._pending_fov_value = None
        self.display_mode_combo.setCurrentIndex(status.get("display_mode", 1))
        self.resolution_edit.setText(status.get("resolution", "2560x1440"))
        self.refresh_rate_spin.setValue(status.get("refresh_rate", 165))
        self.vsync_cb.setChecked(status.get("vsync", True))
        self.draw_fps_cb.setChecked(status.get("draw_fps", False))

    def apply_preset_clicked(self):
        if not self.game_dir or not os.path.exists(self.game_dir):
            write_log(f"Game directory does not exist: {self.game_dir}", "Error", self.log_widget)
            return
        if self.log_widget and hasattr(self, 'lock_config_cb') and self.lock_config_cb.isChecked():
            set_config_readonly(self.game_dir, False, self.log_widget)

        preset_name = self.preset_combo.currentText()
        apply_preset(self.game_dir, preset_name, self.log_widget, self.preset_dict)
        write_log(f"Applied preset '{preset_name}'.", "Success", self.log_widget)

        if self.log_widget and hasattr(self, 'lock_config_cb') and self.lock_config_cb.isChecked():
            set_config_readonly(self.game_dir, True, self.log_widget)

        self.initialize_status()

    def fps_limiter_changed(self):
        if not self.game_dir:
            return
        set_config_value(self.game_dir, "MaxFPS", str(self.fps_limiter_spin.value()), "0 to 1000", self.log_widget)

    def on_fov_slider_changed(self, value):
        self.fov_input.blockSignals(True)
        self.fov_input.setText(str(value))
        self.fov_input.blockSignals(False)
        if not self.game_dir:
            self._pending_fov_value = None
            return
        self._pending_fov_value = value
        self._fov_update_timer.start()

    def on_fov_input_edited(self):
        text = self.fov_input.text().strip()
        if not text:
            self.fov_input.setText(str(self.fov_slider.value()))
            return
        try:
            value = int(text)
        except ValueError:
            value = self.fov_slider.value()
        value = max(65, min(120, value))
        if str(value) != text:
            self.fov_input.setText(str(value))
        if self.fov_slider.value() != value:
            self.fov_slider.blockSignals(True)
            self.fov_slider.setValue(value)
            self.fov_slider.blockSignals(False)
        self._pending_fov_value = value
        if not self.game_dir:
            return
        self._commit_pending_fov_value()

    def _commit_pending_fov_value(self):
        if self._pending_fov_value is None:
            return
        if not self.game_dir:
            self._pending_fov_value = None
            return
        value = self._pending_fov_value
        self._pending_fov_value = None
        self._apply_fov_value(value)

    def _apply_fov_value(self, value):
        self._fov_update_timer.stop()
        if not self.game_dir:
            self._last_applied_fov = value
            self._pending_fov_value = None
            return
        if self._last_applied_fov == value:
            return
        self._last_applied_fov = value
        self._pending_fov_value = None
        set_config_value(self.game_dir, "FOV", str(value), "65 to 120", self.log_widget)

    def display_mode_changed(self):
        if not self.game_dir:
            return
        mode_index = self.display_mode_combo.currentIndex()
        set_config_value(
            self.game_dir,
            "FullScreenMode",
            str(mode_index),
            "0=Windowed,1=Fullscreen,2=Fullscreen Windowed",
            self.log_widget
        )

    def resolution_changed(self):
        if not self.game_dir:
            return
        res = self.resolution_edit.text().strip()
        set_config_value(self.game_dir, "WindowSize", res, "any text", self.log_widget)

    def refresh_rate_changed(self):
        if not self.game_dir:
            return
        set_config_value(
            self.game_dir,
            "RefreshRate",
            str(self.refresh_rate_spin.value()),
            "1 to 240",
            self.log_widget
        )

    def render_res_percent_changed(self):
        if not self.game_dir:
            return
        set_config_value(
            self.game_dir,
            "ResolutionPercent",
            str(self.render_res_spin.value()),
            "50 to 200",
            self.log_widget
        )

    def vsync_changed(self):
        if not self.game_dir:
            return
        val = "1" if self.vsync_cb.isChecked() else "0"
        set_config_value(self.game_dir, "Vsync", val, "0 or 1", self.log_widget)

    def draw_fps_changed(self):
        if not self.game_dir:
            return
        val = "1" if self.draw_fps_cb.isChecked() else "0"
        set_config_value(self.game_dir, "DrawFPS", val, "0 or 1", self.log_widget)

# ================= Advanced Settings Widget =================
class AdvancedSettingsWidget(QWidget):
    reset_to_stock_requested = Signal()

    def __init__(self, parent=None):
        super().__init__(parent)
        self.game_dir = None
        self.mod_files_dir = None
        self.log_widget = None
        self.version_label: Optional[QLabel] = None
        self.init_ui()

    def load_settings(self):
        """Load settings from config.ini and update UI elements"""
        if not self.game_dir:
            return
            
        status = check_essential_status(self.game_dir)
        
        # Update UI elements with loaded settings
        self.smooth_cb.setChecked(status.get("smooth", False))
        self.vram_cb.setChecked(status.get("vram", False))
        self.vram_limit_spin.setEnabled(status.get("vram", False))
        
        # Set VRAM limit using the value from status
        vram_value = status.get("vram_value", 0.75)
        self.vram_limit_spin.setValue(int(vram_value * 100))
        
        self.latency_spin.setValue(status.get("latency", 1))
        self.reduce_cpu_cb.setChecked(status.get("reduce_cpu", False))
        self.all_settings_cb.setChecked(status.get("all_settings", False))
        
        # Check config file read-only status
        config_path = os.path.join(self.game_dir, "players", "config.ini")
        if os.path.exists(config_path):
            self.lock_config_cb.setChecked(not os.access(config_path, os.W_OK))

    def refresh_settings(self):
        """Refresh all advanced settings from the current game directory"""
        if self.game_dir:
            self.load_settings()

    def _add_separator(self, layout, row):
        sep = QFrame()
        sep.setObjectName("DashboardDivider")
        sep.setFrameShape(QFrame.HLine)
        sep.setFixedHeight(1)
        layout.addWidget(sep, row, 0, 1, 4)

    def init_ui(self):
        outer_layout = QVBoxLayout(self)
        outer_layout.setContentsMargins(0, 0, 0, 0)
        outer_layout.setSpacing(0)

        adv_box = QGroupBox("Advanced Settings")
        layout = QGridLayout(adv_box)
        layout.setContentsMargins(14, 14, 14, 14)
        layout.setHorizontalSpacing(10)
        layout.setVerticalSpacing(0)
        row = 0

        # Action buttons
        btn_row = QWidget()
        btn_layout = QHBoxLayout(btn_row)
        btn_layout.setContentsMargins(0, 0, 0, 12)
        btn_layout.setSpacing(10)

        clear_logs_btn = QPushButton("Clear Logs")
        clear_logs_btn.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
        clear_logs_btn.clicked.connect(self.clear_logs)
        btn_layout.addWidget(clear_logs_btn, 1)

        copy_logs_btn = QPushButton("Copy Logs")
        copy_logs_btn.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
        copy_logs_btn.clicked.connect(self.copy_logs_to_clipboard)
        btn_layout.addWidget(copy_logs_btn, 1)

        clear_mod_files_btn = QPushButton("Clear Mod Files")
        clear_mod_files_btn.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
        clear_mod_files_btn.clicked.connect(self.clear_mod_files_action)
        btn_layout.addWidget(clear_mod_files_btn, 1)

        self.reset_stock_btn = QPushButton("Reset to Stock")
        self.reset_stock_btn.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
        self.reset_stock_btn.clicked.connect(self.request_reset_to_stock)
        btn_layout.addWidget(self.reset_stock_btn, 1)

        layout.addWidget(btn_row, row, 0, 1, 4)
        row += 1

        # Smooth Framerate
        self._add_separator(layout, row); row += 1

        smooth_lbl = QLabel("Smooth Framerate")
        smooth_lbl.setObjectName("DashboardStatusName")
        smooth_lbl.setContentsMargins(0, 8, 0, 8)
        self.smooth_cb = QCheckBox("Enable")
        self.smooth_cb.stateChanged.connect(self.smooth_changed)
        layout.addWidget(smooth_lbl, row, 0)
        layout.addWidget(self.smooth_cb, row, 1, 1, 3)
        row += 1

        # VRAM Target
        self._add_separator(layout, row); row += 1

        vram_lbl = QLabel("VRAM Target")
        vram_lbl.setObjectName("DashboardStatusName")
        vram_lbl.setContentsMargins(0, 8, 0, 8)

        self.vram_cb = QCheckBox("Limit")
        self.vram_cb.stateChanged.connect(self.vram_changed)

        vram_val_widget = QWidget()
        vram_val_layout = QHBoxLayout(vram_val_widget)
        vram_val_layout.setContentsMargins(0, 0, 0, 0)
        vram_val_layout.setSpacing(4)
        self.vram_limit_spin = QSpinBox()
        self.vram_limit_spin.setRange(75, 100)
        self.vram_limit_spin.setValue(75)
        self.vram_limit_spin.setEnabled(False)
        self.vram_limit_spin.valueChanged.connect(self.vram_limit_changed)
        vram_val_layout.addWidget(self.vram_limit_spin)
        vram_val_layout.addWidget(QLabel("%"))
        vram_val_layout.addStretch()

        layout.addWidget(vram_lbl, row, 0)
        layout.addWidget(self.vram_cb, row, 1)
        layout.addWidget(vram_val_widget, row, 2, 1, 2)
        row += 1

        # Lower Latency
        self._add_separator(layout, row); row += 1

        latency_lbl = QLabel("Lower Latency")
        latency_lbl.setObjectName("DashboardStatusName")
        latency_lbl.setContentsMargins(0, 8, 0, 8)
        self.latency_spin = QSpinBox()
        self.latency_spin.setRange(0, 4)
        self.latency_spin.setValue(1)
        self.latency_spin.valueChanged.connect(self.latency_changed)
        layout.addWidget(latency_lbl, row, 0)
        layout.addWidget(self.latency_spin, row, 1, 1, 2)
        row += 1

        # Reduce CPU Usage
        self._add_separator(layout, row); row += 1

        reduce_cpu_lbl = QLabel("Reduce CPU Usage")
        reduce_cpu_lbl.setObjectName("DashboardStatusName")
        reduce_cpu_lbl.setContentsMargins(0, 8, 0, 8)
        self.reduce_cpu_cb = QCheckBox("Enable")
        self.reduce_cpu_cb.stateChanged.connect(self.reduce_cpu_changed)
        layout.addWidget(reduce_cpu_lbl, row, 0)
        layout.addWidget(self.reduce_cpu_cb, row, 1, 1, 3)
        row += 1

        # Unlock All Graphics Options
        self._add_separator(layout, row); row += 1

        all_settings_lbl = QLabel("Unlock All Graphics Options")
        all_settings_lbl.setObjectName("DashboardStatusName")
        all_settings_lbl.setContentsMargins(0, 8, 0, 8)
        self.all_settings_cb = QCheckBox("Enable")
        self.all_settings_cb.stateChanged.connect(self.all_settings_changed)
        layout.addWidget(all_settings_lbl, row, 0)
        layout.addWidget(self.all_settings_cb, row, 1, 1, 3)
        row += 1

        # Lock config.ini
        self._add_separator(layout, row); row += 1

        lock_config_lbl = QLabel("Lock config.ini")
        lock_config_lbl.setObjectName("DashboardStatusName")
        lock_config_lbl.setContentsMargins(0, 8, 0, 8)
        self.lock_config_cb = QCheckBox("Read-only")
        self.lock_config_cb.stateChanged.connect(self.lock_config_changed)
        layout.addWidget(lock_config_lbl, row, 0)
        layout.addWidget(self.lock_config_cb, row, 1, 1, 3)
        row += 1

        layout.setColumnStretch(0, 1)
        layout.setColumnStretch(1, 1)
        layout.setColumnStretch(2, 2)
        layout.setColumnStretch(3, 0)
        layout.setRowStretch(row, 1)

        outer_layout.addWidget(adv_box)

        self.version_label = QLabel(f"PatchOpsIII v{APP_VERSION}")
        self.version_label.setObjectName("DashboardStatusName")
        self.version_label.setContentsMargins(4, 6, 0, 4)
        outer_layout.addWidget(self.version_label)

    def set_game_directory(self, game_dir):
        self.game_dir = game_dir
        if self.game_dir:
            self.load_settings()  # Load settings immediately when directory is set
        config_path = os.path.join(game_dir, "players", "config.ini")
        if os.path.exists(config_path):
            self.lock_config_cb.setChecked(not os.access(config_path, os.W_OK))

    def set_mod_files_dir(self, mod_files_dir):
        self.mod_files_dir = mod_files_dir

    def set_log_widget(self, log_widget):
        self.log_widget = log_widget

    def smooth_changed(self):
        val = "1" if self.smooth_cb.isChecked() else "0"
        set_config_value(self.game_dir, "SmoothFramerate", val, "0 or 1", self.log_widget)

    def vram_changed(self):
        config_path = os.path.join(self.game_dir, "players", "config.ini")
        if self.vram_cb.isChecked():
            # When checked, limited VRAM is enabled:
            self.vram_limit_spin.setEnabled(True)
            self.vram_limit_changed()  # Apply limited percentage setting.
        else:
            # When unchecked, full VRAM usage is enabled:
            self.vram_limit_spin.setEnabled(False)
            pattern_replacements = {
                r'^\s*VideoMemory\s*=': 'VideoMemory = "1" // 0.75 to 1',
                r'^\s*StreamMinResident\s*=': 'StreamMinResident = "0" // 0 or 1',
            }
            update_config_values(config_path, pattern_replacements, "Enabled full VRAM usage.", self.log_widget)

    def vram_limit_changed(self):
        config_path = os.path.join(self.game_dir, "players", "config.ini")
        percentage = self.vram_limit_spin.value()
        decimal_value = percentage / 100.0
        pattern_replacements = {
            r'^\s*VideoMemory\s*=': f'VideoMemory = "{decimal_value}" // 0.75 to 1',
            r'^\s*StreamMinResident\s*=': 'StreamMinResident = "1" // 0 or 1',
        }
        update_config_values(config_path, pattern_replacements, f"Limited VRAM usage set to {percentage}%.", self.log_widget)

    def latency_changed(self):
        set_config_value(self.game_dir, "MaxFrameLatency", str(self.latency_spin.value()), "0 to 4", self.log_widget)

    def reduce_cpu_changed(self):
        val = "2" if self.reduce_cpu_cb.isChecked() else "0"
        set_config_value(self.game_dir, "SerializeRender", val, "0 to 2", self.log_widget)

    def all_settings_changed(self):
        val = "0" if self.all_settings_cb.isChecked() else "1"
        set_config_value(self.game_dir, "RestrictGraphicsOptions", val, "0 or 1", self.log_widget)

    def lock_config_changed(self):
        if self.lock_config_cb.isChecked():
            set_config_readonly(self.game_dir, True, self.log_widget)
        else:
            set_config_readonly(self.game_dir, False, self.log_widget)

    def _platform_label(self) -> str:
        system = platform.system()
        machine = platform.machine()

        if system == "Linux":
            distro = None
            try:
                os_release = platform.freedesktop_os_release()
                name = os_release.get("NAME")
                version = os_release.get("VERSION") or os_release.get("VERSION_ID")
                distro = " ".join(part for part in (name, version) if part)
            except Exception:
                distro = None

            if distro and machine:
                return f"{system} - {distro} ({machine})"
            if distro:
                return f"{system} - {distro}"
            if machine:
                return f"{system} ({machine})"
            return system or "Unknown platform"

        release = platform.release()
        if system and release and machine:
            return f"{system} {release} ({machine})"
        if system and release:
            return f"{system} {release}"
        if system and machine:
            return f"{system} ({machine})"
        if system:
            return system
        return "Unknown platform"

    def _build_log_payload(self, log_text: str) -> str:
        header = f"PatchOpsIII {APP_VERSION} - {self._platform_label()} logs:"
        body = log_text if log_text else "(no log entries found)"
        return f"{header}\n```\n{body}\n```"

    def copy_logs_to_clipboard(self):
        log_path = get_log_file_path()
        if not os.path.exists(log_path):
            log_content = ""
            write_log(f"Log file not found at {log_path}. Copying empty log header.", "Warning", self.log_widget)
        else:
            try:
                with open(log_path, "r", encoding="utf-8") as f:
                    log_content = f.read().strip()
            except Exception as exc:
                write_log(f"Unable to read log file: {exc}", "Error", self.log_widget)
                QMessageBox.warning(self, "Copy Logs", f"Could not read logs from {log_path}.\nError: {exc}")
                return

        clipboard = QGuiApplication.clipboard()
        payload = self._build_log_payload(log_content)
        if clipboard:
            clipboard.setText(payload)
        else:
            write_log("Clipboard not available; unable to copy logs.", "Error", self.log_widget)
            QMessageBox.warning(self, "Copy Logs", "Clipboard not available. Please try again.")
            return

        write_log("Logs copied to clipboard.", "Success", self.log_widget)
        msg = QMessageBox(self)
        msg.setWindowTitle("Logs Copied")
        msg.setTextFormat(Qt.RichText)
        msg.setText(
            "Logs copied to clipboard.<br><br>"
            'Have an issue?:<br>'
            '<a href="https://github.com/boggedbrush/PatchOpsIII/issues">Submit it here.</a>'
        )
        msg.setStandardButtons(QMessageBox.Ok)
        msg.exec()

    def clear_logs(self):
        log_path = get_log_file_path()
        if clear_log_file():
            if self.log_widget and hasattr(self.log_widget, "clear"):
                self.log_widget.clear()
        else:
            write_log(f"Failed to clear log file at {log_path}", "Error", self.log_widget)

    def clear_mod_files_action(self):
        if not self.mod_files_dir:
            write_log("Mod files directory not set.", "Error", self.log_widget)
            return

        reply = QMessageBox.question(
            self,
            "Confirm Clear Mod Files",
            f"Are you sure you want to delete all files in:\n{self.mod_files_dir}?\n\nThis will remove downloaded patch files and archives.",
            QMessageBox.Yes | QMessageBox.No,
            QMessageBox.No
        )

        if reply == QMessageBox.Yes:
            try:
                if os.path.exists(self.mod_files_dir):
                    # Remove all contents
                    for item in os.listdir(self.mod_files_dir):
                        item_path = os.path.join(self.mod_files_dir, item)
                        if os.path.isfile(item_path) or os.path.islink(item_path):
                            os.remove(item_path)
                        elif os.path.isdir(item_path):
                            shutil.rmtree(item_path)
                    write_log(f"Cleared mod files in {self.mod_files_dir}", "Success", self.log_widget)
                else:
                    write_log(f"Mod files directory does not exist: {self.mod_files_dir}", "Warning", self.log_widget)
            except Exception as e:
                write_log(f"Failed to clear mod files: {e}", "Error", self.log_widget)

    def request_reset_to_stock(self):
        reply = QMessageBox.warning(
            self,
            "Reset to Stock",
            (
                "This will remove modded installs and reset PatchOpsIII-managed settings to stock defaults.\n\n"
                "This includes Enhanced/Reforged/T7/DXVK files, launch options, and QoL/Advanced settings.\n\n"
                "Continue?"
            ),
            QMessageBox.Yes | QMessageBox.No,
            QMessageBox.No,
        )
        if reply == QMessageBox.Yes:
            self.reset_to_stock_requested.emit()
