# PatchOpsIII native desktop app

This is the primary PatchOpsIII desktop client for Windows x64 and Linux x64,
including Steam Deck desktop mode. It renders Rust/GPUI controls without
Chromium, React, Tauri, or a WebView. The established Python API still performs
game detection, downloads, backups, configuration, and launch operations. The
Electron/React client remains in the repository as a legacy fallback.

The original framework research, architecture decision, comparison criteria,
implemented workflow inventory, and remaining parity work are retained in
[the native desktop migration record](../../docs/gpui-architecture.md).

## Install a release archive

The native build produces `PatchOpsIII-native-windows-x64.zip` and
`PatchOpsIII-native-linux-x64.zip`. Each archive has this layout:

```text
PatchOpsIII/
├── patchopsiii-gpui[.exe]
├── resources/
│   ├── backend-bin/patchops-backend[.exe]
│   ├── package.json
│   ├── presets.json
│   ├── PatchOpsIII.ico
│   └── icon-512.png
├── LICENSE
├── THIRD-PARTY-NOTICES/
├── README.md
└── NATIVE-README.md
```

Extract the whole folder and keep `resources` beside the native executable. On
Linux, make the native executable executable after extraction if the archive
tool did not preserve its mode:

```sh
chmod +x PatchOpsIII/patchopsiii-gpui
./PatchOpsIII/patchopsiii-gpui
```

On Windows, run `PatchOpsIII\patchopsiii-gpui.exe`. The native executable starts
the bundled PyInstaller backend on an OS-assigned loopback port, waits up to 30
seconds for `/api/health`, opens the UI only after the service is ready, and
terminates and reaps the service on normal exit. No Python installation is
needed. Linux still needs a working X11 or Wayland desktop, fonts, and a
Vulkan-capable driver.

## Build and run from source

Install Rust with rustup, Python 3.12, and the normal backend requirements. The
crate pins GPUI 0.2.2 and GPUI Component 0.5.0 with a committed `Cargo.lock` and
Rust 1.99.0. GPUI's upstream main branch has since changed its platform API; use
the pinned published APIs when editing this app.

Linux build dependencies on Ubuntu 24.04:

```sh
sudo apt-get install libfontconfig1-dev libfreetype6-dev libxkbcommon-dev \
  libxkbcommon-x11-dev libwayland-dev libvulkan-dev libx11-xcb-dev \
  libxcb1-dev libasound2-dev libclang-dev
```

Windows needs Visual Studio Build Tools with Desktop development with C++, a
Windows SDK, and Rust's MSVC toolchain.

From the repository root:

```sh
python -m venv .venv
# Linux: source .venv/bin/activate
# Windows PowerShell: .venv\Scripts\Activate.ps1
python -m pip install -r requirements.txt
python scripts/gpui.py build
python scripts/gpui.py run
```

The launcher remains the recommended development command: it starts the source
API with an inherited bound socket, waits for health, injects the resulting URL,
and reaps both processes. A directly started development binary can also find
`backend/api.py` from the repository and uses `.venv` or
`PATCHOPSIII_PYTHON`. `PATCHOPSIII_GPUI_BACKEND_URL` selects an externally owned
loopback API; `PATCHOPSIII_BACKEND_PATH` selects a packaged backend executable.

Electron and GPUI deliberately share backend settings, logs, caches, and game
files. Do not perform overlapping game mutations in the two apps. Native actions
display the selected operation and game directory for confirmation. HTTP
requests run sequentially on a worker; long downloads do not freeze navigation.
GET requests time out after 30 seconds and operations after 10 minutes. A timeout
does not guarantee that the Python operation stopped, so check logs and refresh
before retrying. Wait for file operations to finish before closing the app.

## Build a distributable archive

The backend build commands are shared with the legacy Electron packaging so the
PyInstaller flags cannot drift. Install Bun plus PyInstaller in `.venv`, then:

```sh
# Linux
bun run gpui:build:release
bun run build:backend:linux
python scripts/package_gpui.py \
  --executable native/gpui/target/release/patchopsiii-gpui \
  --backend dist/backend/patchops-backend \
  --output dist/native/PatchOpsIII-native-linux-x64.zip

# Windows PowerShell uses build:backend:win and the corresponding .exe paths.
```

`.github/workflows/gpui-build.yml` runs formatting, Clippy with warnings denied,
Rust and Python tests, the Linux window-close smoke check, both release builds,
and archive creation. Stable and beta release workflows attach both native ZIPs
without changing the existing Electron artifact names or tag behavior.

## Validation

```sh
python -m unittest scripts/test_gpui.py
python scripts/gpui.py check
python scripts/gpui.py test
cargo fmt --manifest-path native/gpui/Cargo.toml --check
cargo clippy --locked --manifest-path native/gpui/Cargo.toml --all-targets -- -D warnings
# Linux with xvfb, openbox, xcompmgr, xdotool and a Vulkan driver installed:
xvfb-run -a python scripts/smoke_gpui_linux.py --start-session \
  --executable native/gpui/target/release/patchopsiii-gpui
```

## Remaining gaps

The native app is the primary implementation, but promotion does not imply full
UI parity. Native installer integration, signing, automatic updates, streaming
WebSocket logs, some richer progress/detail states, accessibility/IME coverage,
and real-device Windows, Wayland, and Steam Deck validation remain open. The
legacy Electron installers stay available while those delivery and parity gaps
are closed. See the migration record for the detailed workflow matrix and manual
review checklist.
