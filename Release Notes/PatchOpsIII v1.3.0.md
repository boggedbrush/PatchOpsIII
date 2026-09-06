# PatchOpsIII v1.3.0 Release Notes

## Overview

PatchOpsIII v1.3.0 brings the rebuilt desktop app to the stable channel. It combines the changes from `v1.3.0-beta`, `v1.3.0-beta2`, and `v1.3.0-beta3`, plus the T7Patch compatibility and DXVK download fixes added after beta3.

## Major Highlights

- Rebuilt desktop interface with a dashboard, live Activity Log, game folder browsing, launch profiles, graphics controls, and mod management.
- Dedicated EXE Swapper for the current Steam build, the compatible March 2023 build, and BO3 Enhanced when available.
- Clearer BO3 Enhanced setup with source validation, requirements, help, and diagnostics.
- Stable and Beta update channels selectable in Tools.
- Windows MSI installer and Linux/Steam Deck AppImage, with SHA-256 hashes and VirusTotal links supplied by the release workflow.
- T7Patch selection based on the verified game executable, and DXVK-GPLAsync downloads pinned to `v3.0-1`.

## Detailed Changes

### Desktop App

- Replaced the older interface with an Electron desktop app, a React/TypeScript interface, and a local Python helper for game maintenance actions.
- Added a responsive dashboard for game status, patch and mod tools, graphics settings, logs, and quick actions.
- Improved folder browsing, Steam actions, status checks, updates, and log handling.
- Improved T7 status and reset controls to better reflect installed files.
- Simplified startup to an “Opening PatchOpsIII” loading screen.
- Made the version label consistent during startup, fixed the beta title bar update button alignment, and made `package.json` the application version source.
- Reorganized Tools so system information, updates, logs, and cache actions are easier to scan.
- Improved Activity Log spacing, including when no entries are available.

### EXE Swapper

- Restored executable switching in a dedicated EXE Swapper section.
- Added options for the latest Steam executable, the compatible March 2023 executable, and BO3 Enhanced when available.
- Added active-build identification, executable integrity checks, and backup status.
- Show which T7Patch variant will be used for the selected executable.

### BO3 Enhanced and Launch Options

- Reworked Enhanced setup with source validation before installation.
- Added status, requirements, install details, help, and diagnostics to explain what is ready and what needs attention.
- Improved Enhanced page spacing and moved uninstall controls into a collapsed Danger Zone.
- Restored BO3 Reforged as a launch option.

### T7Patch

- Moved the main patch source to the maintained Scroptss/T7Patch project, initially using v3.02 during beta1 and following its latest release from beta2 onward.
- Added archive verification using the SHA-256 digest supplied by GitHub release metadata.
- Added Windows administrator approval for T7Patch installation through an elevated helper, including cancellation and failure reporting.
- Added executable-aware patch selection after beta3: current builds use the maintained Scroptss/T7Patch release; the compatible March 2023 build uses legacy T7 Patch 2.04.
- Block T7Patch installation when the game executable cannot be verified.
- Remove compatibility-only `discord_game_sdk.dll` and `zbr2.dll` files when installing the current-build patch.
- Keep LPC files on the legacy shiversoftdev/t7patch source because the maintained release does not provide them.

### DXVK-GPLAsync

- Pin downloads to the `v3.0-1` binary archive at a specific upstream commit.
- Remove dependence on the GitLab latest-release lookup and its archive selection.
- Use the pinned version when deciding which DXVK configuration options to write.

### Updates and Packaging

- Added Stable/Beta update channel controls inside the app.
- Changed Windows packaging during the beta cycle from executable/ZIP distribution to a branded MSI installer that installs to Program Files by default.
- Prefer MSI assets in the Windows updater, with EXE and ZIP handling retained as fallbacks.
- Ship `PatchOpsIII.msi` and a `PatchOpsIII.exe` installer for older updater compatibility.
- Ship `PatchOpsIII.AppImage` for Linux and Steam Deck, with AppImage update metadata when generated.
- Added SHA-256 verification and VirusTotal scanning to the release process, including scan links in published notes.

### Documentation

- Updated the README, wiki, and website for the rebuilt desktop app.
- Documented maintained T7Patch and legacy LPC sources, and added verification links to beta release notes.
- Updated installation instructions for the stable MSI and AppImage downloads.
- Preserved the individual beta release notes for reference.

## Known Issues and Requirements

- **App startup:** Earlier beta reports included startup failures on some systems. If PatchOpsIII does not finish opening, restart it and include logs and platform details in an issue report.
- **Windows updates:** If an in-app MSI update fails, download and run the MSI manually from GitHub.
- **EXE Swapper:** Switching to the compatible build may require the Steam depot download to exist locally. Follow the app's prompt if it is missing.
- **BO3 Enhanced:** Installation requires a user-provided UWP dump source. Follow the Enhanced page's dump guide and validate the source before installing.
- **All-around Enhancement Mod:** The full mod remains unsupported as a launch option. Use the Lite version instead.
- **Linux and Steam Deck launch options:** Behavior can vary across setups. If launch problems occur, remove custom launch options and reapply them incrementally.

## Downloads & Verification

The release workflow fills in the download URLs, SHA-256 hashes, and VirusTotal links below when publishing.

- **Windows MSI**
  - Download: [PatchOpsIII v1.3.0 for Windows]({{WINDOWS_DOWNLOAD_URL}})
  - SHA256: `{{WINDOWS_SHA256}}`
  - VirusTotal: [Windows MSI scan]({{WINDOWS_VT_URL}})
  - Older updater compatibility: [PatchOpsIII.exe](https://github.com/boggedbrush/PatchOpsIII/releases/download/v1.3.0/PatchOpsIII.exe)

- **Linux & Steam Deck**
  - Download: [PatchOpsIII v1.3.0 for Linux & Steam Deck]({{LINUX_DOWNLOAD_URL}})
  - SHA256: `{{LINUX_SHA256}}`
  - VirusTotal: [Linux scan]({{LINUX_VT_URL}})
  - AppImage update metadata is attached to the release when generated.

## Acknowledgements

PatchOpsIII builds on the work of these projects:

- **T7Patch:** Scroptss/T7Patch.
- **Legacy T7Patch and LPC files:** shiversoftdev/t7patch.
- **In memory of shiversoftdev:** Thank you for the original t7patch work and your contributions to the Black Ops III community.
- **DXVK-GPLAsync:** Ph42oN/dxvk-gplasync.
- **ValvePython/vdf.**
- **BO3 Enhanced:** shiversoftdev/BO3Enhanced.
- **BO3 Reforged.**

Report issues through the repository with logs, your platform, and steps to reproduce the problem.
