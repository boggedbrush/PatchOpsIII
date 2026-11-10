#!/usr/bin/env python3
"""PatchOpsIII benchmark harness (stdlib only).

  python3 bench/run.py --target legacy                 # build + GUI runs + headless run   -> bench/results/legacy.{json,md}
  python3 bench/run.py --target gpui                   # same for the Rust/GPUI port (fails gracefully if not built)
  python3 bench/compare.py bench/results/legacy.json bench/results/gpui.json

See bench/README.md and bench/OPS.md.
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
import time
import traceback

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

from benchlib import HARNESS_VERSION, procfs, proc, sysinfo, workload, summarize, report  # noqa: E402
from benchlib.sandbox import Sandbox, FIXTURE_VERSION  # noqa: E402
from benchlib.drivers.common import MemSampler, idle_window  # noqa: E402

REPO = os.path.dirname(HERE)


def parse_args(argv=None):
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--target", choices=["legacy", "gpui"], required=True)
    p.add_argument("--legacy-src", default=REPO, help="checkout of the legacy (Electron+Python) app to benchmark (default: this repo)")
    p.add_argument("--legacy-mode", choices=["auto", "packaged", "dev"], default="auto",
                   help="packaged = electron-builder linux-unpacked + PyInstaller backend; dev = electron . + venv python")
    p.add_argument("--gpui-src", default=REPO, help="checkout containing Cargo.toml for the Rust port (default: this repo)")
    p.add_argument("--gpui-bin", default=None, help="path of the patchops binary (default: <work>/gpui/target/release/patchops)")
    p.add_argument("--phases", default="build,gui,headless", help="comma list of: build,gui,headless,network")
    p.add_argument("--skip-build", action="store_true", help="reuse an existing build in the work dir (no build metrics recorded)")
    p.add_argument("--startup-runs", type=int, default=5, help="cold GUI launches measured for startup (>=5 recommended)")
    p.add_argument("--mem-runs", type=int, default=3, help="GUI launches that also run the full workload with memory sampling")
    p.add_argument("--iterations", type=float, default=1.0, help="scale factor for latency-pass iteration counts")
    p.add_argument("--settle", type=float, default=10.0, help="seconds to settle after ready before the idle window")
    p.add_argument("--idle-window", type=float, default=10.0)
    p.add_argument("--ready-timeout", type=float, default=60.0)
    p.add_argument("--no-idle-guard", action="store_true")
    p.add_argument("--idle-threshold", type=float, default=0.7)
    p.add_argument("--idle-timeout", type=float, default=900.0)
    p.add_argument("--update-golden", action="store_true", help="(legacy only) rewrite bench/golden from this run")
    p.add_argument("--golden-dir", default=os.path.join(HERE, "golden"))
    p.add_argument("--no-golden-compare", action="store_true")
    p.add_argument("--work-dir", default=os.path.join(HERE, ".work"))
    p.add_argument("--out-dir", default=os.path.join(HERE, "results"))
    p.add_argument("--out-name", default=None, help="basename for result files (default: target name)")
    p.add_argument("--run-as", choices=["auto", "root"], default="auto",
                   help="auto: drop to uid 65534 when started as root (needed so chmod-based config read-only behaves like for a real user)")
    p.add_argument("--allow-foreign-steam", action="store_true",
                   help="do not refuse to run when a non-benchmark process named `steam` exists (the workload runs `pkill -x steam`!)")
    p.add_argument("--quick", action="store_true", help="smoke-test settings: 1 startup run, 1 mem run, 0.1x iterations, short settle")
    a = p.parse_args(argv)
    if a.quick:
        a.startup_runs, a.mem_runs, a.iterations, a.settle, a.idle_window = 1, 1, 0.1, 2.0, 3.0
    return a


class Ctx:
    pass


def log(msg):
    print(time.strftime("%H:%M:%S"), msg, flush=True)


def make_target(a, run_as):
    if a.target == "legacy":
        from benchlib.drivers.legacy import LegacyTarget
        return LegacyTarget(a.legacy_src, a.work_dir, a.legacy_mode, run_as, log=log)
    from benchlib.drivers.gpui import GpuiTarget
    return GpuiTarget(a.gpui_src, a.work_dir, a.gpui_bin, run_as, log=log)


OWN_TAGS: set[str] = set()  # PATCHOPS_BENCH_TAG of every sandbox this harness process created: the only tags it may ever kill


def one_run(a, target, kind: str, idx: int, guard, with_workload: bool, golden_mode: str, network: bool = False) -> dict:
    """One process-tree lifecycle: sandbox -> (Xvfb) -> launch -> readiness -> [idle -> workload -> final] -> stop."""
    sb_root = os.path.join(a.work_dir, "sandboxes", f"{kind}-{idx}")
    sb = Sandbox(sb_root, (65534, 65534) if (a.run_as == "auto") else None, network=network).build()
    OWN_TAGS.add(sb.tag)
    run: dict = {"index": idx, "kind": kind, "sandbox_tag": sb.tag}
    xv = None
    inst = None
    try:
        stale = procfs.steam_processes(OWN_TAGS)
        foreign = [pid for pid, tagged in stale if not tagged]
        if foreign and not a.allow_foreign_steam:
            raise RuntimeError(f"refusing to run: foreign process(es) named 'steam' exist {foreign}; the workload runs `pkill -x steam`. "
                               "Close Steam or pass --allow-foreign-steam")
        for pid, tagged in stale:
            if tagged:
                try:
                    os.kill(pid, 9)
                except OSError:
                    pass
        guard.wait(f"{kind}-run-{idx}")
        display = None
        if kind == "gui":
            xv = proc.Xvfb(sb.tag)
            display = xv.start(sb.env())
            run["display"] = display
        run["load1_at_launch"] = procfs.loadavg()[0]
        inst = target.start(sb, kind, display)
        run["cmd"] = inst.cmd
        run["mode"] = inst.mode
        timings = inst.wait_ready(target.ready_probe, timeout=a.ready_timeout, want_window=(kind == "gui"))
        run["timings"] = timings
        log(f"{kind} run {idx}: window={timings.get('window_ms')} health={timings.get('health_ms')} ready={timings.get('ready_ms')} "
            f"ui_conn={timings.get('ui_backend_connected_ms')} failed={timings.get('failed')}")
        if timings.get("failed"):
            run["error"] = timings["failed"]
        elif with_workload:
            log(f"{kind} run {idx}: settling {a.settle}s, then {a.idle_window}s idle window")
            time.sleep(a.settle)
            run["idle"] = idle_window(inst.pid, a.idle_window)
            log(f"  idle: PSS={run['idle']['pss_kb_mean']//1024} MB RSS={run['idle']['rss_kb_mean']//1024} MB procs={run['idle']['procs']} "
                f"threads={run['idle']['threads']} cpu={run['idle']['cpu_percent']}%")
            sampler = MemSampler(inst.pid, 0.5)
            sampler.start()
            guard.wait_quiet("parity-pass")
            meter = procfs.LoadMeter(inst.pid)
            log(f"{kind} run {idx}: parity pass ({golden_mode})")
            run["parity"] = workload.run_parity(inst.client, sb, a.golden_dir, golden_mode, log=log)
            if golden_mode == "update":
                src = target.info().get("source", {})
                json.dump({"recorded_at": sysinfo.now_iso(), "target": a.target, "source": src, "harness_version": HARNESS_VERSION,
                           "fixture_version": FIXTURE_VERSION, "steps": run["parity"]["steps"], "uid": os.geteuid() if not a.run_as == "auto" else 65534,
                           "run_mode": run.get("mode"), "kernel": os.uname().release,
                           "note": "recorded from the legacy GUI instance; every later run compares against these files"},
                          open(os.path.join(a.golden_dir, "_meta.json"), "w"), indent=1)
            log(f"{kind} run {idx}: latency pass")
            run["latency"] = workload.run_latency(inst.client, sb, a.iterations, log=log, quiet=lambda name: guard.wait_quiet(name),
                                                   root_pid=inst.pid)
            run["workload_mem"] = sampler.finish()
            load = meter.read()
            run["workload_mem"]["host_busy_percent_all_cores"] = load["host_busy_percent"]
            run["workload_mem"]["foreign_busy_percent_all_cores"] = load["foreign_busy_percent"]
            run["load1_at_workload_end"] = procfs.loadavg()[0]
            time.sleep(3.0)
            fin = procfs.tree_sample(inst.pid, with_pss=True, detail=True)
            run["final"] = {k: fin[k] for k in ("pss_kb", "rss_kb", "procs", "threads")}
            run["final"]["detail"] = fin["detail"]
            log(f"  peak PSS={run['workload_mem']['peak_pss_kb']//1024} MB final PSS={fin['pss_kb']//1024} MB")
    except Exception as exc:
        run["error"] = f"{type(exc).__name__}: {exc}"
        run["traceback"] = traceback.format_exc()
        log(f"{kind} run {idx} crashed: {run['error']}")
    finally:
        if inst:
            run["leaks"] = inst.stop()
        left = procfs.tagged_pids(sb.tag)
        if left:
            procfs.sweep_tag(sb.tag)
            run.setdefault("leaks", {})["tag_leftover_killed"] = len(left)
        if xv:
            xv.stop()
        shutil.rmtree(sb_root, ignore_errors=True)
    return run


def main(argv=None):
    a = parse_args(argv)
    phases = [x.strip() for x in a.phases.split(",") if x.strip()]
    run_as = (65534, 65534) if (os.geteuid() == 0 and a.run_as == "auto") else None
    guard = sysinfo.IdleGuard(a.idle_threshold, a.idle_timeout, enabled=not a.no_idle_guard, log=log)
    os.makedirs(a.out_dir, exist_ok=True)
    os.makedirs(a.work_dir, exist_ok=True)
    target = make_target(a, run_as)
    res: dict = {
        "harness_version": HARNESS_VERSION, "fixture_version": FIXTURE_VERSION, "target": a.target,
        "started_at": sysinfo.now_iso(), "machine": sysinfo.machine_info(),
        "args": {k: v for k, v in vars(a).items()}, "run_mode": None, "run_as_uid": run_as[0] if run_as else os.geteuid(),
        "source": target.info(), "not_measured": [],
    }
    res["args"]["idle_guard"] = not a.no_idle_guard
    out_base = os.path.join(a.out_dir, a.out_name or a.target)

    def flush(final=False):
        res["idle_guard"] = guard.events
        if final:
            res["finished_at"] = sysinfo.now_iso()
        json.dump(res, open(out_base + ".json", "w"), indent=1, default=str)
        open(out_base + ".md", "w").write(report.render_md(res))

    um = res["not_measured"]
    um.append({"metric": "Per-process RSS of build tools", "reason": "peak build RSS is the maximum over 100 ms samples of the summed RSS of the whole build process tree "
               "(shared pages are double counted); a sampled lower bound of the true instantaneous peak"})
    um.append({"metric": "GPU / VRAM / power", "reason": "headless Xvfb with software rendering (llvmpipe/--disable-gpu); not comparable to real hardware"})
    um.append({"metric": "Network-dependent ops (T7 / DXVK / Enhanced downloads, update-check)", "reason": "excluded from headline numbers; run with --phases ...,network"})
    um.append({"metric": "Window-map time resolution", "reason": "window is polled via xdotool every ~20 ms; the reported value is the first poll that saw a viewable window >=200x150, so it has ~20-40 ms granularity"})
    um.append({"metric": "Steam-dependent ops", "reason": "launch-options / workshop-install / enhanced-uninstall / reset-stock block on a fake `steam` client (5 s sleep + >=2 s stability poll in the legacy code); they are tagged and reported separately"})

    if "build" not in phases or a.skip_build:
        try:
            prev = json.load(open(out_base + ".json"))
            if prev.get("target") == a.target and prev.get("build"):
                res["build"] = prev["build"]
                res["build_carried_over_from"] = prev.get("build_carried_over_from") or prev.get("started_at")
                res["build_idle_guard"] = [e for e in prev.get("idle_guard", []) if e.get("label") == "build"] or prev.get("build_idle_guard")
                log(f"carrying over build section from previous results ({prev.get('started_at')})")
        except (OSError, ValueError):
            pass
    try:
        if "build" in phases and not a.skip_build:
            guard.wait("build")
            if a.target == "gpui" and not os.path.isfile(os.path.join(a.gpui_src, "Cargo.toml")):
                res["build"] = {"skipped": "target not built: no Cargo.toml"}
            else:
                res["build"] = target.build()
            flush()
        reason = target.runnable()
        res["run_mode"] = getattr(target, "mode", None)
        res["source"] = target.info()
        if reason:
            res["unavailable"] = reason
            log(f"target {a.target} unavailable: {reason}")
        golden_mode = "update" if (a.update_golden and a.target == "legacy") else ("none" if a.no_golden_compare else "compare")
        if not reason and "gui" in phases:
            if not shutil_which("Xvfb") or not shutil_which("xdotool"):
                res["gui"] = {"skipped": "Xvfb and/or xdotool missing"}
            else:
                runs = []
                total = max(a.startup_runs, a.mem_runs)
                for i in range(total):
                    full = i < a.mem_runs
                    gm = golden_mode if (full and i == 0) else ("compare" if golden_mode == "update" else golden_mode)
                    if full and i > 0 and golden_mode == "update":
                        gm = "compare"
                    runs.append(one_run(a, target, "gui", i, guard, with_workload=full, golden_mode=gm if full else "none"))
                    res["gui"] = {"runs": runs, "startup": summarize.startup_summary(runs), "memory": summarize.memory_summary(runs),
                                  "workload": summarize.workload_summary(runs)}
                    flush()
        if not reason and "headless" in phases:
            r = one_run(a, target, "headless", 0, guard, with_workload=True,
                        golden_mode=("compare" if golden_mode == "update" else golden_mode))
            res["headless"] = {"run": r, "workload": summarize.workload_summary([r])}
            flush()
        if not reason and "network" in phases:
            res["network"] = run_network(a, target, guard)
            flush()
        if reason and "gui" in phases:
            res["gui"] = {"skipped": reason}
        if reason and "headless" in phases:
            res["headless"] = {"skipped": reason}
    finally:
        left = procfs.all_tagged_pids(OWN_TAGS)
        names = []
        for pid in left:
            st = procfs.read_stat(pid)
            names.append(st[1] if st else str(pid))
            try:
                os.kill(pid, 9)
            except OSError:
                pass
        res["quiet_guard"] = guard.quiet_stats
        res["final_leak_check"] = {"benchmark_processes_still_alive": len(left), "names": names}
        if left:
            log(f"WARNING: {len(left)} benchmark process(es) survived the runs and were killed: {names}")
        flush(final=True)
    log(f"wrote {out_base}.json and .md")
    return 0


def shutil_which(x):
    return shutil.which(x)


def run_network(a, target, guard) -> dict:
    sb_root = os.path.join(a.work_dir, "sandboxes", "network-0")
    sb = Sandbox(sb_root, (65534, 65534) if a.run_as == "auto" else None, network=True).build()
    OWN_TAGS.add(sb.tag)
    inst = None
    try:
        guard.wait("network")
        inst = target.start(sb, "headless", None)
        t = inst.wait_ready(target.ready_probe, timeout=a.ready_timeout, want_window=False)
        if t.get("failed"):
            return {"skipped": f"backend did not start: {t['failed']}"}
        r = workload.run_parity(inst.client, sb, a.golden_dir, "none", steps=workload.network_steps(), log=log, group="network")
        return {"results": r["results"], "wall_s": r["wall_s"]}
    finally:
        if inst:
            inst.stop()
        procfs.sweep_tag(sb.tag)
        shutil.rmtree(sb_root, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
