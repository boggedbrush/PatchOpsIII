"""Aggregate raw per-run data into summaries."""
from __future__ import annotations

import statistics


def mmm(vals: list[float]) -> dict:
    vals = [v for v in vals if v is not None]
    if not vals:
        return {"n": 0}
    return {"n": len(vals), "median": round(statistics.median(vals), 2), "min": round(min(vals), 2), "max": round(max(vals), 2),
            "mean": round(statistics.fmean(vals), 2), "raw": [round(v, 2) for v in vals]}


def startup_summary(runs: list[dict]) -> dict:
    ok = [r for r in runs if r.get("timings") and not r["timings"].get("failed")]
    out = {"runs": len(runs), "ok_runs": len(ok), "failed": [r["timings"].get("failed") for r in runs if r.get("timings", {}).get("failed")]}
    for k in ("window_ms", "health_ms", "ready_ms", "ui_backend_connected_ms"):
        out[k] = mmm([r["timings"].get(k) for r in ok])
    return out


def memory_summary(runs: list[dict]) -> dict:
    ok = [r for r in runs if r.get("idle")]
    out = {"runs": len(ok)}
    if not ok:
        return out
    out["idle_pss_kb"] = mmm([r["idle"]["pss_kb_mean"] for r in ok])
    out["idle_rss_kb"] = mmm([r["idle"]["rss_kb_mean"] for r in ok])
    out["idle_procs"] = mmm([r["idle"]["procs"] for r in ok])
    out["idle_threads"] = mmm([r["idle"]["threads"] for r in ok])
    out["idle_cpu_percent"] = mmm([r["idle"]["cpu_percent"] for r in ok])
    mk = [r for r in ok if r.get("workload_mem")]
    if mk:
        out["peak_pss_kb"] = mmm([r["workload_mem"]["peak_pss_kb"] for r in mk])
        out["peak_rss_kb"] = mmm([r["workload_mem"]["peak_rss_kb"] for r in mk])
        out["peak_procs"] = mmm([r["workload_mem"]["peak_procs"] for r in mk])
        out["peak_threads"] = mmm([r["workload_mem"]["peak_threads"] for r in mk])
    fk = [r for r in ok if r.get("final")]
    if fk:
        out["final_pss_kb"] = mmm([r["final"]["pss_kb"] for r in fk])
        out["final_rss_kb"] = mmm([r["final"]["rss_kb"] for r in fk])
        out["final_procs"] = mmm([r["final"]["procs"] for r in fk])
        out["final_threads"] = mmm([r["final"]["threads"] for r in fk])
    return out


def pool_latency(runs: list[dict]) -> dict:
    """Pool raw samples across runs per label and per op."""
    from .workload import stats_ms
    labels: dict[str, dict] = {}
    for r in runs:
        lat = r.get("latency")
        if not lat:
            continue
        for label, rec in lat["labels"].items():
            d = labels.setdefault(label, {"op": rec["op"], "group": rec["group"], "tags": rec["tags"], "samples_ms": [], "errors": 0,
                                          "app_not_ok": 0})
            d["samples_ms"] += rec["samples_ms"]
            d["errors"] += rec["errors"]
            d["app_not_ok"] += rec["app_not_ok"]
    per_op: dict[str, list] = {}
    for label, d in labels.items():
        d["stats"] = stats_ms(d["samples_ms"])
        key = "status(modded)" if d["group"] == "status-modded" else d["op"]
        per_op.setdefault(key, []).extend(d["samples_ms"])
    return {"labels": {k: {kk: vv for kk, vv in v.items() if kk != "samples_ms"} for k, v in labels.items()},
            "per_op": {k: stats_ms(v) for k, v in per_op.items()}}


def workload_summary(runs: list[dict]) -> dict:
    ok = [r for r in runs if r.get("parity")]
    out = {"runs": len(ok)}
    if not ok:
        return out
    out["parity_wall_s"] = mmm([r["parity"]["wall_s"] for r in ok])
    out["parity_wall_excl_steam_s"] = mmm([r["parity"]["sum_latency_excl_steam_s"] for r in ok])
    out["parity_steam_wait_s"] = mmm([r["parity"]["steam_wait_latency_s"] for r in ok])
    out["latency_pass_wall_s"] = mmm([r["latency"]["wall_s"] for r in ok if r.get("latency")])
    out["parity_steps"] = ok[0]["parity"]["steps"]
    out["parity_mismatches"] = [r["parity"].get("mismatches") for r in ok]
    out["parity_quirk_mismatches"] = [r["parity"].get("quirk_mismatches") for r in ok]
    # errors / unexpected: steps whose envelope status differs between runs
    out.update(pool_latency(ok))
    return out


IDLE_FOREIGN_LIMIT = 8.0       # % of all cores not explained by the app tree / harness, during the 10 s idle window
WORKLOAD_FOREIGN_LIMIT = 15.0  # same, averaged over the workload window and over each latency group


def contamination(runs: list[dict]) -> list[dict]:
    """Runs during which something else loaded the machine (foreign busy % from /proc/stat minus app and harness CPU)."""
    out = []
    for r in runs:
        reasons = []
        idle = (r.get("idle") or {}).get("foreign_busy_percent_all_cores")
        wl = (r.get("workload_mem") or {}).get("foreign_busy_percent_all_cores")
        if idle is not None and idle > IDLE_FOREIGN_LIMIT:
            reasons.append(f"idle window foreign load {idle}% > {IDLE_FOREIGN_LIMIT}%")
        if wl is not None and wl > WORKLOAD_FOREIGN_LIMIT:
            reasons.append(f"workload window foreign load {wl}% > {WORKLOAD_FOREIGN_LIMIT}%")
        lat = r.get("latency") or {}
        busy_groups = [g["group"] for g in lat.get("groups", []) if (g.get("foreign_busy_percent") or 0) > WORKLOAD_FOREIGN_LIMIT
                       and not g.get("tags") and g["wall_s"] > 0.3]
        if busy_groups:
            reasons.append(f"latency groups with foreign load > {WORKLOAD_FOREIGN_LIMIT}% (non-Steam): {', '.join(busy_groups)}")
        if reasons:
            out.append({"index": r.get("index"), "kind": r.get("kind"), "reasons": reasons})
    return out
