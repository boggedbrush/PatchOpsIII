# PatchOpsIII v1.1.0 Release Notes

The first major release of **PatchOpsIII** is here! Version **1.1.0** marks the jump to a new versioning era thanks to built-in automatic updating, making it easier than ever to stay current.

🔍 **VirusTotal Scans:**
- Windows: [Latest Windows Scan]({{WINDOWS_VT_URL}})
- Linux: [Latest Linux Scan]({{LINUX_VT_URL}})

---

## 🚀 New Features & Improvements
- **Automatic Update Flow (Windows & Linux):**
  - Unified update button that runs OS-specific checks and presents available releases.
  - Windows builds now run an automatic update check at startup without duplicating log entries and link directly to the latest GitHub release page when an update is available.
  - Linux now performs an automatic update check shortly after startup, ensuring new builds are surfaced without manual polling.
- **Update-Friendly Versioning:**
  - Version metadata is centralized in `version.py`, aligning builds, tags, and release workflows for consistent update detection.
- **Main Window Enhancements:**
  - Game directory tools are grouped in a vertical layout for clearer navigation.
  - Platform detection is captured once and reused, reducing redundant checks during startup.
- **DXVK-GPLAsync & Launch Workflow:**
  - Installation remains resilient to varied archive structures and preserves downloaded filenames for clarity.
  - Applying Steam launch options and installing the T7 Patch run asynchronously, preventing UI stalls.
- **Direct Game Launch:**
  - Launch Black Ops III directly from the main window via Steam.
- **Refined Steam Integration:**
  - Utility functions are consolidated in `utils.py` for better maintainability and reuse across platforms.

---

## 🛠 Fixes
- Addresses minor bugs and stability issues reported since the last beta.
- Continues to handle DXVK-GPLAsync archive format changes by preferring extractable assets, including `.tar.zst` packages.

---

## ⚠️ Known Issues
- **All-around Enhancement Mod:**
  - Current version doesn’t work with launch options (`Lite` version works fine).
- **Launch Options Stability:**
  - May not work for all Linux distributions—still under testing.
- **Linux/Steam Deck App Icon:**
  - Linux & Steam Deck versions do not currently have an app icon.

---

## 📥 Download the Latest Release
- [Download PatchOpsIII v1.1.0 for Windows]({{WINDOWS_DOWNLOAD_URL}})
- [Download PatchOpsIII v1.1.0 for Linux & Steam Deck (PatchOpsIII.AppImage)]({{LINUX_DOWNLOAD_URL}})

---

## 🏗 Build Metadata
- Windows SHA256: `{{WINDOWS_SHA256}}`
- Linux SHA256: `{{LINUX_SHA256}}`

---

## 🧑‍💻 Acknowledgements
PatchOpsIII is built upon the work of these amazing projects:
- **t7patch:** [t7patch on GitHub](https://github.com/shiversoftdev/t7patch)
- **dxvk-gplasync:** [dxvk-gplasync on GitLab](https://gitlab.com/Ph42oN/dxvk-gplasync)
- **ValvePython/vdf:** [ValvePython/vdf on GitHub](https://github.com/ValvePython/vdf)

---

## 🔮 What's Next?
- **Bug Fixes & Optimizations:** Continuing improvements based on user feedback.
- **[BO3 Enhanced](https://github.com/shiversoftdev/BO3Enhanced) Installation assistant tab for Windows users:** User's specify their Microsoft store dump of the game and PatchOpsIII will automate the installation for you! To learn more about BO3 Enhanced I recommend watching [this video](https://www.youtube.com/watch?v=rBZZTcSJ9_s)
- **Community Feedback:** Your feedback is invaluable! Report issues, suggest improvements, and propose new features.

---

Thank you for supporting **PatchOpsIII**. Happy modding! 🎮
