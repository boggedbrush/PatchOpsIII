# PatchOpsIII Wiki

## Overview

PatchOpsIII is an Electron-first control center for Call of Duty: Black Ops III maintenance, mod setup, and performance tuning. The desktop app uses a React/TypeScript renderer for the user interface and a local Python API backend for filesystem, Steam, and game-configuration operations.

The legacy Python Qt interface and old Python packaging path have been removed from this branch. Development and user-facing work should target the Electron renderer plus the local backend API.

## Architecture

- **Electron desktop shell:** starts and packages the application experience.
- **React renderer:** provides the dashboard, mod controls, graphics settings, launch profile controls, and logs.
- **Local Python API:** performs privileged local work such as game directory detection, file edits, downloads, verification, and Steam launch helpers.
- **Static website:** documents features, downloads, and release highlights.

## Features

### Dashboard

The dashboard surfaces game directory status, installed component state, and the main maintenance workflows without requiring users to move through the old tabbed Python UI.

### Game Directory

PatchOpsIII can detect the default Steam install path for Call of Duty: Black Ops III and also supports a manually selected game directory when the install lives elsewhere.

Default Steam path:

```text
C:\Program Files (x86)\Steam\steamapps\common\Call of Duty Black Ops III
```

### T7 Patch

PatchOpsIII manages the community T7 Patch workflow through backend helpers that no longer depend on Qt widgets. Supported controls include install/update, uninstall, gamertag configuration, password configuration, Friends Only mode, and LPC install handling.

Patch downloads use the maintained [Scroptss/T7Patch](https://github.com/Scroptss/T7Patch) latest release for core T7Patch files and verify the archive against GitHub release metadata. LPC files still come from the legacy release because the maintained fork does not publish that asset.

### BO3 Enhanced

PatchOpsIII includes BO3 Enhanced install, uninstall, verification, status, and launch support. Enhanced is the primary supported compatibility surface for users who need that mod path.

### DXVK

DXVK-GPLAsync install and uninstall support is handled through headless backend helpers. This keeps shader-stutter mitigation available to the Electron frontend without retaining the legacy Python UI.

### Graphics and Quality of Life

PatchOpsIII can edit common Black Ops III configuration settings, including FPS limits, FOV, display mode, render resolution, V-Sync, FPS counter visibility, smooth framerate, full VRAM usage, latency, CPU usage, intro video skipping, and read-only config locking.

### Launch Profiles

PatchOpsIII supports curated launch profiles for:

- Default Steam launch
- Offline launch
- All-around Enhancement Lite
- Ultimate Experience Mod

The deprecated Reforged workflow is no longer part of the active product surface.

### Logs and Tools

The Electron interface includes log output and tool actions so users can inspect what PatchOpsIII changed and diagnose failures without relying on the removed Python terminal window.

## Development

Install dependencies into the project virtual environment and the Electron workspace:

```bash
.venv/bin/pip install -r requirements.txt
bun install
```

Run the local API and Electron frontend together:

```bash
bun run dev
```

Run the desktop shell:

```bash
bun run dev:desktop
```

## Notes

- Do not reintroduce Qt, PySide, or Python packaging-only workflows for new frontend work.
- Keep backend modules headless so Electron can call them without importing legacy UI dependencies.
- Historical release notes may mention removed workflows, but current documentation should describe the Electron-first app.
