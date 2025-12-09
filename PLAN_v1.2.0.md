# PatchOpsIII v1.2.0 Plan — BO3 Enhanced Integration

Status: In progress (POC implemented)

## Objectives
- Add first-class BO3 Enhanced support with automated retrieval of the latest release and bundled dump handling.
- Allow users to import their own dumps with validation and clear manual-install guidance.
- Surface safety rails: warn when Enhanced is active, require acknowledgement, and keep launch options gated to avoid incompatible mods.

## Scope and guardrails
- In scope: download manager for BO3 Enhanced and dumps, validation, storage under `BO3 Mod Files`, UI affordances (warning modal + persistent indicator), config flagging, checksum integrity, retry handling, documentation updates.
- Out of scope: changes to existing T7 Patch, DXVK, or graphics configuration flows except for necessary launch-option gating when Enhanced is active; packaging beyond updating baked version numbers.

## Versioning and resources
- Target app version: `1.2.0` (update `version.py` and any packaging metadata when features land).
- Upstream release feed: `https://api.github.com/repos/shiversoftdev/BO3Enhanced/releases/latest` (fallback to `https://github.com/shiversoftdev/BO3Enhanced/releases/latest` for browser open).
- Primary dump files: `https://gofile.io/d/91Sveo`; backup mirror: `https://www.mediafire.com/file/w3q2fgblfsd4hfn/DUMP.zip`.
- Local storage: `BO3 Mod Files/` (already includes `BO3Enhanced.v1.16.zip` and `DUMP.zip` seeds).

## Workstreams and implementation notes

- ✅ Core POC landed: dedicated Enhanced tab with install/uninstall, dump-only test buttons, dump-first/enhanced-last install order, `.bak` backups, launch-option gating, state/ack handling, and MediaFire direct-link resolution with HTML guards.
- ✅ Basic validation and checksum caching for Enhanced and dump archives; filtered dump copy skips Enhanced DLLs and cosmetic assets.
- ✅ Version bumped to 1.2.0 in code; baked version aligned.
- ⚠️ Remaining gaps: Gofile direct download (or host dump on GitHub Releases), runtime preflight to block installing into a dump path, and clearer “installed” status when no files are tracked.

### 1) Versioning and release readiness
- Bump baked version to `1.2.0` in `version.py`; ensure any installer/spec metadata reads from the central value.
- Add a short release checklist (version bump, release notes, assets staged in `BO3 Mod Files`, smoke tests).

### 2) BO3 Enhanced detection and config flag
- Add `BO3Enhanced.Installed` flag persisted in app settings (new JSON under storage directory, e.g., `PatchOpsIII/config.json`).
- Detection heuristics: look for installed Enhanced binaries in the game directory (e.g., `T7WSBootstrapper.dll`, `T7InternalWS.dll`, `steam_api65.dll`, `WindowsCodecs.dll` matching Enhanced zip), plus an installed marker in storage once PatchOpsIII deploys it.
- On detection, set the flag, record detection timestamp in logs, and expose state to the UI.

### 3) Download manager (auto + fallback)
- Reuse updater patterns (GitHub API, streaming downloads, progress signals) to build `bo3_enhanced.py` service with:
  - Release discovery via GitHub API (prefer `.zip` assets; allow `.7z` future-proofing).
  - Progressive download with resumable temp files, percent progress, cancel support.
  - Checksum: prefer upstream digest if published; otherwise compute SHA-256 locally and store alongside the asset to validate future reuses.
  - Fallback chain: primary dump from GoFile → backup MediaFire mirror → local cached copy. Attach exponential backoff (e.g., 1s, 2s, 4s) across up to 3 attempts per source before falling through.
  - Store downloads in `BO3 Mod Files/` with normalized names (`BO3Enhanced_latest.zip`, `DUMP.zip`).
  - Validate downloaded archives before surfacing success (see validation criteria below).
  - Current status: MediaFire resolved via `Direct-Download`; HTML guard prevents landing-page downloads. Gofile still needs a direct-link resolver or moving dump to GitHub Releases.

### 4) User-provided dump ingestion
- Add file picker (default path `BO3 Mod Files/`) allowing `.zip` or a folder named `DUMP`.
- Validation rules:
  - Must contain `appxmanifest.xml`, `BlackOps3.exe`, `MicrosoftGame.config`, `t7patch.conf`, and core DLLs (`T7InternalWS.dll`, `T7WSBootstrapper.dll`, `steam_api65.dll`, `WindowsCodecs.dll`); fail fast if missing.
  - Minimum size guard (e.g., >70 MB total) and per-file sanity checks (exe > 50 MB).
  - Reject archives with unexpected top-level layout (enforce single `DUMP/` root) or obvious corruption (bad zip CRC).
- After validation, copy/overwrite into `BO3 Mod Files/DUMP` and log the source path. Present manual placement instructions for the game directory.

### 5) Warning and UI/UX changes
- On app start and when switching to Mods tab, if `BO3Enhanced.Installed` is true, show a blocking modal:
  - Message: “Launch options are disabled when BO3 Enhanced is active. Most third-party mods will not be compatible.”
  - Require explicit acknowledgement (persist acknowledged-at timestamp in settings + log entry).
- Disable or gray out launch-option radio buttons while Enhanced is active; include tooltip explaining why.
- Add a persistent indicator in the top bar (e.g., a colored pill near the game directory row) showing `Enhanced Mode Active`. Clicking should open a small info dialog with mitigation guidance.
- Ensure warning repeats if detection happens mid-session (e.g., after download completes) unless acknowledged in the current session.

### 6) Install/apply flow
- Enhanced tab provides:
  - Auto download (Enhanced + dump) with progress and validation.
  - Use local dump picker + validation.
  - Manual instructions link.
  - Install/Update (dump-first, Enhanced-last) with `.bak` backups and tracked files.
  - Uninstall (restore `.bak` or remove tracked files); dump-only install/uninstall buttons for testing.
- Status is shown in-tab; launch options are disabled when Enhanced is active.
- TODO: progress bars for installs and clearer messaging when no files were actually installed.

### 7) Integrity and validation
- Shared validation helper for Enhanced packages:
  - Confirm expected DLL set from `BO3Enhanced.v1.16.zip` (4-file payload) and checksum match against either upstream digest or locally stored hash.
  - For dumps, ensure `appxmanifest.xml` is well-formed XML and references Black Ops III identifiers.
  - Emit descriptive error messages surfaced to the UI and logs.
- Keep a `checksums.json` alongside downloads to avoid re-downloading unchanged assets.

### 8) Logging, resilience, and telemetry
- Log every download attempt, source used, bytes transferred, checksum result, and validation outcome via `write_log`.
- Record user acknowledgements and whether launch options were blocked or bypassed for support diagnostics.
- Add retry/backoff wrapper shared by downloaders and validation to reduce flakiness on slow connections.
- TODO: preflight check to prevent selecting a dump folder as the game directory (avoid config.ini missing spam).

### 9) Documentation and release comms
- Update README and wiki (new “BO3 Enhanced” page) with auto/manual install steps, dump validation rules, and warning behavior.
- Add release notes for v1.2.0 covering new flows and compatibility caveats.
- Keep rollback note: maintain v1.1.x installer link in release notes for users hitting Enhanced-related issues.

### 10) Testing plan
- Unit-level: download URL selection (primary vs fallback), checksum verification paths, dump validator (happy path and missing-file failures), settings persistence for `BO3Enhanced.Installed` and acknowledgement timestamps.
- Integration/manual: end-to-end auto download on Windows, manual import with a tampered dump (expect rejection), warning modal gating launch options, ensure standard mods still apply when Enhanced is not detected.
- Regression: confirm existing updater and DXVK flows still operate; verify Launch Game still works with launch options disabled.

## User flow (implementation target)
1) On launch, detect game directory and Enhanced installation; set `BO3Enhanced.Installed` when applicable.
2) If Enhanced detected: display warning → require acknowledgement → disable launch options and show active indicator.
3) If not detected: offer Auto download, Manual (instructions), or Custom (user-provided dump) paths.
4) After successful download/import: validate → mark installed flag → prompt for manual placement if needed → refresh indicator/warnings.
5) Proceed to main UI with appropriate feature restrictions.

## Acceptance checklist
- [x] Version bumped to 1.2.0 (code).
- [x] GitHub API client for BO3 Enhanced added with multi-source download + retry/backoff.
- [x] Dump and Enhanced package validators implemented with checksum support and HTML/content-type guard.
- [x] UI: warning/ack; launch options disabled under Enhanced; in-tab status indicator; install/uninstall flows; dump-only test buttons.
- [x] “Use local dump” picker and manual instruction flow added.
- [ ] Documentation and release notes updated for new workflows and rollback guidance.
- [ ] Compatibility smoke tests completed against existing mod management features.
- [ ] Direct-downloadable dump source (Gofile resolver or GitHub Release asset) and preflight game-dir sanity checks.

## Risks and mitigations
- Missing upstream checksum: mitigate by computing and storing local hashes and cross-validating across mirrors.
- Slow or blocked mirrors: provide manual download URL copy and cache re-use; ensure backoff does not freeze UI. Move dump to stable host (GitHub Releases) or add Gofile resolver.
- False positives on detection: keep detection heuristic conservative and allow users to clear the flag via settings if detection is incorrect. Add preflight to block dump paths as game dir.
- Dump-only uninstall safety: tracked files and `.bak` restores are in place; ensure idempotency and avoid deleting originals on repeated runs.
