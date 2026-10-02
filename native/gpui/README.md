# PatchOpsIII GPUI alternative

A native Rust/GPUI evaluation app alongside the Electron app, targeting Windows
x64 and Linux x64 (including a future Steam Deck evaluation). It renders native
controls without Chromium, React, Tauri, or a WebView. The existing Python API
still performs game operations. This is an experimental alternative, not a
feature-complete replacement or a production installer.

Research, implementation plan, comparison criteria, remaining work, and review
checks are in [the evaluation document](../../docs/gpui-evaluation.md).

## Build and run from source

Install Rust with rustup, Python 3.12, and the normal backend requirements.
The crate pins GPUI 0.2.2 and GPUI Component 0.5.0 with a committed Cargo.lock;
its toolchain is Rust 1.99.0. GPUI's upstream main branch has since changed its
platform API; use the pinned published API when editing this app.

Linux build dependencies (Ubuntu 24.04):

```sh
sudo apt-get install libfontconfig1-dev libfreetype6-dev libxkbcommon-dev \
  libxkbcommon-x11-dev libwayland-dev libvulkan-dev libx11-xcb-dev \
  libxcb1-dev libasound2-dev libclang-dev
```

Windows needs Visual Studio Build Tools with Desktop development with C++ and
a Windows SDK, plus Rust's MSVC toolchain. Linux runtime needs a working X11 or
Wayland session, fonts, and a Vulkan-capable driver. Software rendering is useful
for smoke checks but does not establish Steam Deck/GPU performance.

From the repository root, use your Python environment's interpreter:

```sh
python -m venv .venv
# Linux: source .venv/bin/activate
# Windows PowerShell: .venv\Scripts\Activate.ps1
python -m pip install -r requirements.txt
python scripts/gpui.py build
python scripts/gpui.py run
```

The launcher waits for a healthy API on an OS-assigned loopback port, launches
the compiled GPUI binary, and stops/reaps both processes on exit, Ctrl+C, or
startup failure. Build separately so launch measurements exclude compilation.
For an optimized build, pass `--release` to both commands.

Electron and GPUI use separate processes and ports, but deliberately share
backend settings, logs, caches, and game files. Do not perform overlapping game
mutations in the two apps. Native actions display the selected operation and
game directory for confirmation. HTTP requests run sequentially on a worker;
long downloads do not freeze navigation. GET requests time out after 30 seconds,
operations after 10 minutes. A timeout reports failure and does **not** guarantee
the Python operation stopped; check logs and refresh before retrying. Closing the
launcher stops its backend, so wait for file operations to finish before closing.

## Evaluation archives

The GPUI workflow checks formatting/lints/tests and builds release executables
for Linux and Windows. Its ZIP artifacts include the native executable, Python
backend source, presets, launcher, and documentation. They require Python and
installed requirements; no Python runtime is bundled yet. They do not publish
releases or replace Electron's MSI/AppImage workflows.

After extracting an archive:

```sh
python -m venv .venv
# Activate the environment as above.
python -m pip install -r requirements.txt
# Linux ZIP extraction may require: chmod +x bin/patchopsiii-gpui
python scripts/gpui.py run --executable bin/patchopsiii-gpui
# Windows: python scripts/gpui.py run --executable bin/patchopsiii-gpui.exe
```

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

See the evaluation document for the hardware/manual checks that remain necessary.
