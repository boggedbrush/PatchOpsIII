#!/usr/bin/env python3
"""Re-render <name>.md (and recompute the summaries) from an existing results JSON.

  python3 bench/tools/rerender.py bench/results/legacy.json
"""
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
from benchlib import report, summarize  # noqa: E402

path = sys.argv[1]
res = json.load(open(path))
g = res.get("gui")
if g and g.get("runs"):
    g["startup"] = summarize.startup_summary(g["runs"])
    g["memory"] = summarize.memory_summary(g["runs"])
    g["workload"] = summarize.workload_summary(g["runs"])
h = res.get("headless")
if h and h.get("run"):
    h["workload"] = summarize.workload_summary([h["run"]])
json.dump(res, open(path, "w"), indent=1, default=str)
open(os.path.splitext(path)[0] + ".md", "w").write(report.render_md(res))
print("re-rendered", path)
