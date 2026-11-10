#!/usr/bin/env python3
"""Replay the recorded legacy goldens (bench/golden) against a patchops binary, with filters.

The bench harness has no per-op filter, so this re-uses its library (sandbox fixture, workload
steps, snapshot normalisation) and runs the *whole* parity sequence headless in a throw-away
sandbox (state carries over between steps, so skipping steps would diverge), then reports only
the steps you ask for.

  python3 tools/parity.py                                                # everything
  python3 tools/parity.py --bin ... --ops config,vram-target,presets/apply   # only those ops
  python3 tools/parity.py --bin ... --ops config --status                    # + every `status` step
  python3 tools/parity.py --bin ... --labels '^t7-'                          # by step label regex
  python3 tools/parity.py --bin ... --ops intro-skip -v                      # full diffs

Exit code 0 when every *reported* step matches. Steps of ops that are not ported yet fail with
"internal: not implemented" - filter them out with --ops.
"""
from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "bench"))

from benchlib import procfs, snapshot as snap, workload  # noqa: E402
from benchlib.drivers.gpui import GpuiTarget  # noqa: E402
from benchlib.sandbox import Sandbox  # noqa: E402


def walk_diff(a, b, path="", out=None, limit=40):
    out = [] if out is None else out
    if len(out) >= limit:
        return out
    if isinstance(a, dict) and isinstance(b, dict):
        for k in sorted(set(a) | set(b)):
            if k not in a:
                out.append(f"  + {path}/{k}: {json.dumps(b[k])[:200]}")
            elif k not in b:
                out.append(f"  - {path}/{k}: {json.dumps(a[k])[:200]}")
            else:
                walk_diff(a[k], b[k], f"{path}/{k}", out, limit)
    elif isinstance(a, list) and isinstance(b, list) and len(a) == len(b):
        for i, (x, y) in enumerate(zip(a, b)):
            walk_diff(x, y, f"{path}[{i}]", out, limit)
    elif a != b:
        out.append(f"  ~ {path}: golden={json.dumps(a)[:200]} actual={json.dumps(b)[:200]}")
    return out


STATE_KEYS = ["appVersion", "platform", "gameDir", "gameDetected", "configExists", "steamUserId", "logPath", "presets",
              "currentLaunchOptions", "activeLaunchProfile", "releaseChannel", "launchProfiles", "enhanced", "exeSwap", "t7",
              "dxvk", "qol", "graphics", "advanced", "maintenance", "mods"]


LOG_REL = "home/.local/share/PatchOpsIII/PatchOpsIII.log"


def strip_log(body):
    res = body.get("result") if isinstance(body, dict) else None
    if isinstance(res, dict):
        for holder in (res, res.get("state")):
            if isinstance(holder, dict) and isinstance(holder.get("maintenance"), dict):
                holder["maintenance"].pop("logPayload", None)


def strip_state(body, drop):
    """Remove top-level state keys `drop` from a normalised response (state itself or {state: ...})."""
    res = body.get("result") if isinstance(body, dict) else None
    if not isinstance(res, dict):
        return
    for holder in (res, res.get("state")):
        if isinstance(holder, dict):
            for k in drop:
                holder.pop(k, None)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--bin", default=os.path.join(REPO, "target", "debug", "patchops-rpc"), help="RPC binary (default target/debug/patchops-rpc; the GUI binary works too)")
    ap.add_argument("--ops", default="", help="comma list of op names to report (default: all)")
    ap.add_argument("--labels", default="", help="regex on step labels to report")
    ap.add_argument("--status", action="store_true", help="also report all `status` steps")
    ap.add_argument("-v", "--verbose", action="store_true", help="print full response/fs diffs for mismatches")
    ap.add_argument("--work-dir", default=None, help="parent of the sandbox (must be reachable by uid 65534 when run as root)")
    ap.add_argument("--keep", action="store_true", help="keep the sandbox for inspection")
    ap.add_argument("--golden-dir", default=os.path.join(REPO, "bench", "golden"))
    ap.add_argument("--keep-state", default="", help="compare only these top-level state keys (comma list, e.g. qol); "
                    "isolates your group from state owned by groups that are not ported yet")
    ap.add_argument("--ignore-log", action="store_true", help="do not compare the whole log file hash / state.maintenance.logPayload "
                    "(they cascade from every unported op that logs); the lines appended by each step are still compared")
    ap.add_argument("--ignore-state", default="", help="ignore these top-level state keys (comma list)")
    ap.add_argument("--quiet", "-q", action="store_true", help="do not print passing steps")
    ap.add_argument("--no-fs", action="store_true", help="compare responses only, ignore filesystem diffs")
    a = ap.parse_args()

    ops = {o.strip() for o in a.ops.split(",") if o.strip()}
    label_re = re.compile(a.labels) if a.labels else None
    drop = {k.strip() for k in a.ignore_state.split(",") if k.strip()}
    if a.keep_state:  # "none" = compare no state keys at all
        keep = {k.strip() for k in a.keep_state.split(",") if k.strip() and k.strip() != "none"}
        drop |= set(STATE_KEYS) - keep

    def selected(step):
        if not ops and not label_re:
            return True
        if ops and step.op in ops:
            return True
        if label_re and label_re.search(step.label):
            return True
        return a.status and step.op == "status"

    work = a.work_dir or tempfile.mkdtemp(prefix="patchops-parity-", dir="/tmp")
    os.chmod(work, 0o755)
    run_as = (65534, 65534) if os.geteuid() == 0 else None
    sb = Sandbox(os.path.join(work, "sb"), run_as).build()
    target = GpuiTarget(REPO, os.path.join(work, "bench-work"), a.bin, run_as, log=lambda m: None)
    reason = target.runnable()
    if reason:
        sys.exit(reason)
    inst = target.start(sb, "headless", None)
    res = inst.wait_ready(target.ready_probe, timeout=60, want_window=False)
    if res.get("failed"):
        sys.exit(f"binary did not become ready: {res['failed']} (see {target.logs}/run)")

    steps = workload.parity_steps()
    ph = workload.placeholders(sb)
    texts: dict = {}
    prev = snap.snapshot(sb.root, texts)
    prev_texts = texts
    shown = bad = 0
    per_op: dict = {}
    t0 = time.time()
    try:
        for i, step in enumerate(steps, 1):
            for fx in step.fx:
                sb.fx(fx)
            if step.fx:
                prev_texts = {}
                prev = snap.snapshot(sb.root, prev_texts)
            resp = inst.client.call(step.op, workload.expand(step.args, ph))
            after_texts: dict = {}
            after = snap.snapshot(sb.root, after_texts)
            d = snap.diff(prev, after)
            body = snap.normalize_json(resp.body(), sb.root)
            if step.op == "browse":
                workload._filter_browse(body)
            strip_state(body, drop)
            fs_text = snap.text_delta(prev_texts, after_texts, d)
            prev, prev_texts = after, after_texts
            if not selected(step):
                continue
            gold_path = os.path.join(a.golden_dir, workload._golden_name(i, step.label))
            gold = json.load(open(gold_path))
            strip_state(gold["response"], drop)
            gold_fs = json.loads(json.dumps(gold["fs_diff"]))
            act_fs = json.loads(json.dumps(d))
            if a.ignore_log:
                strip_log(gold["response"])
                strip_log(body)
                for x in (gold_fs, act_fs):
                    x["added"].pop(LOG_REL, None)
                    x["changed"].pop(LOG_REL, None)
            problems = []
            if gold["response"] != body:
                problems.append("response differs:")
                problems += walk_diff(gold["response"], body)
            if a.ignore_log:
                gl = (gold.get("fs_text") or {}).get(LOG_REL)
                al = fs_text.get(LOG_REL)
                if isinstance(gl, dict) != isinstance(al, dict) or (isinstance(gl, dict) and gl.get("appended") != al.get("appended")):
                    problems.append("log lines differ:")
                    problems.append(f"  golden: {json.dumps(gl.get('appended') if isinstance(gl, dict) else gl)[:700]}")
                    problems.append(f"  actual: {json.dumps(al.get('appended') if isinstance(al, dict) else al)[:700]}")
            if not a.no_fs and gold_fs != act_fs:
                problems.append("fs_diff differs:")
                problems += walk_diff(gold_fs, act_fs)
                if a.verbose:
                    for path, t in fs_text.items():
                        g = (gold.get("fs_text") or {}).get(path)
                        if g != t:
                            problems.append(f"  text {path}:\n    golden: {json.dumps(g)[:600]}\n    actual: {json.dumps(t)[:600]}")
            shown += 1
            tally = per_op.setdefault(step.op, [0, 0])
            tally[1] += 1
            tally[0] += 0 if problems else 1
            if problems:
                bad += 1
                print(f"[{i:3d}] FAIL {step.label}  ({step.op})")
                limit = len(problems) if a.verbose else 8
                for p in problems[:limit]:
                    print(p)
                if len(problems) > limit:
                    print(f"  ... {len(problems) - limit} more (use -v)")
            else:
                if not a.quiet:
                    print(f"[{i:3d}] ok   {step.label}")
    finally:
        inst.stop()
        procfs.sweep_tag(sb.tag)
        if not a.keep:
            shutil.rmtree(work, ignore_errors=True)
        else:
            print(f"sandbox kept at {sb.root}")
    if len(per_op) > 1:
        print("\nper op: " + ", ".join(f"{o} {ok}/{n}" for o, (ok, n) in sorted(per_op.items())))
    print(f"\n{shown - bad}/{shown} reported steps match ({len(steps)} replayed, {time.time() - t0:.1f}s)")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
