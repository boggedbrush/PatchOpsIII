# Packaging

Release artifacts are produced from the single `patchops` binary (`cargo build --release --locked -p patchops`).
File names are what the in-app updater (`crates/patchops-core/src/ops/maintenance.rs`) and every released legacy
version look for, so do not rename them: `PatchOpsIII.AppImage`, `PatchOpsIII.msi`, `PatchOpsIII.exe`
(the release-beta workflow adds the `-Beta` infix when publishing).

| Path | Purpose |
| --- | --- |
| `linux/build-appimage.sh` | Assembles the AppDir and builds `dist/packages/PatchOpsIII.AppImage` (+ `.zsync`). `APPDIR_ONLY=1` stops after the AppDir. |
| `linux/fetch-tools.sh` | Downloads appimagetool 1.9.1 and type2-runtime 20251108 with pinned SHA-256 (bump version and hash together). |
| `linux/verify-appimage.sh` | Checks update information, 0755 SquashFS directories (Firejail), AppRun, glibc ceiling, unresolved libraries. |
| `linux/AppRun`, `linux/PatchOpsIII.desktop` | Entry point and desktop file (`StartupWMClass` = window `app_id` `PatchOpsIII`). |
| `windows/package.ps1` | Builds `dist\packages\PatchOpsIII.msi`, the portable `PatchOpsIII.exe` and `hash.log`. Needs the WiX v5 CLI. |
| `windows/PatchOpsIII.wxs` | WiX v5 source of the MSI. |
| `windows/patchops-msi-*.bmp` | Installer banner/dialog bitmaps. |
| `assets/` | `PatchOpsIII.ico` (embedded into the exe by `crates/patchops/build.rs` and used by the MSI) and `PatchOpsIII-512.png` (AppImage icons). |

## Version

One source of truth: `[workspace.package] version` in the root `Cargo.toml`. Both crates inherit it; `patchops --version`,
the About/Tools screens and the updater's "current version" read `CARGO_PKG_VERSION` (still overridable through
`PATCHOPSIII_VERSION_OVERRIDE` / `PATCHOPSIII_VERSION`). The release workflows derive the tag (`v<version>`, beta:
`v<version>-beta`) and check it against the `Release Notes/` heading. Pre-release versions use semver, e.g. `1.3.5-beta1`.
The MSI ProductVersion is the numeric `x.y.z.0` part.

## Linux

* Built on `ubuntu-22.04` (oldest hosted image), so the binary needs glibc >= 2.35 (`verify-appimage.sh` enforces it).
* Layout: `usr/bin/patchops`, `usr/bin/bo3-enhanced-proton/` (where `enhanced_net::existing_tool_source` looks, next to the
  executable), `usr/lib/` (libxkbcommon, libxkbcommon-x11, libxcb-xkb; found through a RUNPATH, no `LD_LIBRARY_PATH`).
* Not bundled on purpose: libvulkan and GPU drivers (must match the host), libxcb/X11 and Wayland client libraries.
* Optional offline payload: put the Proton tool in `bo3-enhanced-proton/BO3 Enhanced` before building and it is shipped.
* Update information (`UPDATE_INFORMATION=gh-releases-zsync|...`) is embedded for stable and PR builds only.

## Windows

* MSVC toolchain + Windows SDK (`fxc.exe`, `GPUI_FXC_PATH`); see `crates/patchops/GPUI_NOTES.md`.
* The MSI keeps the legacy identity (UpgradeCode `A9A7E56F-6704-5167-96C9-3DF300383A42`, per-machine, Program Files\PatchOpsIII,
  Start menu + desktop shortcuts) so it upgrades an installed Electron-era MSI.
* Upgrading from the old NSIS installer: releases 1.3.0-1.3.4 also shipped an electron-builder NSIS installer named
  `PatchOpsIII.exe` (per-user by default, `%LOCALAPPDATA%\Programs\PatchOpsIII`; optionally all-users, `%ProgramFiles%\PatchOpsIII`).
  The MSI cannot replace it, so those users end up with two installs; `PatchOpsIII.exe` is now the portable app. Tell them to uninstall the
  old one **first** (Windows Settings → Apps; settings and logs are kept), then install the MSI. The old uninstaller matches running processes
  by image name `PatchOpsIII.exe`, which is also the new app, so PatchOpsIII must be closed while it runs. The app shows a dismissible
  notice when it finds the old install (`crates/patchops-core/src/legacy_install.rs`); it never runs the uninstaller itself.
  * Registry written by the NSIS installer (electron-builder 26.8.1; HKCU per-user or HKLM all-users, 64-bit view):
    `Software\Microsoft\Windows\CurrentVersion\Uninstall\a0f3b457-ab4b-5b70-97b3-7be0d5217bca` (no braces; uuid5 of appId
    `com.patchopsiii.app` under electron-builder's namespace `50e065bc-3134-11e6-9bab-38c9862bdaf3`) holds `DisplayName`
    ("PatchOpsIII <ver>"), `UninstallString` (`"<dir>\Uninstall PatchOpsIII.exe" /currentuser` or `/allusers`), `QuietUninstallString`,
    `DisplayVersion`, `DisplayIcon`, `Publisher`, `EstimatedSize` and **no `InstallLocation`**; that is in `Software\<same GUID>`.
  * An all-users NSIS install lives in the same folder as the MSI (`%ProgramFiles%\PatchOpsIII`). If the MSI is installed over it, the MSI
    overwrites its files and the HKLM NSIS entry is stale; running that uninstaller (`RMDir /r $INSTDIR`) would delete the new app and its
    all-users shortcuts. Users who did that should re-run the MSI (repair).
  * The app shows the notice only when the NSIS folder is known (`InstallLocation`, else the folder of the quoted `UninstallString` path),
    still contains `Uninstall PatchOpsIII.exe`, does not contain the running exe, and is not (and does not contain) the MSI's folder. The MSI
    folder is `HKLM\Software\PatchOpsIII` value `InstallLocation` (written by `PatchOpsIII.wxs`) when that value is present and non-empty,
    and only that: a new MSI in a custom folder therefore no longer hides an all-users NSIS install in `%ProgramFiles%\PatchOpsIII`.
    `%ProgramW6432%` / `%ProgramFiles%` `\PatchOpsIII` are used only when the value is absent (older MSI, or no MSI). In any doubtful case it
    stays silent, including the stale all-users entry that points at the MSI's folder (cosmetic: one extra Apps entry).
  * Known false negative, kept on purpose: an all-users NSIS install in Program Files plus a portable-only user (no MSI, hence no
    `InstallLocation`) stays silent, because without the Windows Installer API it looks the same as an NSIS install the MSI overwrote.
    A missed notice is the safe direction; the app never points the user's uninstaller at the new app.
* Release builds are GUI-subsystem; with command-line arguments (`--version`, `--rpc-stdio`, hidden CLI) the process attaches to the
  parent console and keeps inherited pipes untouched (`console` module in `crates/patchops/src/main.rs`).
* Local build: `cargo build --release --locked -p patchops; dotnet tool install --global wix --version 5.0.2; ./packaging/windows/package.ps1`.
