# PatchOpsIII v1.3.3 Release Notes

## Overview

PatchOpsIII v1.3.3 improves Linux AppImage compatibility and update support, and makes the desktop window usable on 800×600 displays.

## Highlights

- Build the Linux AppImage on Ubuntu 22.04 for compatibility with older glibc versions.
- Embed GitHub Release zsync update information in stable AppImages.
- Fit the desktop window to the available display and improve the compact dashboard layout.

## Detailed Changes

### Linux & Steam Deck

- Repack the AppImage with pinned, SHA-256-verified AppImageKit tools and runtime.
- Generate and publish matching zsync metadata for the stable AppImage.
- Verify AppImage extraction, embedded update information, and startup in the Linux build workflow.

### Desktop Layout

- Constrain the initial window size and minimum size to the available display.
- Keep navigation labels and dashboard controls readable at 800×600.
- Allow lower settings to remain accessible by scrolling.

## Upgrade Notes

- No settings migration is required.

## Downloads & Verification

The release workflow fills in the download URLs, SHA-256 hashes, and VirusTotal links below when publishing.

- **Windows MSI**
  - Download: [PatchOpsIII v1.3.3 for Windows]({{WINDOWS_DOWNLOAD_URL}})
  - SHA256: `{{WINDOWS_SHA256}}`
  - VirusTotal: [Windows MSI scan]({{WINDOWS_VT_URL}})
  - Older updater compatibility: [PatchOpsIII.exe](https://github.com/boggedbrush/PatchOpsIII/releases/download/v1.3.3/PatchOpsIII.exe)

- **Linux & Steam Deck**
  - Download: [PatchOpsIII v1.3.3 for Linux & Steam Deck]({{LINUX_DOWNLOAD_URL}})
  - SHA256: `{{LINUX_SHA256}}`
  - VirusTotal: [Linux scan]({{LINUX_VT_URL}})
  - Stable AppImages include update information and a matching zsync file.
