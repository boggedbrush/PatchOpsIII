# PatchOpsIII {{VERSION}} Release Notes

## Overview
{{OVERVIEW_SUMMARY}}

---

## 🚀 Major Highlights
- {{MAJOR_HIGHLIGHT_1}}
- {{MAJOR_HIGHLIGHT_2}}
- {{MAJOR_HIGHLIGHT_3}}
- {{MAJOR_HIGHLIGHT_4}}
- Bumped application version to `{{VERSION}}`.

---

## 📝 Detailed Changes

### App Experience
- {{APP_EXPERIENCE_CHANGE_1}}
- {{APP_EXPERIENCE_CHANGE_2}}
- {{APP_EXPERIENCE_CHANGE_3}}

### T7Patch
- {{T7PATCH_CHANGE_1}}
- {{T7PATCH_CHANGE_2}}
- {{T7PATCH_CHANGE_3}}

### Packaging
- {{PACKAGING_CHANGE_1}}
- {{PACKAGING_CHANGE_2}}
- Published the portable, self-contained AppImage as the primary Linux and Steam Deck package, plus a much smaller `.deb` that uses Debian/Ubuntu system libraries.
- Added SHA-256 files so downloads can be verified.
- {{PACKAGING_CHANGE_3}}

### Documentation
- {{DOCUMENTATION_CHANGE_1}}
- {{DOCUMENTATION_CHANGE_2}}

---

## 🛠 Fixes

### Cross-Platform
- {{FIXES_CROSS_PLATFORM_1}}
- {{FIXES_CROSS_PLATFORM_2}}
- {{FIXES_CROSS_PLATFORM_3}}

### Windows
- {{FIXES_WINDOWS_1}}
- {{FIXES_WINDOWS_2}}
- {{FIXES_WINDOWS_3}}

### Linux and Steam Deck
- {{FIXES_LINUX_STEAM_1}}
- {{FIXES_LINUX_STEAM_2}}
- {{FIXES_LINUX_STEAM_3}}

---

## ⚠️ Known Issues

- **Beta Build**
  - Impact: Some controls and workflows may still need polish.
  - Workaround: Report issues through GitHub with logs and your platform details.
  - Status: Active beta testing.

- **All-around Enhancement Mod**
  - Impact: The full All-around Enhancement Mod remains unsupported as a launch option.
  - Workaround: Use the Lite version when launch options are configured.
  - Status: Upstream mod behavior may change independently of PatchOpsIII.

- **Launch Options Stability on Linux and Steam Deck**
  - Impact: Launch options may not work consistently across all Linux distributions and Steam Deck setups.
  - Workaround: If issues occur, temporarily remove custom launch options and re-apply them incrementally.
  - Status: Behavior is being tested across more systems.

- **Linux Package Coverage**
  - Impact: The `.deb` is intended for Debian/Ubuntu; the AppImage is the portable choice for other Linux distributions and Steam Deck desktop mode.
  - Workaround: Use the AppImage when the Debian package or its system-library requirements do not match the host.
  - Status: CI smoke-tests the AppImage on Ubuntu and a fresh Arch base container and tests the `.deb` lifecycle on Ubuntu, but does not cover every distribution, display server, GPU driver, or Steam Deck configuration.

---

## 📥 Downloads & Verification

- **Windows**
  - Download: [PatchOpsIII {{VERSION}} for Windows]({{WINDOWS_DOWNLOAD_URL}})
  - SHA256: `{{WINDOWS_SHA256}}`
  - SHA256 file: [{{WINDOWS_SHA256_FILENAME}}]({{WINDOWS_SHA256_URL}})
  - VirusTotal: {{WINDOWS_VT_STATUS_OR_URL}}

- **Linux AppImage & Steam Deck (primary)**
  - Download: [PatchOpsIII {{VERSION}} for Linux & Steam Deck]({{LINUX_DOWNLOAD_URL}})
  - SHA256: `{{LINUX_SHA256}}`
  - SHA256 file: [{{LINUX_SHA256_FILENAME}}]({{LINUX_SHA256_URL}})
  - VirusTotal: {{LINUX_VT_STATUS_OR_URL}}

- **Debian/Ubuntu `.deb`**
  - Download: [PatchOpsIII {{VERSION}} for Debian/Ubuntu]({{DEB_DOWNLOAD_URL}})
  - SHA256: `{{DEB_SHA256}}`
  - SHA256 file: [{{DEB_SHA256_FILENAME}}]({{DEB_SHA256_URL}})
  - VirusTotal: {{DEB_VT_STATUS_OR_URL}}

---

## 🧑‍💻 Acknowledgements
PatchOpsIII builds on the work of the following projects:
- **t7patch:** [T7Patch on GitHub](https://github.com/Scroptss/T7Patch)
- **LPC files:** [shiversoftdev/t7patch on GitHub](https://github.com/shiversoftdev/t7patch)
- **In memory of shiversoftdev:** Thank you for the original t7patch work and your contributions to the Black Ops III community.
- **dxvk-gplasync:** [dxvk-gplasync on GitLab](https://gitlab.com/Ph42oN/dxvk-gplasync)
- **BO3 Enhanced:** [BO3 Enhanced on GitHub](https://github.com/shiversoftdev/BO3Enhanced)
- **BO3 Reforged:** [BO3 Reforged](https://bo3reforged.com/)

---

## 🔮 Upcoming Work
- {{UPCOMING_WORK_1}}
- {{UPCOMING_WORK_2}}
- {{UPCOMING_WORK_3}}
- Continue bug fixes and quality-of-life updates based on user reports.

---

If you encounter issues or have suggestions, please open an issue on the repository or share feedback with the community so we can prioritize future improvements.
