# PatchOpsIII v1.3.0-beta3 Release Notes

## Overview
PatchOpsIII v1.3.0-beta3 is a follow-up to beta2. It focuses on easier Windows downloads, a new EXE Swapper section, improved BO3 Enhanced setup, and a simple way to choose Stable or Beta updates.

These notes only cover changes since `v1.3.0-beta2`.

---

## 🚀 Major Highlights
- Windows downloads are now packaged as a zip file for faster, easier updates and fewer false positives.
- EXE Swapper is now available as its own section.
- BO3 Enhanced setup now has a clearer install flow, source validation, help, and diagnostics.
- Users can switch update checks between Stable and Beta.
- BO3 Reforged has returned as a launch option.

---

## 📝 Detailed Changes

### App Experience
- Added a Stable/Beta update channel option in Tools.
- Cleaned up the Tools page layout so system info, updates, logs, and cache actions are easier to scan.
- Reworked the Activity Log so it keeps useful space at the bottom of the app.

### EXE Swapper
- Added the EXE Swapper tab.
- Added options for the latest Steam build, the compatible March 2023 build, and BO3 Enhanced when available.
- Added checks so PatchOpsIII can show what build is active and whether a backup is available.

### BO3 Enhanced
- Rebuilt the Enhanced page around a clearer setup flow.
- Added source validation before install.
- Added status, help, requirements, and install details so users can see what is ready and what still needs attention.
- Kept uninstall controls separated in a collapsed Danger Zone.

### Launch Options
- Added the BO3 Reforged launch option back.
- Kept the existing launch options behavior otherwise unchanged from beta2.

### T7Patch
- No new T7Patch changes in beta3.
- T7Patch still follows the latest maintained Scroptss/T7Patch release.
- LPC files still come from the legacy t7patch release because the maintained release does not include them.

### Packaging
- Windows beta releases now ship as `PatchOpsIII-Beta.zip` to make updates easier and help reduce false positives.
- The in-app updater now supports Windows zip downloads first, with the exe update path kept as a fallback.
- SHA-256 hashes and VirusTotal links remain part of the release process.

---

## 🛠 Fixes

### Cross-Platform
- Fixed the update flow so beta and stable release checks can be selected inside the app.
- Improved Enhanced page spacing and reduced cramped controls.
- Improved empty-log handling so the Activity Log does not collapse into a tiny strip.

### Windows
- Switched Windows release packaging to zip.
- Added updater support for Windows zip assets.
- Added EXE switching workflows for Windows users.

### Linux and Steam Deck
- No major Linux or Steam Deck behavior changes since beta2.
- Linux beta releases still include the AppImage update metadata.
- Existing Linux and Steam Deck launch option behavior remains under beta testing.

---

## ⚠️ Known Issues

- **Beta Build**
  - Impact: Some controls and workflows may still need polish.
  - Workaround: Report issues through GitHub with logs and your platform details.
  - Status: Active beta testing.

- **Windows Zip Updates**
  - Impact: Windows zip downloads are new, so update behavior needs wider testing.
  - Workaround: If the in-app update fails, download the zip from GitHub and extract it manually.
  - Status: Being tested in beta3.

- **EXE Swapper**
  - Impact: Switching to the compatible build may require the Steam depot download to exist locally first.
  - Workaround: Follow the prompt shown by PatchOpsIII if the compatible build is missing.
  - Status: Available in beta3 and being tested.

- **BO3 Enhanced**
  - Impact: BO3 Enhanced still requires a user-provided UWP dump source.
  - Workaround: Use the dump guide from the Enhanced page and validate the source before installing.
  - Status: Setup flow improved in beta3.

- **Launch Options Stability on Linux and Steam Deck**
  - Impact: Launch options may not work consistently across all Linux distributions and Steam Deck setups.
  - Workaround: If issues occur, temporarily remove custom launch options and re-apply them incrementally.
  - Status: Behavior is being tested across more systems.

---

## 📥 Downloads & Verification

- **Windows**
  - Download: [PatchOpsIII v1.3.0-beta3 for Windows]({{WINDOWS_DOWNLOAD_URL}})
  - SHA256: `{{WINDOWS_SHA256}}`
  - VirusTotal: [Windows scan]({{WINDOWS_VT_URL}})

- **Linux & Steam Deck**
  - Download: [PatchOpsIII v1.3.0-beta3 for Linux & Steam Deck]({{LINUX_DOWNLOAD_URL}})
  - SHA256: `{{LINUX_SHA256}}`
  - Update metadata: [PatchOpsIII-Beta.AppImage.zsync](https://github.com/boggedbrush/PatchOpsIII/releases/download/v1.3.0-beta3/PatchOpsIII-Beta.AppImage.zsync)
  - VirusTotal: [Linux scan]({{LINUX_VT_URL}})

---

## 🧑‍💻 Acknowledgements
PatchOpsIII builds on the work of the following projects:
- **t7patch:** [T7Patch on GitHub](https://github.com/Scroptss/T7Patch)
- **LPC files:** [shiversoftdev/t7patch on GitHub](https://github.com/shiversoftdev/t7patch)
- **In memory of shiversoftdev:** Thank you for the original t7patch work and your contributions to the Black Ops III community.
- **dxvk-gplasync:** [dxvk-gplasync on GitLab](https://gitlab.com/Ph42oN/dxvk-gplasync)
- **ValvePython/vdf:** [ValvePython/vdf on GitHub](https://github.com/ValvePython/vdf)
- **BO3 Enhanced:** [BO3 Enhanced on GitHub](https://github.com/shiversoftdev/BO3Enhanced)
- **BO3 Reforged:** [BO3 Reforged](https://bo3reforged.com/)

---

## 🔮 Upcoming Work
- Continue testing Windows zip updates.
- Keep polishing BO3 Enhanced setup.
- Continue testing EXE Swapper behavior across more installs.
- Continue bug fixes and quality-of-life updates based on user reports.

---

If you encounter issues or have suggestions, please open an issue on the repository or share feedback with the community so we can prioritize future improvements.
