# PatchOpsIII architecture benchmark

Raw reports are archived at the linked migration commit. Keep new generated
JSON reports as review artifacts; the harness and summary remain in source.

[`origin-main.json`](https://github.com/boggedbrush/PatchOpsIII/blob/f8b1d9cbf9d579af309f09f9ad4b9a6ee9a562e1/benchmarks/origin-main.json) records the Electron + Python baseline at revision
`f74c87bbf885e3692f77478b2ff32e89f82f0c73`. [`final-tauri.json`](https://github.com/boggedbrush/PatchOpsIII/blob/f8b1d9cbf9d579af309f09f9ad4b9a6ee9a562e1/benchmarks/final-tauri.json) records the
React + Tauri + in-process Rust result on the same host and workload.

## Fixed fixture

Create the deterministic fixture at an absent path (the default path is the one
used by both recorded runs):

```bash
bun benchmarks/create-fixture.mjs /tmp/patchops-benchmark-fixture
```

It contains representative settings, Steam library metadata, T7/DXVK/config
markers, and a physically written 256 MiB zero-filled `BlackOps3.exe`. Remote
transfer and release-lookup latency remain excluded because they are not
reproducible application measurements. Prepared deterministic payloads now
exercise the offline T7, DXVK, Enhanced, and Linux compatibility transaction
cores, including their restoration checks.

## `origin/main` runtime runs

Build revision `f74c87bbf885e3692f77478b2ff32e89f82f0c73`, extract its AppImage, and
run the retained one-run Electron harness from this branch. Run one unrecorded
prime, five warm samples, then three payload-cold samples; the run number gives
each Electron/Python pair unique backend and debugging ports:

```bash
bun /absolute/path/to/benchmarks/runtime-origin.mjs \
  /absolute/path/to/origin/AppDir/AppRun 0

for run in 1 2 3; do
  PATCHOPS_BENCH_CPU_WINDOW_MS=30000 \
    bun /absolute/path/to/benchmarks/runtime-origin.mjs \
    /absolute/path/to/origin/AppDir/AppRun "$run"
done

for run in 4 5; do
  bun /absolute/path/to/benchmarks/runtime-origin.mjs \
    /absolute/path/to/origin/AppDir/AppRun "$run"
done

for run in 6 7 8; do
  PATCHOPS_BENCH_PAYLOAD_COLD=1 \
    bun /absolute/path/to/benchmarks/runtime-origin.mjs \
    /absolute/path/to/origin/AppDir/AppRun "$run"
done
```

The baseline harness defines backend readiness as a successful `/api/health`
response and interactivity as the first successful renderer `/api/status`
request. It otherwise uses the same isolated HOME/XDG/Steam fixture, procfs
process-tree sampling, two-second idle point, three 30-second CPU samples, and
payload-cache advice as the final harness. [`origin-main.json`](https://github.com/boggedbrush/PatchOpsIII/blob/f8b1d9cbf9d579af309f09f9ad4b9a6ee9a562e1/benchmarks/origin-main.json) retains every
runtime sample because the Electron launch times varied substantially.

## Final runtime and operation runs

Build and extract the AppImage, then compile the tiny X11 close helper:

```bash
bun run dist:linux
./src-tauri/target/release/bundle/appimage/*.AppImage --appimage-extract
cc -O2 -Wall -Wextra -Werror benchmarks/close-window.c -lX11 \
  -o /tmp/patchops-close-window
```

Run optimized production Rust helpers against the fixture:

```bash
PATCHOPSIII_BENCHMARK_FIXTURE=/tmp/patchops-benchmark-fixture \
  cargo test --manifest-path src-tauri/Cargo.toml --release --locked \
  benchmark::fixed_fixture_operations -- --ignored --nocapture
```

The operation benchmark emits every raw timing array under `samplesMs` and the
corresponding medians under `mediansMs`; retain both so sub-millisecond results
can be audited. It creates deterministic 4 MiB Deflate ZIP and tar.gz fixtures
before either extraction timer starts. Every extraction sample begins with a
fresh destination, advises the kernel to discard the archive's cached pages on
Linux, and validates and removes the extracted files after the timed region.

Run the extracted application five warm times, three payload-cold times, and
three 30-second idle-CPU samples:

```bash
PATCHOPSIII_BENCHMARK_FIXTURE=/tmp/patchops-benchmark-fixture \
  bun benchmarks/runtime.mjs /absolute/path/to/squashfs-root/AppRun \
  /tmp/patchops-close-window
```

The runtime sampler uses monotonic timestamps; sums the complete observed
process tree's PSS and RSS from procfs; counts live processes; measures CPU from
procfs ticks; sends a native `WM_DELETE_WINDOW`; and waits for every observed
PID to exit. Every launch gets one fresh temporary root containing isolated
`HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME`, `XDG_CONFIG_HOME`, and a mode-0700
`XDG_RUNTIME_DIR`. The fixture's settings are copied into
`XDG_DATA_HOME/PatchOpsIII`, its Steam tree is copied into the isolated home,
and the whole root is removed after success or failure. Display and desktop
session variables are inherited so the native window can use the caller's X11
session. Payload-cold runs advise the kernel to discard cached AppImage files
with `POSIX_FADV_DONTNEED`. This is repeatable but is not equivalent to a reboot
or privileged whole-system cache drop.

## Build and size method

Warm TypeScript, Vite, no-change Rust release, and AppImage packaging commands
are run serially and reported as medians. Clean Rust and full Linux package
builds use new `CARGO_TARGET_DIR` directories while retaining already-downloaded
dependency caches. Record raw bytes for the Rust binary, AppImage, and `.deb`,
and use `du -sb` for the extracted AppImage/AppDir, extracted Debian payload,
and renderer output. Record the AppImage ELF closure and unresolved dynamic
libraries separately from the Debian control metadata and installed-size field.
MSI build/runtime metrics are not fabricated on Linux; the Windows workflow
performs the MSI build on `windows-latest`.

The AppImage is the primary portable, self-contained Linux and Steam Deck
artifact. The much smaller `.deb` is intentionally system-library-dependent and
targets Debian/Ubuntu, so its package size is not a like-for-like replacement
for AppImage size: some runtime storage moves from the artifact to packages
managed by the host. Attribute measured savings separately to the Rust binary,
the AppImage dependency set, and the Debian system-package boundary.

Linux package acceptance uses fresh Ubuntu 22.04 runners to smoke-test AppImage
startup/shutdown and the `.deb` install-upgrade-launch-purge lifecycle, including
preservation of user data. It also launches and closes the AppImage as a non-root
user in a fresh `archlinux:base` container. These headless X11 gates verify the
packaged application path; they do not cover every distribution, display server,
GPU driver, desktop integration, or physical Steam Deck configuration.

Improvements use `(baseline - final) / baseline * 100`; negative values are
regressions. Rows with different semantics or a missing platform measurement do
not receive a percentage.

## Rust and Linux package footprint pass

The footprint pass kept Candidate A: thin LTO, one codegen unit, and Rust symbol
stripping. Fat LTO saved only 61,440 additional compressed AppImage bytes and
failed the 1% memory gate. Adding `opt-level = "s"` to the best passing profile
cut more bytes but regressed warm interactive startup by 53.0%, payload-cold
startup by 42.5%, and 256 MiB hashing by more than 330%, so it was rejected.
`panic = "abort"` was not tested or adopted because transaction code relies on
normal unwind behavior and the accepted profile already met the size target.

The final dependency change removes only the unused direct `zstd` declaration.
ZIP Stored, Deflate, Bzip2, and Zstandard extraction remain enabled and covered
by fixtures. No Rust runtime refactor was retained: the fixed status/config
paths were already sub-millisecond, and the candidate data did not justify a
shared client, cached regex, or new state abstraction.

The table below is the serialized profile-selection snapshot. It is retained
unchanged so later parity and security work cannot rewrite the evidence used to
choose Candidate A. Runtime acceptance used a baseline primed next to that run
because long-running host state shifted WebKit timing and proportional-set
accounting. Negative deltas are improvements. Exact arrays, the earlier drifted
baseline, candidate data, section sizes, dependency decisions, and limitations are in
[`optimization-measurements.json`](https://github.com/boggedbrush/PatchOpsIII/blob/f8b1d9cbf9d579af309f09f9ad4b9a6ee9a562e1/benchmarks/optimization-measurements.json). The largest
30 baseline files and complete static ELF mapping are retained in
[`optimization-baseline-package.json`](https://github.com/boggedbrush/PatchOpsIII/blob/f8b1d9cbf9d579af309f09f9ad4b9a6ee9a562e1/benchmarks/optimization-baseline-package.json);
[`optimization-final-package.json`](https://github.com/boggedbrush/PatchOpsIII/blob/f8b1d9cbf9d579af309f09f9ad4b9a6ee9a562e1/benchmarks/optimization-final-package.json) is the
post-parity final package refresh.

| Metric | Fresh baseline | Selection snapshot | Final - baseline | Change |
| --- | ---: | ---: | ---: | ---: |
| Native Rust executable | 25,676,192 B | 14,790,288 B | -10,885,904 B | 42.4% smaller |
| Bundled Rust executable | 25,833,032 B | 14,691,912 B | -11,141,120 B | 43.1% smaller |
| AppImage | 109,865,464 B | 108,362,232 B | -1,503,232 B | 1.37% smaller |
| Extracted AppDir | 327,547,383 B | 316,406,263 B | -11,141,120 B | 3.40% smaller |
| AppDir `usr/lib` | 301,264,942 B | 301,264,942 B | 0 B | unchanged |
| Raw Tauri `.deb` feasibility artifact | 7,405,484 B | 5,695,022 B | -1,710,462 B | 23.1% smaller |
| `.deb` installed payload | 25,139 KiB | 14,364 KiB | -10,775 KiB | 42.9% smaller |
| Warm interactive | 420.967 ms | 424.333 ms | +3.366 ms | 0.80% slower; passes |
| Payload-cold interactive | 605.385 ms | 604.551 ms | -0.834 ms | 0.14% faster |
| Idle PSS | 278,230 KiB | 277,127 KiB | -1,103 KiB | 0.40% lower |
| Peak PSS | 279,687 KiB | 279,207 KiB | -480 KiB | 0.17% lower |
| Idle RSS | 465,640 KiB | 464,396 KiB | -1,244 KiB | 0.27% lower |
| Peak RSS | 467,164 KiB | 466,660 KiB | -504 KiB | 0.11% lower |
| Idle CPU, 30 s | 0.066657% | 0.066659% | +0.000002 pp | timer-quantized; neutral |
| Process count | 3 | 3 | 0 | unchanged |
| Clean shutdown | 26.967 ms | 26.934 ms | -0.033 ms | neutral |
| 256 MiB SHA-256, payload-cold | 143.765 ms | 140.901 ms | -2.864 ms | 1.99% faster |
| 256 MiB SHA-256, warm | 143.297 ms | 140.081 ms | -3.215 ms | 2.24% faster |
| 4 MiB tar.gz extraction | 2.452 ms | 2.407 ms | -0.045 ms | 1.82% faster |
| 4 MiB Deflate ZIP extraction | 2.212 ms | 2.159 ms | -0.053 ms | 2.39% faster |
| Clean Rust release build | 78.206 s | 137.439 s | +59.233 s | 75.7% slower |
| Release no-change build | 229.606 ms | 212.623 ms | -16.983 ms | 7.40% faster |

### Exact-current transaction refresh

After the parity and packaging work, all four profiles were rebuilt from the
same exact source and run serially on the same host. Each received one
unrecorded normalization run followed by one recorded run. The table shows
seven-sample medians in milliseconds; the status rows are batched warm reads.
Prepared payloads exercise the shared production transaction cores while
download/current-release lookup, global command locking and logging, real
Steam/HOME discovery, and process control remain outside the benchmark.

| Transaction core | Baseline | Candidate A | Candidate B | Candidate C |
| --- | ---: | ---: | ---: | ---: |
| T7 install | 6.102 | 6.043 | 6.169 | 10.126 |
| T7 configure | 1.330 | 1.319 | 1.341 | 1.298 |
| T7 status, warm | 0.00988 | 0.00975 | 0.01004 | 0.01004 |
| T7 uninstall | 2.827 | 2.776 | 2.785 | 5.355 |
| DXVK install | 18.630 | 19.466 | 18.090 | 66.572 |
| DXVK configure | 14.283 | 14.571 | 14.666 | 61.495 |
| DXVK status, warm | 0.01134 | 0.01131 | 0.01139 | 0.01084 |
| DXVK uninstall | 8.075 | 8.116 | 8.011 | 31.375 |
| Enhanced install | 371.410 | 370.077 | 370.841 | 1,367.783 |
| Enhanced status, warm | 0.02530 | 0.02505 | 0.02527 | 0.02551 |
| Enhanced uninstall | 321.892 | 321.846 | 321.444 | 1,292.950 |
| Enhanced Linux compatibility configure | 6.397 | 6.413 | 6.324 | 6.653 |
| Enhanced Linux compatibility cleanup | 2.773 | 2.890 | 2.767 | 2.890 |

Candidate A's payload-cold/warm SHA-256 deltas were `+0.31%` and `+0.04%`,
while tar.gz and Deflate ZIP extraction improved by `3.10%` and `7.23%` in
this refresh. Every transaction assertion passed. Candidate B also passed the
operation gates but remains rejected by the retained memory evidence.
Candidate C again failed: payload-cold/warm SHA-256 regressed by `259.6%` and
`348.3%`, tar.gz extraction regressed by `4.94%`, and its hash-heavy T7,
DXVK, and Enhanced transactions materially slowed. Raw arrays, exact deltas,
profiles, and gate results are retained under `exactCurrentOperationRefresh`
in [`optimization-measurements.json`](https://github.com/boggedbrush/PatchOpsIII/blob/f8b1d9cbf9d579af309f09f9ad4b9a6ee9a562e1/benchmarks/optimization-measurements.json).

The snapshot raw `.deb` is 94.7% smaller than its AppImage because WebKitGTK
and GTK come from Debian/Ubuntu packages rather than the download. It is not a
portable replacement for the AppImage. CI rewrites the beta package version
from Tauri's `1.3.0-beta4` to Debian-safe `1.3.0~beta4`, hashes that canonical
artifact, then tests install, upgrade, launch, clean shutdown, purge, and user
data preservation on Ubuntu. The historical Arch-derived measurement host
lacked `dpkg-deb`, so its hash below remains explicitly the raw,
pre-canonicalization artifact. The exact-current Ubuntu-native release and
canonical package are recorded separately below.

Exact profile-selection hashes (historical snapshot):

- Native executable: `789c131150f83113c807417f13a987ae2ee20a99810a2d6e8e54cc0fe907e592`
- AppImage: `148ec6b71448da8ff09b26ea5f58761e89b8e492103b3ee0ee05e6fa800f0459`
- Raw pre-canonicalization `.deb`: `09e23bbc1c2800431bc3b66d858bf8c8a44dd6dbf702cb09dfde19325c8e4794`
- Windows MSVC cross-linked executable: `d0dd7c097b164f99068bd8c2a6d385aeb1c1f61ed68f6cacb91c1b77c8ce6eb5`

The historical post-parity host artifacts retain the selected profile while
adding the final legacy, rollback, and release safeguards. That snapshot's native binary is
15,063,696 bytes, the AppImage is 108,526,072 bytes, the AppDir is 316,864,183
bytes, and the raw `.deb` is 5,895,776 bytes (14,771 KiB installed). Its
hashes are:

- Native executable: `410225a5690fcc3c5ce838d0b10ea0098d31b010e2db59aa3b5defeb3b0ba643`
- AppImage: `c6253fdcd3cb11154f8c67a597d720fbc42976fbaf6375d37964268419c22b1c`
- Raw pre-canonicalization `.deb`: `a2bb7f5b64bd6f033ad15ea3c5a294374fee7638a57d2079fff0a45d4b412371`
- Windows MSVC cross-linked executable: `052219858a53693043279e6582472dd0b1439f08abf06eb88b806a9784433cc5`

The profile-selection AppImage closure audit found no removable development payload and retained
all WebKitGTK, JavaScriptCoreGTK, ICU, GTK, image, TLS, input, plugin, and helper
files. Its extracted duplicate aliases are already deduplicated by SquashFS, so
converting them would not materially reduce the download. The snapshot's
`usr/lib` was unchanged, so its measured AppDir savings came from `usr/bin`;
the exact-current Ubuntu-native closure is recorded in
[`optimization-final-package.json`](https://github.com/boggedbrush/PatchOpsIII/blob/f8b1d9cbf9d579af309f09f9ad4b9a6ee9a562e1/benchmarks/optimization-final-package.json).

### Exact-current Ubuntu 22.04-native packages

The release artifacts were rebuilt twice from Ubuntu Base 22.04.5 under
`SOURCE_DATE_EPOCH=1779660559`. The native binary and AppImage were
byte-identical across two package-scoped clean builds. Tauri's two raw Debian
containers differed because of timestamps, but independently canonicalizing
each input produced the same release `.deb` byte for byte.

| Artifact or closure | Bytes | SHA-256 or compatibility |
| --- | ---: | --- |
| Native Rust executable | 15,086,240 | `5b0ed287a036cf230cdc446cdaf35ade5e7161bc5383dd274af9b209f4c0963d` |
| Portable AppImage | 83,167,736 | `49f6f548cea81addc171c2bd2d9ace2a91a49d27bd04f8683a44f86db054a2a4` |
| Extracted AppDir | 264,398,455 | maximum `GLIBC_2.35` across 170 ELF files |
| AppDir `usr/bin` | 15,156,171 | main bundled executable is 15,130,096 bytes |
| AppDir `usr/lib` | 248,550,358 | WebKitGTK, JavaScriptCoreGTK, ICU, GTK, and reachable runtime modules retained |
| Raw Tauri `.deb`, build 1 | 5,905,960 | `e4e2e6e95b9d1b1bc0ee45d31d2ebe7c3d331b7f0bcc273f145bbab916467499` |
| Raw Tauri `.deb`, build 2 | 5,905,958 | `14dfbb29248c070eb9ebee351fc4f11b9083b8f0f043a6d9e69297c9373359f3` |
| Canonical release `.deb` | 4,578,248 | `0d68f3db495713d06fe26a245be11830fd5640f25a77ecb655303fcb0543d2fe` |

The canonical package is `patch-ops-iii` `1.3.0~beta4` for `amd64`, reports
14,793 KiB installed, and declares `libbz2-1.0`, `libwebkit2gtk-4.1-0`, and
`libgtk-3-0`. Its download is 94.50% smaller than this AppImage because those
GUI libraries move to distribution-managed dependencies. The AppImage remains
the primary portable/Steam Deck artifact and bundles GTK/WebKit; it still
relies on ordinary host primitives such as glibc and low-level graphics,
font/text, X11, and D-Bus support.

The Ubuntu-native AppImage is materially smaller than the historical CachyOS
artifact, but that difference is not attributed to Rust optimization: the two
distributions supply different WebKitGTK, ICU, codec, and supporting library
builds. The controlled same-host table above is the evidence for profile
savings. The exact Ubuntu closure, duplicate-file audit, dependency mapping,
Debian metadata, and validation results are in
[`optimization-final-package.json`](https://github.com/boggedbrush/PatchOpsIII/blob/f8b1d9cbf9d579af309f09f9ad4b9a6ee9a562e1/benchmarks/optimization-final-package.json).

The exact-current AppImage passed the clean Ubuntu UID 1000 smoke with system
GTK, WebKitGTK, and Ayatana AppIndicator packages absent. The final official
Arch root smoke reached WebKit under the same package constraints but did not
pass because the controlled Xvfb environment could not create an EGL display.
The exact-current Debian install/upgrade/purge rerun has no completion evidence;
only the earlier provisional old/current package GUI smokes completed. These
limitations are recorded without treating environment failure as application
acceptance.

## Recorded comparison

All timed rows are medians and lower is better. `Delta` is final minus
`origin/main`. `N/C` means the values are recorded but not semantically
comparable; `N/M` means not measured.

### Runtime

| Metric | `origin/main` | React + Tauri + Rust | Delta | Improvement or regression |
| --- | ---: | ---: | ---: | ---: |
| Backend/native ready | 630.027 ms | 161.165 ms | -468.862 ms | 74.4% faster |
| Warm interactive | 1,576.341 ms | 919.022 ms | -657.319 ms | 41.7% faster |
| Payload-cold interactive | 1,859.413 ms | 1,118.437 ms | -740.976 ms | 39.8% faster |
| Idle PSS | 235,431 KiB | 319,908 KiB | +84,477 KiB | 35.9% higher |
| Peak PSS | 236,993 KiB | 322,537 KiB | +85,544 KiB | 36.1% higher |
| Idle RSS | 520,680 KiB | 504,168 KiB | -16,512 KiB | 3.2% lower |
| Peak RSS | 522,032 KiB | 506,796 KiB | -15,236 KiB | 2.9% lower |
| Idle CPU, 30 s | 0.2000% | 0.0333% | -0.1667 pp | 83.3% lower |
| Process count | 8 | 3 | -5 | 62.5% fewer |
| Shutdown | 23.212 ms | 31.986 ms | +8.774 ms | 37.8% slower |

PSS is the more conservative process-tree memory result because RSS can count
shared pages more than once. The migration therefore does not claim a memory
reduction: the final host-state refresh put idle PSS 35.9% above the historical
baseline even though aggregate RSS was 3.2% lower. Both observed WebKit timing
and proportional-memory regimes remain in the raw benchmark records.

### Fixed-fixture operations

| Metric | `origin/main` | React + Tauri + Rust | Delta | Improvement or regression |
| --- | ---: | ---: | ---: | ---: |
| Steam detection, payload-cold | 0.055171 ms | 0.014640 ms | -0.040531 ms | 73.5% faster |
| Steam detection, warm | 0.002730 ms | 0.014412 ms | +0.011682 ms | 428.0% slower |
| Game detection, payload-cold | 0.045181 ms | 0.003700 ms | -0.041481 ms | 91.8% faster |
| Game detection, warm | 0.017454 ms | 0.003924 ms | -0.013530 ms | 77.5% faster |
| Config read | 0.017381 ms | 0.004214 ms | -0.013166 ms | 75.8% faster |
| Config write | 0.049981 ms | 0.022180 ms | -0.027801 ms | 55.6% faster |
| 256 MiB SHA-256, payload-cold | 140.692 ms | 140.029 ms | -0.663 ms | 0.5% faster |
| 256 MiB SHA-256, warm | 140.493 ms | 139.193 ms | -1.300 ms | 0.9% faster |
| State/status aggregation | 290.300 ms | 0.036538 ms | N/C | Different boundaries; no claim |
| Install/uninstall wall time | N/M | N/M | N/M | Network/transaction boundaries differ |
| Representative-operation CPU | N/M | N/M | N/M | No claim |

The sub-millisecond rows are useful for detecting gross regressions but are
sensitive to filesystem and timer noise. The SHA-256 result is the meaningful
large-file comparison; the final state path avoids redundant hashes rather than
making an individual 256 MiB hash faster.

### Builds and packages

| Metric | `origin/main` | React + Tauri + Rust | Delta | Improvement or regression |
| --- | ---: | ---: | ---: | ---: |
| TypeScript typecheck | 1,287.000 ms | 1,220.220 ms | -66.780 ms | 5.2% faster |
| Frontend-only build | 1,290.000 ms | 1,273.845 ms | -16.155 ms | 1.3% faster |
| Renderer/desktop-JS pipeline | 3,127.000 ms | 2,489.153 ms | -637.847 ms | 20.4% faster |
| Incremental backend/core build | 340.500 ms | 219.757 ms | -120.743 ms | 35.5% faster |
| AppImage package-only | 2,836.000 ms | 2,607.847 ms | -228.153 ms | 8.0% faster |
| Clean backend/core build | 14,522 ms | 140,931.689 ms | +126,409.689 ms | 870.5% slower |
| Clean full AppImage build | 20,137 ms | 160,623.289 ms | +140,486.289 ms | 697.7% slower |
| Electron main build | 571.000 ms | Removed | N/C | Architecture removed |
| MSI packaging | N/M | N/M | N/M | Requires Windows CI |
| AppImage size | 117.03 MiB | 103.50 MiB | -13.53 MiB | 11.6% smaller |
| Extracted AppImage/AppDir | 247.06 MiB | 302.19 MiB | +55.12 MiB | 22.3% larger |
| Renderer output | 310.68 KiB | 306.96 KiB | -3.72 KiB | 1.2% smaller |
| Python backend/native binary | 24.62 MiB | 14.37 MiB | N/C | Different contents; no claim |
| Raw system-library `.deb` | N/M | 5.62 MiB | N/M | 94.6% smaller than final AppImage |

Clean builds use empty target directories but retain downloaded dependency and
packaging caches. The local sysroot required `NO_STRIP=1` for system libraries,
so the unpacked final size is conservative; Rust symbols remain stripped. The
baseline used Bun 1.3.13 while the final host had Bun 1.4.0, so the small
TypeScript/frontend improvements are observed results rather than a controlled
claim that the architecture alone caused them. The
verified beta4 AppImage is 108,526,072 bytes with SHA-256
`c6253fdcd3cb11154f8c67a597d720fbc42976fbaf6375d37964268419c22b1c`;
it reached both readiness markers and closed cleanly through a native window
event. The Windows MSI workflow and configuration passed static validation, but
an MSI was not built or timed on this Linux host. As an additional cross-platform
gate, cargo-xwin 0.23.1 successfully checked the complete Windows target and linked
a 15,184,896-byte PE32+ GUI release executable (SHA-256
`aa03732b8f6c7b3cff4ef452af74f0ab1fe88a5811bada02574dd9a08ed5d58c`). MSI
authoring and upgrade testing remain Windows-only; the Windows workflow performs
a hash-pinned beta3 Electron MSI to current Tauri MSI upgrade, launches the
installed executable, observes its native window, and closes it before upload.

Overall, the migration materially improves startup, idle CPU, process count,
compressed package size, incremental work, and architectural simplicity. Its
measured tradeoffs are higher aggregate PSS and unpacked size, a slower
shutdown, and much slower clean Rust/full builds. Exact arrays, historical host
data, the exact-current transaction refresh, Ubuntu-native package evidence,
and limitations are retained in [`origin-main.json`](https://github.com/boggedbrush/PatchOpsIII/blob/f8b1d9cbf9d579af309f09f9ad4b9a6ee9a562e1/benchmarks/origin-main.json), [`final-tauri.json`](https://github.com/boggedbrush/PatchOpsIII/blob/f8b1d9cbf9d579af309f09f9ad4b9a6ee9a562e1/benchmarks/final-tauri.json),
[`optimization-measurements.json`](https://github.com/boggedbrush/PatchOpsIII/blob/f8b1d9cbf9d579af309f09f9ad4b9a6ee9a562e1/benchmarks/optimization-measurements.json), and [`optimization-final-package.json`](https://github.com/boggedbrush/PatchOpsIII/blob/f8b1d9cbf9d579af309f09f9ad4b9a6ee9a562e1/benchmarks/optimization-final-package.json).
