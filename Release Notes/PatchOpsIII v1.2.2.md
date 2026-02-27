# PatchOpsIII v1.2.2 Release Notes

## Overview
PatchOpsIII v1.2.2 focuses on BO3 Enhanced reliability and UX, especially on Linux/Steam Deck. This release adds automated Proton compatibility setup/cleanup, fixes UI responsiveness during Enhanced install/uninstall, and tightens status detection so dashboard state matches what is actually installed.

---

## 🚀 Major Highlights
- Added automated Linux BO3 Enhanced compatibility flow:
  - resolves Proton source from local bundled fork or on-demand upstream download,
  - installs compatibility tool as `BO3 Enhanced`,
  - maps AppID `311210` to that tool,
  - sets launch options to `WINEDLLOVERRIDES="WindowsCodecs=n,b" %command%`.
- Added automated Linux BO3 Enhanced uninstall cleanup:
  - clears compatibility mapping,
  - resets launch options,
  - removes installed compatibility tool and PatchOps backup tool directories.
- Moved Enhanced install/uninstall to background workers to prevent the app UI from becoming unresponsive.
- Updated Enhanced status model to match Reforged-style state reporting (`Installed/Not Installed | Active/Inactive`).
- Bumped application version metadata to `1.2.2`.

---

## 📝 Detailed Changes

### BO3 Enhanced (Linux/Steam Deck)
- Introduced Linux helper flow in `utils.py`:
  - `configure_bo3_enhanced_linux(...)`
  - `cleanup_bo3_enhanced_linux(...)`
  - compatibility tool install/remove + Steam `CompatToolMapping` write/clear
  - exact launch options set/clear path for `localconfig.vdf`
- Added on-demand upstream Proton archive caching:
  - source: Weather-OS GDK-Proton `release10-32`
  - archive: `GDK-Proton10-32.tar.gz`
  - cache path under app storage and manifest normalization to display as `BO3 Enhanced`.
- Added Steam process handling hardening on Linux:
  - better running-process detection,
  - safer open/close sequencing,
  - stricter `pkill -x steam` usage,
  - improved launch fallback/error logging.

### Enhanced Installer / UI Flow
- Replaced synchronous Enhanced install/uninstall operations with dedicated worker threads:
  - `EnhancedInstallWorker`
  - `EnhancedUninstallWorker`
- Updated first-run default-install warning behavior:
  - choosing **Enhanced** now routes users to the Enhanced tab,
  - no immediate forced dump picker/open/install action.
- Added missing browse/folder icon on Enhanced tab dump selection button.
- Added composite Enhanced status propagation so dashboard and tab remain consistent unless a worker is actively reporting progress.

### Detection and Compatibility Fixes
- Fixed T7 false-positive “Installed” status by requiring actual T7 binaries (`t7patchloader.dll` or `t7patch.dll`) rather than config-only presence.
- Added `GameChat2.dll` to the BO3 dump whitelist for Enhanced file intake.

### Repository / Documentation
- Added top-level `bo3-enhanced-proton/` fork metadata and source provenance documentation.
- Updated root README with a **Forked Components** section linking to the BO3 Enhanced Proton fork metadata.

### Versioning
- Updated baked application version from `1.2.1` to `1.2.2` in `version.py`.

---

## 🛠 Fixes

### Cross-Platform
- Fixed T7 installation state detection to avoid false positives from Reforged-related config artifacts.
- Improved Enhanced dashboard/tab status consistency and state clarity.
- Added missing Enhanced tab browse icon for UI consistency.

### Windows
- No Windows-only behavioral changes in this release.
- Linux-only Enhanced compatibility automation is platform-gated and does not run on Windows.

### Linux and Steam Deck
- Fixed app unresponsiveness during Enhanced install/uninstall by moving operations off the main UI thread.
- Fixed incomplete Enhanced uninstall cleanup by fully removing compatibility mapping, launch options, and compatibility tool artifacts.
- Fixed stale backup compatibility-tool directories causing `BO3 Enhanced` to remain listed in Steam after uninstall.
- Added robust fallback behavior for obtaining `BO3 Enhanced` Proton source (local fork bundle first, upstream cached download second).

---

## ⚠️ Known Issues

- **BO3 Enhanced on Linux is currently Offline-Only**
  - Impact: BO3 Enhanced currently runs in offline mode only on Linux installs.
  - Clarification: This is not a limitation imposed by PatchOpsIII.
  - Note: The same warning/message is also shown on game startup.
  - Status: Issue is tracked upstream by BO3 Enhanced developers; a fix is pending.

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
  - Download: [PatchOpsIII v1.2.2 for Windows]({{WINDOWS_DOWNLOAD_URL}})
  - SHA256: `{{WINDOWS_SHA256}}`
  - VirusTotal: [Latest Windows Scan]({{WINDOWS_VT_URL}})

- **Linux & Steam Deck**
  - Download: [PatchOpsIII v1.2.2 for Linux & Steam Deck]({{LINUX_DOWNLOAD_URL}})
  - SHA256: `{{LINUX_SHA256}}`
  - VirusTotal: [Latest Linux Scan]({{LINUX_VT_URL}})

---

## 🧑‍💻 Acknowledgements
PatchOpsIII builds on the work of the following projects:
- **t7patch:** [t7patch on GitHub](https://github.com/shiversoftdev/t7patch)
- **dxvk-gplasync:** [dxvk-gplasync on GitLab](https://gitlab.com/Ph42oN/dxvk-gplasync)
- **ValvePython/vdf:** [ValvePython/vdf on GitHub](https://github.com/ValvePython/vdf)
- **BO3 Enhanced:** [BO3 Enhanced on GitHub](https://github.com/shiversoftdev/BO3Enhanced)
- **BO3 Reforged:** [BO3 Reforged](https://bo3reforged.com/)
- **GDK-Proton (upstream for BO3 Enhanced Proton fork):** [Weather-OS/GDK-Proton](https://github.com/Weather-OS/GDK-Proton)

---

## 🔮 Upcoming Work
- Continued Linux/Steam Deck compatibility testing across distributions and Steam client variants.
- Additional polish for mod state detection/reporting and tab status consistency.
- Ongoing bug fixes and QoL updates based on user installation/uninstallation feedback.

---

If you encounter issues or have suggestions, please open an issue on the repository or share feedback with the community so we can prioritize future improvements.
