# PatchOpsIII

[![Latest Release](https://img.shields.io/github/v/release/boggedbrush/PatchOpsIII?style=for-the-badge&color=0a84ff)](https://github.com/boggedbrush/PatchOpsIII/releases)
[![GitHub Downloads (all assets, all releases)](https://img.shields.io/github/downloads/boggedbrush/patchopsiii/total.svg?style=for-the-badge&color=34c759&cacheSeconds=300)](https://github.com/boggedbrush/PatchOpsIII/releases)
[![GitHub Stars](https://img.shields.io/github/stars/boggedbrush/PatchOpsIII?style=for-the-badge&color=ff9f0a)](https://github.com/boggedbrush/PatchOpsIII/stargazers)
[![GitHub Issues](https://img.shields.io/github/issues/boggedbrush/PatchOpsIII?style=for-the-badge&color=ff453a)](https://github.com/boggedbrush/PatchOpsIII/issues)
[![License](https://img.shields.io/github/license/boggedbrush/PatchOpsIII?style=for-the-badge&color=5e5ce6)](LICENSE)

> **PatchOpsIII** is a modern, full-featured control center for Call of Duty: Black Ops III modding, maintenance, and performance tuning.

---

![Program Screenshot](https://github.com/user-attachments/assets/a79e7273-4274-4a43-8d4d-e81a12cbd1ff)

---

## Table of Contents
- [Overview](#overview)
- [Key Features](#key-features)
  - [Mods Tab](#mods-tab)
  - [Graphics Tab](#graphics-tab)
  - [Advanced Tab](#advanced-tab)
  - [Terminal & Logging](#terminal--logging)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Screenshots](#screenshots)
- [Known Issues](#known-issues)
- [Support](#support)
- [Special Thanks](#special-thanks)
- [License](#license)
- [Star History](#star-history)

## Overview
PatchOpsIII streamlines the setup and upkeep of Black Ops III by surfacing popular community tools and quality-of-life tweaks in a single polished interface. The Python application ships with dark/light themes, tabbed navigation (Mods, Graphics, Advanced), and Nuitka builds for Windows and Linux. Whether you are securing your game with T7 Patch, smoothing shader compilation stutter with DXVK, or fine-tuning launch options, PatchOpsIII consolidates every workflow into one cohesive experience.

## Key Features

### Mods Tab
- **Smart Game Directory Detection:** Automatically locates your Black Ops III installation or lets you browse manually.
- **T7 Patch Management:** Install, update, configure gamertags and colors, apply network passwords, toggle Friends Only mode, deploy LPC fixes, and cleanly uninstall.
- **DXVK-GPLAsync Integration:** Deploy and remove Vulkan-based shader compilation to smooth frametimes by reducing shader cache stutter.
- **Workshop Helper:** One-click access to curated Steam Workshop mods and documentation.
- **Launch Profiles:** Preset command-line configurations for Offline play, [All-around Enhancement Lite](https://steamcommunity.com/sharedfiles/filedetails/?id=2994481309), and [Ultimate Experience Mod](https://steamcommunity.com/sharedfiles/filedetails/?id=2942053577).

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
2. **Extract:** Unzip the package to a preferred folder outside of your game directory.
3. **Run:** Launch `PatchOpsIII.exe` on Windows (or the corresponding binary for other platforms as they become available).
4. **Dependencies:** The packaged build bundles all required Python dependencies; no additional setup is needed.

### Developer Setup
```bash
# clone the repository
 git clone https://github.com/boggedbrush/PatchOpsIII.git
 cd PatchOpsIII

# create a virtual environment
 python -m venv .venv
 source .venv/bin/activate  # On Windows use: .venv\Scripts\activate

# install dependencies
 pip install -r requirements.txt

# run the application
 python main.py
```

## Quick Start
1. Launch PatchOpsIII and verify your Black Ops III directory.
2. Apply the **T7 Patch** to secure multiplayer connectivity and remove RCE vulnerabilities.
3. Enable **DXVK-GPLAsync** for async shader compilation and smoother frametimes.
4. Choose a graphics preset or dial in custom display options.
5. Visit the **Advanced** tab to unlock VRAM, tweak frame latency, and set your config to read-only once satisfied.

## Screenshots
<table>
  <tr>
    <td align="center"><img src="https://github.com/user-attachments/assets/a79e7273-4274-4a43-8d4d-e81a12cbd1ff" alt="Mods Tab" /></td>
    <td align="center"><img src="https://github.com/user-attachments/assets/1188883e-7bf2-464e-a4a7-5f9806806fb2" alt="Graphics Tab" /></td>
  </tr>
  <tr>
    <td align="center" colspan="2"><img src="https://github.com/user-attachments/assets/0c6ba5d4-f86a-4645-9b07-a5667c8305b9" alt="Advanced Tab" /></td>
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
  [https://github.com/shiversoftdev/t7patch](https://github.com/shiversoftdev/t7patch)
- **dxvk-gplasync** – Vulkan translation layer with async shader compilation.  
  [https://gitlab.com/Ph42oN/dxvk-gplasync](https://gitlab.com/Ph42oN/dxvk-gplasync)
- **ValvePython/vdf** – Reliable Steam VDF parsing utilities used throughout PatchOpsIII.  
  [https://github.com/ValvePython/vdf](https://github.com/ValvePython/vdf)

## License
PatchOpsIII is released under the [MIT License](LICENSE).

## Star History
[![Star History Chart](https://api.star-history.com/svg?repos=boggedbrush/PatchOpsIII&type=Date)](https://star-history.com/#boggedbrush/PatchOpsIII&Date)
