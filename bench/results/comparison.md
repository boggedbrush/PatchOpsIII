# Benchmark comparison: legacy (A) vs gpui (B)

- A: `legacy` commit `25034853a663`, run 2026-11-10T07:33:55+00:00, Intel(R) Xeon(R) Processor @ 2.10GHz x4, mode packaged, load1 at start max 0.69
- B: `gpui` commit `593ff9a773c1`, run 2026-11-10T09:32:11+00:00, Intel(R) Xeon(R) Processor @ 2.10GHz x4, mode gui+rpc, load1 at start max 0.66


### Build (clean)

| metric | A: legacy | B: gpui | B/A | note |
|---|---:|---:|---:|---|
| total wall | 74.6 s | 480.4 s | 6.44x |  |
| total CPU time | 63.7 s | 878.5 s | 13.79x |  |
| peak build RSS (sum of tree) | 332.5 MiB | 2421.8 MiB | 7.28x |  |
| network-bound (download) time | 12.3 s | 5.9 s | 0.48x | legacy: bun cold-warm + pip download; gpui: cargo fetch |
| compile/link wall, clean | 53.4 s | 327.1 s | 6.12x | legacy: install-from-cache + build + pyinstaller + packaging |
| incremental rebuild (touch 1 file) | n/a | 146.6 s | n/a | no legacy equivalent: `bun run build` is 8.268 s |
| distributable (unpacked app dir / binary) | 247.6 MiB | 13.4 MiB | 0.05x |  |
| backend binary | 25.6 MiB | n/a | n/a | legacy only (PyInstaller onefile) |
| dependency footprint | 749.5 MiB | 1.0 GiB | 1.40x | legacy: node_modules+venv; gpui: CARGO_HOME |

### Startup (cold, median of runs)

| metric | A: legacy | B: gpui | B/A | note |
|---|---:|---:|---:|---|
| first window mapped | 391 ms | 175 ms | 0.45x | n=5/5 |
| backend/RPC health | 1123 ms | 6 ms | 0.0052x | n=5/5 |
| app ready (status loads) | 1134 ms | 6 ms | 0.0051x | n=5/5 |
| renderer connects to backend | 1256 ms | n/a | n/a | legacy only |

### Memory (whole process tree)

| metric | A: legacy | B: gpui | B/A | note |
|---|---:|---:|---:|---|
| idle PSS | 376.0 MiB | 103.5 MiB | 0.28x |  |
| peak PSS (workload) | 393.1 MiB | 106.5 MiB | 0.27x |  |
| final PSS (after workload) | 392.1 MiB | 106.4 MiB | 0.27x |  |
| idle RSS (sum) | 682.5 MiB | 105.7 MiB | 0.15x |  |
| peak RSS (sum) | 706.2 MiB | 108.8 MiB | 0.15x |  |
| idle processes | 8 | 1 | 0.12x |  |
| idle threads | 73 | 21 | 0.29x |  |
| peak processes | 9 | 1 | 0.11x |  |
| idle CPU | 0.20 % | 0.00 % | 0.0000x | of one core, 10 s window |

### Workload totals

| metric | A: legacy | B: gpui | B/A | note |
|---|---:|---:|---:|---|
| parity pass wall (GUI instance) | 76.46 s | 5.80 s | 0.08x |  |
|   excluding Steam-wait ops | 0.47 s | 0.14 s | 0.30x | the Steam-wait ops are dominated by legacy sleeps |
| parity pass wall (headless) | 78.46 s | 5.74 s | 0.07x |  |
| headless idle PSS | 62.0 MiB | 7.3 MiB | 0.12x |  |
| headless peak PSS | 65.7 MiB | 9.0 MiB | 0.14x |  |
| headless ready | 782 ms | 4 ms | 0.0056x |  |

Golden parity mismatches per run (GUI instance / headless): A=[0, 0, 0] / [0], B=[0, 0, 0] / [0] (B must reproduce the legacy golden snapshots; quirk-tagged steps: A=[0, 0, 0], B=[0, 0, 0])

### Per-op latency, GUI instance (median / p95 ms)

| op | A median | A p95 | B median | B p95 | B/A median |
|---|---:|---:|---:|---:|---:|
| all-intros-skip | 4.37 ms | 6.14 ms | 1.04 ms | 1.32 ms | 0.24x |
| browse | 1.69 ms | 2.31 ms | 0.14 ms | 0.34 ms | 0.08x |
| config | 4.00 ms | 5.16 ms | 1.54 ms | 2.34 ms | 0.38x |
| config-readonly | 3.72 ms | 4.71 ms | 1.01 ms | 1.52 ms | 0.27x |
| d3dcompiler | 3.79 ms | 5.35 ms | 0.88 ms | 1.38 ms | 0.23x |
| dxvk-config | 4.23 ms | 5.35 ms | 1.31 ms | 1.87 ms | 0.31x |
| dxvk-uninstall | 4.93 ms | 7.60 ms | 0.78 ms | 1.11 ms | 0.16x |
| enhanced-uninstall | 7107.29 ms | 7123.39 ms | 525.67 ms | 529.61 ms | 0.07x |
| enhanced-validate | 3.86 ms | 5.31 ms | 1.76 ms | 2.29 ms | 0.46x |
| exe-swap/compatible | 1.64 ms | 2.88 ms | 0.09 ms | 0.39 ms | 0.05x |
| exe-swap/compatible-depot | 3.11 ms | 4.42 ms | 0.84 ms | 1.29 ms | 0.27x |
| exe-swap/current | 1.70 ms | 2.37 ms | 0.19 ms | 0.36 ms | 0.11x |
| exe-swap/enhanced | 1.63 ms | 2.19 ms | 0.12 ms | 0.27 ms | 0.07x |
| game-directory | 3.52 ms | 4.34 ms | 1.45 ms | 2.36 ms | 0.41x |
| health | 0.37 ms | 0.84 ms | 0.03 ms | 0.07 ms | 0.09x |
| intro-skip | 3.77 ms | 5.12 ms | 0.88 ms | 1.31 ms | 0.23x |
| launch | 1.99 ms | 3.17 ms | 2.17 ms | 2.73 ms | 1.09x |
| launch-options | 7106.46 ms | 7114.48 ms | 521.87 ms | 524.66 ms | 0.07x |
| logs/clear | 3.41 ms | 4.52 ms | 0.72 ms | 1.09 ms | 0.21x |
| logs/payload | 1.22 ms | 2.02 ms | 0.26 ms | 0.60 ms | 0.21x |
| mod-files/clear | 4.33 ms | 5.89 ms | 1.27 ms | 2.05 ms | 0.29x |
| presets/apply | 4.99 ms | 6.58 ms | 2.34 ms | 2.98 ms | 0.47x |
| release-channel | 3.41 ms | 4.39 ms | 1.46 ms | 2.19 ms | 0.43x |
| reset-stock | 13720.25 ms | 14095.35 ms | 1050.16 ms | 1054.17 ms | 0.08x |
| status | 2.57 ms | 3.06 ms | 0.58 ms | 0.98 ms | 0.23x |
| status(modded) | 2.61 ms | 4.28 ms | 0.52 ms | 0.90 ms | 0.20x |
| t7-config | 4.25 ms | 5.65 ms | 1.97 ms | 2.88 ms | 0.46x |
| t7-uninstall | 5.24 ms | 7.75 ms | 1.46 ms | 1.99 ms | 0.28x |
| vram-target | 4.26 ms | 5.27 ms | 1.48 ms | 2.21 ms | 0.35x |
| workshop-install | 7109.13 ms | 7112.48 ms | 524.21 ms | 525.41 ms | 0.07x |

### Per-op latency, headless (median / p95 ms)

| op | A median | A p95 | B median | B p95 | B/A median |
|---|---:|---:|---:|---:|---:|
| all-intros-skip | 5.10 ms | 6.16 ms | 0.88 ms | 1.43 ms | 0.17x |
| browse | 1.83 ms | 2.60 ms | 0.16 ms | 0.20 ms | 0.09x |
| config | 3.86 ms | 5.38 ms | 1.31 ms | 2.22 ms | 0.34x |
| config-readonly | 3.70 ms | 5.03 ms | 1.10 ms | 1.46 ms | 0.30x |
| d3dcompiler | 3.57 ms | 4.91 ms | 0.83 ms | 1.41 ms | 0.23x |
| dxvk-config | 3.99 ms | 5.23 ms | 1.36 ms | 1.91 ms | 0.34x |
| dxvk-uninstall | 3.79 ms | 8.15 ms | 0.89 ms | 1.65 ms | 0.23x |
| enhanced-uninstall | 7106.98 ms | 7106.99 ms | 523.66 ms | 525.42 ms | 0.07x |
| enhanced-validate | 3.84 ms | 4.79 ms | 1.46 ms | 1.85 ms | 0.38x |
| exe-swap/compatible | 1.67 ms | 2.36 ms | 0.07 ms | 0.16 ms | 0.04x |
| exe-swap/compatible-depot | 3.12 ms | 6.62 ms | 1.07 ms | 1.37 ms | 0.34x |
| exe-swap/current | 1.74 ms | 2.28 ms | 0.19 ms | 0.29 ms | 0.11x |
| exe-swap/enhanced | 1.73 ms | 2.54 ms | 0.10 ms | 0.17 ms | 0.06x |
| game-directory | 3.37 ms | 4.05 ms | 2.17 ms | 2.64 ms | 0.65x |
| health | 0.29 ms | 0.53 ms | 0.03 ms | 0.05 ms | 0.11x |
| intro-skip | 4.60 ms | 6.03 ms | 0.82 ms | 1.22 ms | 0.18x |
| launch | 1.85 ms | 2.35 ms | 2.10 ms | 2.49 ms | 1.14x |
| launch-options | 7102.92 ms | 7109.30 ms | 520.45 ms | 533.99 ms | 0.07x |
| logs/clear | 3.55 ms | 5.01 ms | 0.38 ms | 0.76 ms | 0.11x |
| logs/payload | 1.27 ms | 1.61 ms | 0.46 ms | 0.58 ms | 0.36x |
| mod-files/clear | 4.16 ms | 5.17 ms | 1.07 ms | 1.38 ms | 0.26x |
| presets/apply | 5.27 ms | 7.70 ms | 1.99 ms | 2.55 ms | 0.38x |
| release-channel | 3.21 ms | 3.82 ms | 1.60 ms | 1.90 ms | 0.50x |
| reset-stock | 14198.01 ms | 14205.34 ms | 1043.81 ms | 1046.37 ms | 0.07x |
| status | 2.64 ms | 3.10 ms | 0.45 ms | 1.02 ms | 0.17x |
| status(modded) | 2.65 ms | 3.99 ms | 0.55 ms | 0.72 ms | 0.21x |
| t7-config | 4.19 ms | 6.39 ms | 1.81 ms | 2.31 ms | 0.43x |
| t7-uninstall | 4.74 ms | 5.82 ms | 1.29 ms | 1.71 ms | 0.27x |
| vram-target | 3.88 ms | 4.72 ms | 1.29 ms | 1.71 ms | 0.33x |
| workshop-install | 6853.44 ms | 7079.35 ms | 523.53 ms | 524.38 ms | 0.08x |

