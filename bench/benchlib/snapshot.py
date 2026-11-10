"""Fixture snapshots + normalisation of volatile values.

Snapshot: relative path -> {"t": "f"|"d"|"l", "sha256": ..., "mode": "0644"} for the roots that the app may touch.
Text files are normalised (sandbox path -> <SB>, log timestamps -> <TS>) before hashing so that
snapshots are identical between runs, machines and implementations.
"""
from __future__ import annotations

import hashlib
import json
import os
import re
import stat

TS_RE = re.compile(r"\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}( - )")
PLATFORM_RE = re.compile(r"(PatchOpsIII \S+ - ).+? logs:")
TMP_STAMP_RE = re.compile(r"\d{8}-\d{6}")

# Relative to the sandbox root. Chromium/Electron profile dirs (.config, .cache) are deliberately excluded.
SNAPSHOT_ROOTS = ("home/.local/share/PatchOpsIII", "home/.steam", "games", "fakebin-log", "dumps")
MAX_TEXT = 4 * 1024 * 1024


MISSING_RE = re.compile(r"(Dump source missing required files: )([^\n]+)")


def _sort_missing(m):
    # the legacy code iterates a Python set here, so the order is hash-seed dependent
    return m.group(1) + ", ".join(sorted(x.strip() for x in m.group(2).split(",")))


def normalize_text(s: str, sandbox_root: str) -> str:
    s = s.replace(sandbox_root, "<SB>")
    s = MISSING_RE.sub(_sort_missing, s)
    s = TS_RE.sub(r"<TS>\1", s)
    s = PLATFORM_RE.sub(r"\1<PLATFORM> logs:", s)
    return s


def _hash_file(path: str, sandbox_root: str, texts: dict | None = None) -> str:
    with open(path, "rb") as fh:
        raw = fh.read()
    if len(raw) <= MAX_TEXT and b"\0" not in raw:
        try:
            txt = raw.decode("utf-8")
        except UnicodeDecodeError:
            return "bin:" + hashlib.sha256(raw).hexdigest()
        norm = normalize_text(txt, sandbox_root)
        if texts is not None and len(norm) <= 48 * 1024:
            texts[os.path.relpath(path, sandbox_root)] = norm
        # keep CR/LF distinctions: hash bytes of the normalised text
        return "txt:" + hashlib.sha256(norm.encode("utf-8")).hexdigest()
    return "bin:" + hashlib.sha256(raw).hexdigest()


def snapshot(sandbox_root: str, texts: dict | None = None) -> dict:
    """texts (optional out-param): relpath -> normalised content of small text files."""
    out: dict[str, dict] = {}
    for rel_root in SNAPSHOT_ROOTS:
        base = os.path.join(sandbox_root, rel_root)
        if not os.path.lexists(base):
            continue
        stack = [base]
        while stack:
            cur = stack.pop()
            rel = os.path.relpath(cur, sandbox_root)
            try:
                st = os.lstat(cur)
            except OSError:
                continue
            mode = "%04o" % stat.S_IMODE(st.st_mode)
            if stat.S_ISLNK(st.st_mode):
                out[rel] = {"t": "l", "to": normalize_text(os.readlink(cur), sandbox_root), "mode": mode}
            elif stat.S_ISDIR(st.st_mode):
                out[rel] = {"t": "d", "mode": mode}
                try:
                    stack.extend(os.path.join(cur, n) for n in os.listdir(cur))
                except OSError:
                    pass
            else:
                try:
                    h = _hash_file(cur, sandbox_root, texts)
                except OSError as exc:
                    h = f"unreadable:{exc.errno}"
                out[rel] = {"t": "f", "sha256": h, "size": st.st_size if h.startswith("bin:") else None, "mode": mode}
    # sizes of text files vary with the sandbox path length, so only keep size for binaries
    for v in out.values():
        if v.get("size") is None:
            v.pop("size", None)
    return dict(sorted(out.items()))


def digest(snap: dict) -> str:
    return hashlib.sha256(json.dumps(snap, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def diff(old: dict, new: dict) -> dict:
    d: dict = {"added": {}, "removed": [], "changed": {}}
    for k, v in new.items():
        if k not in old:
            d["added"][k] = v
        elif old[k] != v:
            d["changed"][k] = v
    d["removed"] = sorted(k for k in old if k not in new)
    return d


def apply_diff(old: dict, d: dict) -> dict:
    cur = dict(old)
    for k in d["removed"]:
        cur.pop(k, None)
    cur.update(d["added"])
    cur.update(d["changed"])
    return dict(sorted(cur.items()))


def normalize_json(obj, sandbox_root: str, drop_keys=("logs",)):
    """Normalise an op response: strings via normalize_text, volatile keys removed."""
    if isinstance(obj, str):
        return normalize_text(obj, sandbox_root)
    if isinstance(obj, list):
        return [normalize_json(x, sandbox_root, drop_keys) for x in obj]
    if isinstance(obj, dict):
        return {k: normalize_json(v, sandbox_root, drop_keys) for k, v in obj.items() if k not in drop_keys}
    return obj


def text_delta(old_texts: dict, new_texts: dict, d: dict) -> dict:
    """Human-readable content of added/changed text files (logs: only the appended lines). Informational only."""
    out = {}
    for path in list(d["added"]) + list(d["changed"]):
        if path not in new_texts:
            continue
        new = new_texts[path]
        old = old_texts.get(path)
        if path.endswith(".log") and old is not None and new.startswith(old):
            out[path] = {"appended": new[len(old):].splitlines()}
        else:
            out[path] = new
    return out
