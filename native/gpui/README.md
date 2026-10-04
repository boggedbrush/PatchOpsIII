# PatchOpsIII native desktop app

The primary Windows x64 and Linux x64 client is one Rust process: GPUI controls
and the toolkit-independent `patchops-core` library. No Python interpreter,
PyInstaller service, loopback HTTP server, Tauri, or WebView runs in the native
app. The legacy Electron client and its Python backend remain available.

See [the architecture and migration record](../../docs/gpui-architecture.md)
for the dispatcher contract, progress hook, operation differences, and hardware
validation gates.

## Install a release archive

Extract `PatchOpsIII-native-windows-x64.zip` or
`PatchOpsIII-native-linux-x64.zip`. Each contains:

```text
PatchOpsIII/
├── patchopsiii-gpui[.exe]
├── resources/                 # metadata, presets, application icons
├── LICENSE
├── THIRD-PARTY-NOTICES/Lucide.txt
├── README.md
├── NATIVE-README.md
└── docs/native-desktop.md
```

Run `patchopsiii-gpui.exe` on Windows. On Linux:

```sh
chmod +x PatchOpsIII/patchopsiii-gpui
./PatchOpsIII/patchopsiii-gpui
```

Keep the resources directory alongside the executable. Linux needs X11 or
Wayland, fonts, and a Vulkan-capable graphics driver. Native ZIPs do not provide
an automatic installer or updater. Update checks offer the matching native ZIP;
the legacy MSI/NSIS/AppImage assets remain separate.

## Build and run from source

Install Rust through rustup. `native/rust-toolchain.toml` pins Rust 1.99.0 with
rustfmt and Clippy. `native/Cargo.lock` locks the workspace; GPUI 0.2.2 and GPUI
Component 0.5.0 remain pinned. The original lockfile's libc 0.2.189 is retained
for GPUI's xattr 0.2.3 dependency.

Ubuntu 24.04 build packages:

```sh
sudo apt-get install libfontconfig1-dev libfreetype6-dev libxkbcommon-dev \
  libxkbcommon-x11-dev libwayland-dev libvulkan-dev libx11-xcb-dev \
  libxcb1-dev libasound2-dev libclang-dev
```

Windows needs Visual Studio Build Tools with Desktop development with C++, a
Windows SDK, and Rust's MSVC toolchain. Rust builds do not require Python or Bun:

```sh
cargo +1.99.0 build --locked --manifest-path native/Cargo.toml -p patchopsiii-gpui
./native/target/debug/patchopsiii-gpui
```

Python's standard library is sufficient for the optional convenience scripts:

```sh
python scripts/gpui.py build
python scripts/gpui.py run
# Existing Bun aliases: bun run gpui:build / bun run gpui:run
```

`CARGO_TARGET_DIR` selects a custom build directory; relative paths in the
launcher resolve against the repository root. Default output is `native/target`.
`PATCHOPSIII_DATA_DIR` optionally selects isolated settings/log/cache storage,
useful for disposable tests. The old Python/backend URL environment variables
are no longer used. The normal data directory still shares
`electron-settings.json`, logs and legacy ownership state with Electron. Avoid
simultaneous mutations from both clients.

Requests execute serially on a worker thread. Navigation remains responsive
while an operation runs. Download timeouts and integrity checks live in the
core; there is no HTTP request timeout between the UI and worker. Wait for file
operations to finish before closing: cancellation and joining the worker on
window close are not implemented.

## Build a distributable archive

```sh
python scripts/gpui.py build --release
python scripts/package_gpui.py \
  --executable native/target/release/patchopsiii-gpui \
  --output dist/native/PatchOpsIII-native-linux-x64.zip
# Windows: use native/target/release/patchopsiii-gpui.exe and the Windows ZIP name.
```

The archive contains one product executable and resources/docs. Presets and
release version are also embedded at compile time; release workflows derive the
runtime version from package.json. Native CI builds/tests the Rust workspace and
runs standard-library Python launcher/archive checks. Stable and beta release
workflows attach both native ZIPs under their existing artifact names.

## Validation

```sh
cargo +1.99.0 fmt --manifest-path native/Cargo.toml --all --check
cargo +1.99.0 clippy --locked --manifest-path native/Cargo.toml --workspace --all-targets -- -D warnings
cargo +1.99.0 test --locked --manifest-path native/Cargo.toml -p patchops-core
cargo +1.99.0 test --locked --manifest-path native/Cargo.toml -p patchopsiii-gpui
python -m unittest scripts/test_gpui.py
# Read-only status on a disposable fake game, without a desktop:
python scripts/smoke_gpui_linux.py --status-only --executable native/target/debug/patchopsiii-gpui
# With xvfb, openbox, xcompmgr, xdotool and a Vulkan driver:
xvfb-run -a python scripts/smoke_gpui_linux.py --start-session \
  --executable native/target/debug/patchopsiii-gpui
```

The binary's `--smoke-status` option starts the real worker, prints status JSON,
and exits before creating a window. The smoke script selects a temporary data
directory and fake executable/config; it never requests a game mutation.

## Remaining validation

Native signing/installers, automatic updates, accessibility/IME, Windows UAC,
Wayland, Steam Deck, real downloads and restoration on disposable installations
need hardware/manual checks. PR #40 uses stricter ownership and verified
transactional backups than Python; legacy payloads not provably owned may require
manual recovery. The parity fixtures document exact Python behavior separately.
