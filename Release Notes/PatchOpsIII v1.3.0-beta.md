# PatchOpsIII v1.3.0-beta Release Notes

PatchOpsIII v1.3.0-beta is a beta release focused on the new Electron desktop experience, the local API backend, and the maintained T7Patch v3.02 source update.

## Features

- PR #34: Electron, React, TypeScript, Bun, Vite, and Tailwind frontend overhaul.
  - User impact: PatchOpsIII now opens as a desktop Electron control center with a responsive dashboard, titlebar controls, live logs, folder browsing, launch profiles, graphics controls, and mod management views.
  - Validation: `bun install`, `bun run typecheck`, `bun run build`, integrated branch build validation, and final main build validation.
  - Known limitations: This is a beta UI migration; some advanced controls remain beta-tested and should be reported through GitHub issues.
- PR #34: Local FastAPI/WebSocket backend service.
  - User impact: Filesystem, Steam, game config, status, update, folder browse, and live log operations now run through a local Python API used by the Electron frontend.
  - Validation: `python -m compileall backend`, `python -m compileall .`, and `python -m py_compile t7_patch.py utils.py dxvk_manager.py bo3_enhanced.py`.
  - Known limitations: Backend is local-only and depends on the packaged backend process starting successfully with the desktop app.

## Fixes

- PR #34: T7 reset state and runtime control refinements.
  - User impact: T7-related dashboard state and reset controls are better aligned with installed component status.
  - Validation: Electron typecheck/build and Python compile checks after integration with PR #32.
  - Known limitations: T7 installation still requires valid game directory permissions and may require elevation on Windows.

## Packaging

- PR #34: Electron Builder packaging path.
  - User impact: Windows beta packaging now produces a portable Electron executable and SHA-256 hash through the Windows build workflow; Linux builds use the Electron AppImage path.
  - Validation: `bun run build`; Windows workflow reviewed for artifact name `PatchOpsIII-windows`, upload of `dist/packages/PatchOpsIII.exe`, and generation of `dist/packages/hash.log`.
  - Known limitations: PR #30 standalone ZIP and optional VirusTotal scan behavior is not included in this beta because it conflicts with the Electron packaging change and was not merged.

## Dependencies

- PR #34: Added frontend and desktop toolchain dependencies.
  - User impact: Local development now requires Bun for the Electron renderer and packaging workflow.
  - Validation: `bun install` completed on PR head, integrated branch, and final main.
  - Known limitations: Package build scripts expect a Python virtual environment with PyInstaller for `dist:win` and `dist:linux`.
- PR #32: Updated T7Patch source references to Scroptss/T7Patch v3.02.
  - User impact: Core T7Patch downloads now use the maintained Scroptss v3.02 release while LPC assets remain on the legacy release because the maintained fork does not publish LPC.
  - Validation: `python -m compileall main.py updater.py utils.py t7_patch.py dxvk_manager.py bo3_enhanced.py config.py`, `python -m py_compile t7_patch.py main.py utils.py`, and post-integration compile checks.
  - Known limitations: LPC download source remains legacy by design.

## Documentation

- PR #34: README and wiki updated for Electron-first development and usage.
  - User impact: Setup guidance now points developers toward Bun, Electron, and the local backend instead of the legacy Python Qt UI.
  - Validation: Documentation reviewed during PR audit and integration conflict resolution.
  - Known limitations: Historical release notes still mention removed legacy workflows.
- PR #32: README, wiki, and release-note template refreshed for maintained T7Patch source.
  - User impact: T7Patch links now point users toward Scroptss/T7Patch for maintained core patch files.
  - Validation: Documentation reviewed and preserved during #34 integration.
  - Known limitations: Legacy source references remain only where LPC asset sourcing requires them.

## Internal Changes

- PR #34: Removed legacy Qt frontend and moved app control flow behind the backend/API boundary.
  - User impact: Future UI work should target the React renderer and local backend.
  - Validation: Full final main validation with Python compile and Electron typecheck/build.
  - Known limitations: Existing automation or scripts that call removed legacy Python UI entry points must be updated.

## Known Issues

- PR #34: Windows beta artifact is an Electron portable executable, not the standalone ZIP proposed in PR #30.
  - User impact: Users should verify the published SHA-256 hash before running the beta artifact.
  - Validation: Windows workflow reviewed for hash generation and release artifact upload behavior.
  - Known limitations: Optional VirusTotal CLI scanning is not part of the merged Windows workflow.
- PR #34: Full All-around Enhancement Mod remains unsupported as a launch option.
  - User impact: Use the Lite version when launch options are configured.
  - Validation: README known issue retained after merge.
  - Known limitations: Upstream mod behavior may change independently of PatchOpsIII.

## Migration Notes

- PR #34: Developers should migrate from the old `python main.py` Qt flow to the Electron scripts.
  - User impact: Use `bun run dev` for the local API plus renderer and `bun run dev:desktop` for the desktop shell.
  - Validation: `bun run typecheck` and `bun run build` passed.
  - Known limitations: Existing local scripts that assume `main.py`, `updater.py`, or `config.py` exist need updates.

## Testing Notes

- PR #32: Python dependency install, targeted compileall, and py_compile passed before merge and after integration.
- PR #34: Bun install, TypeScript typecheck, Vite/Electron build, backend compileall, and targeted Python compile checks passed before merge and after integration.
- Final main validation: `python -m pip install -r requirements.txt`, `python -m compileall .`, `bun install`, `bun run typecheck`, and `bun run build` passed.
