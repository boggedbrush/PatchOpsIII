# PatchOpsIII v1.3.0-beta4 Release Notes

## Overview

This beta replaces the Electron and Python desktop stack with a smaller React + Tauri application whose native workflows run directly in Rust. Existing settings remain compatible, while install, update, uninstall, backup, and rollback paths now use explicit ownership records and fail closed when legacy ownership is ambiguous.

## Highlights

- Tauri is now the only desktop host; normal operation starts no Electron, Python, FastAPI, localhost API, sidecar, or worker process.
- Steam detection, configuration, T7 Patch, DXVK, executable swapping, BO3 Enhanced, downloads, archives, settings, and maintenance workflows now run in-process in Rust.
- T7, DXVK, BO3 Enhanced, and Linux compatibility installs preserve exact originals, verify hashes, and refuse destructive cleanup when ownership or recovery data is incomplete.
- Windows releases use MSI packaging. Linux and Steam Deck releases use the portable, self-contained AppImage as the Linux package.
- Linux CI smoke-tests AppImage startup and shutdown on Ubuntu and a fresh Arch base container.

## Compatibility and safety

- Existing `electron-settings.json` data is retained so configured game paths and release channels survive the migration.
- Steam's most-recent account is preferred when launch options are read or updated.
- Legacy T7 and DXVK installs are adopted only when their DLLs exactly match one of the official releases the public legacy app could install; custom configs and every LPC original remain preserved. Modified, incomplete, mixed-release, or unknown payloads are left untouched with an actionable error.
- Legacy BO3 Enhanced state is adopted only when the checksum-verified cached archive and selected dump prove every tracked active file. Existing installations without enough ownership evidence remain untouched for manual recovery.
- The Windows installer retains the beta3 MSI family identity and uses an ordered beta version, so Windows CI can verify an in-place beta3-to-beta4 upgrade, native window startup, and clean shutdown before publishing.
- Release tags are immutable: the release workflow creates a missing version tag but refuses to move an existing tag.

## Downloads and verification

- **Windows**
  - Download: [PatchOpsIII v1.3.0-beta4 for Windows]({{WINDOWS_DOWNLOAD_URL}})
  - SHA256: `{{WINDOWS_SHA256}}`
  - SHA256 file: [{{WINDOWS_SHA256_FILENAME}}]({{WINDOWS_SHA256_URL}})
  - VirusTotal: {{WINDOWS_VT_STATUS_OR_URL}}

- **Linux AppImage and Steam Deck**
  - Download: [PatchOpsIII v1.3.0-beta4 for Linux and Steam Deck]({{LINUX_DOWNLOAD_URL}})
  - SHA256: `{{LINUX_SHA256}}`
  - SHA256 file: [{{LINUX_SHA256_FILENAME}}]({{LINUX_SHA256_URL}})
  - VirusTotal: {{LINUX_VT_STATUS_OR_URL}}

## Known limitations

- MSI creation and execution are Windows-only. The Windows GitHub Actions runner builds the installer, upgrade-smokes it, opens its native window, and closes it cleanly before publishing; a Linux host cannot perform that final gate.
- AppImage package smoke coverage includes Ubuntu 22.04 and a non-root run in a fresh Arch base container under headless X11. It does not establish compatibility with every distribution, display server, GPU driver, or physical Steam Deck setup.
