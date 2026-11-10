# PatchOpsIII benchmark: `legacy`

- Started: 2026-11-10T07:33:55+00:00  |  finished: 2026-11-10T07:51:59+00:00
- Source: `/home/user/PatchOpsIII` @ `25034853a663` (claude/serene-brown-26fl09, dirty tracked files: 1)
- Machine: Intel(R) Xeon(R) Processor @ 2.10GHz x4, 16086 MB RAM, Linux-6.18.44-fc-v338-x86_64-with-glibc2.39
- Run mode: packaged  |  run-as uid: 65534  |  harness 1.0
- Idle guard: threshold 0.7, 6 checks, max load1 at section start 0.69, timeouts: 0, enabled: True

## Build

_Build section measured by the invocation started 2026-11-10T07:15:20+00:00 (carried over into this file); idle guard at its start: load1 0.67._

| step | wall | CPU (user+sys) | peak RSS (sum of tree, sampled) | exit |
|---|---:|---:|---:|---:|
| bun-install-cold | 11.2 s | 8.3 s | 146.3 MiB | 0 |
| bun-install-warm | 3.8 s | 4.6 s | 116.2 MiB | 0 |
| venv-create | 2.8 s | 2.5 s | 92.1 MiB | 0 |
| pip-download-cold | 4.9 s | 2.1 s | 112.3 MiB | 0 |
| venv-create-2 | 2.6 s | 2.3 s | 91.4 MiB | 0 |
| pip-install-from-wheelhouse | 4.0 s | 3.3 s | 70.6 MiB | 0 |
| bun-run-build | 8.3 s | 10.9 s | 332.5 MiB | 0 |
| pyinstaller-backend | 26.9 s | 25.7 s | 147.5 MiB | 0 |
| electron-builder-linux-dir | 8.0 s | 3.9 s | 189.4 MiB | 0 |
| **total** | **74.6 s** | **63.7 s** | max 332.5 MiB | |

Network-bound vs local: bun_download_estimate_s=7.39, pip_download_s=4.918, network_bound_s_estimate=12.31, local_install_and_compile_s=53.42

Sizes: distributable_unpacked_app_dir_bytes=247.6 MiB, backend_binary_bytes=25.6 MiB, dependency_footprint_bytes=749.5 MiB

| artefact | apparent size | disk usage | files |
|---|---:|---:|---:|
| bun_cache | 401.6 MiB | 433.9 MiB | 12358 |
| electron_cache | 100.6 MiB | 100.6 MiB | 1 |
| node_modules | 663.8 MiB | 697.7 MiB | 12940 |
| wheelhouse | 17.6 MiB | 17.7 MiB | 31 |
| venv | 85.7 MiB | 100.5 MiB | 5481 |
| dist_main | 7.8 KiB | 12.0 KiB | 2 |
| dist_renderer | 311.1 KiB | 316.0 KiB | 4 |
| backend_binary | 25.6 MiB | 25.6 MiB | 1 |
| packaged_app_dir | 247.6 MiB | 247.7 MiB | 24 |

- note: Electron binary was downloaded by bun's postinstall as part of bun-install-cold (and unpacked from the local electron cache in bun-install-warm)

## GUI runs

### Startup (cold launch under Xvfb)

| milestone | median (min-max) |
|---|---|
| first top-level window mapped | 391 ms (299-418, n=5) |
| backend/RPC health answers | 1123 ms (951-1244, n=5) |
| app ready (status loaded) | 1134 ms (960-1255, n=5) |
| renderer first connects to backend | 1256 ms (1169-1343, n=5) |

runs: 5 (ok 5); failures: []

### Memory (whole process tree)

| metric | idle (10 s window after 10 s settle) | peak during workload | final after workload |
|---|---|---|---|
| PSS | 376.0 MiB (375.4 MiB-378.0 MiB) | 393.1 MiB (392.1 MiB-394.6 MiB) | 392.1 MiB (390.4 MiB-392.6 MiB) |
| RSS (sum) | 682.5 MiB (681.0 MiB-686.6 MiB) | 706.2 MiB (705.4 MiB-710.5 MiB) | 704.7 MiB (703.2 MiB-707.4 MiB) |
| processes | 8 (8-8) | 9 (9-9) | 8 (8-8) |
| threads | 73 (71-74) | 75 (74-76) | 74 (72-74) |

Idle CPU: 0.20 % (0.20-0.40, n=3) of one core. runs: 3

Idle process breakdown (first run):

| role | pid | PSS | RSS | threads |
|---|---:|---:|---:|---:|
| main | 10609 | 117.5 MiB | 197.7 MiB | 34 |
| gpu-process | 10669 | 70.0 MiB | 127.2 MiB | 14 |
| renderer | 10693 | 69.0 MiB | 112.6 MiB | 9 |
| patchops-backen | 10728 | 61.5 MiB | 64.0 MiB | 6 |
| utility | 10683 | 29.2 MiB | 74.4 MiB | 8 |
| zygote | 10617 | 14.2 MiB | 52.5 MiB | 1 |
| zygote | 10618 | 13.7 MiB | 50.7 MiB | 1 |
| patchops-backen | 10679 | 331.0 KiB | 2.1 MiB | 1 |

**Host-load check:** all measured windows were quiet (foreign CPU load <= 8.0% idle / <= 15.0% workload, in percent of all cores).

### Capability workload

Parity pass: 161 steps, wall 76.46 s (75.89-77.42, n=3); of which waiting on the (fake) Steam client 75.70 s (75.14-76.69, n=3); **excluding Steam waits 0.470 s (0.470-0.480, n=3)**.
Latency pass wall: 130.9 s (130.4-131.0, n=3). Golden mismatches per run: [0, 0, 0] (quirk steps: [0, 0, 0]).

| op (label) | n | min | median | p95 | max | errors |
|---|---:|---:|---:|---:|---:|---:|
| all-intros-skip(off) | 90 | 3.66 | 4.40 | 5.87 | 6.99 | 0 |
| all-intros-skip(on) | 90 | 3.51 | 4.35 | 6.51 | 8.16 | 0 |
| browse | 120 | 1.23 | 1.82 | 2.60 | 4.63 | 0 |
| browse(common) | 120 | 1.00 | 1.55 | 2.15 | 3.44 | 0 |
| browse(game) | 120 | 1.15 | 1.71 | 2.16 | 2.45 | 0 |
| config(DrawFPS) | 90 | 3.02 | 4.03 | 5.17 | 6.74 | 0 |
| config(FOV) | 90 | 3.18 | 3.92 | 4.88 | 9.84 | 0 |
| config(FullScreenMode) | 90 | 3.16 | 3.92 | 5.21 | 8.27 | 0 |
| config(MaxFPS) | 90 | 3.23 | 4.02 | 5.42 | 14.59 | 0 |
| config(MaxFrameLatency) | 90 | 3.25 | 3.96 | 4.94 | 6.29 | 0 |
| config(RefreshRate) | 90 | 3.13 | 4.05 | 5.29 | 6.15 | 0 |
| config(ResolutionPercent) | 90 | 2.78 | 3.96 | 4.84 | 5.26 | 0 |
| config(RestrictGraphicsOptions) | 90 | 3.20 | 4.02 | 5.28 | 7.03 | 0 |
| config(SerializeRender) | 90 | 3.09 | 3.98 | 4.89 | 7.03 | 0 |
| config(SmoothFramerate) | 90 | 2.98 | 4.08 | 5.24 | 7.01 | 0 |
| config(Vsync) | 90 | 2.82 | 4.10 | 5.05 | 7.06 | 0 |
| config(WindowSize) | 90 | 3.20 | 4.06 | 4.95 | 6.00 | 0 |
| config-readonly(off) | 90 | 2.94 | 3.71 | 4.56 | 5.55 | 0 |
| config-readonly(on) | 90 | 2.99 | 3.75 | 4.76 | 5.79 | 0 |
| d3dcompiler(off) | 90 | 2.87 | 3.87 | 5.14 | 8.37 | 0 |
| d3dcompiler(on) | 90 | 2.84 | 3.77 | 5.58 | 9.36 | 0 |
| dxvk-config(default) | 90 | 3.63 | 4.22 | 5.58 | 6.84 | 0 |
| dxvk-config(tuned) | 90 | 3.61 | 4.24 | 5.26 | 7.34 | 0 |
| dxvk-uninstall | 60 | 3.59 | 4.93 | 7.60 | 9.45 | 0 |
| enhanced-uninstall | 6 | 6606.58 | 7107.29 | 7123.39 | 7126.77 | 0 |
| enhanced-validate(invalid) | 90 | 3.19 | 3.92 | 4.94 | 6.69 | 0 |
| enhanced-validate(valid) | 90 | 3.02 | 3.81 | 5.50 | 7.14 | 0 |
| exe-swap/compatible | 90 | 1.07 | 1.64 | 2.88 | 5.80 | 0 |
| exe-swap/compatible-depot | 90 | 2.52 | 3.11 | 4.42 | 6.03 | 0 |
| exe-swap/current | 90 | 1.23 | 1.70 | 2.37 | 3.00 | 0 |
| exe-swap/enhanced | 90 | 1.15 | 1.63 | 2.19 | 3.90 | 0 |
| game-directory | 120 | 2.80 | 3.52 | 4.34 | 12.05 | 0 |
| health | 300 | 0.22 | 0.37 | 0.84 | 3.12 | 0 |
| intro-skip(off) | 90 | 2.98 | 3.91 | 4.99 | 6.35 | 0 |
| intro-skip(on) | 90 | 2.86 | 3.66 | 5.23 | 7.16 | 0 |
| launch | 45 | 1.69 | 1.99 | 3.17 | 3.60 | 0 |
| launch-options(default) | 9 | 6595.20 | 7106.82 | 7116.47 | 7119.40 | 0 |
| launch-options(offline) | 9 | 6608.70 | 7106.09 | 7112.67 | 7113.61 | 0 |
| logs/clear | 90 | 2.49 | 3.41 | 4.52 | 5.68 | 0 |
| logs/payload | 90 | 0.62 | 1.22 | 2.02 | 3.07 | 0 |
| mod-files/clear | 90 | 3.16 | 4.33 | 5.89 | 8.62 | 0 |
| presets/apply(Low-Latency) | 90 | 4.16 | 5.01 | 7.05 | 10.58 | 0 |
| presets/apply(Smooth) | 90 | 4.10 | 4.97 | 6.47 | 7.67 | 0 |
| release-channel(beta) | 90 | 2.74 | 3.43 | 4.82 | 6.39 | 0 |
| release-channel(stable) | 90 | 2.78 | 3.40 | 4.31 | 5.41 | 0 |
| reset-stock | 6 | 13705.84 | 13720.25 | 14095.35 | 14219.16 | 0 |
| status | 180 | 2.05 | 2.57 | 3.06 | 4.17 | 0 |
| status(modded) | 120 | 2.06 | 2.61 | 4.28 | 5.78 | 0 |
| t7-config(friends) | 90 | 3.50 | 4.30 | 5.62 | 8.27 | 0 |
| t7-config(gamertag) | 90 | 3.48 | 4.15 | 5.84 | 6.20 | 0 |
| t7-config(password) | 90 | 3.49 | 4.23 | 5.44 | 6.50 | 0 |
| t7-uninstall | 60 | 4.48 | 5.24 | 7.75 | 10.76 | 0 |
| vram-target(full) | 90 | 3.38 | 4.19 | 5.18 | 6.55 | 0 |
| vram-target(limited) | 90 | 3.36 | 4.29 | 5.66 | 8.88 | 0 |
| workshop-install | 6 | 6614.88 | 7109.13 | 7112.48 | 7112.64 | 0 |

Per-op aggregate (ms):

| op | n | median | p95 |
|---|---:|---:|---:|
| all-intros-skip | 180 | 4.37 | 6.14 |
| browse | 360 | 1.69 | 2.31 |
| config | 1080 | 4.00 | 5.16 |
| config-readonly | 180 | 3.72 | 4.71 |
| d3dcompiler | 180 | 3.79 | 5.35 |
| dxvk-config | 180 | 4.23 | 5.35 |
| dxvk-uninstall | 60 | 4.93 | 7.60 |
| enhanced-uninstall | 6 | 7107.29 | 7123.39 |
| enhanced-validate | 180 | 3.86 | 5.31 |
| exe-swap/compatible | 90 | 1.64 | 2.88 |
| exe-swap/compatible-depot | 90 | 3.11 | 4.42 |
| exe-swap/current | 90 | 1.70 | 2.37 |
| exe-swap/enhanced | 90 | 1.63 | 2.19 |
| game-directory | 120 | 3.52 | 4.34 |
| health | 300 | 0.37 | 0.84 |
| intro-skip | 180 | 3.77 | 5.12 |
| launch | 45 | 1.99 | 3.17 |
| launch-options | 18 | 7106.46 | 7114.48 |
| logs/clear | 90 | 3.41 | 4.52 |
| logs/payload | 90 | 1.22 | 2.02 |
| mod-files/clear | 90 | 4.33 | 5.89 |
| presets/apply | 180 | 4.99 | 6.58 |
| release-channel | 180 | 3.41 | 4.39 |
| reset-stock | 6 | 13720.25 | 14095.35 |
| status | 180 | 2.57 | 3.06 |
| status(modded) | 120 | 2.61 | 4.28 |
| t7-config | 270 | 4.25 | 5.65 |
| t7-uninstall | 60 | 5.24 | 7.75 |
| vram-target | 180 | 4.26 | 5.27 |
| workshop-install | 6 | 7109.13 | 7112.48 |

### Parity pass steps (first run; also the golden step list)

| # | step | op | latency | envelope ok | app ok | tags | golden |
|---:|---|---|---:|---|---|---|---|
| 1 | health | health | 1.7 ms | True | True |  | ok |
| 2 | status-initial | status | 3.7 ms | True | None |  | ok |
| 3 | browse-default | browse | 2.6 ms | True | None |  | ok |
| 4 | browse-common-dir | browse | 2.4 ms | True | None |  | ok |
| 5 | browse-game-dir | browse | 2.4 ms | True | None |  | ok |
| 6 | browse-home | browse | 2.3 ms | True | None |  | ok |
| 7 | browse-missing-dir | browse | 2.1 ms | True | None |  | ok |
| 8 | game-directory-not-a-game | game-directory | 1.8 ms | True | False |  | ok |
| 9 | game-directory-missing-arg | game-directory | 1.1 ms | False | None |  | ok |
| 10 | game-directory-empty | game-directory | 1.5 ms | False | None |  | ok |
| 11 | game-directory-alt-install | game-directory | 3.9 ms | True | True |  | ok |
| 12 | status-alt-install | status | 2.5 ms | True | None |  | ok |
| 13 | config-without-config-ini | config | 1.9 ms | True | False |  | ok |
| 14 | config-readonly-without-config-ini | config-readonly | 1.4 ms | True | False |  | ok |
| 15 | browse-after-saved-dir | browse | 1.5 ms | True | None |  | ok |
| 16 | game-directory-main | game-directory | 3.0 ms | True | True |  | ok |
| 17 | status-main | status | 2.3 ms | True | None |  | ok |
| 18 | release-channel-beta | release-channel | 3.6 ms | True | True |  | ok |
| 19 | release-channel-invalid | release-channel | 0.8 ms | True | False |  | ok |
| 20 | release-channel-missing | release-channel | 0.7 ms | False | None |  | ok |
| 21 | release-channel-stable | release-channel | 3.9 ms | True | True |  | ok |
| 22 | config-MaxFPS | config | 4.1 ms | True | True |  | ok |
| 23 | config-FOV | config | 4.8 ms | True | True |  | ok |
| 24 | config-FullScreenMode | config | 5.0 ms | True | True |  | ok |
| 25 | config-WindowSize | config | 4.0 ms | True | True |  | ok |
| 26 | config-RefreshRate | config | 4.5 ms | True | True |  | ok |
| 27 | config-ResolutionPercent | config | 3.8 ms | True | True |  | ok |
| 28 | config-Vsync | config | 4.1 ms | True | True |  | ok |
| 29 | config-DrawFPS | config | 4.3 ms | True | True |  | ok |
| 30 | config-SmoothFramerate | config | 4.7 ms | True | True |  | ok |
| 31 | config-RestrictGraphicsOptions | config | 4.3 ms | True | True |  | ok |
| 32 | config-SerializeRender | config | 4.4 ms | True | True |  | ok |
| 33 | config-MaxFrameLatency | config | 5.3 ms | True | True |  | ok |
| 34 | config-append-new-key | config | 4.5 ms | True | True |  | ok |
| 35 | config-bool-value | config | 4.1 ms | True | True |  | ok |
| 36 | config-float-value | config | 5.7 ms | True | True |  | ok |
| 37 | config-default-comment | config | 4.0 ms | True | True |  | ok |
| 38 | config-backslash-n | config | 4.6 ms | True | True | quirk | ok |
| 39 | config-backslash-n-existing | config | 4.0 ms | True | True | quirk | ok |
| 40 | config-bad-escape | config | 1.6 ms | True | False | quirk | ok |
| 41 | config-empty-key | config | 1.0 ms | False | None |  | ok |
| 42 | config-missing-value | config | 0.8 ms | False | None |  | ok |
| 43 | config-null-value | config | 0.7 ms | False | None |  | ok |
| 44 | vram-target-limited-80 | vram-target | 4.3 ms | True | True |  | ok |
| 45 | vram-target-limited-100 | vram-target | 3.9 ms | True | True |  | ok |
| 46 | vram-target-limited-default | vram-target | 4.0 ms | True | True |  | ok |
| 47 | vram-target-full | vram-target | 4.0 ms | True | True |  | ok |
| 48 | vram-target-too-low | vram-target | 0.9 ms | False | None |  | ok |
| 49 | vram-target-too-high | vram-target | 1.0 ms | False | None |  | ok |
| 50 | vram-target-missing-limited | vram-target | 1.0 ms | False | None |  | ok |
| 51 | preset-smooth | presets/apply | 6.2 ms | True | True |  | ok |
| 52 | preset-low-latency | presets/apply | 4.9 ms | True | True |  | ok |
| 53 | preset-unknown | presets/apply | 1.5 ms | True | False |  | ok |
| 54 | preset-missing-name | presets/apply | 0.8 ms | False | None |  | ok |
| 55 | intro-skip-on | intro-skip | 3.5 ms | True | True |  | ok |
| 56 | intro-skip-on-again | intro-skip | 3.9 ms | True | True |  | ok |
| 57 | intro-skip-off | intro-skip | 3.6 ms | True | True |  | ok |
| 58 | intro-skip-off-again | intro-skip | 3.5 ms | True | True |  | ok |
| 59 | intro-skip-bad-arg | intro-skip | 0.9 ms | False | None |  | ok |
| 60 | intro-skip-on-then-all | intro-skip | 3.3 ms | True | True |  | ok |
| 61 | all-intros-on | all-intros-skip | 5.0 ms | True | True |  | ok |
| 62 | all-intros-on-again | all-intros-skip | 4.0 ms | True | True |  | ok |
| 63 | status-all-intros | status | 2.6 ms | True | None |  | ok |
| 64 | all-intros-off | all-intros-skip | 5.0 ms | True | True |  | ok |
| 65 | all-intros-off-again | all-intros-skip | 3.5 ms | True | True |  | ok |
| 66 | all-intros-no-video-dir | all-intros-skip | 1.4 ms | True | False |  | ok |
| 67 | intro-skip-no-video-dir | intro-skip | 3.3 ms | True | True |  | ok |
| 68 | restore-video-dir | status | 2.5 ms | True | None |  | ok |
| 69 | d3dcompiler-on | d3dcompiler | 4.0 ms | True | True |  | ok |
| 70 | d3dcompiler-on-again | d3dcompiler | 3.6 ms | True | True |  | ok |
| 71 | d3dcompiler-off | d3dcompiler | 4.0 ms | True | True |  | ok |
| 72 | d3dcompiler-off-again | d3dcompiler | 1.8 ms | True | False |  | ok |
| 73 | config-readonly-on | config-readonly | 6.0 ms | True | True |  | ok |
| 74 | status-readonly | status | 2.6 ms | True | None |  | ok |
| 75 | config-write-while-readonly | config | 1.9 ms | True | False | quirk | ok |
| 76 | preset-while-readonly | presets/apply | 2.8 ms | True | False | quirk | ok |
| 77 | vram-while-readonly | vram-target | 2.0 ms | True | False | quirk | ok |
| 78 | config-readonly-off | config-readonly | 3.9 ms | True | True |  | ok |
| 79 | config-after-readonly-off | config | 4.6 ms | True | True |  | ok |
| 80 | launch-options-offline | launch-options | 7109.5 ms | True | True | steam | ok |
| 81 | launch-options-unsupported | launch-options | 3.3 ms | True | False |  | ok |
| 82 | launch-options-workshop-all-around | launch-options | 7102.1 ms | True | True | steam | ok |
| 83 | launch-options-preserve-fs-game | launch-options | 6602.2 ms | True | True | steam | ok |
| 84 | launch-options-default | launch-options | 7104.3 ms | True | True | steam | ok |
| 85 | workshop-install-unknown | workshop-install | 1.2 ms | True | False |  | ok |
| 86 | workshop-install-empty-id | workshop-install | 1.1 ms | False | None |  | ok |
| 87 | workshop-install-ultimate | workshop-install | 7109.7 ms | True | True | steam | ok |
| 88 | status-launch-profiles | status | 2.7 ms | True | None |  | ok |
| 89 | launch | launch | 2.0 ms | True | True |  | ok |
| 90 | t7-config-without-conf | t7-config | 1.4 ms | True | False |  | ok |
| 91 | t7-password-without-conf | t7-config | 1.2 ms | True | False |  | ok |
| 92 | t7-friends-without-conf | t7-config | 1.4 ms | True | False |  | ok |
| 93 | status-fake-t7 | status | 2.8 ms | True | None |  | ok |
| 94 | t7-gamertag | t7-config | 3.7 ms | True | True |  | ok |
| 95 | t7-gamertag-trim | t7-config | 4.5 ms | True | True |  | ok |
| 96 | t7-gamertag-empty | t7-config | 1.4 ms | True | False |  | ok |
| 97 | t7-gamertag-too-long | t7-config | 1.3 ms | True | False |  | ok |
| 98 | t7-password-set | t7-config | 4.3 ms | True | True |  | ok |
| 99 | t7-password-trim | t7-config | 4.1 ms | True | True |  | ok |
| 100 | t7-password-clear | t7-config | 4.0 ms | True | True |  | ok |
| 101 | t7-friends-on | t7-config | 3.6 ms | True | True |  | ok |
| 102 | t7-friends-off | t7-config | 4.4 ms | True | True |  | ok |
| 103 | t7-all-fields | t7-config | 5.8 ms | True | True |  | ok |
| 104 | t7-no-fields | t7-config | 3.0 ms | True | True |  | ok |
| 105 | t7-colorcode-only | t7-config | 2.7 ms | True | True |  | ok |
| 106 | t7-uninstall | t7-uninstall | 5.4 ms | True | True |  | ok |
| 107 | t7-uninstall-again | t7-uninstall | 3.7 ms | True | True |  | ok |
| 108 | t7-config-after-uninstall | t7-config | 1.1 ms | True | False |  | ok |
| 109 | dxvk-config-without-install | dxvk-config | 3.7 ms | True | True |  | ok |
| 110 | dxvk-uninstall-conf-only | dxvk-uninstall | 4.3 ms | True | True |  | ok |
| 111 | status-fake-dxvk | status | 2.6 ms | True | None |  | ok |
| 112 | dxvk-config-recommended | dxvk-config | 3.8 ms | True | True |  | ok |
| 113 | dxvk-config-tuned | dxvk-config | 4.1 ms | True | True |  | ok |
| 114 | status-dxvk-tuned | status | 2.6 ms | True | None |  | ok |
| 115 | dxvk-config-bogus-tearfree | dxvk-config | 3.9 ms | True | True |  | ok |
| 116 | dxvk-config-threads-too-high | dxvk-config | 1.0 ms | False | None |  | ok |
| 117 | dxvk-config-fps-too-high | dxvk-config | 0.9 ms | False | None |  | ok |
| 118 | dxvk-config-latency-too-high | dxvk-config | 0.9 ms | False | None |  | ok |
| 119 | dxvk-config-negative | dxvk-config | 0.9 ms | False | None |  | ok |
| 120 | dxvk-uninstall | dxvk-uninstall | 4.1 ms | True | True |  | ok |
| 121 | dxvk-uninstall-again | dxvk-uninstall | 2.9 ms | True | True |  | ok |
| 122 | enhanced-validate-empty | enhanced-validate | 2.6 ms | True | True |  | ok |
| 123 | enhanced-validate-valid | enhanced-validate | 3.7 ms | True | True |  | ok |
| 124 | enhanced-validate-invalid | enhanced-validate | 3.4 ms | True | True |  | ok |
| 125 | enhanced-validate-missing-path | enhanced-validate | 4.0 ms | True | True |  | ok |
| 126 | enhanced-validate-missing-arg | enhanced-validate | 0.8 ms | False | None |  | ok |
| 127 | enhanced-install-bad-source | enhanced-install | 1.7 ms | True | False |  | ok |
| 128 | enhanced-install-invalid-source | enhanced-install | 1.6 ms | True | False |  | ok |
| 129 | exe-swap-compatible-unverified | exe-swap/compatible | 1.8 ms | True | False |  | ok |
| 130 | exe-swap-depot-absent | exe-swap/compatible-depot | 3.0 ms | True | True |  | ok |
| 131 | exe-swap-current-unverified | exe-swap/current | 1.8 ms | True | False |  | ok |
| 132 | exe-swap-enhanced-unverified | exe-swap/enhanced | 1.8 ms | True | False |  | ok |
| 133 | status-fake-enhanced | status | 2.3 ms | True | None |  | ok |
| 134 | exe-swap-compatible-enhanced-active | exe-swap/compatible | 3.6 ms | True | False |  | ok |
| 135 | exe-swap-current-enhanced-active | exe-swap/current | 1.8 ms | True | False |  | ok |
| 136 | exe-swap-enhanced-already-active | exe-swap/enhanced | 3.2 ms | True | True |  | ok |
| 137 | enhanced-uninstall | enhanced-uninstall | 7103.6 ms | True | True | steam | ok |
| 138 | enhanced-uninstall-again | enhanced-uninstall | 6600.1 ms | True | True | steam | ok |
| 139 | status-enhanced-removed | status | 2.9 ms | True | None |  | ok |
| 140 | exe-swap-depot-fake-present | exe-swap/compatible-depot | 3.8 ms | True | True |  | ok |
| 141 | exe-swap-compatible-depot-bad-hash | exe-swap/compatible | 1.6 ms | True | False |  | ok |
| 142 | exe-swap-compatible-known-hash | exe-swap/compatible | 4.7 ms | True | False |  | ok |
| 143 | exe-swap-current-known-hash | exe-swap/current | 2.0 ms | True | False |  | ok |
| 144 | exe-swap-enhanced-known-hash | exe-swap/enhanced | 4.7 ms | True | True |  | ok |
| 145 | status-known-enhanced-hash | status | 5.3 ms | True | None |  | ok |
| 146 | unseed-hash | status | 3.6 ms | True | None |  | ok |
| 147 | logs-payload | logs/payload | 1.3 ms | True | True |  | ok |
| 148 | logs-clear | logs/clear | 4.3 ms | True | True |  | ok |
| 149 | logs-payload-after-clear | logs/payload | 1.5 ms | True | True |  | ok |
| 150 | mod-files-clear | mod-files/clear | 5.7 ms | True | True |  | ok |
| 151 | mod-files-clear-empty | mod-files/clear | 4.2 ms | True | True |  | ok |
| 152 | status-everything-modded | status | 2.7 ms | True | None |  | ok |
| 153 | pre-reset-intros | all-intros-skip | 5.3 ms | True | True |  | ok |
| 154 | pre-reset-d3d | d3dcompiler | 3.9 ms | True | True |  | ok |
| 155 | pre-reset-preset | presets/apply | 5.8 ms | True | True |  | ok |
| 156 | pre-reset-vram | vram-target | 4.6 ms | True | True |  | ok |
| 157 | pre-reset-launch-options | launch-options | 7118.5 ms | True | True | steam | ok |
| 158 | reset-stock | reset-stock | 14232.7 ms | True | True | steam | ok |
| 159 | status-after-reset | status | 3.9 ms | True | None |  | ok |
| 160 | reset-stock-again | reset-stock | 6604.4 ms | True | True | steam | ok |
| 161 | status-final | status | 3.3 ms | True | None |  | ok |


## Headless backend only (no GUI)

- startup: ready 781.5 ms (health 771.0 ms)
- idle: PSS 62.0 MiB, RSS 66.0 MiB, procs 2, threads 7, CPU 0.2%
- peak during workload: PSS 65.7 MiB, RSS 71.9 MiB
- final: PSS 63.9 MiB

### Backend-only workload

Parity pass: 161 steps, wall 78.46 s (78.46-78.46, n=1); of which waiting on the (fake) Steam client 77.66 s (77.66-77.66, n=1); **excluding Steam waits 0.530 s (0.530-0.530, n=1)**.
Latency pass wall: 132.0 s (132.0-132.0, n=1). Golden mismatches per run: [0] (quirk steps: [0]).

| op (label) | n | min | median | p95 | max | errors |
|---|---:|---:|---:|---:|---:|---:|
| all-intros-skip(off) | 30 | 3.73 | 5.32 | 6.03 | 6.39 | 0 |
| all-intros-skip(on) | 30 | 3.64 | 5.01 | 6.31 | 6.64 | 0 |
| browse | 40 | 1.43 | 1.86 | 2.61 | 3.52 | 0 |
| browse(common) | 40 | 1.16 | 1.75 | 2.31 | 2.60 | 0 |
| browse(game) | 40 | 1.27 | 1.89 | 2.66 | 3.01 | 0 |
| config(DrawFPS) | 30 | 3.21 | 3.95 | 4.86 | 5.78 | 0 |
| config(FOV) | 30 | 3.19 | 3.81 | 4.98 | 5.77 | 0 |
| config(FullScreenMode) | 30 | 3.09 | 3.72 | 4.86 | 4.95 | 0 |
| config(MaxFPS) | 30 | 3.14 | 3.77 | 4.99 | 5.38 | 0 |
| config(MaxFrameLatency) | 30 | 3.22 | 3.88 | 8.57 | 24.54 | 0 |
| config(RefreshRate) | 30 | 3.02 | 3.76 | 5.39 | 7.34 | 0 |
| config(ResolutionPercent) | 30 | 3.14 | 3.78 | 4.95 | 6.67 | 0 |
| config(RestrictGraphicsOptions) | 30 | 3.13 | 4.07 | 6.35 | 10.49 | 0 |
| config(SerializeRender) | 30 | 3.19 | 3.92 | 5.50 | 6.94 | 0 |
| config(SmoothFramerate) | 30 | 3.20 | 3.82 | 5.50 | 5.82 | 0 |
| config(Vsync) | 30 | 3.01 | 3.87 | 4.83 | 6.07 | 0 |
| config(WindowSize) | 30 | 3.18 | 3.83 | 5.17 | 6.33 | 0 |
| config-readonly(off) | 30 | 3.15 | 3.72 | 4.95 | 5.16 | 0 |
| config-readonly(on) | 30 | 3.07 | 3.66 | 5.02 | 9.90 | 0 |
| d3dcompiler(off) | 30 | 3.27 | 3.58 | 4.86 | 5.17 | 0 |
| d3dcompiler(on) | 30 | 3.02 | 3.56 | 5.38 | 5.93 | 0 |
| dxvk-config(default) | 30 | 3.37 | 4.07 | 5.17 | 5.55 | 0 |
| dxvk-config(tuned) | 30 | 3.40 | 3.95 | 5.15 | 5.81 | 0 |
| dxvk-uninstall | 20 | 3.06 | 3.79 | 8.15 | 13.82 | 0 |
| enhanced-uninstall | 2 | 7106.98 | 7106.98 | 7106.99 | 7106.99 | 0 |
| enhanced-validate(invalid) | 30 | 3.00 | 3.95 | 4.84 | 5.44 | 0 |
| enhanced-validate(valid) | 30 | 3.02 | 3.79 | 4.71 | 5.26 | 0 |
| exe-swap/compatible | 30 | 1.29 | 1.67 | 2.36 | 3.44 | 0 |
| exe-swap/compatible-depot | 30 | 2.63 | 3.12 | 6.62 | 8.93 | 0 |
| exe-swap/current | 30 | 1.35 | 1.74 | 2.28 | 2.51 | 0 |
| exe-swap/enhanced | 30 | 1.34 | 1.73 | 2.54 | 2.71 | 0 |
| game-directory | 40 | 2.99 | 3.37 | 4.05 | 6.37 | 0 |
| health | 100 | 0.22 | 0.29 | 0.53 | 1.53 | 0 |
| intro-skip(off) | 30 | 3.27 | 4.58 | 5.88 | 6.02 | 0 |
| intro-skip(on) | 30 | 3.07 | 4.63 | 6.33 | 6.83 | 0 |
| launch | 15 | 1.45 | 1.85 | 2.35 | 2.86 | 0 |
| launch-options(default) | 3 | 7097.22 | 7102.81 | 7104.41 | 7104.59 | 0 |
| launch-options(offline) | 3 | 7100.67 | 7103.03 | 7110.09 | 7110.87 | 0 |
| logs/clear | 30 | 3.02 | 3.55 | 5.01 | 5.17 | 0 |
| logs/payload | 30 | 0.98 | 1.27 | 1.61 | 2.36 | 0 |
| mod-files/clear | 30 | 3.28 | 4.16 | 5.17 | 6.36 | 0 |
| presets/apply(Low-Latency) | 30 | 4.34 | 5.12 | 7.38 | 7.92 | 0 |
| presets/apply(Smooth) | 30 | 4.32 | 5.31 | 10.54 | 13.83 | 0 |
| release-channel(beta) | 30 | 2.86 | 3.21 | 3.79 | 5.46 | 0 |
| release-channel(stable) | 30 | 2.70 | 3.18 | 3.79 | 5.36 | 0 |
| reset-stock | 2 | 14189.86 | 14198.01 | 14205.34 | 14206.15 | 0 |
| status | 60 | 2.14 | 2.64 | 3.10 | 3.84 | 0 |
| status(modded) | 40 | 1.80 | 2.65 | 3.99 | 6.72 | 0 |
| t7-config(friends) | 30 | 3.51 | 4.21 | 6.86 | 8.07 | 0 |
| t7-config(gamertag) | 30 | 3.57 | 4.13 | 5.46 | 5.87 | 0 |
| t7-config(password) | 30 | 3.49 | 4.24 | 5.37 | 7.64 | 0 |
| t7-uninstall | 20 | 4.09 | 4.74 | 5.82 | 6.72 | 0 |
| vram-target(full) | 30 | 3.42 | 3.90 | 4.70 | 5.67 | 0 |
| vram-target(limited) | 30 | 3.23 | 3.87 | 4.61 | 5.16 | 0 |
| workshop-install | 2 | 6602.42 | 6853.44 | 7079.35 | 7104.45 | 0 |

Per-op aggregate (ms):

| op | n | median | p95 |
|---|---:|---:|---:|
| all-intros-skip | 60 | 5.10 | 6.16 |
| browse | 120 | 1.83 | 2.60 |
| config | 360 | 3.86 | 5.38 |
| config-readonly | 60 | 3.70 | 5.03 |
| d3dcompiler | 60 | 3.57 | 4.91 |
| dxvk-config | 60 | 3.99 | 5.23 |
| dxvk-uninstall | 20 | 3.79 | 8.15 |
| enhanced-uninstall | 2 | 7106.98 | 7106.99 |
| enhanced-validate | 60 | 3.84 | 4.79 |
| exe-swap/compatible | 30 | 1.67 | 2.36 |
| exe-swap/compatible-depot | 30 | 3.12 | 6.62 |
| exe-swap/current | 30 | 1.74 | 2.28 |
| exe-swap/enhanced | 30 | 1.73 | 2.54 |
| game-directory | 40 | 3.37 | 4.05 |
| health | 100 | 0.29 | 0.53 |
| intro-skip | 60 | 4.60 | 6.03 |
| launch | 15 | 1.85 | 2.35 |
| launch-options | 6 | 7102.92 | 7109.30 |
| logs/clear | 30 | 3.55 | 5.01 |
| logs/payload | 30 | 1.27 | 1.61 |
| mod-files/clear | 30 | 4.16 | 5.17 |
| presets/apply | 60 | 5.27 | 7.70 |
| release-channel | 60 | 3.21 | 3.82 |
| reset-stock | 2 | 14198.01 | 14205.34 |
| status | 60 | 2.64 | 3.10 |
| status(modded) | 40 | 2.65 | 3.99 |
| t7-config | 90 | 4.19 | 6.39 |
| t7-uninstall | 20 | 4.74 | 5.82 |
| vram-target | 60 | 3.88 | 4.72 |
| workshop-install | 2 | 6853.44 | 7079.35 |

### Backend-only parity steps

| # | step | op | latency | envelope ok | app ok | tags | golden |
|---:|---|---|---:|---|---|---|---|
| 1 | health | health | 1.5 ms | True | True |  | ok |
| 2 | status-initial | status | 4.3 ms | True | None |  | ok |
| 3 | browse-default | browse | 2.9 ms | True | None |  | ok |
| 4 | browse-common-dir | browse | 2.0 ms | True | None |  | ok |
| 5 | browse-game-dir | browse | 1.9 ms | True | None |  | ok |
| 6 | browse-home | browse | 1.8 ms | True | None |  | ok |
| 7 | browse-missing-dir | browse | 1.6 ms | True | None |  | ok |
| 8 | game-directory-not-a-game | game-directory | 1.1 ms | True | False |  | ok |
| 9 | game-directory-missing-arg | game-directory | 1.1 ms | False | None |  | ok |
| 10 | game-directory-empty | game-directory | 0.7 ms | False | None |  | ok |
| 11 | game-directory-alt-install | game-directory | 3.2 ms | True | True |  | ok |
| 12 | status-alt-install | status | 2.2 ms | True | None |  | ok |
| 13 | config-without-config-ini | config | 2.0 ms | True | False |  | ok |
| 14 | config-readonly-without-config-ini | config-readonly | 1.5 ms | True | False |  | ok |
| 15 | browse-after-saved-dir | browse | 1.7 ms | True | None |  | ok |
| 16 | game-directory-main | game-directory | 3.2 ms | True | True |  | ok |
| 17 | status-main | status | 2.4 ms | True | None |  | ok |
| 18 | release-channel-beta | release-channel | 3.9 ms | True | True |  | ok |
| 19 | release-channel-invalid | release-channel | 1.2 ms | True | False |  | ok |
| 20 | release-channel-missing | release-channel | 1.0 ms | False | None |  | ok |
| 21 | release-channel-stable | release-channel | 4.7 ms | True | True |  | ok |
| 22 | config-MaxFPS | config | 4.2 ms | True | True |  | ok |
| 23 | config-FOV | config | 4.6 ms | True | True |  | ok |
| 24 | config-FullScreenMode | config | 3.7 ms | True | True |  | ok |
| 25 | config-WindowSize | config | 4.0 ms | True | True |  | ok |
| 26 | config-RefreshRate | config | 5.0 ms | True | True |  | ok |
| 27 | config-ResolutionPercent | config | 5.1 ms | True | True |  | ok |
| 28 | config-Vsync | config | 4.5 ms | True | True |  | ok |
| 29 | config-DrawFPS | config | 3.7 ms | True | True |  | ok |
| 30 | config-SmoothFramerate | config | 3.8 ms | True | True |  | ok |
| 31 | config-RestrictGraphicsOptions | config | 4.1 ms | True | True |  | ok |
| 32 | config-SerializeRender | config | 4.0 ms | True | True |  | ok |
| 33 | config-MaxFrameLatency | config | 3.7 ms | True | True |  | ok |
| 34 | config-append-new-key | config | 4.3 ms | True | True |  | ok |
| 35 | config-bool-value | config | 4.5 ms | True | True |  | ok |
| 36 | config-float-value | config | 5.1 ms | True | True |  | ok |
| 37 | config-default-comment | config | 4.4 ms | True | True |  | ok |
| 38 | config-backslash-n | config | 4.8 ms | True | True | quirk | ok |
| 39 | config-backslash-n-existing | config | 4.7 ms | True | True | quirk | ok |
| 40 | config-bad-escape | config | 1.9 ms | True | False | quirk | ok |
| 41 | config-empty-key | config | 1.3 ms | False | None |  | ok |
| 42 | config-missing-value | config | 0.8 ms | False | None |  | ok |
| 43 | config-null-value | config | 0.8 ms | False | None |  | ok |
| 44 | vram-target-limited-80 | vram-target | 6.4 ms | True | True |  | ok |
| 45 | vram-target-limited-100 | vram-target | 5.7 ms | True | True |  | ok |
| 46 | vram-target-limited-default | vram-target | 6.5 ms | True | True |  | ok |
| 47 | vram-target-full | vram-target | 5.7 ms | True | True |  | ok |
| 48 | vram-target-too-low | vram-target | 0.9 ms | False | None |  | ok |
| 49 | vram-target-too-high | vram-target | 1.1 ms | False | None |  | ok |
| 50 | vram-target-missing-limited | vram-target | 0.8 ms | False | None |  | ok |
| 51 | preset-smooth | presets/apply | 8.0 ms | True | True |  | ok |
| 52 | preset-low-latency | presets/apply | 5.6 ms | True | True |  | ok |
| 53 | preset-unknown | presets/apply | 2.1 ms | True | False |  | ok |
| 54 | preset-missing-name | presets/apply | 0.9 ms | False | None |  | ok |
| 55 | intro-skip-on | intro-skip | 4.0 ms | True | True |  | ok |
| 56 | intro-skip-on-again | intro-skip | 4.2 ms | True | True |  | ok |
| 57 | intro-skip-off | intro-skip | 5.5 ms | True | True |  | ok |
| 58 | intro-skip-off-again | intro-skip | 3.4 ms | True | True |  | ok |
| 59 | intro-skip-bad-arg | intro-skip | 1.0 ms | False | None |  | ok |
| 60 | intro-skip-on-then-all | intro-skip | 4.1 ms | True | True |  | ok |
| 61 | all-intros-on | all-intros-skip | 5.1 ms | True | True |  | ok |
| 62 | all-intros-on-again | all-intros-skip | 4.4 ms | True | True |  | ok |
| 63 | status-all-intros | status | 2.5 ms | True | None |  | ok |
| 64 | all-intros-off | all-intros-skip | 5.5 ms | True | True |  | ok |
| 65 | all-intros-off-again | all-intros-skip | 4.5 ms | True | True |  | ok |
| 66 | all-intros-no-video-dir | all-intros-skip | 1.6 ms | True | False |  | ok |
| 67 | intro-skip-no-video-dir | intro-skip | 3.4 ms | True | True |  | ok |
| 68 | restore-video-dir | status | 3.2 ms | True | None |  | ok |
| 69 | d3dcompiler-on | d3dcompiler | 4.2 ms | True | True |  | ok |
| 70 | d3dcompiler-on-again | d3dcompiler | 3.8 ms | True | True |  | ok |
| 71 | d3dcompiler-off | d3dcompiler | 3.5 ms | True | True |  | ok |
| 72 | d3dcompiler-off-again | d3dcompiler | 1.4 ms | True | False |  | ok |
| 73 | config-readonly-on | config-readonly | 3.8 ms | True | True |  | ok |
| 74 | status-readonly | status | 2.9 ms | True | None |  | ok |
| 75 | config-write-while-readonly | config | 2.2 ms | True | False | quirk | ok |
| 76 | preset-while-readonly | presets/apply | 4.6 ms | True | False | quirk | ok |
| 77 | vram-while-readonly | vram-target | 2.0 ms | True | False | quirk | ok |
| 78 | config-readonly-off | config-readonly | 3.4 ms | True | True |  | ok |
| 79 | config-after-readonly-off | config | 6.0 ms | True | True |  | ok |
| 80 | launch-options-offline | launch-options | 7103.9 ms | True | True | steam | ok |
| 81 | launch-options-unsupported | launch-options | 3.2 ms | True | False |  | ok |
| 82 | launch-options-workshop-all-around | launch-options | 7102.4 ms | True | True | steam | ok |
| 83 | launch-options-preserve-fs-game | launch-options | 7100.0 ms | True | True | steam | ok |
| 84 | launch-options-default | launch-options | 7105.6 ms | True | True | steam | ok |
| 85 | workshop-install-unknown | workshop-install | 1.2 ms | True | False |  | ok |
| 86 | workshop-install-empty-id | workshop-install | 0.9 ms | False | None |  | ok |
| 87 | workshop-install-ultimate | workshop-install | 7108.8 ms | True | True | steam | ok |
| 88 | status-launch-profiles | status | 3.0 ms | True | None |  | ok |
| 89 | launch | launch | 2.4 ms | True | True |  | ok |
| 90 | t7-config-without-conf | t7-config | 1.9 ms | True | False |  | ok |
| 91 | t7-password-without-conf | t7-config | 1.6 ms | True | False |  | ok |
| 92 | t7-friends-without-conf | t7-config | 1.6 ms | True | False |  | ok |
| 93 | status-fake-t7 | status | 3.5 ms | True | None |  | ok |
| 94 | t7-gamertag | t7-config | 4.3 ms | True | True |  | ok |
| 95 | t7-gamertag-trim | t7-config | 4.7 ms | True | True |  | ok |
| 96 | t7-gamertag-empty | t7-config | 1.7 ms | True | False |  | ok |
| 97 | t7-gamertag-too-long | t7-config | 1.3 ms | True | False |  | ok |
| 98 | t7-password-set | t7-config | 4.7 ms | True | True |  | ok |
| 99 | t7-password-trim | t7-config | 5.1 ms | True | True |  | ok |
| 100 | t7-password-clear | t7-config | 5.2 ms | True | True |  | ok |
| 101 | t7-friends-on | t7-config | 5.2 ms | True | True |  | ok |
| 102 | t7-friends-off | t7-config | 4.7 ms | True | True |  | ok |
| 103 | t7-all-fields | t7-config | 11.1 ms | True | True |  | ok |
| 104 | t7-no-fields | t7-config | 2.8 ms | True | True |  | ok |
| 105 | t7-colorcode-only | t7-config | 3.1 ms | True | True |  | ok |
| 106 | t7-uninstall | t7-uninstall | 5.2 ms | True | True |  | ok |
| 107 | t7-uninstall-again | t7-uninstall | 3.1 ms | True | True |  | ok |
| 108 | t7-config-after-uninstall | t7-config | 1.3 ms | True | False |  | ok |
| 109 | dxvk-config-without-install | dxvk-config | 4.2 ms | True | True |  | ok |
| 110 | dxvk-uninstall-conf-only | dxvk-uninstall | 3.5 ms | True | True |  | ok |
| 111 | status-fake-dxvk | status | 2.6 ms | True | None |  | ok |
| 112 | dxvk-config-recommended | dxvk-config | 8.1 ms | True | True |  | ok |
| 113 | dxvk-config-tuned | dxvk-config | 6.0 ms | True | True |  | ok |
| 114 | status-dxvk-tuned | status | 3.4 ms | True | None |  | ok |
| 115 | dxvk-config-bogus-tearfree | dxvk-config | 4.7 ms | True | True |  | ok |
| 116 | dxvk-config-threads-too-high | dxvk-config | 1.0 ms | False | None |  | ok |
| 117 | dxvk-config-fps-too-high | dxvk-config | 1.0 ms | False | None |  | ok |
| 118 | dxvk-config-latency-too-high | dxvk-config | 0.7 ms | False | None |  | ok |
| 119 | dxvk-config-negative | dxvk-config | 0.9 ms | False | None |  | ok |
| 120 | dxvk-uninstall | dxvk-uninstall | 4.8 ms | True | True |  | ok |
| 121 | dxvk-uninstall-again | dxvk-uninstall | 4.1 ms | True | True |  | ok |
| 122 | enhanced-validate-empty | enhanced-validate | 3.9 ms | True | True |  | ok |
| 123 | enhanced-validate-valid | enhanced-validate | 4.7 ms | True | True |  | ok |
| 124 | enhanced-validate-invalid | enhanced-validate | 4.7 ms | True | True |  | ok |
| 125 | enhanced-validate-missing-path | enhanced-validate | 4.5 ms | True | True |  | ok |
| 126 | enhanced-validate-missing-arg | enhanced-validate | 1.1 ms | False | None |  | ok |
| 127 | enhanced-install-bad-source | enhanced-install | 2.1 ms | True | False |  | ok |
| 128 | enhanced-install-invalid-source | enhanced-install | 1.9 ms | True | False |  | ok |
| 129 | exe-swap-compatible-unverified | exe-swap/compatible | 1.9 ms | True | False |  | ok |
| 130 | exe-swap-depot-absent | exe-swap/compatible-depot | 4.1 ms | True | True |  | ok |
| 131 | exe-swap-current-unverified | exe-swap/current | 5.9 ms | True | False |  | ok |
| 132 | exe-swap-enhanced-unverified | exe-swap/enhanced | 4.3 ms | True | False |  | ok |
| 133 | status-fake-enhanced | status | 2.8 ms | True | None |  | ok |
| 134 | exe-swap-compatible-enhanced-active | exe-swap/compatible | 4.1 ms | True | False |  | ok |
| 135 | exe-swap-current-enhanced-active | exe-swap/current | 2.1 ms | True | False |  | ok |
| 136 | exe-swap-enhanced-already-active | exe-swap/enhanced | 3.6 ms | True | True |  | ok |
| 137 | enhanced-uninstall | enhanced-uninstall | 6597.2 ms | True | True | steam | ok |
| 138 | enhanced-uninstall-again | enhanced-uninstall | 7106.8 ms | True | True | steam | ok |
| 139 | status-enhanced-removed | status | 5.9 ms | True | None |  | ok |
| 140 | exe-swap-depot-fake-present | exe-swap/compatible-depot | 3.3 ms | True | True |  | ok |
| 141 | exe-swap-compatible-depot-bad-hash | exe-swap/compatible | 2.3 ms | True | False |  | ok |
| 142 | exe-swap-compatible-known-hash | exe-swap/compatible | 7.3 ms | True | False |  | ok |
| 143 | exe-swap-current-known-hash | exe-swap/current | 2.0 ms | True | False |  | ok |
| 144 | exe-swap-enhanced-known-hash | exe-swap/enhanced | 6.5 ms | True | True |  | ok |
| 145 | status-known-enhanced-hash | status | 5.1 ms | True | None |  | ok |
| 146 | unseed-hash | status | 5.4 ms | True | None |  | ok |
| 147 | logs-payload | logs/payload | 3.4 ms | True | True |  | ok |
| 148 | logs-clear | logs/clear | 4.5 ms | True | True |  | ok |
| 149 | logs-payload-after-clear | logs/payload | 1.1 ms | True | True |  | ok |
| 150 | mod-files-clear | mod-files/clear | 3.8 ms | True | True |  | ok |
| 151 | mod-files-clear-empty | mod-files/clear | 2.7 ms | True | True |  | ok |
| 152 | status-everything-modded | status | 4.3 ms | True | None |  | ok |
| 153 | pre-reset-intros | all-intros-skip | 5.4 ms | True | True |  | ok |
| 154 | pre-reset-d3d | d3dcompiler | 4.5 ms | True | True |  | ok |
| 155 | pre-reset-preset | presets/apply | 5.8 ms | True | True |  | ok |
| 156 | pre-reset-vram | vram-target | 4.6 ms | True | True |  | ok |
| 157 | pre-reset-launch-options | launch-options | 7116.7 ms | True | True | steam | ok |
| 158 | reset-stock | reset-stock | 14212.0 ms | True | True | steam | ok |
| 159 | status-after-reset | status | 3.4 ms | True | None |  | ok |
| 160 | reset-stock-again | reset-stock | 7106.5 ms | True | True | steam | ok |
| 161 | status-final | status | 3.4 ms | True | None |  | ok |


## Not measured / caveats

- **Per-process RSS of build tools**: peak build RSS is the maximum over 100 ms samples of the summed RSS of the whole build process tree (shared pages are double counted); a sampled lower bound of the true instantaneous peak
- **GPU / VRAM / power**: headless Xvfb with software rendering (llvmpipe/--disable-gpu); not comparable to real hardware
- **Network-dependent ops (T7 / DXVK / Enhanced downloads, update-check)**: excluded from headline numbers; run with --phases ...,network
- **Window-map time resolution**: window is polled via xdotool every ~20 ms; the reported value is the first poll that saw a viewable window >=200x150, so it has ~20-40 ms granularity
- **Steam-dependent ops**: launch-options / workshop-install / enhanced-uninstall / reset-stock block on a fake `steam` client (5 s sleep + >=2 s stability poll in the legacy code); they are tagged and reported separately

