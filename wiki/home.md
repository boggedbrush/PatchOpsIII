# PatchOpsIII Wiki

## Overview

PatchOpsIII is a React and Tauri desktop control center for Call of Duty: Black Ops III maintenance, mod setup, launch profiles, and performance tuning. The Rust workflow modules run in the desktop process; the app does not start a local API or helper process.

## Architecture

- **Tauri desktop shell:** owns the native window, file dialogs, external links, commands, events, and packaging.
- **React renderer:** provides the dashboard, mod controls, graphics settings, launch profiles, and activity log.
- **Rust workflows:** perform game discovery, verified downloads, safe file edits, Steam integration, configuration, logging, and rollback.
- **Static website:** documents features, downloads, and release highlights.

## Features

### Game directory

PatchOpsIII detects Steam library locations and supports a manually selected Black Ops III directory when the game is installed elsewhere.

Default Windows Steam path:

```text
C:\Program Files (x86)\Steam\steamapps\common\Call of Duty Black Ops III
```

### T7 Patch

Supported controls include install/update, uninstall, gamertag and color configuration, network passwords, Friends Only mode, and LPC files. Core patch downloads use the maintained [Scroptss/T7Patch](https://github.com/Scroptss/T7Patch) release and are validated before installation.

### BO3 Enhanced

PatchOpsIII supports source validation, install, uninstall, verification, status, executable selection, and platform-specific launch setup for BO3 Enhanced.

### DXVK

DXVK-GPLAsync can be installed, configured, verified, and removed from the Mods page. PatchOpsIII preserves original game files before replacement.

### Graphics and quality of life

PatchOpsIII edits common Black Ops III settings including FPS limits, FOV, display mode, render resolution, V-Sync, FPS counter visibility, smooth framerate, VRAM usage, frame latency, CPU usage, intro video skipping, and read-only config locking.

### Launch profiles

Curated profiles include:

- Default Steam launch
- Offline launch
- All-around Enhancement Lite
- Ultimate Experience Mod
- BO3 Reforged

### Logs and maintenance

The in-app activity log records actions and errors. Maintenance tools clear managed caches and restore supported backups without deleting unrelated game files.

## Development

Install Bun, stable Rust, and the Tauri prerequisites for your platform, then run:

```bash
bun install --frozen-lockfile
bun run verify
bun run dev
```

Build native packages on their target platform:

```bash
bun run dist:win    # Windows MSI
bun run dist:linux  # Linux AppImage
```

Historical release notes may describe previous implementations. Current development should use typed Tauri commands and in-process Rust workflows.
