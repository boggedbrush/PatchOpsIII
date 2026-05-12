# PatchOpsIII v1.3.0-beta2 Release Notes

## Overview
PatchOpsIII v1.3.0-beta2 is a small follow-up to beta1. It keeps the new desktop app experience from beta1 and focuses on safer T7Patch downloads, a cleaner startup screen, and better release verification.

---

## 🚀 Major Highlights
- T7Patch now follows the latest maintained release instead of staying locked to beta1's release.
- PatchOpsIII checks the main T7Patch download before installing it.
- The startup screen is simpler and less alarming.
- The update button in the title bar is centered properly on beta builds.
- Bumped application version to `v1.3.0-beta2`.

---

## 📝 Detailed Changes

### App Experience
- Replaced the old startup message with a simple "Opening PatchOpsIII" loading screen.
- Made the beta version button in the title bar line up correctly.
- Made the app's version label more consistent while it is opening.

### T7Patch
- The main T7Patch download now uses the latest maintained Scroptss/T7Patch release.
- PatchOpsIII checks the main T7Patch download before installing it.
- LPC files still come from the legacy t7patch release because the maintained release does not include them.

### Packaging
- Added VirusTotal checks for Windows and Linux beta builds.
- Added VirusTotal links for published beta downloads.
- Kept SHA-256 hashes in the release notes for users who want to verify downloads.

### Documentation
- Added VirusTotal links to the beta1 release notes.
- Updated the wiki to explain that T7Patch now follows the latest maintained release.

---

## 🛠 Fixes

### Cross-Platform
- Fixed startup wording so users do not see technical text while PatchOpsIII opens.
- Fixed the beta2 version label in more places.
- Fixed T7Patch download checks so the main patch archive is verified from GitHub before install.

### Windows
- Fixed the beta title bar update button alignment.
- Kept T7Patch install behavior the same for users, including admin permission when Windows requires it.
- Added VirusTotal scan links for the Windows beta build.

### Linux and Steam Deck
- Added VirusTotal scan links for the Linux beta build.
- Kept the AppImage update file attached to beta releases.
- Kept Linux and Steam Deck behavior otherwise unchanged from beta1.

---

## ⚠️ Known Issues

- **Beta Build**
  - Impact: Some controls and workflows may still need polish.
  - Workaround: Report issues through GitHub with logs and your platform details.
  - Status: Active beta testing.

- **App Startup**
  - Impact: PatchOpsIII may fail to finish opening on some systems.
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
  - Download: [PatchOpsIII v1.3.0-beta2 for Windows]({{WINDOWS_DOWNLOAD_URL}})
  - SHA256: `{{WINDOWS_SHA256}}`
  - VirusTotal: [Windows scan]({{WINDOWS_VT_URL}})

- **Linux & Steam Deck**
  - Download: [PatchOpsIII v1.3.0-beta2 for Linux & Steam Deck]({{LINUX_DOWNLOAD_URL}})
  - SHA256: `{{LINUX_SHA256}}`
  - Update metadata: [PatchOpsIII-Beta.AppImage.zsync](https://github.com/boggedbrush/PatchOpsIII/releases/download/v1.3.0-beta2/PatchOpsIII-Beta.AppImage.zsync)
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
- Continue beta testing the new desktop app.
- Keep polishing startup, update checks, and downloads.
- Continue fixes based on beta feedback.
- Continue bug fixes and quality-of-life updates based on user reports.

---

If you encounter issues or have suggestions, please open an issue on the repository or share feedback with the community so we can prioritize future improvements.
