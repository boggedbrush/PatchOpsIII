# BO3 Enhanced Proton

This directory stores metadata for the PatchOpsIII-maintained Proton fork used for BO3.
The full local tool payload (`bo3-enhanced-proton/BO3 Enhanced`) is optional, development-only,
and intentionally gitignored because of its size.

## Upstream
- Repository: https://github.com/Weather-OS/GDK-Proton
- Base release currently forked: https://github.com/Weather-OS/GDK-Proton/releases/tag/release10-32

## Fork Identity
- Fork/tool name: `BO3 Enhanced`
- Upstream release label: `GDK-Proton10-32`

## Purpose
- Provide a PatchOpsIII-branded compatibility tool for Black Ops III Enhanced.
- Keep upstream provenance explicit for troubleshooting and updates.

## PatchOpsIII Linux Flow
When BO3 Enhanced is installed from PatchOpsIII on Linux, the app now automates:
- Resolving Proton source in this order:
  - Local repo bundle at `bo3-enhanced-proton/BO3 Enhanced` (optional; development/offline only)
  - Cached download from upstream release `release10-32` (on demand)
- Installing the resolved tool into Steam `compatibilitytools.d` as `BO3 Enhanced`.
- Mapping BO3 (`AppID 311210`) to `BO3 Enhanced` in Steam `CompatToolMapping`.
- Setting BO3 launch options to `WINEDLLOVERRIDES="WindowsCodecs=n,b" %command%`.

## Repo Policy
- Keep this folder lightweight and metadata-focused.
- Do not commit the full `BO3 Enhanced` payload to this repository unless there is a specific release need.

## Update Policy
1. Start from a tagged upstream release.
2. Record the exact upstream tag in this file.
3. Document local changes (if any) before publishing a new fork build.
