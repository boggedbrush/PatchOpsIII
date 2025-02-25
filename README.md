# PatchOpsIII

PatchOpsIII is a modding utility for **Call of Duty: Black Ops III** that simplifies tweaking graphics settings, installing performance patches, and customizing your in-game experience. Developed by **boggedbrush**, this tool offers a friendly interface built with PySide6.

---
# Wiki

Before you start using the application, please take a look at the Wiki for setup instructions and configuration details. You can either click the Wiki page at the top of the GitHub repo or visit this link.

---

## Features

- **Graphics Settings Manager:**  
  Adjust FPS limiter, FOV, resolution, and more. Easily apply presets from a JSON file to optimize visuals or performance.

- **T7 Patch Management:**  
  Install/update the T7 Patch to customize your gamertag (with optional color codes) and adjust game settings. Automatically handles Windows Defender exclusions and administrative rights.

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

1. **Clone the Repository:**

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

The latest VirusTotal scan shows a detection rate of **7/71**. You can review the detailed report [here](https://www.virustotal.com/gui/file/dcb513ebe42d737b6647e92939d98cdaceed06031c363e19ca2bf674cb4e7874).

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
