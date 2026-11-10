# PatchOpsIII benchmark: `gpui`

- Started: 2026-11-10T09:32:11+00:00  |  finished: 2026-11-10T09:45:04+00:00
- Source: `/home/user/PatchOpsIII` @ `593ff9a773c1` (claude/serene-brown-26fl09, dirty tracked files: 0)
- Machine: Intel(R) Xeon(R) Processor @ 2.10GHz x4, 16086 MB RAM, Linux-6.18.44-fc-v338-x86_64-with-glibc2.39
- Run mode: gui+rpc  |  run-as uid: 65534  |  harness 1.0
- Idle guard: threshold 0.7, 7 checks, max load1 at section start 0.66, timeouts: 0, enabled: True

## Build

| step | wall | CPU (user+sys) | peak RSS (sum of tree, sampled) | exit |
|---|---:|---:|---:|---:|
| cargo-fetch-cold | 5.9 s | 5.5 s | 280.7 MiB | 0 |
| cargo-build-release-clean | 327.1 s | 727.5 s | 2.4 GiB | 0 |
| cargo-build-release-incremental | 146.6 s | 145.1 s | 2.2 GiB | 0 |
| cargo-build-release-noop | 0.5 s | 0.5 s | 124.5 MiB | 0 |
| **total** | **480.4 s** | **878.5 s** | max 2.4 GiB | |

Network-bound vs local: network_bound_s=5.897, compile_clean_s=327.132, compile_incremental_s=146.55

Sizes: binary_bytes=13.4 MiB, dependency_footprint_bytes=1.0 GiB

| artefact | apparent size | disk usage | files |
|---|---:|---:|---:|
| cargo_home | 1.0 GiB | 1.1 GiB | 37730 |
| target_dir | 1.5 GiB | 1.5 GiB | 5132 |
| binary | 13.4 MiB | 13.4 MiB | 1 |

## GUI runs

### Startup (cold launch under Xvfb)

| milestone | median (min-max) |
|---|---|
| first top-level window mapped | 175 ms (147-189, n=5) |
| backend/RPC health answers | 6 ms (4-8, n=5) |
| app ready (status loaded) | 6 ms (4-8, n=5) |

runs: 5 (ok 5); failures: []

### Memory (whole process tree)

| metric | idle (10 s window after 10 s settle) | peak during workload | final after workload |
|---|---|---|---|
| PSS | 103.5 MiB (103.5 MiB-103.8 MiB) | 106.5 MiB (106.4 MiB-106.9 MiB) | 106.4 MiB (106.4 MiB-106.8 MiB) |
| RSS (sum) | 105.7 MiB (105.7 MiB-106.1 MiB) | 108.8 MiB (108.8 MiB-109.2 MiB) | 108.8 MiB (108.8 MiB-109.2 MiB) |
| processes | 1 (1-1) | 1 (1-1) | 1 (1-1) |
| threads | 21 (21-21) | 21 (21-21) | 21 (21-21) |

Idle CPU: 0.00 % (0.00-0.10, n=3) of one core. runs: 3

Idle process breakdown (first run):

| role | pid | PSS | RSS | threads |
|---|---:|---:|---:|---:|
| main | 29653 | 103.5 MiB | 105.7 MiB | 21 |

**Host-load check:** all measured windows were quiet (foreign CPU load <= 8.0% idle / <= 15.0% workload, in percent of all cores).

### Capability workload

Parity pass: 161 steps, wall 5.80 s (5.79-5.82, n=3); of which waiting on the (fake) Steam client 5.35 s (5.33-5.38, n=3); **excluding Steam waits 0.140 s (0.140-0.140, n=3)**.
Latency pass wall: 36.5 s (36.4-36.5, n=3). Golden mismatches per run: [0, 0, 0] (quirk steps: [0, 0, 0]).

| op (label) | n | min | median | p95 | max | errors |
|---|---:|---:|---:|---:|---:|---:|
| all-intros-skip(off) | 90 | 0.77 | 1.06 | 1.32 | 2.20 | 0 |
| all-intros-skip(on) | 90 | 0.72 | 1.04 | 1.31 | 1.59 | 0 |
| browse | 120 | 0.13 | 0.15 | 0.38 | 0.99 | 0 |
| browse(common) | 120 | 0.11 | 0.13 | 0.27 | 0.69 | 0 |
| browse(game) | 120 | 0.12 | 0.14 | 0.30 | 0.64 | 0 |
| config(DrawFPS) | 90 | 1.01 | 1.52 | 2.35 | 3.77 | 0 |
| config(FOV) | 90 | 1.07 | 1.49 | 2.35 | 2.52 | 0 |
| config(FullScreenMode) | 90 | 1.01 | 1.56 | 2.30 | 2.82 | 0 |
| config(MaxFPS) | 90 | 1.16 | 1.50 | 2.32 | 3.72 | 0 |
| config(MaxFrameLatency) | 90 | 1.05 | 1.52 | 2.30 | 3.04 | 0 |
| config(RefreshRate) | 90 | 1.07 | 1.57 | 2.17 | 3.15 | 0 |
| config(ResolutionPercent) | 90 | 1.07 | 1.54 | 2.37 | 2.86 | 0 |
| config(RestrictGraphicsOptions) | 90 | 1.09 | 1.62 | 2.54 | 4.94 | 0 |
| config(SerializeRender) | 90 | 1.07 | 1.54 | 2.25 | 3.16 | 0 |
| config(SmoothFramerate) | 90 | 1.08 | 1.52 | 2.38 | 2.84 | 0 |
| config(Vsync) | 90 | 1.07 | 1.56 | 2.42 | 3.31 | 0 |
| config(WindowSize) | 90 | 1.11 | 1.54 | 2.07 | 3.59 | 0 |
| config-readonly(off) | 90 | 0.66 | 1.02 | 1.61 | 5.24 | 0 |
| config-readonly(on) | 90 | 0.62 | 1.00 | 1.42 | 1.83 | 0 |
| d3dcompiler(off) | 90 | 0.68 | 0.87 | 1.22 | 2.11 | 0 |
| d3dcompiler(on) | 90 | 0.66 | 0.90 | 1.46 | 2.15 | 0 |
| dxvk-config(default) | 90 | 0.85 | 1.29 | 1.78 | 4.48 | 0 |
| dxvk-config(tuned) | 90 | 0.90 | 1.33 | 1.89 | 2.09 | 0 |
| dxvk-uninstall | 60 | 0.61 | 0.78 | 1.11 | 1.91 | 0 |
| enhanced-uninstall | 6 | 524.23 | 525.67 | 529.61 | 530.35 | 0 |
| enhanced-validate(invalid) | 90 | 1.06 | 1.73 | 2.20 | 2.80 | 0 |
| enhanced-validate(valid) | 90 | 1.03 | 1.81 | 2.83 | 4.26 | 0 |
| exe-swap/compatible | 90 | 0.06 | 0.09 | 0.39 | 2.59 | 0 |
| exe-swap/compatible-depot | 90 | 0.67 | 0.84 | 1.29 | 2.37 | 0 |
| exe-swap/current | 90 | 0.09 | 0.19 | 0.36 | 0.59 | 0 |
| exe-swap/enhanced | 90 | 0.07 | 0.12 | 0.27 | 0.58 | 0 |
| game-directory | 120 | 0.89 | 1.45 | 2.36 | 2.71 | 0 |
| health | 300 | 0.03 | 0.03 | 0.07 | 0.29 | 0 |
| intro-skip(off) | 90 | 0.69 | 0.87 | 1.29 | 1.91 | 0 |
| intro-skip(on) | 90 | 0.70 | 0.91 | 1.32 | 1.99 | 0 |
| launch | 45 | 1.70 | 2.17 | 2.73 | 3.84 | 0 |
| launch-options(default) | 9 | 520.50 | 521.89 | 525.21 | 525.70 | 0 |
| launch-options(offline) | 9 | 314.31 | 521.85 | 524.05 | 524.28 | 0 |
| logs/clear | 90 | 0.45 | 0.72 | 1.09 | 2.66 | 0 |
| logs/payload | 90 | 0.20 | 0.26 | 0.60 | 0.87 | 0 |
| mod-files/clear | 90 | 0.80 | 1.27 | 2.05 | 5.57 | 0 |
| presets/apply(Low-Latency) | 90 | 1.72 | 2.30 | 2.81 | 3.52 | 0 |
| presets/apply(Smooth) | 90 | 1.76 | 2.37 | 3.06 | 3.49 | 0 |
| release-channel(beta) | 90 | 1.04 | 1.46 | 2.25 | 3.66 | 0 |
| release-channel(stable) | 90 | 1.10 | 1.47 | 2.12 | 2.67 | 0 |
| reset-stock | 6 | 1046.34 | 1050.16 | 1054.17 | 1054.86 | 0 |
| status | 180 | 0.39 | 0.58 | 0.98 | 1.56 | 0 |
| status(modded) | 120 | 0.41 | 0.52 | 0.90 | 1.50 | 0 |
| t7-config(friends) | 90 | 1.26 | 1.99 | 2.87 | 3.02 | 0 |
| t7-config(gamertag) | 90 | 1.35 | 1.96 | 3.29 | 4.80 | 0 |
| t7-config(password) | 90 | 1.23 | 1.95 | 2.81 | 3.48 | 0 |
| t7-uninstall | 60 | 1.05 | 1.46 | 1.99 | 8.64 | 0 |
| vram-target(full) | 90 | 0.93 | 1.46 | 2.05 | 2.75 | 0 |
| vram-target(limited) | 90 | 1.11 | 1.51 | 2.35 | 3.22 | 0 |
| workshop-install | 6 | 523.68 | 524.21 | 525.41 | 525.49 | 0 |

Per-op aggregate (ms):

| op | n | median | p95 |
|---|---:|---:|---:|
| all-intros-skip | 180 | 1.04 | 1.32 |
| browse | 360 | 0.14 | 0.34 |
| config | 1080 | 1.54 | 2.34 |
| config-readonly | 180 | 1.01 | 1.52 |
| d3dcompiler | 180 | 0.88 | 1.38 |
| dxvk-config | 180 | 1.31 | 1.87 |
| dxvk-uninstall | 60 | 0.78 | 1.11 |
| enhanced-uninstall | 6 | 525.67 | 529.61 |
| enhanced-validate | 180 | 1.76 | 2.29 |
| exe-swap/compatible | 90 | 0.09 | 0.39 |
| exe-swap/compatible-depot | 90 | 0.84 | 1.29 |
| exe-swap/current | 90 | 0.19 | 0.36 |
| exe-swap/enhanced | 90 | 0.12 | 0.27 |
| game-directory | 120 | 1.45 | 2.36 |
| health | 300 | 0.03 | 0.07 |
| intro-skip | 180 | 0.88 | 1.31 |
| launch | 45 | 2.17 | 2.73 |
| launch-options | 18 | 521.87 | 524.66 |
| logs/clear | 90 | 0.72 | 1.09 |
| logs/payload | 90 | 0.26 | 0.60 |
| mod-files/clear | 90 | 1.27 | 2.05 |
| presets/apply | 180 | 2.34 | 2.98 |
| release-channel | 180 | 1.46 | 2.19 |
| reset-stock | 6 | 1050.16 | 1054.17 |
| status | 180 | 0.58 | 0.98 |
| status(modded) | 120 | 0.52 | 0.90 |
| t7-config | 270 | 1.97 | 2.88 |
| t7-uninstall | 60 | 1.46 | 1.99 |
| vram-target | 180 | 1.48 | 2.21 |
| workshop-install | 6 | 524.21 | 525.41 |

### Parity pass steps (first run; also the golden step list)

| # | step | op | latency | envelope ok | app ok | tags | golden |
|---:|---|---|---:|---|---|---|---|
| 1 | health | health | 0.2 ms | True | True |  | ok |
| 2 | status-initial | status | 0.7 ms | True | None |  | ok |
| 3 | browse-default | browse | 0.4 ms | True | None |  | ok |
| 4 | browse-common-dir | browse | 0.3 ms | True | None |  | ok |
| 5 | browse-game-dir | browse | 0.4 ms | True | None |  | ok |
| 6 | browse-home | browse | 0.4 ms | True | None |  | ok |
| 7 | browse-missing-dir | browse | 0.3 ms | True | None |  | ok |
| 8 | game-directory-not-a-game | game-directory | 0.4 ms | True | False |  | ok |
| 9 | game-directory-missing-arg | game-directory | 0.3 ms | False | None |  | ok |
| 10 | game-directory-empty | game-directory | 0.2 ms | False | None |  | ok |
| 11 | game-directory-alt-install | game-directory | 2.5 ms | True | True |  | ok |
| 12 | status-alt-install | status | 0.4 ms | True | None |  | ok |
| 13 | config-without-config-ini | config | 0.5 ms | True | False |  | ok |
| 14 | config-readonly-without-config-ini | config-readonly | 0.2 ms | True | False |  | ok |
| 15 | browse-after-saved-dir | browse | 0.4 ms | True | None |  | ok |
| 16 | game-directory-main | game-directory | 2.0 ms | True | True |  | ok |
| 17 | status-main | status | 0.5 ms | True | None |  | ok |
| 18 | release-channel-beta | release-channel | 1.8 ms | True | True |  | ok |
| 19 | release-channel-invalid | release-channel | 0.2 ms | True | False |  | ok |
| 20 | release-channel-missing | release-channel | 0.1 ms | False | None |  | ok |
| 21 | release-channel-stable | release-channel | 1.8 ms | True | True |  | ok |
| 22 | config-MaxFPS | config | 1.6 ms | True | True |  | ok |
| 23 | config-FOV | config | 1.7 ms | True | True |  | ok |
| 24 | config-FullScreenMode | config | 1.8 ms | True | True |  | ok |
| 25 | config-WindowSize | config | 1.6 ms | True | True |  | ok |
| 26 | config-RefreshRate | config | 1.5 ms | True | True |  | ok |
| 27 | config-ResolutionPercent | config | 1.4 ms | True | True |  | ok |
| 28 | config-Vsync | config | 1.5 ms | True | True |  | ok |
| 29 | config-DrawFPS | config | 1.6 ms | True | True |  | ok |
| 30 | config-SmoothFramerate | config | 1.4 ms | True | True |  | ok |
| 31 | config-RestrictGraphicsOptions | config | 1.4 ms | True | True |  | ok |
| 32 | config-SerializeRender | config | 1.5 ms | True | True |  | ok |
| 33 | config-MaxFrameLatency | config | 1.6 ms | True | True |  | ok |
| 34 | config-append-new-key | config | 1.8 ms | True | True |  | ok |
| 35 | config-bool-value | config | 1.5 ms | True | True |  | ok |
| 36 | config-float-value | config | 1.6 ms | True | True |  | ok |
| 37 | config-default-comment | config | 1.6 ms | True | True |  | ok |
| 38 | config-backslash-n | config | 1.6 ms | True | True | quirk | ok |
| 39 | config-backslash-n-existing | config | 1.5 ms | True | True | quirk | ok |
| 40 | config-bad-escape | config | 0.2 ms | True | False | quirk | ok |
| 41 | config-empty-key | config | 0.1 ms | False | None |  | ok |
| 42 | config-missing-value | config | 0.1 ms | False | None |  | ok |
| 43 | config-null-value | config | 0.1 ms | False | None |  | ok |
| 44 | vram-target-limited-80 | vram-target | 2.2 ms | True | True |  | ok |
| 45 | vram-target-limited-100 | vram-target | 1.4 ms | True | True |  | ok |
| 46 | vram-target-limited-default | vram-target | 1.5 ms | True | True |  | ok |
| 47 | vram-target-full | vram-target | 1.9 ms | True | True |  | ok |
| 48 | vram-target-too-low | vram-target | 0.1 ms | False | None |  | ok |
| 49 | vram-target-too-high | vram-target | 0.1 ms | False | None |  | ok |
| 50 | vram-target-missing-limited | vram-target | 0.2 ms | False | None |  | ok |
| 51 | preset-smooth | presets/apply | 2.6 ms | True | True |  | ok |
| 52 | preset-low-latency | presets/apply | 2.3 ms | True | True |  | ok |
| 53 | preset-unknown | presets/apply | 0.4 ms | True | False |  | ok |
| 54 | preset-missing-name | presets/apply | 0.2 ms | False | None |  | ok |
| 55 | intro-skip-on | intro-skip | 0.7 ms | True | True |  | ok |
| 56 | intro-skip-on-again | intro-skip | 1.3 ms | True | True |  | ok |
| 57 | intro-skip-off | intro-skip | 0.7 ms | True | True |  | ok |
| 58 | intro-skip-off-again | intro-skip | 0.6 ms | True | True |  | ok |
| 59 | intro-skip-bad-arg | intro-skip | 0.2 ms | False | None |  | ok |
| 60 | intro-skip-on-then-all | intro-skip | 0.7 ms | True | True |  | ok |
| 61 | all-intros-on | all-intros-skip | 0.7 ms | True | True |  | ok |
| 62 | all-intros-on-again | all-intros-skip | 0.7 ms | True | True |  | ok |
| 63 | status-all-intros | status | 0.5 ms | True | None |  | ok |
| 64 | all-intros-off | all-intros-skip | 0.6 ms | True | True |  | ok |
| 65 | all-intros-off-again | all-intros-skip | 0.8 ms | True | True |  | ok |
| 66 | all-intros-no-video-dir | all-intros-skip | 0.2 ms | True | False |  | ok |
| 67 | intro-skip-no-video-dir | intro-skip | 0.6 ms | True | True |  | ok |
| 68 | restore-video-dir | status | 0.6 ms | True | None |  | ok |
| 69 | d3dcompiler-on | d3dcompiler | 1.0 ms | True | True |  | ok |
| 70 | d3dcompiler-on-again | d3dcompiler | 0.6 ms | True | True |  | ok |
| 71 | d3dcompiler-off | d3dcompiler | 0.6 ms | True | True |  | ok |
| 72 | d3dcompiler-off-again | d3dcompiler | 0.2 ms | True | False |  | ok |
| 73 | config-readonly-on | config-readonly | 0.6 ms | True | True |  | ok |
| 74 | status-readonly | status | 0.5 ms | True | None |  | ok |
| 75 | config-write-while-readonly | config | 0.2 ms | True | False | quirk | ok |
| 76 | preset-while-readonly | presets/apply | 0.8 ms | True | False | quirk | ok |
| 77 | vram-while-readonly | vram-target | 0.3 ms | True | False | quirk | ok |
| 78 | config-readonly-off | config-readonly | 0.9 ms | True | True |  | ok |
| 79 | config-after-readonly-off | config | 2.3 ms | True | True |  | ok |
| 80 | launch-options-offline | launch-options | 317.6 ms | True | True | steam | ok |
| 81 | launch-options-unsupported | launch-options | 0.8 ms | True | False |  | ok |
| 82 | launch-options-workshop-all-around | launch-options | 520.4 ms | True | True | steam | ok |
| 83 | launch-options-preserve-fs-game | launch-options | 526.2 ms | True | True | steam | ok |
| 84 | launch-options-default | launch-options | 522.8 ms | True | True | steam | ok |
| 85 | workshop-install-unknown | workshop-install | 0.2 ms | True | False |  | ok |
| 86 | workshop-install-empty-id | workshop-install | 0.1 ms | False | None |  | ok |
| 87 | workshop-install-ultimate | workshop-install | 525.7 ms | True | True | steam | ok |
| 88 | status-launch-profiles | status | 1.0 ms | True | None |  | ok |
| 89 | launch | launch | 2.3 ms | True | True |  | ok |
| 90 | t7-config-without-conf | t7-config | 0.2 ms | True | False |  | ok |
| 91 | t7-password-without-conf | t7-config | 0.1 ms | True | False |  | ok |
| 92 | t7-friends-without-conf | t7-config | 0.1 ms | True | False |  | ok |
| 93 | status-fake-t7 | status | 1.5 ms | True | None |  | ok |
| 94 | t7-gamertag | t7-config | 2.1 ms | True | True |  | ok |
| 95 | t7-gamertag-trim | t7-config | 1.8 ms | True | True |  | ok |
| 96 | t7-gamertag-empty | t7-config | 0.2 ms | True | False |  | ok |
| 97 | t7-gamertag-too-long | t7-config | 0.2 ms | True | False |  | ok |
| 98 | t7-password-set | t7-config | 1.9 ms | True | True |  | ok |
| 99 | t7-password-trim | t7-config | 1.7 ms | True | True |  | ok |
| 100 | t7-password-clear | t7-config | 1.6 ms | True | True |  | ok |
| 101 | t7-friends-on | t7-config | 1.4 ms | True | True |  | ok |
| 102 | t7-friends-off | t7-config | 1.7 ms | True | True |  | ok |
| 103 | t7-all-fields | t7-config | 2.8 ms | True | True |  | ok |
| 104 | t7-no-fields | t7-config | 0.7 ms | True | True |  | ok |
| 105 | t7-colorcode-only | t7-config | 0.7 ms | True | True |  | ok |
| 106 | t7-uninstall | t7-uninstall | 1.5 ms | True | True |  | ok |
| 107 | t7-uninstall-again | t7-uninstall | 0.7 ms | True | True |  | ok |
| 108 | t7-config-after-uninstall | t7-config | 0.2 ms | True | False |  | ok |
| 109 | dxvk-config-without-install | dxvk-config | 0.9 ms | True | True |  | ok |
| 110 | dxvk-uninstall-conf-only | dxvk-uninstall | 0.8 ms | True | True |  | ok |
| 111 | status-fake-dxvk | status | 0.6 ms | True | None |  | ok |
| 112 | dxvk-config-recommended | dxvk-config | 0.8 ms | True | True |  | ok |
| 113 | dxvk-config-tuned | dxvk-config | 1.1 ms | True | True |  | ok |
| 114 | status-dxvk-tuned | status | 0.6 ms | True | None |  | ok |
| 115 | dxvk-config-bogus-tearfree | dxvk-config | 1.1 ms | True | True |  | ok |
| 116 | dxvk-config-threads-too-high | dxvk-config | 0.1 ms | False | None |  | ok |
| 117 | dxvk-config-fps-too-high | dxvk-config | 0.2 ms | False | None |  | ok |
| 118 | dxvk-config-latency-too-high | dxvk-config | 0.1 ms | False | None |  | ok |
| 119 | dxvk-config-negative | dxvk-config | 0.1 ms | False | None |  | ok |
| 120 | dxvk-uninstall | dxvk-uninstall | 1.1 ms | True | True |  | ok |
| 121 | dxvk-uninstall-again | dxvk-uninstall | 0.7 ms | True | True |  | ok |
| 122 | enhanced-validate-empty | enhanced-validate | 0.8 ms | True | True |  | ok |
| 123 | enhanced-validate-valid | enhanced-validate | 1.9 ms | True | True |  | ok |
| 124 | enhanced-validate-invalid | enhanced-validate | 1.9 ms | True | True |  | ok |
| 125 | enhanced-validate-missing-path | enhanced-validate | 1.5 ms | True | True |  | ok |
| 126 | enhanced-validate-missing-arg | enhanced-validate | 0.1 ms | False | None |  | ok |
| 127 | enhanced-install-bad-source | enhanced-install | 0.2 ms | True | False |  | ok |
| 128 | enhanced-install-invalid-source | enhanced-install | 0.2 ms | True | False |  | ok |
| 129 | exe-swap-compatible-unverified | exe-swap/compatible | 0.1 ms | True | False |  | ok |
| 130 | exe-swap-depot-absent | exe-swap/compatible-depot | 0.6 ms | True | True |  | ok |
| 131 | exe-swap-current-unverified | exe-swap/current | 0.2 ms | True | False |  | ok |
| 132 | exe-swap-enhanced-unverified | exe-swap/enhanced | 0.1 ms | True | False |  | ok |
| 133 | status-fake-enhanced | status | 0.6 ms | True | None |  | ok |
| 134 | exe-swap-compatible-enhanced-active | exe-swap/compatible | 0.6 ms | True | False |  | ok |
| 135 | exe-swap-current-enhanced-active | exe-swap/current | 0.2 ms | True | False |  | ok |
| 136 | exe-swap-enhanced-already-active | exe-swap/enhanced | 0.7 ms | True | True |  | ok |
| 137 | enhanced-uninstall | enhanced-uninstall | 393.2 ms | True | True | steam | ok |
| 138 | enhanced-uninstall-again | enhanced-uninstall | 526.8 ms | True | True | steam | ok |
| 139 | status-enhanced-removed | status | 0.8 ms | True | None |  | ok |
| 140 | exe-swap-depot-fake-present | exe-swap/compatible-depot | 0.7 ms | True | True |  | ok |
| 141 | exe-swap-compatible-depot-bad-hash | exe-swap/compatible | 0.3 ms | True | False |  | ok |
| 142 | exe-swap-compatible-known-hash | exe-swap/compatible | 1.2 ms | True | False |  | ok |
| 143 | exe-swap-current-known-hash | exe-swap/current | 0.2 ms | True | False |  | ok |
| 144 | exe-swap-enhanced-known-hash | exe-swap/enhanced | 0.8 ms | True | True |  | ok |
| 145 | status-known-enhanced-hash | status | 0.6 ms | True | None |  | ok |
| 146 | unseed-hash | status | 0.8 ms | True | None |  | ok |
| 147 | logs-payload | logs/payload | 0.2 ms | True | True |  | ok |
| 148 | logs-clear | logs/clear | 0.7 ms | True | True |  | ok |
| 149 | logs-payload-after-clear | logs/payload | 0.2 ms | True | True |  | ok |
| 150 | mod-files-clear | mod-files/clear | 1.5 ms | True | True |  | ok |
| 151 | mod-files-clear-empty | mod-files/clear | 0.8 ms | True | True |  | ok |
| 152 | status-everything-modded | status | 0.8 ms | True | None |  | ok |
| 153 | pre-reset-intros | all-intros-skip | 1.1 ms | True | True |  | ok |
| 154 | pre-reset-d3d | d3dcompiler | 1.0 ms | True | True |  | ok |
| 155 | pre-reset-preset | presets/apply | 3.6 ms | True | True |  | ok |
| 156 | pre-reset-vram | vram-target | 2.2 ms | True | True |  | ok |
| 157 | pre-reset-launch-options | launch-options | 472.9 ms | True | True | steam | ok |
| 158 | reset-stock | reset-stock | 1046.8 ms | True | True | steam | ok |
| 159 | status-after-reset | status | 0.5 ms | True | None |  | ok |
| 160 | reset-stock-again | reset-stock | 523.1 ms | True | True | steam | ok |
| 161 | status-final | status | 1.3 ms | True | None |  | ok |


## Headless backend only (no GUI)

- startup: ready 4.4 ms (health 4.4 ms)
- idle: PSS 7.3 MiB, RSS 8.8 MiB, procs 1, threads 1, CPU 0.0%
- peak during workload: PSS 9.0 MiB, RSS 10.8 MiB
- final: PSS 9.0 MiB

### Backend-only workload

Parity pass: 161 steps, wall 5.74 s (5.74-5.74, n=1); of which waiting on the (fake) Steam client 5.34 s (5.34-5.34, n=1); **excluding Steam waits 0.120 s (0.120-0.120, n=1)**.
Latency pass wall: 36.3 s (36.3-36.3, n=1). Golden mismatches per run: [0] (quirk steps: [0]).

| op (label) | n | min | median | p95 | max | errors |
|---|---:|---:|---:|---:|---:|---:|
| all-intros-skip(off) | 30 | 0.72 | 0.89 | 1.23 | 1.42 | 0 |
| all-intros-skip(on) | 30 | 0.71 | 0.87 | 2.13 | 5.12 | 0 |
| browse | 40 | 0.15 | 0.16 | 0.20 | 0.48 | 0 |
| browse(common) | 40 | 0.13 | 0.14 | 0.17 | 0.20 | 0 |
| browse(game) | 40 | 0.15 | 0.16 | 0.22 | 0.27 | 0 |
| config(DrawFPS) | 30 | 0.99 | 1.29 | 2.19 | 2.33 | 0 |
| config(FOV) | 30 | 1.02 | 1.35 | 2.44 | 7.33 | 0 |
| config(FullScreenMode) | 30 | 1.02 | 1.32 | 2.41 | 2.53 | 0 |
| config(MaxFPS) | 30 | 0.97 | 1.34 | 2.52 | 3.53 | 0 |
| config(MaxFrameLatency) | 30 | 0.99 | 1.30 | 2.11 | 2.58 | 0 |
| config(RefreshRate) | 30 | 0.96 | 1.33 | 2.12 | 2.33 | 0 |
| config(ResolutionPercent) | 30 | 0.94 | 1.27 | 2.24 | 2.28 | 0 |
| config(RestrictGraphicsOptions) | 30 | 0.96 | 1.33 | 2.15 | 2.19 | 0 |
| config(SerializeRender) | 30 | 1.01 | 1.21 | 2.10 | 2.22 | 0 |
| config(SmoothFramerate) | 30 | 1.02 | 1.27 | 2.14 | 2.27 | 0 |
| config(Vsync) | 30 | 1.00 | 1.30 | 2.15 | 2.28 | 0 |
| config(WindowSize) | 30 | 1.06 | 1.33 | 2.10 | 2.42 | 0 |
| config-readonly(off) | 30 | 0.62 | 1.09 | 1.44 | 1.98 | 0 |
| config-readonly(on) | 30 | 0.62 | 1.13 | 1.43 | 1.91 | 0 |
| d3dcompiler(off) | 30 | 0.68 | 0.81 | 1.28 | 1.42 | 0 |
| d3dcompiler(on) | 30 | 0.65 | 0.87 | 1.42 | 1.61 | 0 |
| dxvk-config(default) | 30 | 1.03 | 1.35 | 1.91 | 2.06 | 0 |
| dxvk-config(tuned) | 30 | 0.97 | 1.40 | 1.86 | 3.15 | 0 |
| dxvk-uninstall | 20 | 0.77 | 0.89 | 1.65 | 2.04 | 0 |
| enhanced-uninstall | 2 | 521.71 | 523.66 | 525.42 | 525.61 | 0 |
| enhanced-validate(invalid) | 30 | 1.13 | 1.48 | 1.73 | 1.83 | 0 |
| enhanced-validate(valid) | 30 | 1.11 | 1.45 | 2.31 | 2.50 | 0 |
| exe-swap/compatible | 30 | 0.06 | 0.07 | 0.16 | 0.56 | 0 |
| exe-swap/compatible-depot | 30 | 0.78 | 1.07 | 1.37 | 1.72 | 0 |
| exe-swap/current | 30 | 0.14 | 0.19 | 0.29 | 0.48 | 0 |
| exe-swap/enhanced | 30 | 0.06 | 0.10 | 0.17 | 0.24 | 0 |
| game-directory | 40 | 1.81 | 2.17 | 2.64 | 3.49 | 0 |
| health | 100 | 0.03 | 0.03 | 0.05 | 0.23 | 0 |
| intro-skip(off) | 30 | 0.64 | 0.82 | 1.30 | 3.73 | 0 |
| intro-skip(on) | 30 | 0.62 | 0.81 | 1.18 | 1.35 | 0 |
| launch | 15 | 1.76 | 2.10 | 2.49 | 2.73 | 0 |
| launch-options(default) | 3 | 518.90 | 522.46 | 536.29 | 537.83 | 0 |
| launch-options(offline) | 3 | 313.85 | 520.03 | 520.78 | 520.86 | 0 |
| logs/clear | 30 | 0.32 | 0.38 | 0.76 | 2.54 | 0 |
| logs/payload | 30 | 0.21 | 0.46 | 0.58 | 0.69 | 0 |
| mod-files/clear | 30 | 0.85 | 1.07 | 1.38 | 1.90 | 0 |
| presets/apply(Low-Latency) | 30 | 1.70 | 1.98 | 2.52 | 2.61 | 0 |
| presets/apply(Smooth) | 30 | 1.80 | 2.03 | 2.58 | 2.85 | 0 |
| release-channel(beta) | 30 | 1.12 | 1.59 | 2.24 | 2.55 | 0 |
| release-channel(stable) | 30 | 1.11 | 1.61 | 1.76 | 2.00 | 0 |
| reset-stock | 2 | 1040.98 | 1043.81 | 1046.37 | 1046.65 | 0 |
| status | 60 | 0.38 | 0.45 | 1.02 | 1.41 | 0 |
| status(modded) | 40 | 0.47 | 0.55 | 0.72 | 1.28 | 0 |
| t7-config(friends) | 30 | 1.34 | 1.80 | 2.29 | 2.43 | 0 |
| t7-config(gamertag) | 30 | 1.36 | 1.81 | 2.17 | 3.44 | 0 |
| t7-config(password) | 30 | 1.28 | 1.86 | 2.31 | 2.87 | 0 |
| t7-uninstall | 20 | 1.00 | 1.29 | 1.71 | 2.00 | 0 |
| vram-target(full) | 30 | 1.07 | 1.26 | 1.64 | 1.74 | 0 |
| vram-target(limited) | 30 | 1.14 | 1.32 | 1.72 | 2.39 | 0 |
| workshop-install | 2 | 522.59 | 523.53 | 524.38 | 524.48 | 0 |

Per-op aggregate (ms):

| op | n | median | p95 |
|---|---:|---:|---:|
| all-intros-skip | 60 | 0.88 | 1.43 |
| browse | 120 | 0.16 | 0.20 |
| config | 360 | 1.31 | 2.22 |
| config-readonly | 60 | 1.10 | 1.46 |
| d3dcompiler | 60 | 0.83 | 1.41 |
| dxvk-config | 60 | 1.36 | 1.91 |
| dxvk-uninstall | 20 | 0.89 | 1.65 |
| enhanced-uninstall | 2 | 523.66 | 525.42 |
| enhanced-validate | 60 | 1.46 | 1.85 |
| exe-swap/compatible | 30 | 0.07 | 0.16 |
| exe-swap/compatible-depot | 30 | 1.07 | 1.37 |
| exe-swap/current | 30 | 0.19 | 0.29 |
| exe-swap/enhanced | 30 | 0.10 | 0.17 |
| game-directory | 40 | 2.17 | 2.64 |
| health | 100 | 0.03 | 0.05 |
| intro-skip | 60 | 0.82 | 1.22 |
| launch | 15 | 2.10 | 2.49 |
| launch-options | 6 | 520.45 | 533.99 |
| logs/clear | 30 | 0.38 | 0.76 |
| logs/payload | 30 | 0.46 | 0.58 |
| mod-files/clear | 30 | 1.07 | 1.38 |
| presets/apply | 60 | 1.99 | 2.55 |
| release-channel | 60 | 1.60 | 1.90 |
| reset-stock | 2 | 1043.81 | 1046.37 |
| status | 60 | 0.45 | 1.02 |
| status(modded) | 40 | 0.55 | 0.72 |
| t7-config | 90 | 1.81 | 2.31 |
| t7-uninstall | 20 | 1.29 | 1.71 |
| vram-target | 60 | 1.29 | 1.71 |
| workshop-install | 2 | 523.53 | 524.38 |

### Backend-only parity steps

| # | step | op | latency | envelope ok | app ok | tags | golden |
|---:|---|---|---:|---|---|---|---|
| 1 | health | health | 0.3 ms | True | True |  | ok |
| 2 | status-initial | status | 0.5 ms | True | None |  | ok |
| 3 | browse-default | browse | 0.3 ms | True | None |  | ok |
| 4 | browse-common-dir | browse | 0.3 ms | True | None |  | ok |
| 5 | browse-game-dir | browse | 0.4 ms | True | None |  | ok |
| 6 | browse-home | browse | 0.3 ms | True | None |  | ok |
| 7 | browse-missing-dir | browse | 0.2 ms | True | None |  | ok |
| 8 | game-directory-not-a-game | game-directory | 0.3 ms | True | False |  | ok |
| 9 | game-directory-missing-arg | game-directory | 0.1 ms | False | None |  | ok |
| 10 | game-directory-empty | game-directory | 0.1 ms | False | None |  | ok |
| 11 | game-directory-alt-install | game-directory | 2.4 ms | True | True |  | ok |
| 12 | status-alt-install | status | 0.3 ms | True | None |  | ok |
| 13 | config-without-config-ini | config | 0.1 ms | True | False |  | ok |
| 14 | config-readonly-without-config-ini | config-readonly | 0.1 ms | True | False |  | ok |
| 15 | browse-after-saved-dir | browse | 0.4 ms | True | None |  | ok |
| 16 | game-directory-main | game-directory | 1.6 ms | True | True |  | ok |
| 17 | status-main | status | 0.4 ms | True | None |  | ok |
| 18 | release-channel-beta | release-channel | 2.1 ms | True | True |  | ok |
| 19 | release-channel-invalid | release-channel | 0.1 ms | True | False |  | ok |
| 20 | release-channel-missing | release-channel | 0.1 ms | False | None |  | ok |
| 21 | release-channel-stable | release-channel | 1.4 ms | True | True |  | ok |
| 22 | config-MaxFPS | config | 1.0 ms | True | True |  | ok |
| 23 | config-FOV | config | 1.4 ms | True | True |  | ok |
| 24 | config-FullScreenMode | config | 1.3 ms | True | True |  | ok |
| 25 | config-WindowSize | config | 1.2 ms | True | True |  | ok |
| 26 | config-RefreshRate | config | 1.1 ms | True | True |  | ok |
| 27 | config-ResolutionPercent | config | 1.1 ms | True | True |  | ok |
| 28 | config-Vsync | config | 1.1 ms | True | True |  | ok |
| 29 | config-DrawFPS | config | 1.0 ms | True | True |  | ok |
| 30 | config-SmoothFramerate | config | 1.2 ms | True | True |  | ok |
| 31 | config-RestrictGraphicsOptions | config | 1.9 ms | True | True |  | ok |
| 32 | config-SerializeRender | config | 1.5 ms | True | True |  | ok |
| 33 | config-MaxFrameLatency | config | 0.8 ms | True | True |  | ok |
| 34 | config-append-new-key | config | 1.3 ms | True | True |  | ok |
| 35 | config-bool-value | config | 1.1 ms | True | True |  | ok |
| 36 | config-float-value | config | 1.6 ms | True | True |  | ok |
| 37 | config-default-comment | config | 1.6 ms | True | True |  | ok |
| 38 | config-backslash-n | config | 1.7 ms | True | True | quirk | ok |
| 39 | config-backslash-n-existing | config | 1.6 ms | True | True | quirk | ok |
| 40 | config-bad-escape | config | 0.2 ms | True | False | quirk | ok |
| 41 | config-empty-key | config | 0.0 ms | False | None |  | ok |
| 42 | config-missing-value | config | 0.1 ms | False | None |  | ok |
| 43 | config-null-value | config | 0.1 ms | False | None |  | ok |
| 44 | vram-target-limited-80 | vram-target | 1.9 ms | True | True |  | ok |
| 45 | vram-target-limited-100 | vram-target | 1.2 ms | True | True |  | ok |
| 46 | vram-target-limited-default | vram-target | 1.4 ms | True | True |  | ok |
| 47 | vram-target-full | vram-target | 1.5 ms | True | True |  | ok |
| 48 | vram-target-too-low | vram-target | 0.1 ms | False | None |  | ok |
| 49 | vram-target-too-high | vram-target | 0.1 ms | False | None |  | ok |
| 50 | vram-target-missing-limited | vram-target | 0.1 ms | False | None |  | ok |
| 51 | preset-smooth | presets/apply | 2.0 ms | True | True |  | ok |
| 52 | preset-low-latency | presets/apply | 2.0 ms | True | True |  | ok |
| 53 | preset-unknown | presets/apply | 0.2 ms | True | False |  | ok |
| 54 | preset-missing-name | presets/apply | 0.2 ms | False | None |  | ok |
| 55 | intro-skip-on | intro-skip | 0.6 ms | True | True |  | ok |
| 56 | intro-skip-on-again | intro-skip | 0.7 ms | True | True |  | ok |
| 57 | intro-skip-off | intro-skip | 0.6 ms | True | True |  | ok |
| 58 | intro-skip-off-again | intro-skip | 0.6 ms | True | True |  | ok |
| 59 | intro-skip-bad-arg | intro-skip | 0.1 ms | False | None |  | ok |
| 60 | intro-skip-on-then-all | intro-skip | 0.5 ms | True | True |  | ok |
| 61 | all-intros-on | all-intros-skip | 0.7 ms | True | True |  | ok |
| 62 | all-intros-on-again | all-intros-skip | 0.6 ms | True | True |  | ok |
| 63 | status-all-intros | status | 0.6 ms | True | None |  | ok |
| 64 | all-intros-off | all-intros-skip | 0.6 ms | True | True |  | ok |
| 65 | all-intros-off-again | all-intros-skip | 0.5 ms | True | True |  | ok |
| 66 | all-intros-no-video-dir | all-intros-skip | 0.1 ms | True | False |  | ok |
| 67 | intro-skip-no-video-dir | intro-skip | 0.7 ms | True | True |  | ok |
| 68 | restore-video-dir | status | 0.5 ms | True | None |  | ok |
| 69 | d3dcompiler-on | d3dcompiler | 0.5 ms | True | True |  | ok |
| 70 | d3dcompiler-on-again | d3dcompiler | 0.7 ms | True | True |  | ok |
| 71 | d3dcompiler-off | d3dcompiler | 0.6 ms | True | True |  | ok |
| 72 | d3dcompiler-off-again | d3dcompiler | 0.2 ms | True | False |  | ok |
| 73 | config-readonly-on | config-readonly | 0.7 ms | True | True |  | ok |
| 74 | status-readonly | status | 0.5 ms | True | None |  | ok |
| 75 | config-write-while-readonly | config | 0.3 ms | True | False | quirk | ok |
| 76 | preset-while-readonly | presets/apply | 0.8 ms | True | False | quirk | ok |
| 77 | vram-while-readonly | vram-target | 0.2 ms | True | False | quirk | ok |
| 78 | config-readonly-off | config-readonly | 1.1 ms | True | True |  | ok |
| 79 | config-after-readonly-off | config | 2.6 ms | True | True |  | ok |
| 80 | launch-options-offline | launch-options | 313.3 ms | True | True | steam | ok |
| 81 | launch-options-unsupported | launch-options | 0.8 ms | True | False |  | ok |
| 82 | launch-options-workshop-all-around | launch-options | 521.4 ms | True | True | steam | ok |
| 83 | launch-options-preserve-fs-game | launch-options | 519.5 ms | True | True | steam | ok |
| 84 | launch-options-default | launch-options | 520.8 ms | True | True | steam | ok |
| 85 | workshop-install-unknown | workshop-install | 0.3 ms | True | False |  | ok |
| 86 | workshop-install-empty-id | workshop-install | 0.3 ms | False | None |  | ok |
| 87 | workshop-install-ultimate | workshop-install | 524.8 ms | True | True | steam | ok |
| 88 | status-launch-profiles | status | 0.6 ms | True | None |  | ok |
| 89 | launch | launch | 2.0 ms | True | True |  | ok |
| 90 | t7-config-without-conf | t7-config | 0.1 ms | True | False |  | ok |
| 91 | t7-password-without-conf | t7-config | 0.1 ms | True | False |  | ok |
| 92 | t7-friends-without-conf | t7-config | 0.1 ms | True | False |  | ok |
| 93 | status-fake-t7 | status | 1.2 ms | True | None |  | ok |
| 94 | t7-gamertag | t7-config | 1.9 ms | True | True |  | ok |
| 95 | t7-gamertag-trim | t7-config | 1.6 ms | True | True |  | ok |
| 96 | t7-gamertag-empty | t7-config | 0.1 ms | True | False |  | ok |
| 97 | t7-gamertag-too-long | t7-config | 0.1 ms | True | False |  | ok |
| 98 | t7-password-set | t7-config | 1.9 ms | True | True |  | ok |
| 99 | t7-password-trim | t7-config | 2.0 ms | True | True |  | ok |
| 100 | t7-password-clear | t7-config | 2.5 ms | True | True |  | ok |
| 101 | t7-friends-on | t7-config | 2.7 ms | True | True |  | ok |
| 102 | t7-friends-off | t7-config | 1.9 ms | True | True |  | ok |
| 103 | t7-all-fields | t7-config | 2.4 ms | True | True |  | ok |
| 104 | t7-no-fields | t7-config | 0.8 ms | True | True |  | ok |
| 105 | t7-colorcode-only | t7-config | 0.7 ms | True | True |  | ok |
| 106 | t7-uninstall | t7-uninstall | 1.3 ms | True | True |  | ok |
| 107 | t7-uninstall-again | t7-uninstall | 0.7 ms | True | True |  | ok |
| 108 | t7-config-after-uninstall | t7-config | 0.1 ms | True | False |  | ok |
| 109 | dxvk-config-without-install | dxvk-config | 0.7 ms | True | True |  | ok |
| 110 | dxvk-uninstall-conf-only | dxvk-uninstall | 0.9 ms | True | True |  | ok |
| 111 | status-fake-dxvk | status | 0.7 ms | True | None |  | ok |
| 112 | dxvk-config-recommended | dxvk-config | 0.8 ms | True | True |  | ok |
| 113 | dxvk-config-tuned | dxvk-config | 1.1 ms | True | True |  | ok |
| 114 | status-dxvk-tuned | status | 0.6 ms | True | None |  | ok |
| 115 | dxvk-config-bogus-tearfree | dxvk-config | 1.0 ms | True | True |  | ok |
| 116 | dxvk-config-threads-too-high | dxvk-config | 0.2 ms | False | None |  | ok |
| 117 | dxvk-config-fps-too-high | dxvk-config | 0.1 ms | False | None |  | ok |
| 118 | dxvk-config-latency-too-high | dxvk-config | 0.2 ms | False | None |  | ok |
| 119 | dxvk-config-negative | dxvk-config | 0.1 ms | False | None |  | ok |
| 120 | dxvk-uninstall | dxvk-uninstall | 1.3 ms | True | True |  | ok |
| 121 | dxvk-uninstall-again | dxvk-uninstall | 0.7 ms | True | True |  | ok |
| 122 | enhanced-validate-empty | enhanced-validate | 0.6 ms | True | True |  | ok |
| 123 | enhanced-validate-valid | enhanced-validate | 2.0 ms | True | True |  | ok |
| 124 | enhanced-validate-invalid | enhanced-validate | 2.0 ms | True | True |  | ok |
| 125 | enhanced-validate-missing-path | enhanced-validate | 1.5 ms | True | True |  | ok |
| 126 | enhanced-validate-missing-arg | enhanced-validate | 0.1 ms | False | None |  | ok |
| 127 | enhanced-install-bad-source | enhanced-install | 0.2 ms | True | False |  | ok |
| 128 | enhanced-install-invalid-source | enhanced-install | 0.1 ms | True | False |  | ok |
| 129 | exe-swap-compatible-unverified | exe-swap/compatible | 0.1 ms | True | False |  | ok |
| 130 | exe-swap-depot-absent | exe-swap/compatible-depot | 0.8 ms | True | True |  | ok |
| 131 | exe-swap-current-unverified | exe-swap/current | 0.1 ms | True | False |  | ok |
| 132 | exe-swap-enhanced-unverified | exe-swap/enhanced | 0.1 ms | True | False |  | ok |
| 133 | status-fake-enhanced | status | 0.7 ms | True | None |  | ok |
| 134 | exe-swap-compatible-enhanced-active | exe-swap/compatible | 0.7 ms | True | False |  | ok |
| 135 | exe-swap-current-enhanced-active | exe-swap/current | 0.2 ms | True | False |  | ok |
| 136 | exe-swap-enhanced-already-active | exe-swap/enhanced | 0.8 ms | True | True |  | ok |
| 137 | enhanced-uninstall | enhanced-uninstall | 394.4 ms | True | True | steam | ok |
| 138 | enhanced-uninstall-again | enhanced-uninstall | 528.0 ms | True | True | steam | ok |
| 139 | status-enhanced-removed | status | 1.0 ms | True | None |  | ok |
| 140 | exe-swap-depot-fake-present | exe-swap/compatible-depot | 0.9 ms | True | True |  | ok |
| 141 | exe-swap-compatible-depot-bad-hash | exe-swap/compatible | 0.7 ms | True | False |  | ok |
| 142 | exe-swap-compatible-known-hash | exe-swap/compatible | 1.1 ms | True | False |  | ok |
| 143 | exe-swap-current-known-hash | exe-swap/current | 0.3 ms | True | False |  | ok |
| 144 | exe-swap-enhanced-known-hash | exe-swap/enhanced | 0.7 ms | True | True |  | ok |
| 145 | status-known-enhanced-hash | status | 0.9 ms | True | None |  | ok |
| 146 | unseed-hash | status | 0.8 ms | True | None |  | ok |
| 147 | logs-payload | logs/payload | 0.2 ms | True | True |  | ok |
| 148 | logs-clear | logs/clear | 0.6 ms | True | True |  | ok |
| 149 | logs-payload-after-clear | logs/payload | 0.1 ms | True | True |  | ok |
| 150 | mod-files-clear | mod-files/clear | 1.7 ms | True | True |  | ok |
| 151 | mod-files-clear-empty | mod-files/clear | 0.7 ms | True | True |  | ok |
| 152 | status-everything-modded | status | 0.9 ms | True | None |  | ok |
| 153 | pre-reset-intros | all-intros-skip | 0.7 ms | True | True |  | ok |
| 154 | pre-reset-d3d | d3dcompiler | 0.6 ms | True | True |  | ok |
| 155 | pre-reset-preset | presets/apply | 2.5 ms | True | True |  | ok |
| 156 | pre-reset-vram | vram-target | 1.4 ms | True | True |  | ok |
| 157 | pre-reset-launch-options | launch-options | 445.0 ms | True | True | steam | ok |
| 158 | reset-stock | reset-stock | 1047.3 ms | True | True | steam | ok |
| 159 | status-after-reset | status | 1.1 ms | True | None |  | ok |
| 160 | reset-stock-again | reset-stock | 523.6 ms | True | True | steam | ok |
| 161 | status-final | status | 0.9 ms | True | None |  | ok |


## Not measured / caveats

- **Per-process RSS of build tools**: peak build RSS is the maximum over 100 ms samples of the summed RSS of the whole build process tree (shared pages are double counted); a sampled lower bound of the true instantaneous peak
- **GPU / VRAM / power**: headless Xvfb with software rendering (llvmpipe/--disable-gpu); not comparable to real hardware
- **Network-dependent ops (T7 / DXVK / Enhanced downloads, update-check)**: excluded from headline numbers; run with --phases ...,network
- **Window-map time resolution**: window is polled via xdotool every ~20 ms; the reported value is the first poll that saw a viewable window >=200x150, so it has ~20-40 ms granularity
- **Steam-dependent ops**: launch-options / workshop-install / enhanced-uninstall / reset-stock block on a fake `steam` client (5 s sleep + >=2 s stability poll in the legacy code); they are tagged and reported separately

