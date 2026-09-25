# PatchOpsIII v1.3.2 Release Notes

## Overview

PatchOpsIII v1.3.2 improves T7Patch installation reliability by resolving the latest Scroptss release and locating its current archive, even when upstream changes the ZIP filename.

## Highlights

- Resolve T7Patch downloads from the upstream latest release.
- Discover renamed ZIP assets when the standard or versioned archive names are unavailable.
- Select a compatible platform archive, or a clearly universal archive when one is available.
- Require the GitHub-provided SHA-256 digest before selecting a discovered archive.

## Detailed Changes

### T7Patch

- Added support for versioned release archive names and renamed release ZIP assets.
- Keep archive selection limited to compatible or platform-neutral packages; reject explicitly incompatible platform archives.
- Verify discovered archives using the SHA-256 digest published by GitHub.

## Upgrade Notes

- No settings migration is required.
- T7Patch setup continues to download the latest supported upstream release.

## Known Issues and Requirements

- **EXE Swapper:** Switching to the compatible build may require the Steam depot download to exist locally. Follow the app's prompt if it is missing.
- **BO3 Enhanced:** Installation requires a user-provided UWP dump source. Follow the Enhanced page's dump guide and validate the source before installing.
- **All-around Enhancement Mod:** The full mod remains unsupported as a launch option. Use the Lite version instead.
- **Linux and Steam Deck launch options:** Behavior can vary across setups. If launch problems occur, remove custom launch options and reapply them incrementally.

## Downloads & Verification

The release workflow fills in the download URLs, SHA-256 hashes, and VirusTotal links below when publishing.

- **Windows MSI**
  - Download: [PatchOpsIII v1.3.2 for Windows]({{WINDOWS_DOWNLOAD_URL}})
  - SHA256: `{{WINDOWS_SHA256}}`
  - VirusTotal: [Windows MSI scan]({{WINDOWS_VT_URL}})
  - Older updater compatibility: [PatchOpsIII.exe](https://github.com/boggedbrush/PatchOpsIII/releases/download/v1.3.2/PatchOpsIII.exe)

- **Linux & Steam Deck**
  - Download: [PatchOpsIII v1.3.2 for Linux & Steam Deck]({{LINUX_DOWNLOAD_URL}})
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
