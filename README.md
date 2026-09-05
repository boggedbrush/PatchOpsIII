# PatchOpsIII

[![Latest Release](https://img.shields.io/github/v/release/boggedbrush/PatchOpsIII?style=for-the-badge&color=0a84ff)](https://github.com/boggedbrush/PatchOpsIII/releases)
[![Downloads](https://img.shields.io/github/downloads/boggedbrush/PatchOpsIII/total.svg?style=for-the-badge&color=34c759&cacheSeconds=300)](https://github.com/boggedbrush/PatchOpsIII/releases)
[![Stars](https://img.shields.io/github/stars/boggedbrush/PatchOpsIII?style=for-the-badge&color=ff9f0a)](https://github.com/boggedbrush/PatchOpsIII/stargazers)
[![Issues](https://img.shields.io/github/issues/boggedbrush/PatchOpsIII?style=for-the-badge&color=ff453a)](https://github.com/boggedbrush/PatchOpsIII/issues)
[![License](https://img.shields.io/github/license/boggedbrush/PatchOpsIII?style=for-the-badge&color=5e5ce6)](LICENSE)

PatchOpsIII is a desktop control center for Call of Duty: Black Ops III maintenance, mod setup, launch profiles, and performance tuning.

![PatchOpsIII Dashboard](https://raw.githubusercontent.com/boggedbrush/PatchOpsIII/main/website/assets/img/screenshots/dashboard.png)

## What it manages

- Automatic or manual Black Ops III installation discovery.
- T7 Patch installation, updates, configuration, LPC fixes, and removal.
- BO3 Enhanced setup, source validation, diagnostics, and removal.
- DXVK-GPLAsync installation, configuration, and removal.
- Current, compatible, and Enhanced executable switching with backup recovery.
- Steam launch profiles and curated Workshop profiles.
- Graphics presets, display settings, frame limits, intro skipping, VRAM tuning, and advanced config values.
- Maintenance actions, cache cleanup, and an in-app activity log.

PatchOpsIII validates downloads and archives before replacing game files. Mutating workflows preserve backups and use rollback or atomic replacement where the platform supports it.

## Architecture

The desktop app has two in-process layers:

- React, TypeScript, and Vite render the interface in the operating system webview.
- Tauri 2 hosts the window and calls the Rust workflow modules directly through typed commands and events.

There is no local HTTP API, WebSocket service, sidecar, or separate runtime to install. T7 Patch, DXVK, Enhanced, executable switching, Steam integration, configuration, logging, and filesystem safety checks all run inside the Tauri process.

Existing settings remain in the platform's `PatchOpsIII` application-data directory, including the legacy `electron-settings.json` filename, so upgrading does not discard a configured game path or release channel.

When upgrading an older PatchOpsIII-managed mod install, the Rust backend adopts legacy ownership only when exact verified payload hashes and recovery evidence are available. The recognition tables cover every T7 Patch and DXVK release the public legacy app could install; custom configs and all legacy LPC originals remain recoverable. Unknown, modified, incomplete, or linked files are left untouched instead of being guessed or deleted.

## Installation and platform support

Release packages are available from [GitHub Releases](https://github.com/boggedbrush/PatchOpsIII/releases):

- Windows 10/11 x64: `PatchOpsIII.msi` (or `PatchOpsIII-Beta.msi` for prereleases).
- Linux x86_64 and Steam Deck desktop mode: `PatchOpsIII.AppImage` (or `PatchOpsIII-Beta.AppImage`) is the portable application bundle.

macOS is not currently packaged. Platform-specific game workflows are shown only where they apply.

On Windows, run the MSI. On Linux, make the AppImage executable and launch it:

```bash
chmod +x PatchOpsIII.AppImage
./PatchOpsIII.AppImage
```

Each release includes a matching `.sha256` file. Verify a Linux download with `sha256sum -c PatchOpsIII.AppImage.sha256`, or compare the Windows value with `Get-FileHash PatchOpsIII.msi -Algorithm SHA256` in PowerShell. Release notes also link to the VirusTotal hash lookup for each package; CI submits a scan when `VT_API_KEY` is configured.

## Developer setup

Install [Bun](https://bun.com/docs/installation), the stable [Rust toolchain](https://www.rust-lang.org/tools/install), and the [Tauri system prerequisites](https://v2.tauri.app/start/prerequisites/) for your platform. On Ubuntu 22.04, the local build dependencies are:

```bash
sudo apt-get install build-essential curl file libayatana-appindicator3-dev \
  librsvg2-dev libssl-dev libwebkit2gtk-4.1-dev libx11-dev libxdo-dev \
  patchelf unzip wget xdg-utils
```

Then install dependencies and run the desktop app:

```bash
bun install --frozen-lockfile
bun run dev
```

The Vite server used by `tauri dev` serves only development UI assets; application operations still run as direct in-process Rust commands.

Useful checks and builds:

```bash
bun run typecheck       # TypeScript
bun run test:renderer   # renderer tests
bun run test:rust       # Rust tests
bun run verify          # all checks and tests
bun run build           # production renderer assets
bun run dist:linux      # AppImage, on Linux
bun run dist:win        # MSI, on Windows
```

Local Tauri packages are written below `src-tauri/target/release/bundle/`. CI tests before packaging and publishes canonical `PatchOpsIII.AppImage` and `PatchOpsIII.msi` artifacts with SHA-256 files. Linux CI starts and cleanly closes the AppImage on an Ubuntu 22.04 runner and repeats the AppImage startup check as a non-root user in a fresh `archlinux:base` container. These are headless X11 package smoke tests, not proof of compatibility with every distribution, display server, GPU driver, or Steam Deck configuration. Before uploading a Windows artifact, CI also installs the hash-pinned beta3 Electron MSI, verifies that the new Tauri MSI replaces it with one current product registration, launches the installed executable, observes its native window, and closes it cleanly.

The Windows MSI keeps the original Electron installer's UpgradeCode so an existing installation upgrades in place. `bun run verify:release` also enforces an ordered three-field MSI version: beta `M.m.p-betaN` maps to `M.m.(p*256+N)`, while stable `M.m.p` maps to `M.m.(p*256+255)`. Beta numbers must be 1–254 and patch numbers 0–255, matching [Windows Installer's three-field comparison rules](https://learn.microsoft.com/en-us/windows/win32/msi/productversion).

## Typical workflow

1. Launch PatchOpsIII and confirm the detected Black Ops III directory.
2. Install T7 Patch before using multiplayer.
3. Apply a launch profile and any desired graphics or quality-of-life settings.
4. Install DXVK or BO3 Enhanced only if they fit your platform and setup.
5. Review the activity log after file-changing operations.

## Forked components

- [BO3 Enhanced Proton fork metadata](bo3-enhanced-proton/README.md)
- Upstream base: [Weather-OS/GDK-Proton](https://github.com/Weather-OS/GDK-Proton), release `release10-32`
- The optional local `bo3-enhanced-proton/BO3 Enhanced` content is ignored; normal Linux installs download and cache the validated upstream release when needed.

## Screenshots

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
</table>

## Known issues and support

- The full [All-around Enhancement Mod](https://steamcommunity.com/sharedfiles/filedetails/?id=2631943123) is not exposed as a launch profile because it can crash before the game finishes launching; use the Lite profile.
- Steam launch-option behavior can vary across Linux distributions and Proton versions.
- AppImage smoke coverage currently includes Ubuntu 22.04 and an Arch base container, not every Linux environment or physical Steam Deck hardware.
- Report bugs through [GitHub Issues](https://github.com/boggedbrush/PatchOpsIII/issues) and include the in-app log plus your platform details. More usage notes live in the [project wiki](wiki/home.md).

## Acknowledgements

- [Scroptss/T7Patch](https://github.com/Scroptss/T7Patch), continuing the original work by shiversoftdev.
- [dxvk-gplasync](https://gitlab.com/Ph42oN/dxvk-gplasync).
- [BO3 Enhanced](https://github.com/shiversoftdev/BO3Enhanced).
- [BO3 Reforged](https://bo3reforged.com/).

PatchOpsIII is released under the [MIT License](LICENSE).
