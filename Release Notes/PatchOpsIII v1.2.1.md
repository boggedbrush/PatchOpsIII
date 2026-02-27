# PatchOpsIII v1.2.1 Release Notes

## Overview
PatchOpsIII v1.2.1 improves Linux/Steam Deck reliability for BO3 Reforged installation and introduces UI readability/theming improvements across the app.

---

## 🚀 Major Highlights
- Fixed a Linux/Steam Deck Reforged installer failure caused by TLS certificate verification in some packaged runtime environments.
- Improved download-path resilience while preserving existing Reforged executable integrity validation (SHA-256 trust list).
- Added runtime theme switching with saved preference support (`system`, `light`, `dark`).
- Improved readability in light mode and refreshed control styling (inputs, dropdowns, spin boxes, sliders, status labels, and sidebar states).
- Bumped application version metadata to `1.2.1`.

---

## 📝 Detailed Changes

### Reforged Installer (Linux/Steam Deck)
- Reworked Reforged executable download flow to use a requests-based streaming path.
- Added CA bundle resolution priority for HTTPS verification:
  - `SSL_CERT_FILE` when present,
  - `certifi` bundle when available,
  - system/default requests verification as fallback.
- Clarified Reforged tab messaging to explicitly state that install also applies the BO3 Reforged Workshop mod and downgrades `BlackOps3.exe` to the build before the February 19, 2026 update.
- Kept existing security checks unchanged after download:
  - non-empty file validation,
  - executable signature check (`MZ`),
  - SHA-256 allowlist verification before replacement.

### Theme and Readability
- Added persisted theme preference storage in `PatchOpsIII_settings.json` with valid modes:
  - `system`
  - `light`
  - `dark`
- Added `--theme` CLI option (`system`, `light`, `dark`) to set and persist theme preference.
- Added runtime theme synchronization so system theme changes are reflected without restarting when using `system` mode.
- Improved status/readability contrast for light mode and tuned color states for dashboard/workshop indicators.
- Updated form control styling for better legibility and consistency:
  - `QLineEdit`, `QComboBox`, and `QSpinBox` borders/background/disabled states
  - themed up/down/drop-down arrows
  - slider groove/fill/handle contrast
- Refreshed icon tone assets to better match the updated theme/readability pass.

### Versioning
- Updated baked application version from `1.2.0` to `1.2.1` in `version.py`.

---

## 🛠 Fixes

### Cross-Platform
- Added runtime theme switching and improved readability in both light and dark themed UI paths.
- Updated dashboard/workshop status coloring for clearer visual state feedback.

### Windows
- No Windows-only fixes; theme/readability and UI improvements apply on Windows as part of the cross-platform updates.

### Linux and Steam Deck
- Fixed Reforged install failures where HTTPS download could fail with:
  - `SSL: CERTIFICATE_VERIFY_FAILED`
  - `unable to get local issuer certificate`
- Improved compatibility with SteamOS/AppImage-like bundled runtime certificate layouts.
- Theme/readability improvements and runtime theme switching also apply on Linux and Steam Deck.

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
  - Download: [PatchOpsIII v1.2.1 for Windows]({{WINDOWS_DOWNLOAD_URL}})
  - SHA256: `{{WINDOWS_SHA256}}`
  - VirusTotal: [Latest Windows Scan]({{WINDOWS_VT_URL}})

- **Linux & Steam Deck**
  - Download: [PatchOpsIII v1.2.1 for Linux & Steam Deck]({{LINUX_DOWNLOAD_URL}})
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

---

## 🔮 Upcoming Work
- Additional robustness improvements around mod/binary download and validation workflows.
- Expanded Linux and Steam Deck compatibility checks for launcher and workshop flows.
- Continued bug fixes and quality-of-life improvements based on user reports.

---

If you encounter issues or have suggestions, please open an issue on the repository or share feedback with the community so we can prioritize future improvements.
