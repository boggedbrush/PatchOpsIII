# PatchOpsIII v1.1.0 Release Notes

## Overview
PatchOpsIII v1.1.0 is the first stable release following the v1.0.4 beta, focused on a built-in cross-platform updater, hardened Linux/AppImage packaging, and safer configuration handling. This release is recommended for all users on v1.0.x, especially those on Linux or Steam Deck. The primary highlight is the integrated auto-updater for Windows and Linux with more reliable release detection. 🎮

---

## 🚀 Major Highlights
- Auto-updater for Windows and Linux.
- AppImage-based packaging for Linux and Steam Deck.
- Improved Steam, DXVK, and launch option handling.
- UI/UX refinements and safer configuration behavior.

---

## 📝 Detailed Changes

### Updater
- Added a unified update button that runs OS-specific checks for both Windows and Linux (#23).
- Enabled automatic update checks on Windows at startup with links to the latest GitHub release when a newer build is available (#23).
- Integrated Linux update flow with Gear Lever (an AppImage management tool) to apply AppImage updates when a newer AppImage is detected (#23).

### Packaging (Linux)
- Switched Linux builds to AppDir-based AppImages with proper icons, bundled presets, persistent logs/backups, and update-friendly metadata (#18).
- Limited Linux release asset selection to real `.AppImage` and `.zsync` artifacts to improve both manual and automatic update detection (#18).

### Game Management Improvements
- Centralized version data in `version.py` so packaged builds, tags, and updater prompts stay consistent across platforms (#23, #5, #7).
- Grouped game directory tools with a dedicated update button and cached platform detection to reduce redundant checks (#23).
- Improved game directory detection to validate and remember user selections across runs, including Nuitka onefile builds (#5, #7).

### Linux, Steam Deck, and DXVK Enhancements
- Improved DXVK-GPLAsync installs to remain robust against upstream archive changes (including `.tar.zst`) and to preserve downloaded filenames for clarity (#9).
- Adjusted default Linux paths and Steam process handling to better accommodate different distributions and Steam Deck setups (#9).
- Ensured launch options are preserved on Linux T7 Patch installs, keeping existing `fs_game` and mod settings intact (#9).

### UI/UX and Configuration
- Promoted v1.0.4 beta quality-of-life improvements to stable: background threading for applying Steam launch options and installing the T7 Patch to keep the UI responsive.
- Refined direct “Launch Game” behavior via Steam so the main window Launch Game button works reliably.
- Added admin checks for T7 Patch installs on Windows to reduce failures due to missing elevation.
- Updated FOV slider behavior and graphics presets to avoid overwriting existing user settings unexpectedly.
- Guarded configuration writes so they only occur once a valid install directory has been detected, reducing the risk of invalid or partial configuration files (#5, #7).
- Improved log copy resilience and enabled automatic log cleanup every three launches to keep log files small and manageable.

### Documentation and Build System
- Refreshed the README with updated badges and clearer usage and setup guidance (#20).
- Updated release workflows to improve tag selection and note publishing across both beta and stable channels (#20, #16, #12, #11, #10).

---

## 🛠 Fixes

### Cross-Platform
- **Resolved false positives when discovering the install directory in packaged builds and ensured configuration writes only occur once a valid path exists (#5, #7).**
- Reduced log noise by tightening update-related logging and making log copy operations more resilient.
- Adjusted FOV slider and graphics presets to prevent them from overwriting existing user-defined settings.

### Windows
- **Fixed Windows auto-update handling to prevent duplicate update checks and ensure staged updates are applied safely (#23).**
- **Corrected Windows elevation detection for T7 Patch installs to reduce failed installations caused by insufficient permissions.**
- Reduced Steam shutdown-related noise in logs on Windows when starting or closing the game.

### Linux and Steam Deck
- **Fixed DXVK auto-install failures by preferring extractable assets and surfacing lookup errors more clearly (#9).**
- Ensured manual Linux update checks consistently surface the latest release via the GitHub API (#23).
- Preserved existing `fs_game` and mod settings during Linux T7 Patch installs to avoid unintended changes to launch options.

---

## ⚠️ Known Issues

- **All-around Enhancement Mod**
  - Impact: The current All-around Enhancement Mod does not work correctly when launch options are used; the Lite version remains compatible.
  - Workaround: Use the Lite version of the All-around Enhancement Mod when launch options are configured.
  - Status: Fix under investigation.

- **Launch Options Stability on Linux and Steam Deck**
  - Impact: Launch options may not work consistently across all Linux distributions and Steam Deck setups.
  - Workaround: If issues occur, temporarily remove custom launch options and re-apply them incrementally.
  - Status: Behavior is being evaluated across additional distributions and Steam Deck configurations.

---

## 📥 Downloads & Verification

- **Windows**
  - Download: [PatchOpsIII v1.1.0 for Windows]({{WINDOWS_DOWNLOAD_URL}})
  - SHA256: `{{WINDOWS_SHA256}}`
  - VirusTotal: [Latest Windows Scan]({{WINDOWS_VT_URL}})

- **Linux & Steam Deck**
  - Download: [PatchOpsIII v1.1.0 for Linux & Steam Deck]({{LINUX_DOWNLOAD_URL}})
  - SHA256: `{{LINUX_SHA256}}`
  - VirusTotal: [Latest Linux Scan]({{LINUX_VT_URL}})

---

## 🧑‍💻 Acknowledgements
PatchOpsIII builds on the work of the following projects:
- **t7patch:** [t7patch on GitHub](https://github.com/shiversoftdev/t7patch)
- **dxvk-gplasync:** [dxvk-gplasync on GitLab](https://gitlab.com/Ph42oN/dxvk-gplasync)
- **ValvePython/vdf:** [ValvePython/vdf on GitHub](https://github.com/ValvePython/vdf)

---

## 🔮 Upcoming Work
- Bug fixes and performance optimizations based on user reports.
- [BO3 Enhanced](https://github.com/shiversoftdev/BO3Enhanced) installation assistant tab for Windows users to automate installation from a Microsoft Store dump.
- Additional improvements for Linux and Steam Deck launch option handling and broader distribution coverage.

---

If you encounter issues or have suggestions, please open an issue on the repository or share feedback with the community so we can prioritize future improvements.
