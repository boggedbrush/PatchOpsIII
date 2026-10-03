# Native GPUI architecture and migration record

Research date: 2026-10-02. Original baseline: the Electron/React/Python app,
which now remains as the legacy fallback.
Related alternatives: [Tauri shell + Rust core PR #36](https://github.com/boggedbrush/PatchOpsIII/pull/36)
and [in-process Rust/Tauri PR #40](https://github.com/boggedbrush/PatchOpsIII/pull/40).
The GPUI implementation started from `main`, independently of both Tauri branches,
and has since been selected as PatchOpsIII's primary desktop client.

## Findings and architecture decision

GPUI is Zed's GPU-accelerated Rust framework, combining retained entity state
with declarative view rendering. It is pre-1.0 and APIs can break between
versions. It replaces the renderer as well as the desktop shell: our React,
CSS, browser accessibility, DOM tests, and Electron IPC cannot simply transfer.
The upstream main README now describes a separate `gpui_platform` package;
the published GPUI 0.2.2 selected here still uses `Application::new()` and
includes the platform implementations. Mixing these APIs would break builds.

GPUI Component supplies keyboard-aware buttons and text inputs, text editing,
focus handling, and the root/theme setup, reducing custom control work. Pinning
the compatible GPUI 0.2.2 / Component 0.5.0 pair and committing the lockfile
avoids adopting an evolving Git revision or silently upgrading the toolkit.
The optional WebView feature is not enabled. Both dependencies use Apache-2.0;
distribution still needs an audit of notices for the complete dependency tree.

Reusing the current Python API provided a controlled first comparison of desktop
renderers and remains the production architecture today. It retains the existing download, install, backup, hash, Steam, UAC,
and config semantics. It also retains Python startup, packaging cost, and HTTP
serialization; removing Chromium does not prove a smaller or faster complete
application. A later in-process Rust port should use a toolkit-independent core
so Tauri and GPUI can exercise identical business logic. This PR does not import
the unmerged Rust rewrite or duplicate delicate game-file operations in Rust.

The promotion decision changes product ownership, not the evidence: GPUI is now
the primary client, native archives bundle the Python service, and Electron stays
buildable as a fallback. The hardware, accessibility, updater, and richer UX gaps
listed below remain follow-up work rather than being treated as completed parity.

Sources inspected:

- [GPUI upstream README](https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md): architecture, pre-1.0 status, platform requirements.
- [Published GPUI 0.2.2 source](https://crates.io/crates/gpui/0.2.2): standalone application, async contexts, platform folder prompts.
- [GPUI Component 0.5.0 source](https://crates.io/crates/gpui-component/0.5.0): compatible GPUI dependency, inputs, buttons, Root and theme setup.
- Repository `src/main/main.ts`, `src/renderer/lib/api.ts`, `src/renderer/main.tsx`, and `backend/api.py`: actual lifecycle, payloads, config normalization, state, error responses, and logs.
- PR #36 and #40 metadata: the shell-only migration and the subsequent backend rewrite have different scopes and costs.

## Implemented architecture

1. Add an isolated Cargo application and compatible native controls. Preserve the
   app's dark charcoal/red control-center visual language and use native window
   chrome, seven navigation sections, scrollable settings, and a log panel.
2. Launch a supervised Python service on an ephemeral loopback port. Packaged
   binaries locate the PyInstaller service in `resources/backend-bin`; source
   binaries fall back to `backend/api.py`. Wait for health before opening GPUI
   and always reap owned processes on normal/error exits.
3. Execute API calls on one HTTP worker. Reflect backend-reported state, preserve
   unsuccessful HTTP-200 responses and depot instructions, disable overlapping
   actions, and require an explicit operation confirmation.
4. Implement representative real workflows throughout the app, using existing
   payloads and config value mappings. Keep input drafts intact during refresh;
   provide an explicit button to load current values.
5. Add meaningful protocol/lifecycle checks, Windows/Linux build CI,
   self-contained archives, a common process-tree sampler, and a parity/manual
   review checklist. Stable and beta releases attach native archives alongside
   the unchanged legacy Electron deliverables.

## Stack comparison

| Criterion | Legacy Electron | Tauri #36 / #40 | Primary GPUI client |
|---|---|---|---|
| UI technology | React/CSS + Chromium | React/CSS + OS WebView | Native Rust controls + GPU rendering |
| Backend | Python child + HTTP/WS | #36 Python + small Rust core; #40 in-process Rust | Same Python + HTTP |
| Renderer reuse | Baseline | High | Rewrite required |
| Platform work | Existing Windows/Linux packages | WebView availability and new Rust packaging | GPU/driver compatibility, native UI/input and new packaging |
| Accessibility | Browser infrastructure; app still needs testing | WebView infrastructure; app still needs testing | Must validate controls and screen readers on target OSes |
| Delivery | Existing MSI/NSIS/AppImage fallback | Independent open PRs | Self-contained Windows/Linux ZIPs with a bundled PyInstaller backend |
| Startup/RAM/package size | Measure | Measure each PR separately | Measure complete stack; no claimed improvement |
| Migration cost | Existing maintenance | Shell changes; #40 also ports operations | Entire UI rewrite; later backend port optional |

## Workflow coverage

| Workflow | Native implementation | Remaining parity work |
|---|---|---|
| Installation detection/directory | Actual status and native folder picker; manual path entry; existing backend validation | Folder picker on Windows, Wayland portals and Steam Deck |
| Launch/profiles | Steam launch, backend-provided profiles, Workshop open/install request | Workshop progress presentation and detailed profile states |
| T7 Patch | Install/update, uninstall, gamertag/color/password, friends-only | Real install/UAC cancellation, name preview, password editing UX |
| EXE Swapper | Compatible/current/Enhanced restore, hash and integrity state, depot error text | Guided depot dialog/polling/copy action; real restore/backup checks |
| Enhanced | Pick/enter source, validate, install, uninstall | Real Windows/Linux game/proton workflows |
| Graphics | Presets, FPS, FOV, refresh, render %, resolution, display modes, V-Sync and FPS counter | Slider UX, valid hardware-specific mode list, side-by-side visual polish |
| Advanced/QoL | Smoothing, hidden options, CPU reduction, frame latency, VRAM target, intro/compiler/read-only toggles | Real config/backup verification on disposable installs |
| DXVK | Install/uninstall, async/cache/HUD, threads, FPS, latency and tear-free settings | Real download/install/proton behavior |
| Logs/maintenance | Up to 80 recent lines, five-second refresh, clear logs/cache, reset stock, update check/channel | Streaming WS logs, copy/export UX and platform updater integration |
| Delivery | Windows/Linux CI and self-contained release archives | Signed native installers, updater, dependency notices and broader release testing |

Status refresh uses the complete existing status endpoint, including executable
hashing. Polling every five seconds can cost more idle CPU than Electron's event
driven updates; include that cost in measurements. No new background game scans
or fabricated installation states are introduced. Actions preserve backend
validation and require confirmation; all game-modifying smoke tests must use
an installation whose backups can be inspected and restored.

## Measurement protocol retained from the selection process

Use the same Windows PC and Steam Deck/Linux PC for all three candidates. Record
OS, GPU/driver, display scale, power profile, game-directory state, and commit
SHA. Build optimized release artifacts. Exclude compilation, debugger and dev
servers. Compare Electron, #36, #40 and GPUI separately; a Python-backed GPUI
versus #40 compares both renderer and backend changes.

For regression tracking and any future comparison, collect ten cold launches and
ten warm launches per candidate. Record
process start to first painted interactive window **and** service-ready status
with screen recording or platform tracing. Include backend startup. Report
median and p95, retaining raw observations. Do not label cargo build time as app
startup time, or GPUI entity creation as first paint.

Measure idle for 60 seconds after settling, then the same navigation/config and
log workload. Use the supervisor PID that owns both UI and backend processes:

```sh
python -m pip install psutil
python scripts/measure_desktop.py --pid 12345 --label gpui-release \
  --seconds 60 --artifact dist/native/PatchOpsIII-native-linux-x64.zip \
  --output measurements/gpui-idle.json
```

The sampler captures RSS and CPU for the process tree, including children; RSS
can double-count shared pages and excludes GPU memory. Collect GPU usage with
platform tools. Native archives include the PyInstaller backend, so measure the
complete extracted deliverable. Elevated/detached children must be measured
separately. Evaluate input latency, scrolling, text entry/IME, scaling, keyboard
focus, screen readers, offline/error behavior, and update/install/uninstall
reliability when comparing future changes. GPUI is the selected product client;
that decision does not substitute for collecting hardware measurements.

## Validation history and current gates

On the supplied Linux cloud environment:

- Debug and optimized release builds completed with the locked dependency tree.
- Four Rust protocol tests passed, including an actual local HTTP worker request
  and HTTP error response. Clippy with warnings denied and rustfmt checks passed.
- Eight Python launcher/packaging tests pass, covering child reaping, startup/UI
  failures, missing binaries, early service exit, custom target-directory
  consistency, required bundle inputs, and the self-contained archive layout.
- The existing Electron renderer TypeScript check (`tsc --noEmit`) passed.
- A real Python service bound an ephemeral port, returned health/status with no
  game installation, and stopped accepting connections after launcher cleanup.
- The native window rendered under Xvfb/Openbox with Mesa's software Vulkan
  driver and an X compositor. Dashboard/Graphics navigation and text entry were
  inspected; an edited FPS draft remained unchanged across status refreshes.
- The normal X11 close path exposed a GPUI 0.2.2 callback borrow panic. Quit is
  now scheduled on a later event-loop turn. A read-only native lifecycle smoke
  check exercises the real platform close callback, and Linux CI runs it too.
- The original archive contents and process-tree sampler were smoke checked. The sampler
  captured the supervisor, Python API and native GUI; its debug/software-rendered
  samples are not comparative performance evidence.

![GPUI release dashboard in the Linux smoke session](images/gpui-dashboard.png)

A GUI Apply-click smoke check was rejected by automatic approval review because
it could activate a configuration/file-changing action. No game-mutating UI
clicks were executed. Confirmation behavior was reviewed in code; actual game
operations, Windows, Wayland, Steam Deck, accessibility, and comparative
performance remain manual/hardware validation gates. Native Windows/Linux CI
is configured in this PR; its hosted results are separate from these local checks.

## Reviewer smoke checklist

- Build with the pinned lockfile; open the native window on Windows and Linux.
- Start Electron simultaneously; confirm distinct ports/processes. Close GPUI
  and confirm its service exits while Electron stays alive.
- With no game directory, verify status/errors and manual folder input. Browse
  to a disposable BO3 install, confirm selection, refresh, and relaunch.
- Navigate every section with mouse and keyboard. Edit a field while refresh
  runs; confirm the draft remains. Verify current-values reload is explicit.
- Cancel an operation confirmation and verify no files changed. Verify the
  selected operation and target before confirming a config mutation.
- Apply a preset and individual config values; inspect config.ini and backend
  logs, including readonly error handling and special CPU/hidden-option values.
- On a test install verify each mod install/uninstall and EXE restore preserves
  expected backups. Exercise compatible-depot-required and UAC cancellation.
- Verify API startup failure, disconnect, download errors, and clean shutdown.
  Wait for active file operations before closing; cancellation is not implemented.
- Test X11, Wayland, actual Steam Deck, high DPI, IME and screen-reader behavior.
- Measure optimized complete stacks and record results as native delivery evolves.
  Hardware results and full packaged parity remain follow-up gates after the
  product-level promotion.
