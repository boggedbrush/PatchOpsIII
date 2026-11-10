#!/usr/bin/env python3
"""Regenerate the examples section of bench/OPS.md from bench/golden/*.json."""
import glob
import json
import os
import re

HERE = os.path.dirname(os.path.abspath(__file__))
GOLD = os.path.join(HERE, "..", "golden")
OPS = os.path.join(HERE, "..", "OPS.md")


def elide(obj, keep_state):
    if isinstance(obj, dict):
        return {k: ("{...state object, see section 4...}" if (k == "state" and not keep_state) else elide(v, keep_state)) for k, v in obj.items()}
    if isinstance(obj, list):
        return [elide(x, keep_state) for x in obj]
    return obj


def main():
    steps = []
    for f in sorted(glob.glob(os.path.join(GOLD, "[0-9][0-9][0-9]-*.json"))):
        if f.endswith("000-initial-fs.json"):
            continue
        steps.append(json.load(open(f)))
    seen_ok, seen_err = set(), set()
    out = []
    for s in steps:
        r = s["response"]
        op = s["op"]
        if not r["envelope_ok"]:
            kind = "validation"
            if (op, kind) in seen_err:
                continue
            seen_err.add((op, kind))
            ex = f'`{{"id":N,"ok":false,"error":"validation: ..."}}`   (golden step `{s["label"]}`, args `{json.dumps(s["args"])}`)'
            out.append(f"* `{op}` invalid args: {ex}")
            continue
        res = r["result"]
        app_ok = res.get("ok") if isinstance(res, dict) else None
        key = (op, app_ok)
        if key in seen_ok:
            continue
        seen_ok.add(key)
        body = json.dumps(elide(res, keep_state=(op == "status")), indent=1 if op == "status" else None, sort_keys=True)
        if op == "status" and len(body) > 9000:
            body = body[:9000] + "\n ...truncated..."
        out.append(f"**`{op}`** `{json.dumps(s['args'])}` -> envelope ok, app ok={app_ok} (golden step `{s['label']}`)\n\n```json\n{body}\n```\n")
    text = "\n".join(out)
    md = open(OPS).read()
    md = re.sub(r"<!-- EXAMPLES:BEGIN -->.*<!-- EXAMPLES:END -->", lambda m: "<!-- EXAMPLES:BEGIN -->\n" + text + "\n<!-- EXAMPLES:END -->", md, flags=re.S)
    open(OPS, "w").write(md)
    print("examples:", len(out))


main()
