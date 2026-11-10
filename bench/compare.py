#!/usr/bin/env python3
"""Render a side-by-side markdown comparison of two result files.

  python3 bench/compare.py bench/results/legacy.json bench/results/gpui.json [-o out.md]

Column order: A (baseline) | B (candidate) | B/A. Lower is better for every row except where marked.
"""
from __future__ import annotations

import argparse
import json
import sys


def g(d, *path, default=None):
    for p in path:
        if d is None:
            return default
        if isinstance(d, dict):
            d = d.get(p)
        else:
            return default
    return d if d is not None else default


def bytes_h(n):
    if n is None:
        return "n/a"
    n = float(n)
    for u in ("B", "KiB", "MiB", "GiB"):
        if n < 1024 or u == "GiB":
            return f"{n:.0f} {u}" if u == "B" else f"{n:.1f} {u}"
        n /= 1024


def ratio(a, b):
    if a in (None, 0) or b is None:
        return "n/a"
    r = b / a
    return f"{r:.2f}x" if r >= 0.01 else f"{r:.4f}x"


class Table:
    def __init__(self, title, cols):
        self.title, self.cols, self.rows = title, cols, []

    def row(self, name, a, b, fmt=lambda v: "n/a" if v is None else f"{v:.1f}", note=""):
        self.rows.append((name, fmt(a) if a is not None else "n/a", fmt(b) if b is not None else "n/a", ratio(a, b), note))

    def md(self):
        out = [f"### {self.title}", "", "| metric | " + " | ".join(self.cols) + " | B/A | note |", "|---|" + "---:|" * (len(self.cols) + 1) + "---|"]
        for r in self.rows:
            out.append(f"| {r[0]} | {r[1]} | {r[2]} | {r[3]} | {r[4]} |")
        out.append("")
        return "\n".join(out)


ms = lambda v: "n/a" if v is None else f"{v:.0f} ms"
ms2 = lambda v: "n/a" if v is None else f"{v:.2f} ms"
sec = lambda v: "n/a" if v is None else f"{v:.1f} s"
sec2 = lambda v: "n/a" if v is None else f"{v:.2f} s"
mb = lambda v: "n/a" if v is None else f"{v/1024:.1f} MiB"
pct = lambda v: "n/a" if v is None else f"{v:.2f} %"
num = lambda v: "n/a" if v is None else f"{v:.0f}"


def compare(A, B):
    la, lb = A["target"], B["target"]
    cols = [f"A: {la}", f"B: {lb}"]
    out = [f"# Benchmark comparison: {la} (A) vs {lb} (B)", ""]
    for tag, R in (("A", A), ("B", B)):
        s = g(R, "source", "source", default={}) or {}
        out.append(f"- {tag}: `{R['target']}` commit `{(s.get('commit') or '?')[:12]}`, run {R['started_at']}, {R['machine']['cpu_model']} x{R['machine']['cpu_count']}, "
                   f"mode {R.get('run_mode')}, load1 at start max {max([e['load1_at_start'] for e in R.get('idle_guard', [{'load1_at_start': 0}])] or [0]):.2f}")
    out.append("")
    for tag, R in (("A", A), ("B", B)):
        if R.get("unavailable"):
            out.append(f"> {tag} ({R['target']}) was not runnable: {R['unavailable']}")
    out.append("")

    t = Table("Build (clean)", cols)
    ba, bb = A.get("build") or {}, B.get("build") or {}
    sa, sb_ = ba.get("steps", {}), bb.get("steps", {})
    t.row("total wall", ba.get("total_wall_s"), bb.get("total_wall_s"), sec)
    t.row("total CPU time", ba.get("total_cpu_s"), bb.get("total_cpu_s"), sec)
    t.row("peak build RSS (sum of tree)", ba.get("total_peak_rss_sum_kb"), bb.get("total_peak_rss_sum_kb"), mb)
    t.row("network-bound (download) time", g(ba, "derived", "network_bound_s_estimate"), g(bb, "derived", "network_bound_s"), sec, "legacy: bun cold-warm + pip download; gpui: cargo fetch")
    t.row("compile/link wall, clean", g(ba, "derived", "local_install_and_compile_s"), g(bb, "derived", "compile_clean_s"), sec, "legacy: install-from-cache + build + pyinstaller + packaging")
    t.row("incremental rebuild (touch 1 file)", None, g(bb, "derived", "compile_incremental_s"), sec, "no legacy equivalent: `bun run build` is " + (f"{sa.get('bun-run-build',{}).get('wall_s','?')} s" if sa else "?"))
    t.row("distributable (unpacked app dir / binary)", g(ba, "footprint", "distributable_unpacked_app_dir_bytes"), g(bb, "footprint", "binary_bytes"), bytes_h)
    t.row("backend binary", g(ba, "footprint", "backend_binary_bytes"), None, bytes_h, "legacy only (PyInstaller onefile)")
    t.row("dependency footprint", g(ba, "footprint", "dependency_footprint_bytes"), g(bb, "footprint", "dependency_footprint_bytes"), bytes_h, "legacy: node_modules+venv; gpui: CARGO_HOME")
    out.append(t.md())

    sa_, sb2 = g(A, "gui", "startup", default={}), g(B, "gui", "startup", default={})
    t = Table("Startup (cold, median of runs)", cols)
    for k, label in (("window_ms", "first window mapped"), ("health_ms", "backend/RPC health"), ("ready_ms", "app ready (status loads)")):
        t.row(label, g(sa_, k, "median"), g(sb2, k, "median"), ms, f"n={g(sa_, k, 'n')}/{g(sb2, k, 'n')}")
    t.row("renderer connects to backend", g(sa_, "ui_backend_connected_ms", "median"), g(sb2, "ui_backend_connected_ms", "median"), ms, "legacy only")
    out.append(t.md())

    ma, mbm = g(A, "gui", "memory", default={}), g(B, "gui", "memory", default={})
    t = Table("Memory (whole process tree)", cols)
    for k, label in (("idle_pss_kb", "idle PSS"), ("peak_pss_kb", "peak PSS (workload)"), ("final_pss_kb", "final PSS (after workload)"),
                     ("idle_rss_kb", "idle RSS (sum)"), ("peak_rss_kb", "peak RSS (sum)")):
        t.row(label, g(ma, k, "median"), g(mbm, k, "median"), mb)
    t.row("idle processes", g(ma, "idle_procs", "median"), g(mbm, "idle_procs", "median"), num)
    t.row("idle threads", g(ma, "idle_threads", "median"), g(mbm, "idle_threads", "median"), num)
    t.row("peak processes", g(ma, "peak_procs", "median"), g(mbm, "peak_procs", "median"), num)
    t.row("idle CPU", g(ma, "idle_cpu_percent", "median"), g(mbm, "idle_cpu_percent", "median"), pct, "of one core, 10 s window")
    out.append(t.md())

    wa, wb = g(A, "gui", "workload", default={}), g(B, "gui", "workload", default={})
    ha, hb = g(A, "headless", "workload", default={}), g(B, "headless", "workload", default={})
    ra, rb = g(A, "headless", "run", default={}), g(B, "headless", "run", default={})
    t = Table("Workload totals", cols)
    t.row("parity pass wall (GUI instance)", g(wa, "parity_wall_s", "median"), g(wb, "parity_wall_s", "median"), sec2)
    t.row("  excluding Steam-wait ops", g(wa, "parity_wall_excl_steam_s", "median"), g(wb, "parity_wall_excl_steam_s", "median"), sec2, "the Steam-wait ops are dominated by legacy sleeps")
    t.row("parity pass wall (headless)", g(ha, "parity_wall_s", "median"), g(hb, "parity_wall_s", "median"), sec2)
    t.row("headless idle PSS", g(ra, "idle", "pss_kb_mean"), g(rb, "idle", "pss_kb_mean"), mb)
    t.row("headless peak PSS", g(ra, "workload_mem", "peak_pss_kb"), g(rb, "workload_mem", "peak_pss_kb"), mb)
    t.row("headless ready", g(ra, "timings", "ready_ms"), g(rb, "timings", "ready_ms"), ms)
    out.append(t.md())
    pm_a, pm_b = wa.get("parity_mismatches"), wb.get("parity_mismatches")
    hm_a, hm_b = ha.get("parity_mismatches"), hb.get("parity_mismatches")
    out.append(f"Golden parity mismatches per run (GUI instance / headless): A={pm_a} / {hm_a}, B={pm_b} / {hm_b} (B must reproduce the legacy golden snapshots; quirk-tagged steps: A={wa.get('parity_quirk_mismatches')}, B={wb.get('parity_quirk_mismatches')})")
    out.append("")

    for title, ka, kb in (("Per-op latency, GUI instance (median / p95 ms)", wa, wb), ("Per-op latency, headless (median / p95 ms)", ha, hb)):
        la_ = (ka or {}).get("per_op", {})
        lb_ = (kb or {}).get("per_op", {})
        if not la_ and not lb_:
            continue
        out.append(f"### {title}")
        out.append("")
        out.append(f"| op | A median | A p95 | B median | B p95 | B/A median |")
        out.append("|---|---:|---:|---:|---:|---:|")
        for op in sorted(set(la_) | set(lb_)):
            x, y = la_.get(op), lb_.get(op)
            out.append(f"| {op} | {ms2(g(x,'median'))} | {ms2(g(x,'p95'))} | {ms2(g(y,'median'))} | {ms2(g(y,'p95'))} | {ratio(g(x,'median'), g(y,'median'))} |")
        out.append("")
    return "\n".join(out) + "\n"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("a")
    ap.add_argument("b")
    ap.add_argument("-o", "--output")
    args = ap.parse_args()
    A, B = json.load(open(args.a)), json.load(open(args.b))
    md = compare(A, B)
    if args.output:
        open(args.output, "w").write(md)
    else:
        sys.stdout.write(md)


if __name__ == "__main__":
    main()
