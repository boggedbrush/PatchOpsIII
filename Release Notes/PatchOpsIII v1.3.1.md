# PatchOpsIII v1.3.1 Release Notes

## Overview

PatchOpsIII v1.3.1 is a hotfix release that restores EXE Swapper support for the latest Steam version of Call of Duty: Black Ops III. The September 10, 2026 game update replaced `BlackOps3.exe`, so earlier PatchOpsIII releases could no longer recognize the current executable as trusted or preserve it when switching builds.

## Hotfix Highlights

- Added support for Black Ops III Steam BuildID `24784313`, released September 10, 2026.
- Updated executable integrity verification for the new `BlackOps3.exe` shipped in depot `311211` manifest `8824612235115253119`.
- Restored switching between the latest Steam executable and the compatible March 2023 executable.
- Updated EXE Swapper labels to show the latest build date consistently.

## Detailed Changes

### EXE Swapper

- Replaced the previous February 2026 current-build identifier and executable hash with the September 2026 values.
- The latest Steam executable is now recognized as a trusted current build instead of an unverified executable.
- New backups of the latest executable use BuildID `24784313`, allowing PatchOpsIII to preserve and restore the correct file when changing EXE profiles.
- The compatible target remains Steam BuildID `10650222` from March 3, 2023.

### App Interface

- Updated the Latest Build status to identify the September 10, 2026 Steam release.
- Made the Latest Build card use the build date reported by the local helper, preventing its description and status indicator from showing conflicting dates.

## Upgrade Notes

- This update is recommended for anyone whose Black Ops III installation has received Steam BuildID `24784313`.
- No settings migration is required.
- Existing compatible-build and BO3 Enhanced backups are unchanged.
- If Steam has not finished updating Black Ops III, allow the update to complete before using the EXE Swapper.

## Known Issues and Requirements

- **EXE Swapper:** Switching to the compatible build may require the Steam depot download to exist locally. Follow the app's prompt if it is missing.
- **BO3 Enhanced:** Installation requires a user-provided UWP dump source. Follow the Enhanced page's dump guide and validate the source before installing.
- **All-around Enhancement Mod:** The full mod remains unsupported as a launch option. Use the Lite version instead.
- **Linux and Steam Deck launch options:** Behavior can vary across setups. If launch problems occur, remove custom launch options and reapply them incrementally.

## Downloads & Verification

The release workflow fills in the download URLs, SHA-256 hashes, and VirusTotal links below when publishing.

- **Windows MSI**
  - Download: [PatchOpsIII v1.3.1 for Windows]({{WINDOWS_DOWNLOAD_URL}})
  - SHA256: `{{WINDOWS_SHA256}}`
  - VirusTotal: [Windows MSI scan]({{WINDOWS_VT_URL}})
  - Older updater compatibility: [PatchOpsIII.exe](https://github.com/boggedbrush/PatchOpsIII/releases/download/v1.3.1/PatchOpsIII.exe)

- **Linux & Steam Deck**
  - Download: [PatchOpsIII v1.3.1 for Linux & Steam Deck]({{LINUX_DOWNLOAD_URL}})
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
