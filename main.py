#!/usr/bin/env python
import sys, os
from PySide6.QtWidgets import (
    QApplication, QMainWindow, QWidget, QVBoxLayout, QHBoxLayout, QLineEdit,
    QPushButton, QLabel, QFileDialog, QTextEdit
)
from PySide6.QtGui import QIcon
from t7_patch import T7PatchWidget
from dxvk_manager import DXVKWidget
from config import GraphicsSettingsWidget
from utils import write_log

def resource_path(relative_path):
    """
    Get absolute path to a resource, works for development and for PyInstaller onefile mode.
    Checks the _MEIPASS folder first, then falls back to the current directory.
    """
    base_path = getattr(sys, '_MEIPASS', os.path.abspath("."))
    candidate = os.path.join(base_path, relative_path)
    if os.path.exists(candidate):
        return candidate
    # Fallback: try the current working directory.
    return os.path.join(os.path.abspath("."), relative_path)

DEFAULT_GAME_DIR = r"C:\Program Files (x86)\Steam\steamapps\common\Call of Duty Black Ops III"
MOD_FILES_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "BO3 Mod Files")
if not os.path.exists(MOD_FILES_DIR):
    os.makedirs(MOD_FILES_DIR)

class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("PatchOpsIII")

        # Load and set the window icon.
        icon_path = resource_path("PatchOpsIII.ico")
        icon = QIcon(icon_path)
        if icon.isNull():
            write_log(f"Icon not found or invalid: {icon_path}", "Warning")
        self.setWindowIcon(icon)

        self.resize(1280, 720)
        self.init_ui()

    def init_ui(self):
        central = QWidget()
        main_layout = QVBoxLayout(central)

        # --- Game Directory Section ---
        game_dir_widget = QWidget()
        gd_layout = QHBoxLayout(game_dir_widget)
        self.game_dir_edit = QLineEdit(DEFAULT_GAME_DIR)
        browse_btn = QPushButton("Browse")
        browse_btn.clicked.connect(self.browse_game_dir)
        gd_layout.addWidget(QLabel("Game Directory:"))
        gd_layout.addWidget(self.game_dir_edit)
        gd_layout.addWidget(browse_btn)
        main_layout.addWidget(game_dir_widget)

        # --- T7 Patch GUI ---
        self.t7_patch_widget = T7PatchWidget(MOD_FILES_DIR)
        main_layout.addWidget(self.t7_patch_widget)

        # --- DXVK GUI ---
        self.dxvk_widget = DXVKWidget(MOD_FILES_DIR)
        main_layout.addWidget(self.dxvk_widget)

        # --- Graphics Settings GUI ---
        self.graphics_widget = GraphicsSettingsWidget()
        main_layout.addWidget(self.graphics_widget)

        # --- Log Window ---
        self.log_text = QTextEdit()
        self.log_text.setReadOnly(True)
        self.log_text.setStyleSheet("background-color: black; color: white; font-family: Consolas;")
        main_layout.addWidget(self.log_text)

        self.setCentralWidget(central)

        # Propagate game directory and log widget to other modules.
        game_dir = self.game_dir_edit.text().strip()
        self.t7_patch_widget.set_game_directory(game_dir)
        self.dxvk_widget.set_game_directory(game_dir)
        self.graphics_widget.set_game_directory(game_dir)

        self.t7_patch_widget.set_log_widget(self.log_text)
        self.dxvk_widget.set_log_widget(self.log_text)
        self.graphics_widget.set_log_widget(self.log_text)

    def browse_game_dir(self):
        directory = QFileDialog.getExistingDirectory(
            self, "Select Black Ops III Game Directory", self.game_dir_edit.text()
        )
        if directory:
            self.game_dir_edit.setText(directory)
            # Update game directory in dependent widgets.
            self.t7_patch_widget.set_game_directory(directory)
            self.dxvk_widget.set_game_directory(directory)
            self.graphics_widget.set_game_directory(directory)

if __name__ == "__main__":
    app = QApplication(sys.argv)

    # Set a global icon for the application.
    global_icon_path = resource_path("PatchOpsIII.ico")
    global_icon = QIcon(global_icon_path)
    if global_icon.isNull():
        write_log(f"Global icon not found or invalid: {global_icon_path}", "Warning")
    app.setWindowIcon(global_icon)

    window = MainWindow()
    window.show()
    sys.exit(app.exec())
