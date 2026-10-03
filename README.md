# PatchOpsIII

[![Latest Release](https://img.shields.io/github/v/release/boggedbrush/PatchOpsIII?style=for-the-badge&color=0a84ff)](https://github.com/boggedbrush/PatchOpsIII/releases)
[![GitHub Downloads (all assets, all releases)](https://img.shields.io/github/downloads/boggedbrush/patchopsiii/total.svg?style=for-the-badge&color=34c759&cacheSeconds=300)](https://github.com/boggedbrush/PatchOpsIII/releases)
[![GitHub Stars](https://img.shields.io/github/stars/boggedbrush/PatchOpsIII?style=for-the-badge&color=ff9f0a)](https://github.com/boggedbrush/PatchOpsIII/stargazers)
[![GitHub Issues](https://img.shields.io/github/issues/boggedbrush/PatchOpsIII?style=for-the-badge&color=ff453a)](https://github.com/boggedbrush/PatchOpsIII/issues)
[![License](https://img.shields.io/github/license/boggedbrush/PatchOpsIII?style=for-the-badge&color=5e5ce6)](LICENSE)

> **PatchOpsIII** is a modern, full-featured control center for Call of Duty: Black Ops III modding, maintenance, and performance tuning.

---

![PatchOpsIII Dashboard](https://raw.githubusercontent.com/boggedbrush/PatchOpsIII/main/website/assets/img/screenshots/dashboard.png)

---

## Table of Contents
- [Overview](#overview)
- [Key Features](#key-features)
  - [Mods Tab](#mods-tab)
  - [Graphics Tab](#graphics-tab)
  - [Advanced Tab](#advanced-tab)
  - [Terminal & Logging](#terminal--logging)
- [Installation](#installation)
- [Native desktop architecture](#native-desktop-architecture)
- [Legacy Electron app](#legacy-electron-app)
- [Forked Components](#forked-components)
- [Quick Start](#quick-start)
- [Screenshots](#screenshots)
- [Known Issues](#known-issues)
- [Support](#support)
- [Special Thanks](#special-thanks)
- [License](#license)
- [Star History](#star-history)

## Overview
PatchOpsIII streamlines the setup and upkeep of Black Ops III by surfacing popular community tools and quality-of-life tweaks in a native Rust/GPUI desktop interface backed by a local Python API. Whether you are securing your game with T7 Patch, smoothing shader compilation stutter with DXVK, or fine-tuning launch options, PatchOpsIII consolidates every workflow into one cohesive experience. The previous Electron/React client remains available in this repository as a legacy fallback.

## Key Features

### Mods Tab
- **Smart Game Directory Detection:** Automatically locates your Black Ops III installation or lets you browse manually.
- **T7 Patch Management:** Install, update, configure gamertags and colors, apply network passwords, toggle Friends Only mode, deploy LPC fixes, and cleanly uninstall.
- **DXVK-GPLAsync Integration:** Deploy and remove Vulkan-based shader compilation to smooth frametimes by reducing shader cache stutter.
- **Workshop Helper:** One-click access to curated Steam Workshop mods and documentation.
- **Launch Profiles:** Preset command-line configurations for Default, Play Offline, [All-around Enhancement Lite](https://steamcommunity.com/sharedfiles/filedetails/?id=2994481309), and [Ultimate Experience Mod](https://steamcommunity.com/sharedfiles/filedetails/?id=2942053577).

### Graphics Tab
- **Preset Loader:** Apply curated JSON presets to instantly switch between visual configurations.
- **Convenience Sliders:** Tweak FOV, display mode, resolution, refresh rate, render resolution %, V-Sync, and FPS counters.
- **Intro Skip & FPS Limiter:** Automate `.mkv` renames and adjust FPS limits from 0–1000 for faster load times and smoother gameplay.

### Advanced Tab
- **Power Tweaks:** Toggle SmoothFramerate, unlock full VRAM usage, reduce CPU pressure, manage frame latency, and expose hidden graphics options by editing `config.ini` safely.
- **Stutter Fixes:** Automate DirectX DLL renaming to keep shader compilation modern and responsive.
- **Config Safeguards:** Set configuration files read-only to preserve your optimized setup.

### Terminal & Logging
- Embedded console view provides live feedback on every action.
- Automatic `PatchOpsIII.log` generation captures a detailed audit trail for troubleshooting and support.

## Installation
1. **Download:** Grab the latest release from the [Releases page](https://github.com/boggedbrush/PatchOpsIII/releases).
2. **Windows:** Download `PatchOpsIII-native-windows-x64.zip`, extract the entire `PatchOpsIII` folder, and run `patchopsiii-gpui.exe` from that folder.
3. **Linux & Steam Deck:** Download `PatchOpsIII-native-linux-x64.zip`, extract it, run `chmod +x PatchOpsIII/patchopsiii-gpui`, then launch that executable from a desktop session.
4. **Dependencies:** Keep the executable and its `resources` directory together. The archive bundles the Python backend, presets, and icons; Python is not required on the destination system. Linux still requires a supported X11/Wayland session, system fonts, and working graphics drivers.

The existing `PatchOpsIII.msi`, `PatchOpsIII.exe`, and `PatchOpsIII.AppImage` release assets are the legacy Electron fallback while native installer/updater work remains open.

### Developer Setup

The primary desktop app uses Rust 1.99.0, GPUI 0.2.2, and GPUI Component 0.5.0. Python runs the existing local operations API.

```bash
# install Python service dependencies
python -m venv .venv
source .venv/bin/activate  # On Windows use: .venv\Scripts\activate
pip install -r requirements.txt

# build and run the native app with the source backend
python scripts/gpui.py build
python scripts/gpui.py run
# Equivalent package scripts: bun run gpui:build / bun run gpui:run
```

See [native/gpui/README.md](native/gpui/README.md) for platform build packages, release packaging, validation commands, and known parity gaps. The development launcher gives its Python API an OS-assigned loopback port. A packaged native binary locates and owns the bundled backend itself.

## Native desktop architecture

The primary Rust/GPUI client lives in [native/gpui](native/gpui/README.md). It uses native controls without Chromium, React, a WebView, or Tauri, and supervises the packaged Python operations service on an ephemeral loopback port. See [the native desktop architecture and migration record](docs/gpui-architecture.md) for the original research, comparison data, supported workflows, and remaining parity work.

## Legacy Electron app

The Electron/React application in `src/` remains buildable as a fallback. To work on it, install Bun dependencies with `bun install`, then use `bun run dev:desktop`; `bun run dist:linux` and `bun run dist:win` retain the established AppImage/MSI/NSIS builds. The browser development server remains available through `bun run dev`. Electron and GPUI share backend settings, logs, caches, and game files, so do not perform overlapping game mutations in both clients.

## Forked Components

- **BO3 Enhanced Proton fork metadata:** [bo3-enhanced-proton/README.md](bo3-enhanced-proton/README.md)
  - Upstream source: https://github.com/Weather-OS/GDK-Proton
  - Current base release: `release10-32`
  - Local `bo3-enhanced-proton/BO3 Enhanced` content is optional for development/offline workflow and is intentionally gitignored.
  - Normal Linux installs do not require this local bundle; PatchOpsIII downloads and caches the upstream release on demand.

## Quick Start
1. Launch PatchOpsIII and verify your Black Ops III directory.
2. Apply the **T7 Patch** to secure multiplayer connectivity and remove RCE vulnerabilities.
3. Enable **DXVK-GPLAsync** for async shader compilation and smoother frametimes.
4. Choose a graphics preset or dial in custom display options.
5. Visit the **Advanced** tab to unlock VRAM, tweak frame latency, and set your config to read-only once satisfied.

## Screenshots

These screenshots show the legacy Electron client; updated native screenshots are tracked with the native UI work.
<table>
  <tr>
    <td align="center"><img src="https://raw.githubusercontent.com/boggedbrush/PatchOpsIII/main/website/assets/img/screenshots/dashboard.png" alt="Dashboard" /><br/><sub>Dashboard</sub></td>
    <td align="center"><img src="https://raw.githubusercontent.com/boggedbrush/PatchOpsIII/main/website/assets/img/screenshots/t7patch.png" alt="T7 Patch" /><br/><sub>T7 Patch</sub></td>
  </tr>
  <tr>
    <td align="center"><img src="https://raw.githubusercontent.com/boggedbrush/PatchOpsIII/main/website/assets/img/screenshots/enhanced.png" alt="BO3 Enhanced" /><br/><sub>Enhanced</sub></td>
    <td align="center"><img src="https://raw.githubusercontent.com/boggedbrush/PatchOpsIII/main/website/assets/img/screenshots/graphics.png" alt="Graphics" /><br/><sub>Graphics</sub></td>
  </tr>
  <tr>
    <td align="center" colspan="2"><img src="https://raw.githubusercontent.com/boggedbrush/PatchOpsIII/main/website/assets/img/screenshots/dxvk.png" alt="DXVK" /><br/><sub>DXVK</sub></td>
  </tr>
  <tr>
    <td align="center" colspan="2"><img src="https://raw.githubusercontent.com/boggedbrush/PatchOpsIII/main/website/assets/img/screenshots/advanced.png" alt="Advanced" /><br/><sub>Advanced</sub></td>
  </tr>
</table>

## Known Issues
- Full version of the [All-around Enhancement Mod](https://steamcommunity.com/sharedfiles/filedetails/?id=2631943123) currently crashes before the game finishes launching, so it is not exposed as a launch option in PatchOpsIII.
- Launch option stability can vary between systems; experiment to find a stable configuration.
- A few advanced toggles remain in beta testing—report issues via GitHub.

## Support
- 📚 Explore detailed usage notes in the [project wiki](wiki/home.md).
- 🐛 Report bugs or request features through [GitHub Issues](https://github.com/boggedbrush/PatchOpsIII/issues).
- 💬 Join the community discussion on Discord *(coming soon)*.

## Special Thanks
This project would not be possible without the incredible work of the broader community:

- **t7patch** – Security and stability backbone for Black Ops III multiplayer.  
  Original work by `shiversoftdev`, continued by `Scroptss`: [https://github.com/Scroptss/T7Patch](https://github.com/Scroptss/T7Patch)
- **dxvk-gplasync** – Vulkan translation layer with async shader compilation.  
  [https://gitlab.com/Ph42oN/dxvk-gplasync](https://gitlab.com/Ph42oN/dxvk-gplasync)
- **ValvePython/vdf** – Reliable Steam VDF parsing utilities used throughout PatchOpsIII.  
  [https://github.com/ValvePython/vdf](https://github.com/ValvePython/vdf)

## License
PatchOpsIII is released under the [MIT License](LICENSE).

## Star History
[![Star History Chart](https://api.star-history.com/svg?repos=boggedbrush/PatchOpsIII&type=Date)](https://star-history.com/#boggedbrush/PatchOpsIII&Date)
