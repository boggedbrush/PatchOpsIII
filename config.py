#!/usr/bin/env python
import os
import re
import json
import stat
import platform
from typing import Optional
from PySide6.QtWidgets import (
    QMessageBox, QWidget, QGroupBox, QVBoxLayout, QHBoxLayout, QLabel,
    QComboBox, QPushButton, QFormLayout, QCheckBox, QSpinBox, QLineEdit, QSlider,
    QSizePolicy
)
from PySide6.QtGui import QIntValidator, QGuiApplication
from PySide6.QtCore import Qt, QTimer
from version import APP_VERSION
from utils import write_log, get_log_file_path

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
    dll_bak = dll_file + ".bak"
    if reduce_stutter:
        if os.path.exists(dll_file):
            try:
                os.rename(dll_file, dll_bak)
                write_log("Renamed d3dcompiler_46.dll to reduce stuttering.", "Success", log_widget)
            except Exception:
                write_log("Failed to rename d3dcompiler_46.dll.", "Error", log_widget)
        elif os.path.exists(dll_bak):
            write_log("Stutter reduction already enabled.", "Success", log_widget)
        else:
            write_log("d3dcompiler_46.dll not found.", "Warning", log_widget)
    else:
        if os.path.exists(dll_bak):
            try:
                os.rename(dll_bak, dll_file)
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
        intro_bak = os.path.join(video_dir, "BO3_Global_Logo_LogoSequence.mkv.bak")
        status["skip_intro"] = os.path.exists(intro_bak)
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

    def init_ui(self):
        main_layout = QVBoxLayout(self)
        main_layout.setContentsMargins(0, 0, 0, 0)
        main_layout.setSpacing(0)

        # ================= Graphics Presets =================
        presets_group = QGroupBox("Graphics Presets")
        presets_layout = QHBoxLayout(presets_group)
        presets_layout.addWidget(QLabel("Select Preset:"))

        self.preset_combo = QComboBox()
        self.preset_combo.addItems(["Quality", "Balanced", "Performance", "Ultra Performance", "Custom"])
        presets_layout.addWidget(self.preset_combo)

        self.apply_preset_btn = QPushButton("Apply Preset")
        self.apply_preset_btn.clicked.connect(self.apply_preset_clicked)
        presets_layout.addWidget(self.apply_preset_btn)

        if self.dxvk_widget:
            self.dxvk_widget.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
            presets_group.setSizePolicy(QSizePolicy.Expanding, QSizePolicy.Fixed)
            top_row_layout = QHBoxLayout()
            top_row_layout.addWidget(self.dxvk_widget)
            top_row_layout.addWidget(presets_group)
            main_layout.addLayout(top_row_layout)
        else:
            main_layout.addWidget(presets_group)

        # ================= Graphics Settings Section =================
        settings_group = QGroupBox("Graphics Settings")
        settings_layout = QVBoxLayout(settings_group)

        settings_form = QFormLayout()

        # Create horizontal layout for checkboxes
        checkbox_layout = QHBoxLayout()
        self.vsync_cb = QCheckBox("Enable V-Sync")
        self.vsync_cb.stateChanged.connect(self.vsync_changed)
        self.draw_fps_cb = QCheckBox("Show FPS Counter")
        self.draw_fps_cb.stateChanged.connect(self.draw_fps_changed)
        checkbox_layout.addWidget(self.vsync_cb)
        checkbox_layout.addWidget(self.draw_fps_cb)
        checkbox_layout.addStretch()
        settings_form.addRow(checkbox_layout)

        self.fps_limiter_spin = QSpinBox()
        self.fps_limiter_spin.setRange(0, 1000)
        self.fps_limiter_spin.setValue(165)
        self.fps_limiter_spin.valueChanged.connect(self.fps_limiter_changed)
        settings_form.addRow("FPS Limiter (0=Unlimited):", self.fps_limiter_spin)

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

        settings_form.addRow("FOV:", fov_container)

        self.display_mode_combo = QComboBox()
        self.display_mode_combo.addItems(["Windowed", "Fullscreen", "Fullscreen Windowed"])
        self.display_mode_combo.currentIndexChanged.connect(self.display_mode_changed)
        settings_form.addRow("Display Mode:", self.display_mode_combo)

        self.resolution_edit = QLineEdit("2560x1440")
        self.resolution_edit.editingFinished.connect(self.resolution_changed)
        settings_form.addRow("Resolution:", self.resolution_edit)

        self.refresh_rate_spin = QSpinBox()
        self.refresh_rate_spin.setRange(1, 240)
        self.refresh_rate_spin.setValue(165)
        self.refresh_rate_spin.valueChanged.connect(self.refresh_rate_changed)
        settings_form.addRow("Refresh Rate:", self.refresh_rate_spin)

        self.render_res_spin = QSpinBox()
        self.render_res_spin.setRange(50, 200)
        self.render_res_spin.setSingleStep(10)
        self.render_res_spin.setValue(100)
        self.render_res_spin.valueChanged.connect(self.render_res_percent_changed)
        settings_form.addRow("Render Res %:", self.render_res_spin)

        settings_layout.addLayout(settings_form)
        main_layout.addWidget(settings_group)

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
    def __init__(self, parent=None):
        super().__init__(parent)
        self.game_dir = None
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

    def init_ui(self):
        layout = QVBoxLayout(self)
        adv_box = QGroupBox("Advanced Settings")
        adv_form = QFormLayout(adv_box)

        self.smooth_cb = QCheckBox("Smooth Framerate (Enables the smoothframe rate option)")
        self.smooth_cb.stateChanged.connect(self.smooth_changed)
        adv_form.addRow(self.smooth_cb)

        # VRAM settings
        self.vram_cb = QCheckBox("Set VRAM Usage (Enables the VideoMemory and StreamMinResident options)")
        self.vram_cb.stateChanged.connect(self.vram_changed)
        adv_form.addRow(self.vram_cb)

        # New spin box for limited VRAM percentage
        self.vram_limit_spin = QSpinBox()
        self.vram_limit_spin.setRange(75, 100)
        self.vram_limit_spin.setValue(75)
        self.vram_limit_spin.setEnabled(False)
        self.vram_limit_spin.valueChanged.connect(self.vram_limit_changed)
        adv_form.addRow("Set VRAM target to (%):", self.vram_limit_spin)

        self.latency_spin = QSpinBox()
        self.latency_spin.setRange(0, 4)
        self.latency_spin.setValue(1)
        self.latency_spin.valueChanged.connect(self.latency_changed)
        adv_form.addRow("Lower Latency (0-4, determines the number of frames to queue before rendering):", self.latency_spin)

        self.reduce_cpu_cb = QCheckBox("Reduce CPU Usage (Changes the SerializeRender option, only recommended for weak CPUs)")
        self.reduce_cpu_cb.stateChanged.connect(self.reduce_cpu_changed)
        adv_form.addRow(self.reduce_cpu_cb)

        self.all_settings_cb = QCheckBox("Unlock All Graphics Options (Allows all graphics options to be changed)")
        self.all_settings_cb.stateChanged.connect(self.all_settings_changed)
        adv_form.addRow(self.all_settings_cb)

        self.lock_config_cb = QCheckBox("Lock config.ini (read-only mode, prevents changes to the config file)")
        self.lock_config_cb.stateChanged.connect(self.lock_config_changed)
        adv_form.addRow(self.lock_config_cb)

        layout.addWidget(adv_box)
        layout.addStretch(1)

        footer_layout = QHBoxLayout()
        self.version_label = QLabel(f"PatchOpsIII v{APP_VERSION}")
        footer_layout.addWidget(self.version_label)
        footer_layout.addStretch(1)

        copy_logs_btn = QPushButton("Copy Logs")
        copy_logs_btn.clicked.connect(self.copy_logs_to_clipboard)
        footer_layout.addWidget(copy_logs_btn)

        layout.addLayout(footer_layout)

    def set_game_directory(self, game_dir):
        self.game_dir = game_dir
        if self.game_dir:
            self.load_settings()  # Load settings immediately when directory is set
        config_path = os.path.join(game_dir, "players", "config.ini")
        if os.path.exists(config_path):
            self.lock_config_cb.setChecked(not os.access(config_path, os.W_OK))

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
