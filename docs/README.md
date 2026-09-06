# Documentation

[Download PatchOpsIII](https://github.com/boggedbrush/PatchOpsIII/releases) · [Back to the overview](../README.md)

## Installation

- **Windows:** download and run the `.msi` installer.
- **Linux / Steam Deck:** install [Gear Lever](https://flathub.org/apps/it.mijorus.gearlever), then open the downloaded `.AppImage` with it to add PatchOpsIII to your app menu. On Steam Deck, use Desktop Mode.

The README download links point to the latest stable release. For betas, visit [GitHub Releases](https://github.com/boggedbrush/PatchOpsIII/releases).

## Architecture

The desktop app has two in-process layers:

- React, TypeScript, and Vite render the interface in the operating system webview.
- Tauri 2 hosts the window and calls the Rust workflow modules directly through typed commands and events.

There is no local HTTP API, WebSocket service, sidecar, or separate runtime to install. T7 Patch, DXVK, Enhanced, executable switching, Steam integration, configuration, logging, and filesystem safety checks all run inside the Tauri process.

Existing settings remain in the platform's `PatchOpsIII` application-data directory, including the legacy `electron-settings.json` filename, so upgrading does not discard a configured game path or release channel.

When upgrading an older PatchOpsIII-managed mod install, the Rust backend adopts legacy ownership only when exact verified payload hashes and recovery evidence are available. The recognition tables cover every T7 Patch and DXVK release the public legacy app could install; custom configs and all legacy LPC originals remain recoverable. Unknown, modified, incomplete, or linked files are left untouched instead of being guessed or deleted.

## Verify a download

Each release includes a matching `.sha256` file. Verify a Linux download with `sha256sum -c PatchOpsIII.AppImage.sha256`, or compare the Windows value with `Get-FileHash PatchOpsIII.msi -Algorithm SHA256` in PowerShell. Release notes link to the VirusTotal report for each package. CI verifies each artifact against its build hash and submits it separately when `VT_API_KEY` is configured.

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

Local Tauri packages are written below `src-tauri/target/release/bundle/`. The Checks workflow runs tests on Linux and Windows independently of packaging. Releases require checks and both package workflows to pass for the same commit. The build workflows publish canonical `PatchOpsIII.AppImage` and `PatchOpsIII.msi` artifacts with SHA-256 files. Linux CI starts and cleanly closes the AppImage on an Ubuntu 22.04 runner and repeats the check as a non-root user in a clean Ubuntu 22.04 container without system GTK or WebKit. These are headless X11 package smoke tests, not proof of compatibility with every distribution, display server, GPU driver, or Steam Deck configuration. Before uploading a Windows artifact, CI also installs the hash-pinned beta3 Electron MSI, verifies that the new Tauri MSI replaces it with one current product registration, launches the installed executable, observes its native window, and closes it cleanly.

The Windows MSI keeps the original Electron installer's UpgradeCode so an existing installation upgrades in place. `bun run verify:release` also enforces an ordered three-field MSI version: beta `M.m.p-betaN` maps to `M.m.(p*256+N)`, while stable `M.m.p` maps to `M.m.(p*256+255)`. Beta numbers must be 1–254 and patch numbers 0–255, matching [Windows Installer's three-field comparison rules](https://learn.microsoft.com/en-us/windows/win32/msi/productversion).

The workflows share `.github/actions/setup` for Bun, Rust, build caching, and dependency installation. Platform-specific system dependencies and package smoke tests stay in the build workflows. VirusTotal submission uses a reusable workflow, with separate artifacts, hashes, and reports for MSI and AppImage.

## Typical workflow

1. Launch PatchOpsIII and confirm the detected Black Ops III directory.
2. Install T7 Patch before using multiplayer.
3. Apply a launch profile and any desired graphics or quality-of-life settings.
4. Install DXVK or BO3 Enhanced only if they fit your platform and setup.
5. Review the activity log after file-changing operations.

## Forked components

- [BO3 Enhanced Proton fork metadata](../bo3-enhanced-proton/README.md)
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
- AppImage smoke coverage targets Ubuntu 22.04; other Linux environments and physical Steam Deck hardware are not covered.
- Report bugs through [GitHub Issues](https://github.com/boggedbrush/PatchOpsIII/issues) and include the in-app log plus your platform details. More usage notes live in the [project wiki](../wiki/home.md).

## Acknowledgements

- [Scroptss/T7Patch](https://github.com/Scroptss/T7Patch), continuing the original work by shiversoftdev.
- [dxvk-gplasync](https://gitlab.com/Ph42oN/dxvk-gplasync).
- [BO3 Enhanced](https://github.com/shiversoftdev/BO3Enhanced).
- [BO3 Reforged](https://bo3reforged.com/).

PatchOpsIII is released under the [MIT License](../LICENSE).
