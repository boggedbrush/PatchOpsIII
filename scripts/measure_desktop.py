#!/usr/bin/env python3
"""Sample an already running app and descendants consistently across desktop stacks.

Install the optional measurement dependency with: python -m pip install psutil
"""
import argparse
import json
import platform
from pathlib import Path
import statistics
import time


def main():
    import psutil

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pid", type=int, required=True, help="Supervisor PID including backend and GUI descendants")
    parser.add_argument("--label", required=True)
    parser.add_argument("--seconds", type=int, default=60)
    parser.add_argument("--artifact", type=Path, action="append", default=[], help="Complete deliverable(s) to measure, not just the UI binary")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.seconds < 1:
        parser.error("--seconds must be positive")
    root = psutil.Process(args.pid)
    samples = []
    previous_cpu = None
    previous_time = None
    for _ in range(args.seconds):
        now = time.monotonic()
        if not root.is_running():
            raise RuntimeError("App supervisor exited before measurement finished")
        processes = [root, *root.children(recursive=True)]
        rss = 0
        cpu = 0
        names = []
        skipped = []
        for process in processes:
            try:
                rss += process.memory_info().rss
                times = process.cpu_times()
                cpu += times.user + times.system
                names.append({"pid": process.pid, "name": process.name()})
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                skipped.append(process.pid)
        # Aggregate core-percent: 100 is one CPU core. Process churn can make
        # cumulative counters fall; omit that interval rather than invent data.
        percent = None
        if previous_cpu is not None and cpu >= previous_cpu:
            percent = 100 * (cpu - previous_cpu) / (now - previous_time)
        samples.append({"rssBytes": rss, "cpuCorePercent": percent, "processes": names, "skippedPids": skipped})
        previous_cpu, previous_time = cpu, now
        time.sleep(1)
    cpu_samples = [s["cpuCorePercent"] for s in samples if s["cpuCorePercent"] is not None]
    result = {
        "label": args.label, "platform": platform.platform(), "sampleSeconds": args.seconds,
        "medianTreeRssBytes": statistics.median(s["rssBytes"] for s in samples),
        "peakTreeRssBytes": max(s["rssBytes"] for s in samples),
        "medianCpuCorePercent": statistics.median(cpu_samples) if cpu_samples else None,
        "artifacts": [{"path": str(p), "bytes": p.stat().st_size} for p in args.artifact],
        "notes": ["RSS sums can double-count shared pages; use the same method for every candidate.", "GPU memory and detached/elevated processes are excluded.", "Measure startup separately; this sampler measures an already running app."],
        "samples": samples,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(args.output)


if __name__ == "__main__":
    main()
