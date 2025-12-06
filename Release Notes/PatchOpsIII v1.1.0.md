# PatchOpsIII v1.1.0 Release Notes

PatchOpsIII v1.1.0 is our first stable release after the v1.0.4 beta, focused on built-in updates and hardened cross-platform installs.

🔍 **VirusTotal Scans:**
- Windows: [Latest Windows Scan]({{WINDOWS_VT_URL}})
- Linux: [Latest Linux Scan]({{LINUX_VT_URL}})

---

## 🚀 New Features & Improvements
- **Built-in auto-updater for Windows & Linux (#23):**
  - Unified update button runs OS-specific checks; Windows now auto-checks at startup and links to the latest GitHub release when a newer build exists, while Linux prompts Gear Lever to apply AppImage updates.
  - Update metadata is cached to avoid duplicate log noise and surfaces release notes in-app so you can review what is changing.
- **AppImage packaging & release reliability (#18):**
  - Linux builds now use AppDir-based AppImages with proper icons, bundled presets, persistent logs/backups, and update-friendly metadata for smoother installs.
  - Release asset selection filters to real `.AppImage`/`.zsync` artifacts, improving manual and automatic update detection on Linux.
- **Versioning & UI polish (#23, #5, #7):**
  - Version data is centralized in `version.py`, aligning packaged builds, tags, and updater prompts.
  - Game directory tools are grouped with a dedicated update button, platform detection is cached, and the Launch Game button now works reliably from the main window.
  - Game directory detection validates and remembers selections across runs, including Nuitka onefile builds.
- **Linux and DXVK resiliency (#9):**
  - DXVK-GPLAsync installs stay robust against upstream archive changes (including `.tar.zst`) and preserve downloaded filenames for clarity.
  - Default Linux paths and Steam process handling are more tolerant of different distros and Steam Deck setups.
- **Beta QoL promoted to stable (from v1.0.4):**
  - Applying Steam launch options and installing the T7 Patch now run on background threads to keep the UI responsive.
  - Direct "Launch Game" via Steam, admin checks for T7 Patch installs on Windows, and refined launch option handling that preserves existing `fs_game` settings.
- **Docs & release automation (#20, #16, #12, #11, #10):**
  - README refreshed with badges and clearer guidance, and release workflows updated for accurate tag selection and note publishing across beta and stable channels.

---

## 🛠 Fixes
- Resolved false positives when discovering the install directory in packaged builds and guarded configuration writes until a valid path exists (#5, #7).
- Fixed DXVK auto-install failures by preferring extractable assets and surfacing lookup errors more clearly (#9).
- Prevented duplicate update checks/log lines, ensured manual Linux update checks surface the latest release, and staged Windows updates safely (#23).
- Corrected Windows elevation detection for T7 Patch installs and reduced Steam shutdown noise in logs.
- Updated FOV slider behavior and graphics presets to avoid over-writing user settings unexpectedly.

---

## ⚠️ Known Issues
- **All-around Enhancement Mod:**
  - Current version doesn’t work with launch options (`Lite` version works fine).
- **Launch Options Stability:**
  - May not work for all Linux distributions—still under testing.

---

## 📥 Download the Latest Release
- [Download PatchOpsIII v1.1.0 for Windows]({{WINDOWS_DOWNLOAD_URL}})
- [Download PatchOpsIII v1.1.0 for Linux & Steam Deck]({{LINUX_DOWNLOAD_URL}})

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
