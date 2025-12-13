# PatchOpsIII v1.2.0 Release Notes

## Overview
PatchOpsIII v1.2.0 introduces a preview BO3 Enhanced workflow with guided dump handling and safety rails, along with a modernized UI (sidebar navigation + new icons) and new maintenance actions in Advanced. This release is recommended for users who want to try BO3 Enhanced while keeping launch options safely gated.

---

## 🚀 Major Highlights
- New **BO3 Enhanced (Preview)** tab with Install/Update and Uninstall.
- Downloads the latest BO3 Enhanced release from GitHub and validates the archive before install.
- Manual UWP dump import (zip or folder) with validation and a strict copy whitelist (PatchOpsIII does not download game dumps).
- Enhanced Mode safety rails: persistent status, warning prompt, and automatic launch-option disabling while Enhanced is active.
- Modern UI refresh with QtModernRedux styling, sidebar navigation, and new tab/action icons.

---

## 📝 Detailed Changes

### BO3 Enhanced (Preview)
- Added `bo3_enhanced.py` utilities for:
  - Latest-release discovery via the GitHub API and download of the Enhanced archive.
  - Checksum caching (`bo3_enhanced_checksums.json`) and basic archive validation.
  - Install flow that applies whitelisted dump files first, then overlays Enhanced DLLs, creating `.bak` backups for safe rollback.
  - Uninstall flow that restores backups when present and avoids deleting core game files (protects `BlackOps3.exe` if a backup is missing).
- Added an in-app guided dump selection dialog that supports selecting `DUMP.zip`, a dump folder, or `BlackOps3.exe` from within the dump directory, with validation before install.
- Added “Enhanced Mode Active” status and a compatibility warning; launch options are disabled while Enhanced is active.

### UI/UX
- Added QtModernRedux-based styling with a lightweight theme overlay.
- Replaced the standard tab bar with a sidebar navigation layout and added new SVG icons for tabs and common actions.

### Advanced Tools
- Added **Clear Logs** and **Clear Mod Files** actions to simplify troubleshooting and reset downloaded assets.

### Packaging & Dependencies
- Added `QtModernRedux6` dependency for the updated UI styling.
- Updated Linux AppImage build script to bundle the `icons/` directory.

---

## 🛠 Fixes

### Cross-Platform
- Improved Advanced tab refresh behavior by storing the Advanced tab index for more reliable state management.
- Improved uninstall resilience for BO3 Enhanced workflows by preventing accidental deletion of protected game files when backups are missing.

### Windows
- No Windows-only fixes called out in this release.

### Linux and Steam Deck
- No Linux/Steam Deck-only fixes called out in this release.

---

## ⚠️ Known Issues

- **BO3 Enhanced (Preview) requires a manual game dump**
  - Impact: PatchOpsIII cannot download game dumps automatically; you must provide a valid UWP dump source.
  - Workaround: Use the in-app dump dialog and follow the linked guide to obtain a dump, then select `DUMP.zip` or the dump folder.
  - Status: Expected behavior (legal/safety constraint).

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
  - Download: [PatchOpsIII v1.2.0 for Windows]({{WINDOWS_DOWNLOAD_URL}})
  - SHA256: `{{WINDOWS_SHA256}}`
  - VirusTotal: [Latest Windows Scan]({{WINDOWS_VT_URL}})

- **Linux & Steam Deck**
  - Download: [PatchOpsIII v1.2.0 for Linux & Steam Deck]({{LINUX_DOWNLOAD_URL}})
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
- Documentation updates for BO3 Enhanced workflows and rollback guidance.
- Additional validation and preflight checks around dump sources and install paths.
- Bug fixes and performance optimizations based on user reports.

---

If you encounter issues or have suggestions, please open an issue on the repository or share feedback with the community so we can prioritize future improvements.
