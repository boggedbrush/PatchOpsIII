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
* Release builds are GUI-subsystem; with command-line arguments (`--version`, `--rpc-stdio`, hidden CLI) the process attaches to the
  parent console and keeps inherited pipes untouched (`console` module in `crates/patchops/src/main.rs`).
* Local build: `cargo build --release --locked -p patchops; dotnet tool install --global wix --version 5.0.2; ./packaging/windows/package.ps1`.
