# PatchOpsIII v1.0.4 Release Notes

The latest release of **PatchOpsIII** is here! Version **1.0.4** brings significant improvements in user experience, stability, and functionality.

🔍 **VirusTotal Scan:** [Latest VirusTotal Scan](https://www.virustotal.com/gui/file/YOUR_VIRUSTOTAL_LINK_HERE)
---

## 🚀 **New Features & Improvements:**
- **Enhanced User Experience (UI Responsiveness):**
  - Applying Steam launch options and installing the T7 Patch now run asynchronously in separate threads, preventing the UI from freezing during these operations.
- **Improved DXVK-GPLAsync Installation:**
  - The DXVK-GPLAsync installation is more robust, as it now recursively searches for the necessary DLL files within the extracted archive, making it less dependent on a specific folder structure.
  - Downloaded DXVK-GPLAsync archives will retain their original filenames for better clarity.
- **Direct Game Launch:**
  - A new "Launch Game" button has been added to the main window, allowing users to directly launch Black Ops III via Steam from within the application.
- **Refactored Codebase:**
  - Common utility functions related to Steam integration and launch options have been moved to a new `utils.py` module, improving code organization and maintainability.
- **Linux Compatibility Improvements:**
  - A default game directory path for Linux installations has been added.
  - Improved handling of Steam processes on Linux with more robust process checks and timeouts.
- **Robust Launch Options Management:**
  - The logic for setting Steam launch options has been refined to better handle existing `fs_game` parameters, ensuring correct application of new options without conflicts.
  - Timeouts have been added to Steam process management commands for more reliable execution.
- **T7 Patch Installation Enhancements:**
  - The T7 Patch installation now includes a check for administrator privileges on Windows and will prompt for elevation if necessary, ensuring successful installation.

---

## 🛠 **Fixes:**
- Addressed various minor bugs and stability issues.
- Fixed DXVK-GPLAsync auto-installation failures caused by new upstream archive formats by preferring extractable assets and supporting `.tar.zst` packages out of the box.

---

## ⚠️ **Known Issues:**
- **All-around Enhancement Mod:**
  - Current version doesn’t work with launch options (`Lite` version works fine).
- **Launch Options Stability:**
  - May not work for all Linux distributions—still under testing.
- **Linux/Steam Deck App Icon:**
  - Linux & Steam Deck versions do not currently have an app icon.

---

## 📥 **Download the Latest Release:**
[Download PatchOpsIII v1.0.4 for Windows](https://github.com/boggedbrush/PatchOpsIII/releases/download/1.0.4/PatchOpsIII.exe)
[Download PatchOpsIII v1.0.4 for Linux & Steam Deck](https://github.com/boggedbrush/PatchOpsIII/releases/download/1.0.4/PatchOpsIII)

---

## 🧑‍💻 **Acknowledgements:**
PatchOpsIII is built upon the work of these amazing projects:
- **t7patch:** [t7patch on GitHub](https://github.com/shiversoftdev/t7patch)
- **dxvk-gplasync:** [dxvk-gplasync on GitLab](https://gitlab.com/Ph42oN/dxvk-gplasync)

---

## 🔮 **What’s Next?**
- **Bug Fixes & Optimizations:** Continuing improvements based on user feedback.
- **[BO3 Enhanced](https://github.com/shiversoftdev/BO3Enhanced) Installation assistant tab for Windows users:** User's specify their Microsoft store dump of the game and PatchOpsIII will automate the installation for you! To learn more about BO3 Enhanced I recommend watching [this video](https://www.youtube.com/watch?v=rBZZTcSJ9_s)
- **Community Feedback:** Your feedback is invaluable! Report issues, suggest improvements, and propose new features.

---

Thank you for supporting **PatchOpsIII**. Happy modding! 🎮
