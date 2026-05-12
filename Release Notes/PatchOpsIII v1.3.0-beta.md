# PatchOpsIII v1.3.0-beta Release Notes

## Overview
PatchOpsIII v1.3.0-beta is a beta release with a rebuilt desktop app, smoother app controls, updated T7Patch downloads, and new Windows and Linux test builds. This release is meant for users who want to try the new PatchOpsIII experience early and report any rough edges.

---

## 🚀 Major Highlights
- Rebuilt PatchOpsIII as a modern desktop control center.
- Added a cleaner dashboard with live logs, folder browsing, launch profiles, graphics controls, and mod management.
- Updated T7Patch downloads to the maintained v3.02 release.
- Added beta Windows and Linux downloads.
- Updated setup and usage notes for the new app.
- Bumped application version to `v1.3.0-beta`.

---

## 📝 Detailed Changes

### App Experience
- Replaced the older app layout with a new desktop interface.
- Added a responsive dashboard for game status, mod tools, graphics settings, logs, and quick actions.
- Improved how the app handles file browsing, Steam actions, status checks, updates, and logs.
- Added better separation between the visible app and the local helper process that performs background actions.

### T7Patch
- Updated the main T7Patch download source to the maintained Scroptss/T7Patch v3.02 release.
- Kept LPC files on the older source because the maintained release does not provide those files.
- Updated documentation so users can tell which source is used for each T7Patch component.

### Packaging
- Added a Windows beta executable.
- Added a Linux and Steam Deck beta AppImage.
- Added SHA-256 files so downloads can be verified.
- VirusTotal scan links are not included in this beta.

### Documentation
- Updated the README and wiki for the new app experience.
- Kept older release notes available for users on previous versions.

---

## 🛠 Fixes

### Cross-Platform
- Improved T7 status and reset controls so the dashboard better reflects what is installed.
- Improved file, Steam, status, update, and log handling.
- Updated T7Patch source handling to use the maintained v3.02 release.

### Windows
- Added a beta Windows executable.
- Added a SHA-256 verification file for the Windows beta download.
- T7 installation may still require administrator permission depending on the game folder.

### Linux and Steam Deck
- Added a beta AppImage.
- Added a SHA-256 verification file for the Linux beta download.
- Launch option behavior is still being tested across more Linux and Steam Deck setups.

---

## ⚠️ Known Issues

- **Beta Build**
  - Impact: Some controls and workflows may still need polish.
  - Workaround: Report issues through GitHub with logs and your platform details.
  - Status: Active beta testing.

- **Local Helper Startup**
  - Impact: Some app actions may not work if the local helper process fails to start.
  - Workaround: Restart PatchOpsIII and include logs in a bug report if the issue continues.
  - Status: Being improved during beta.

- **All-around Enhancement Mod**
  - Impact: The full All-around Enhancement Mod remains unsupported as a launch option.
  - Workaround: Use the Lite version when launch options are configured.
  - Status: Upstream mod behavior may change independently of PatchOpsIII.

- **Launch Options Stability on Linux and Steam Deck**
  - Impact: Launch options may not work consistently across all Linux distributions and Steam Deck setups.
  - Workaround: If issues occur, temporarily remove custom launch options and re-apply them incrementally.
  - Status: Behavior is being tested across more systems.

---

## 📥 Downloads & Verification

- **Windows**
  - Download: [PatchOpsIII v1.3.0-beta for Windows](https://github.com/boggedbrush/PatchOpsIII/releases/download/v1.3.0-beta/PatchOpsIII-Beta.exe)
  - SHA256: `2d42210dcf7447b219dcd438f9a58c8326fe4628fa47fdbe871a77af42385e5b`
  - SHA256 file: [PatchOpsIII-Beta.exe.sha256](https://github.com/boggedbrush/PatchOpsIII/releases/download/v1.3.0-beta/PatchOpsIII-Beta.exe.sha256)
  - VirusTotal: Not published for this beta.

- **Linux & Steam Deck**
  - Download: [PatchOpsIII v1.3.0-beta for Linux & Steam Deck](https://github.com/boggedbrush/PatchOpsIII/releases/download/v1.3.0-beta/PatchOpsIII-Beta.AppImage)
  - SHA256: `ea4af37f33e548027ffd4d4071293f70cf7606851a2ce445bf4cc2cedc98bb4d`
  - SHA256 file: [PatchOpsIII-Beta.AppImage.sha256](https://github.com/boggedbrush/PatchOpsIII/releases/download/v1.3.0-beta/PatchOpsIII-Beta.AppImage.sha256)
  - Update metadata: [PatchOpsIII-Beta.AppImage.zsync](https://github.com/boggedbrush/PatchOpsIII/releases/download/v1.3.0-beta/PatchOpsIII-Beta.AppImage.zsync)
  - VirusTotal: Not published for this beta.

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
- Continue beta testing the new desktop app.
- Improve startup reliability and release packaging.
- Polish advanced controls, logs, folder browsing, and launch option handling.
- Continue bug fixes and quality-of-life updates based on user reports.

---

If you encounter issues or have suggestions, please open an issue on the repository or share feedback with the community so we can prioritize future improvements.
