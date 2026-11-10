# PatchOps benchmark harness

Stdlib-only Python harness (`/proc` is read directly, no psutil) that measures the **legacy** app (Electron + React + Python/FastAPI) today and
the **gpui** port (Rust + GPUI) later, with the same code, the same fixture and the same workload, so the two result files can be compared
side by side.

```
bench/run.py            orchestrator (build, startup, memory, workload, headless, network)
bench/compare.py        side-by-side markdown table from two result JSONs
bench/OPS.md            the op catalogue / driver contract the Rust port must implement (+ sandbox spec, quirks)
bench/golden/           legacy behaviour snapshots: one file per workload step (response + filesystem diff)
bench/results/          legacy.json / legacy.md (committed baseline), gpui.* later
bench/benchlib/         harness modules (procfs, sandbox fixture, workload, drivers, report)
bench/tools/            legacy_rpc_shim.py + mock-patchops (a test double that speaks the gpui contract on top of the legacy backend)
bench/.work/            scratch: copies of the sources under test, build outputs, caches, sandboxes, raw logs (git-ignored)
```

## Requirements

Linux, Python 3.9+, `Xvfb`, `xdotool` (`apt install xvfb xdotool x11-utils`), `pgrep`/`pkill` (procps).
Legacy build additionally needs `bun`, `node`, `python3 -m venv`, `objdump` (binutils) and network access to npm, PyPI and GitHub releases
(Electron binary). GPUI build needs `cargo`/`rustc` plus whatever system libraries the port needs and network access to crates.io.
When started as root the target processes are dropped to uid 65534 (see "Run-as" below).

## Running

```bash
# full official baseline of the legacy app (build + 5 startup runs + 3 memory/workload runs + headless backend run)
python3 bench/run.py --target legacy

# the legacy code may live elsewhere, e.g. a worktree of the baseline commit once it is deleted from the port branch:
git worktree add /tmp/patchops-baseline <baseline-commit>
python3 bench/run.py --target legacy --legacy-src /tmp/patchops-baseline

# the Rust port (the binary does not exist yet -> the run is recorded as "target not built")
python3 bench/run.py --target gpui                          # builds <gpui-src> in a scratch copy, then runs target/release/patchops
python3 bench/run.py --target gpui --gpui-bin /path/to/patchops --phases gui,headless

python3 bench/compare.py bench/results/legacy.json bench/results/gpui.json -o bench/results/compare.md
```

Useful flags (`--help` for all):

| flag | meaning |
|---|---|
| `--phases build,gui,headless,network` | default `build,gui,headless`. `--skip-build` reuses `.work/<target>`; a previous `build` section is carried over into the new result file |
| `--startup-runs 5 --mem-runs 3` | cold GUI launches (>=5); how many of them also settle 10 s, take the idle window, run the whole workload with memory sampling and a final sample |
| `--iterations 1.0` | scale factor for latency-pass iteration counts |
| `--no-idle-guard`, `--idle-threshold 0.7`, `--idle-timeout 900` | before every timed section the harness waits for the 1-min loadavg to drop below the threshold (it gives up after the timeout and records that) |
| `--update-golden` | (legacy only) rewrite `bench/golden/` from this run; every other run compares against it |
| `--legacy-mode auto\|packaged\|dev` | `packaged` = electron-builder `linux-unpacked` + PyInstaller backend (what users run, default if built); `dev` = `electron .` + venv python |
| `--quick` | smoke test (1 run, 0.1x iterations, short settle) - never use for numbers |
| `--run-as root` | do not drop privileges (breaks `config-readonly` parity, see OPS.md) |

## What is measured

1. **Build** (from a clean state; wall, CPU user+sys from `wait4`, peak summed RSS of the whole build process tree sampled at 10 Hz).
   *legacy*: `bun install` with a cold cache (download+link) and again with a warm cache (link only), `pip download` (network) then
   `pip install --no-index` from the wheelhouse (local), `bun run build`, `build:backend:linux` (PyInstaller), `electron-builder --linux dir`.
   *gpui*: `cargo fetch` with an empty `CARGO_HOME` (network), `cargo build --release --offline` from an empty target dir, a rebuild after touching the
   binary's `main.rs`, a no-op rebuild. Sizes: unpacked app dir / binary, backend binary, `node_modules`, venv, `CARGO_HOME`.
2. **Startup** (cold, under Xvfb): ms from just before `exec` to (a) first viewable top-level window >= 200x150 (`xdotool` polling every ~20 ms),
   (b) backend/RPC health, (c) app ready = `status` answers, (legacy) renderer's first connection to the backend. Median/min/max over all runs.
3. **Memory**: sum of PSS (`smaps_rollup`) and RSS over **every descendant** of the launched process, process and thread counts. Idle = 1 Hz samples over 10 s starting 10 s
   after ready (+ CPU% from utime+stime deltas); peak = max over 0.5 s samples while the whole workload runs against the live GUI process; final = 3 s after the workload.
   Xvfb itself is not part of the tree. Host busy % over the same windows is recorded so interference from other processes is visible.
4. **Workload** (the app's capabilities + behavioural parity suite): see `OPS.md`. A *parity pass* runs ~160 deterministic steps once, snapshotting the fixture
   after each; a *latency pass* repeats groups of ops many times (median/p95/min/max of per-call client-side latency, all raw samples stored). It runs
   in the GUI process (`PATCHOPS_BENCH_RPC=1` / Electron + HTTP) and once against the bare backend (`--rpc-stdio` / python backend only).
5. **Network group** (`--phases network`): `update-check`, DXVK/T7/Enhanced installs against the real internet. Never part of headline numbers.

Failures are recorded, never dropped: failed build steps keep their log tail, an op that errors is counted in `errors`/`app_not_ok`, a mismatching
parity step is listed with a short diff, and a run that crashed carries its traceback. Anything the harness could not measure appears under "Not measured / caveats".

## Fixture and golden files

`benchlib/sandbox.py` builds, for each run, a fake `HOME`, Steam installation (library list, `localconfig.vdf`, workshop items), a Black Ops III install
(exe, `players/config.ini` with CRLF/invalid bytes/duplicates, intro videos, `d3dcompiler_46.dll`, `LPC`), fake `steam` and `xdg-open` binaries on `PATH`
and dump sources for Enhanced validation. The environment variables handed to the target are listed in `OPS.md`.

`bench/golden/NNN-<step>.json` stores, for every parity step, the normalised op response and the filesystem delta of the sandbox
(path -> sha256 + mode). Volatile data (timestamps, sandbox path, platform string, set ordering) is normalised before hashing.
The legacy run determinism is checked by every subsequent run (3 GUI runs + the headless run compare against the golden recorded by the first one).
To regenerate after an intentional fixture/workload change: `python3 bench/run.py --target legacy --update-golden`, then
`python3 bench/tools/gen_ops_examples.py` to refresh the examples in `OPS.md`.

## Implementing the gpui target

Follow `OPS.md`: `patchops --rpc-stdio` and `PATCHOPS_BENCH_RPC=1 patchops` must speak the JSON-lines protocol. To check a port quickly without the harness
overhead, `bench/tools/mock-patchops` shows a reference implementation of the protocol on top of the legacy backend:
`BENCH_SHIM_SRC=<legacy checkout> BENCH_SHIM_PY=<venv python> python3 bench/run.py --target gpui --gpui-bin bench/tools/mock-patchops --phases headless --quick`.

## Run-as

`config-readonly` is a `chmod`, and the legacy `configReadonly` status is `os.access(path, W_OK)`, which is always true for root. Since the sandbox
must behave like a real user's, the harness drops the target processes to uid/gid 65534 when it is started as root (sandbox files are chowned accordingly;
the work directory must be reachable by that uid, i.e. not under `/root`). Memory/PSS sampling stays in the harness (root).

## Caveats

* Rendering is software (Xvfb, `--disable-gpu`); there is no GPU, VSync or compositor. Numbers are comparable between targets on the same machine, not absolute.
* Build timings depend on network speed; the cold/warm split (and `pip download` vs `pip install --no-index`, `cargo fetch` vs offline build) is provided so
  download time can be separated.
* The egress proxy caches upstream downloads: the very first `pip download` of the legacy dependency set in this sandbox took ~327 s (a development run), while repeats
  of the same download finish in a few seconds. Network-bound build timings are therefore only comparable when both targets are measured with the same proxy cache state
  (compare the `*_cold` steps with that in mind, and look at the byte sizes of the caches recorded under `build.sizes`).
* Besides the 1-min loadavg guard before every launch, a finer guard (`wait_quiet`: host CPU busy < 25 % for 1 s) runs before the parity pass and before every latency group,
  and the host-wide CPU not explained by the benchmarked process tree or the harness ("foreign load") is measured for the idle window, the workload window and every latency group;
  runs above 8 % (idle) / 15 % (workload) are flagged `CONTAMINATED` in the report. `python3 bench/tools/rerender.py <results.json>` rebuilds the `.md` and summaries from a JSON.
* The idle guard cannot guarantee a quiet machine; the load average at the start of each section, host busy % per window and any guard timeout are stored in the JSON.
