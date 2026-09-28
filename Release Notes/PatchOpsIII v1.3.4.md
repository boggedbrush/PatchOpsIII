# PatchOpsIII v1.3.4 Release Notes

## Overview

PatchOpsIII v1.3.4 fixes Linux AppImage startup in sandboxed environments, including the AppImage catalog's Firejail test.

## Highlights

- Correct directory permissions inside the AppImage so Firejail can access the application.
- Verify the packaged filesystem's directory permissions before release.
- Test the AppImage at 800×600 using both Firejail and extraction-based launch.

## Detailed Changes

### Linux & Steam Deck

- Normalize all AppImage directories to mode `0755` before repacking. The v1.3.3 AppImage stored its directories as root-owned `0700`, which prevented Firejail from launching it.
- Inspect the stored SquashFS filesystem to catch inaccessible directories.
- Run the automated GUI smoke test with the AppImage catalog's Firejail runtime and with extraction-based startup.

## Upgrade Notes

- Linux users affected by the v1.3.3 AppImage launch failure should install v1.3.4.
- No settings migration is required.

## Downloads & Verification

The release workflow fills in the download URLs, SHA-256 hashes, and VirusTotal links below when publishing.

- **Windows MSI**
  - Download: [PatchOpsIII v1.3.4 for Windows]({{WINDOWS_DOWNLOAD_URL}})
  - SHA256: `{{WINDOWS_SHA256}}`
  - VirusTotal: [Windows MSI scan]({{WINDOWS_VT_URL}})
  - Older updater compatibility: [PatchOpsIII.exe](https://github.com/boggedbrush/PatchOpsIII/releases/download/v1.3.4/PatchOpsIII.exe)

- **Linux & Steam Deck**
  - Download: [PatchOpsIII v1.3.4 for Linux & Steam Deck]({{LINUX_DOWNLOAD_URL}})
  - SHA256: `{{LINUX_SHA256}}`
  - VirusTotal: [Linux scan]({{LINUX_VT_URL}})
  - Stable AppImages include update information and a matching zsync file.
