# PatchOpsIII

PatchOpsIII (v1.0.2) is a modding utility for **Call of Duty: Black Ops III** that simplifies tweaking graphics settings, installing performance patches, and customizing your in-game experience. Developed by **boggedbrush**, this tool offers a friendly interface built with PySide6.

---
# Wiki

Before you start using the application, please take a look at the Wiki for setup instructions and configuration details. You can either click the Wiki page at the top of the GitHub repo or visit this [link](https://github.com/boggedbrush/PatchOpsIII/wiki).

---

## Features

- **Enhanced GUI with Tabs:**  
  Organized interface with Mods, Graphics, and Advanced tabs for better navigation.

- **Graphics Settings Manager:**  
  Adjust FPS limiter, FOV, resolution, and more. Easily apply presets from a JSON file to optimize visuals or performance.

- **Quality of Life Features:**
  - Skip all intro videos
  - Support for Play Offline mode
  - Integration with All-around Enhancement Lite and Ultimate Experience Mod
  - Clickable help buttons for additional information

- **T7 Patch Management:**  
  Install/update the T7 Patch with enhanced features:
  - LPC Installation support
  - Network password configuration
  - Friends Only Mode toggle
  - Uninstall capability
  - Customizable gamertag with color codes

- **DXVK-GPLAsync Manager:**  
  Install or uninstall DXVK-GPLAsync to reduce stuttering via async shader compilation.

- **Logging:**  
  All actions are logged both in-app and to a file for easy troubleshooting.

- **Executable & Future Linux Support:**  
  A dedicated **PatchOpsIII.exe** is available for straightforward Windows execution, with Linux support planned for the future.

---

![{9BAEFD91-221E-402A-8D1B-14676D179B9D}](https://github.com/user-attachments/assets/857e3460-98b4-45c7-bc4e-cd1fcdfef9fb)

## Prerequisites

- **Call of Duty: Black Ops III**  
  Ensure the game is installed. By default, PatchOpsIII looks for:
  ```
  C:\Program Files (x86)\Steam\steamapps\common\Call of Duty Black Ops III
  ```
  Change the game directory using the Browse button in the app if needed.

---

## Installation

1. **Download the Latest Release:**
   Download [PatchOpsIII v1.0.2](https://github.com/boggedbrush/PatchOpsIII/releases/download/1.0.2/PatchOpsIII.exe)
   
   Or clone the repository:
   ```bash
   git clone https://github.com/boggedbrush/PatchOpsIII.git
   cd PatchOpsIII
   ```

2. **Install Dependencies:**

   Ensure Python (3.7+) is installed and run:

   ```bash
   pip install PySide6 requests
   ```

---

## Usage

- **Using the Executable (PatchOpsIII.exe):**  
  For Windows users, simply double-click **PatchOpsIII.exe** to launch the application without needing to run it via Python.

- **Running via Python:**  
  Alternatively, run the application with:

  ```bash
  python main.py
  ```

- **Features in Action:**  
  - **Graphics Settings:** Adjust visuals or apply presets from `presets.json`.
  - **T7 Patch:** Manage your gamertag and other game settings.
  - **DXVK-GPLAsync:** Quickly install or uninstall DXVK-GPLAsync for smoother performance.

---

## VirusTotal Scan

The latest VirusTotal scan shows improved compatibility with antivirus software. Review the detailed report [here](https://www.virustotal.com/gui/file/622afd122d4f8e539c90efb33aae0ee2a4fda9c999795200b1ea7d9d2e8b55e2/summary).

## Known Issues

- All-around Enhancement Mod (full version) may have compatibility issues with launch options
- Launch options feature is still under testing and may not work for all users

---

## Acknowledgements

This project wouldn't exist without the hard work and contributions of other projects:

- **t7patch:**  
  [https://github.com/shiversoftdev/t7patch](https://github.com/shiversoftdev/t7patch)

- **dxvk-gplasync:**  
  [https://gitlab.com/Ph42oN/dxvk-gplasync](https://gitlab.com/Ph42oN/dxvk-gplasync)

---

## Contributing

Contributions are welcome! Fork the repo and submit pull requests. Please follow the existing code style and include tests for new features.

---

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.
