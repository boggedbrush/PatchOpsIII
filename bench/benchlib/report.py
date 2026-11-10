"""Render results JSON as markdown."""
from __future__ import annotations

from . import summarize


def fmt_bytes(n) -> str:
    if n is None:
        return "n/a"
    n = float(n)
    for unit in ("B", "KiB", "MiB", "GiB"):
        if n < 1024 or unit == "GiB":
            return f"{n:.0f} {unit}" if unit == "B" else f"{n:.1f} {unit}"
        n /= 1024
    return str(n)


def fmt_kb(kb) -> str:
    return "n/a" if kb is None else fmt_bytes(kb * 1024)


def fmt_s(s) -> str:
    return "n/a" if s is None else f"{s:.1f} s"


def mm(d, key="median", unit="ms", digits=0) -> str:
    if not d or not d.get("n"):
        return "n/a"
    f = lambda v: f"{v:.{digits}f}"
    return f"{f(d['median'])} {unit} ({f(d['min'])}-{f(d['max'])}, n={d['n']})"


def render_md(res: dict) -> str:
    L: list[str] = []
    a = L.append
    t = res["target"]
    a(f"# PatchOpsIII benchmark: `{t}`")
    a("")
    m = res["machine"]
    src = res.get("source", {}).get("source", res.get("source", {}))
    a(f"- Started: {res['started_at']}  |  finished: {res.get('finished_at')}")
    a(f"- Source: `{src.get('path')}` @ `{(src.get('commit') or '')[:12]}` ({src.get('branch')}, dirty tracked files: {src.get('dirty_tracked_files')})")
    a(f"- Machine: {m['cpu_model']} x{m['cpu_count']}, {m['mem_total_mb']} MB RAM, {m['platform']}")
    a(f"- Run mode: {res.get('run_mode')}  |  run-as uid: {res.get('run_as_uid')}  |  harness {res['harness_version']}")
    ig = res.get("idle_guard", [])
    if ig:
        worst = max(e["load1_at_start"] for e in ig)
        a(f"- Idle guard: threshold {res['args'].get('idle_threshold')}, {len(ig)} checks, max load1 at section start {worst:.2f}, "
          f"timeouts: {sum(1 for e in ig if e['timed_out'])}, enabled: {res['args'].get('idle_guard')}")
    a("")
    if res.get("unavailable"):
        a(f"> **Not run:** {res['unavailable']}")
        a("")
    b = res.get("build")
    if b:
        a("## Build")
        a("")
        if res.get("build_carried_over_from"):
            bg = res.get("build_idle_guard") or []
            a(f"_Build section measured by the invocation started {res['build_carried_over_from']} (carried over into this file)"
              + (f"; idle guard at its start: load1 {bg[0]['load1_at_start']:.2f}" if bg else "") + "._")
            a("")
        if b.get("skipped"):
            a(f"Skipped: {b['skipped']}")
        else:
            a("| step | wall | CPU (user+sys) | peak RSS (sum of tree, sampled) | exit |")
            a("|---|---:|---:|---:|---:|")
            for name, s in b["steps"].items():
                a(f"| {name} | {s['wall_s']:.1f} s | {s['cpu_total_s']:.1f} s | {fmt_kb(s['peak_rss_sum_kb'])} | {s['exit_code']} |")
            a(f"| **total** | **{b.get('total_wall_s')} s** | **{b.get('total_cpu_s')} s** | max {fmt_kb(b.get('total_peak_rss_sum_kb'))} | |")
            a("")
            d = b.get("derived") or {}
            if d:
                a("Network-bound vs local: " + ", ".join(f"{k}={v}" for k, v in d.items()))
                a("")
            fp = b.get("footprint") or {}
            a("Sizes: " + ", ".join(f"{k}={fmt_bytes(v)}" for k, v in fp.items() if v is not None))
            a("")
            a("| artefact | apparent size | disk usage | files |")
            a("|---|---:|---:|---:|")
            for name, s in b.get("sizes", {}).items():
                a(f"| {name} | {fmt_bytes(s['apparent_bytes'])} | {fmt_bytes(s['disk_bytes'])} | {s['files']} |")
            for n in b.get("notes", []):
                a(f"\n- note: {n}")
            for name, s in b["steps"].items():
                if s["exit_code"] != 0:
                    a(f"\n**FAILED `{name}`** (exit {s['exit_code']}): ```{s.get('log_tail','')[-600:]}```")
        a("")
    g = res.get("gui")
    if g:
        a("## GUI runs")
        a("")
        if g.get("skipped"):
            a(f"Skipped: {g['skipped']}")
        else:
            su = g.get("startup", {})
            a("### Startup (cold launch under Xvfb)")
            a("")
            a("| milestone | median (min-max) |")
            a("|---|---|")
            a(f"| first top-level window mapped | {mm(su.get('window_ms'))} |")
            a(f"| backend/RPC health answers | {mm(su.get('health_ms'))} |")
            a(f"| app ready (status loaded) | {mm(su.get('ready_ms'))} |")
            if su.get("ui_backend_connected_ms", {}).get("n"):
                a(f"| renderer first connects to backend | {mm(su.get('ui_backend_connected_ms'))} |")
            a(f"\nruns: {su.get('runs')} (ok {su.get('ok_runs')}); failures: {su.get('failed')}")
            a("")
            mem = g.get("memory", {})
            if mem.get("runs"):
                a("### Memory (whole process tree)")
                a("")
                a("| metric | idle (10 s window after 10 s settle) | peak during workload | final after workload |")
                a("|---|---|---|---|")
                kb = lambda d: ("n/a" if not d or not d.get("n") else f"{fmt_kb(d['median'])} ({fmt_kb(d['min'])}-{fmt_kb(d['max'])})")
                num = lambda d: ("n/a" if not d or not d.get("n") else f"{d['median']:.0f} ({d['min']:.0f}-{d['max']:.0f})")
                a(f"| PSS | {kb(mem.get('idle_pss_kb'))} | {kb(mem.get('peak_pss_kb'))} | {kb(mem.get('final_pss_kb'))} |")
                a(f"| RSS (sum) | {kb(mem.get('idle_rss_kb'))} | {kb(mem.get('peak_rss_kb'))} | {kb(mem.get('final_rss_kb'))} |")
                a(f"| processes | {num(mem.get('idle_procs'))} | {num(mem.get('peak_procs'))} | {num(mem.get('final_procs'))} |")
                a(f"| threads | {num(mem.get('idle_threads'))} | {num(mem.get('peak_threads'))} | {num(mem.get('final_threads'))} |")
                a(f"\nIdle CPU: {mm(mem.get('idle_cpu_percent'), unit='%', digits=2)} of one core. runs: {mem['runs']}")
                a("")
                first = next((r for r in g["runs"] if r.get("idle")), None)
                if first:
                    a("Idle process breakdown (first run):")
                    a("")
                    a("| role | pid | PSS | RSS | threads |")
                    a("|---|---:|---:|---:|---:|")
                    for p in sorted(first["idle"]["detail"], key=lambda x: -(x["pss_kb"] or 0)):
                        a(f"| {p['role']} | {p['pid']} | {fmt_kb(p['pss_kb'])} | {fmt_kb(p['rss_kb'])} | {p['threads']} |")
                    a("")
            cont = summarize.contamination(g["runs"])
            a("**Host-load check:** " + ("all measured windows were quiet (foreign CPU load <= %s%% idle / <= %s%% workload, in percent of all cores)." % (
                summarize.IDLE_FOREIGN_LIMIT, summarize.WORKLOAD_FOREIGN_LIMIT)
                if not cont else "**CONTAMINATED runs: " + "; ".join(f"run {c['index']}: {', '.join(c['reasons'])}" for c in cont) + "**"))
            a("")
            wl = g.get("workload", {})
            if wl.get("runs"):
                a_workload(L, wl)
                a_parity(L, g["runs"])
    L.append("")
    h = res.get("headless")
    if h:
        a("## Headless backend only (no GUI)")
        a("")
        if h.get("skipped"):
            a(f"Skipped: {h['skipped']}")
        else:
            r = h["run"]
            if r.get("error") or r.get("timings", {}).get("failed"):
                a(f"- **run failed:** {r.get('error') or r['timings'].get('failed')}")
            a(f"- startup: ready {r['timings'].get('ready_ms')} ms (health {r['timings'].get('health_ms')} ms)")
            if r.get("idle"):
                a(f"- idle: PSS {fmt_kb(r['idle']['pss_kb_mean'])}, RSS {fmt_kb(r['idle']['rss_kb_mean'])}, procs {r['idle']['procs']}, threads {r['idle']['threads']}, CPU {r['idle']['cpu_percent']}%")
            if r.get("workload_mem"):
                a(f"- peak during workload: PSS {fmt_kb(r['workload_mem']['peak_pss_kb'])}, RSS {fmt_kb(r['workload_mem']['peak_rss_kb'])}")
            if r.get("final"):
                a(f"- final: PSS {fmt_kb(r['final']['pss_kb'])}")
            if h.get("workload", {}).get("runs"):
                a("")
                a_workload(L, h["workload"], heading="### Backend-only workload")
                a_parity(L, [h["run"]], heading="### Backend-only parity steps")
        a("")
    n = res.get("network")
    if n:
        a("## Network group (optional, excluded from headline numbers)")
        a("")
        if n.get("skipped"):
            a(f"Skipped: {n['skipped']}")
        else:
            a("| step | op | latency | envelope ok | app ok | error |")
            a("|---|---|---:|---|---|---|")
            for r in n["results"]:
                a(f"| {r['label']} | {r['op']} | {r['latency_ms']:.0f} ms | {r['envelope_ok']} | {r['app_ok']} | {(r.get('error') or '')[:80]} |")
        a("")
    um = res.get("not_measured", [])
    if um:
        a("## Not measured / caveats")
        a("")
        for u in um:
            a(f"- **{u['metric']}**: {u['reason']}")
        a("")
    return "\n".join(L) + "\n"


def a_workload(L, wl, heading="### Capability workload"):
    a = L.append
    a(heading)
    a("")
    a(f"Parity pass: {wl['parity_steps']} steps, wall {mm(wl.get('parity_wall_s'), unit='s', digits=2)}; "
      f"of which waiting on the (fake) Steam client {mm(wl.get('parity_steam_wait_s'), unit='s', digits=2)}; "
      f"**excluding Steam waits {mm(wl.get('parity_wall_excl_steam_s'), unit='s', digits=3)}**.")
    a(f"Latency pass wall: {mm(wl.get('latency_pass_wall_s'), unit='s', digits=1)}. "
      f"Golden mismatches per run: {wl.get('parity_mismatches')} (quirk steps: {wl.get('parity_quirk_mismatches')}).")
    a("")
    a("| op (label) | n | min | median | p95 | max | errors |")
    a("|---|---:|---:|---:|---:|---:|---:|")
    for label, rec in sorted(wl.get("labels", {}).items(), key=lambda kv: (kv[1]["group"], kv[0])):
        s = rec["stats"]
        a(f"| {label} | {s['n']} | {s['min']:.2f} | {s['median']:.2f} | {s['p95']:.2f} | {s['max']:.2f} | {rec['errors']} |")
    a("")
    a("Per-op aggregate (ms):")
    a("")
    a("| op | n | median | p95 |")
    a("|---|---:|---:|---:|")
    for op, s in sorted(wl.get("per_op", {}).items()):
        a(f"| {op} | {s['n']} | {s['median']:.2f} | {s['p95']:.2f} |")
    a("")


def a_parity(L, runs, heading="### Parity pass steps (first run; also the golden step list)"):
    a = L.append
    first = next((r for r in runs if r.get("parity")), None)
    if not first:
        return
    p = first["parity"]
    bad = [(r["index"], x) for r in runs if r.get("parity") for x in r["parity"]["results"] if not x.get("match", True)]
    if bad:
        a(f"**Golden mismatches ({len(bad)}):**")
        a("")
        for idx, x in bad[:25]:
            a(f"- run {idx} step {x['i']} `{x['label']}`: " + "; ".join(m[:300] for m in x.get("mismatch", [])))
        a("")
    a(heading)
    a("")
    a("| # | step | op | latency | envelope ok | app ok | tags | golden |")
    a("|---:|---|---|---:|---|---|---|---|")
    for x in p["results"]:
        m = x.get("match")
        a(f"| {x['i']} | {x['label']} | {x['op']} | {x['latency_ms']:.1f} ms | {x['envelope_ok']} | {x['app_ok']} | {','.join(x.get('tags', []))} | "
          f"{'-' if m is None else ('ok' if m else 'MISMATCH')} |")
    a("")
