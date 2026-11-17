#!/usr/bin/env python3
"""difffuzz - differential fuzzer: legacy Python backend vs. the Rust port (patchops-rpc).

Differential fuzzing
====================
For every generated case the tool builds two byte-identical sandboxes (bench/benchlib/sandbox.py fixture + a seeded
set of fixture perturbations), starts the legacy FastAPI backend on one and `patchops-rpc --rpc-stdio` on the other,
runs the SAME random op sequence (1-12 ops) against both and, after EVERY op, compares

  * the response, normalised exactly like tools/parity.py (sandbox path -> <SB>, timestamps -> <TS>, `logs` key
    dropped, envelope errors reduced to their class `validation`/`unknown op`/`internal`), and
  * the whole filesystem snapshot (benchlib.snapshot: same roots, same hashing/normalisation, same modes; the log
    file is part of it, so the appended log lines are compared too).

On the first divergence the case stops, is MINIMISED (drop steps / perturbations / args while the same divergence
persists) and a self-contained repro (JSON case + both sides' diverging response / file diff) is printed.

Setting up the legacy side (the legacy sources are no longer in the tree)
-------------------------------------------------------------------------
    git worktree add <scratch>/baseline 2503485          # the legacy Python backend (backend/, utils.py, ...)
    python3 -m venv <scratch>/venv && <scratch>/venv/bin/pip install -r <scratch>/baseline/requirements.txt
    # the targets run as uid 65534 (like the bench harness; needed for read-only-file semantics), so both the legacy
    # source dir and the python/venv must be readable by it. If <scratch> is mode 700, copy:
    cp -r <scratch>/baseline /tmp/dfz-legacy-src && chmod -R a+rX /tmp/dfz-legacy-src
    cargo build -p patchops-core --bin patchops-rpc      # or from a clean `git worktree` of HEAD with its own CARGO_TARGET_DIR
    cp <target>/debug/patchops-rpc /tmp/dfz-bin/         # must be reachable by uid 65534 as well
    git worktree remove <scratch>/baseline               # when done
A previously built bench/.work/legacy (venv + sources) is auto-detected as the default for --legacy-src/--legacy-python.

Running
-------
    python3 tools/difffuzz.py --bin /tmp/dfz-bin/patchops-rpc --legacy-src /tmp/dfz-legacy-src \\
        --legacy-python /path/to/venv/bin/python --seed 1 --cases 200 --jobs 3
    ... --ops config,vram-target        focus on some ops (plus the implicit final `status`)
    ... --time-budget 1800              stop starting new cases after N seconds
    ... --steam                         profile with the slow Steam-dependent ops (each legacy cycle ~7 s)
    ... --keep                          keep the sandboxes (and process logs) of failing cases
    ... --replay tools/difffuzz_corpus/foo.json [--minimise]   re-run a saved case
    ... --save-dir DIR                  write one JSON file per divergence (un-minimised + minimised) into DIR
Case i of seed S depends only on (S, i, profile, --ops): results do not depend on --jobs. Exit code 1 if a
non-allowlisted divergence was found.

Process model / Steam isolation: both targets read paths from the environment at start-up (and cache settings/Steam
state per process) so each case restarts both. Each target runs in its OWN pid namespace
(`unshare --pid --fork --mount-proc`, then setpriv to uid 65534): the fake `steam` clients (found by process name, and
killed with `pkill -x steam`) of the two sandboxes - and of other workers or other agents' parity runs - cannot see
each other, so both targets can run concurrently, and killing the namespace init kills every fake steam by PID.
Without root/unshare the tool degrades to --jobs 1 (and warns); the two targets are then NOT isolated from each other's fake steam.

Throughput (4 cores, both processes restarted per case: ~0.45 s of the ~0.65 s a case takes is process start-up, so a
persistent-process variant would at best double it, but settings/Steam/game-dir caches are per process, so restarting is
the only way to keep cases independent): fast profile ~87 cases/min with --jobs 1 and ~230-240 cases/min with --jobs 3
(~3.5 ops per case). Steam profile: ~10 s per case (every Steam-dependent op costs the legacy ~7.1 s), ~11 cases/min with --jobs 2.
Minimisation re-runs the case up to --min-runs times per divergence (at most --min-per-sig divergences per signature are minimised).

Adding a generator
------------------
* Fixture perturbation: write a function `p_xxx(g)` returning a list of fx primitives (dicts, see FX_PRIMITIVES:
  write/rm/mkdir/chmod/symlink/zip/many/named) built with g.rng, and register it in PERTURBATIONS with a weight
  (and mid=True if it is safe to apply between ops: it must not touch cached files such as settings or Steam vdf).
* Op arguments: write `a_<name>(g)` returning the args dict and register it in OP_TABLE (weight per profile).
  Raw JSON numbers can be sent as {"$raw": "1e3"}; a whole non-object `args` as {"$rawargs": "null"}.
* Everything in a case is plain JSON (case = {"seed","profile","steps":[{"fx":...}|{"op":..,"args":..}]}); leading fx
  steps are applied before the processes start, later ones between ops. Placeholders in strings:
  {sb} {home} {game_dir} {common} {alt} {valid_dump} {invalid_dump} {steam_root} {userdata} {appdata} {lib2} {dumps}.

Known intentional divergences (ALLOWLIST / TEXT_NORMALISERS below; each entry: id, one-line reason, narrow matcher)
-----------------------------------------------------------------------------------------------------------------
Not generated / invisible by construction (nothing to allowlist): the port's write-once first-seen copies live outside the
snapshot roots; the `MostRecent` Steam user preference (needs config/loginusers.vdf, which is never generated); the port
refusing to edit Steam vdf when Steam will not close (the fake steam always closes); the log size bound (2 MiB, never reached).
Allowlisted (see the code; reported as `allowlisted`, never as passes; every entry is a narrow matcher with its reason, and the
table is repeated in crates/patchops-core/README.md): `vdf-string-and-section` (a vdf key used as both string and section, Steam-writing
ops only, after a perturbation tagged vdf-string-section-clash); legacy crashes where the port answers sensibly, matched on the legacy
side answering envelope `internal:` (`legacy-500-numeric-config`, `-t7-conf`, `-launchoptions-section`, `-browse-root`,
`-game-directory`); `vdf-lenient-localconfig` (python-vdf's line-oriented leniency, the port refuses to rewrite such a file);
`rmtree-symlinked-dir` (the port unlinks a symlink where `shutil.rmtree` refuses; never the target); `settings-hand-edited-types`;
`null-args-body-op`. The text normaliser `first-seen-copy-note` removes the sentence the port appends to some vdf error log lines
to point at its first-seen copy. Files that are not valid UTF-8 are compared after the same sandbox-path / timestamp normalisation as
text files. Anything not understood is reported, never allowlisted.

Steering a campaign past already-triaged bugs: `--avoid id1,id2,...` (`--avoid list` prints the ids) rejection-samples cases
whose JSON contains the feature (see KNOWN: each id is a documented root cause with a predicate over the case). Without it every
divergence is reported, which is dominated by a handful of root causes (config number formatting, in-place writes, ...);
with all ids avoided only ~1-4 % of the cases still diverge.
"""
from __future__ import annotations

import argparse
import base64
import concurrent.futures as cf
import copy
import hashlib
import http.client
import json
import os
import random
import re
import select
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unicodedata
import urllib.parse
import zipfile
import io
from types import SimpleNamespace

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "bench"))

from benchlib import procfs, proc as bproc, snapshot as snap, workload  # noqa: E402
from benchlib.clients import LEGACY_ROUTES, Resp  # noqa: E402
from benchlib.sandbox import Sandbox, CONFIG_INI_LINES, build_config_ini, LOCALCONFIG_VDF, STEAM_CONFIG_VDF  # noqa: E402

RUN_AS = (65534, 65534)
IS_ROOT = os.geteuid() == 0
UNSHARE = shutil.which("unshare")
SETPRIV = shutil.which("setpriv")
NET_OPS = {"update-check", "dxvk-install", "t7-install", "enhanced-install"}
STEAM_OPS = {"launch-options", "workshop-install", "enhanced-uninstall", "reset-stock"}
GET_OPS = {op for op, (m, _p) in LEGACY_ROUTES.items() if m == "GET"}
ALL_OPS = [o for o in LEGACY_ROUTES if o not in NET_OPS]
# ops whose side effects are asynchronous in the legacy (Popen without wait): give the fake `steam`/`xdg-open` time to log
SETTLE = {"launch": 0.6, "workshop-install": 0.6}


# =====================================================================================================
# JSON encoding of args (so that raw numbers like 1e3 / -0 can be sent byte-exact to both targets)
# =====================================================================================================
def enc(obj) -> str:
    if isinstance(obj, dict):
        if set(obj) == {"$raw"}:
            return obj["$raw"]
        return "{" + ",".join(json.dumps(k, ensure_ascii=False) + ":" + enc(v) for k, v in obj.items()) + "}"
    if isinstance(obj, list):
        return "[" + ",".join(enc(v) for v in obj) + "]"
    return json.dumps(obj, ensure_ascii=False)


def args_text(args) -> str:
    if isinstance(args, dict) and set(args) == {"$rawargs"}:
        return args["$rawargs"]
    return enc(args if args is not None else {})


# =====================================================================================================
# Transport clients (raw-text args)
# =====================================================================================================
class HttpTarget:
    def __init__(self, port, timeout=120.0):
        self.port, self.timeout, self.conn = port, timeout, None

    def call(self, op, text) -> Resp:
        if op not in LEGACY_ROUTES:
            return Resp(False, error=f"unknown op: {op}")
        method, path = LEGACY_ROUTES[op]
        headers = {"Accept": "application/json"}
        body = None
        if method == "GET":
            try:
                a = json.loads(text)
            except ValueError:
                a = {}
            q = {k: v for k, v in (a.items() if isinstance(a, dict) else []) if v is not None}
            if q:
                path += "?" + urllib.parse.urlencode(q)
        else:
            body = text.encode("utf-8", "surrogatepass")
            headers["Content-Type"] = "application/json"
        last = None
        for _ in (0, 1):
            try:
                if self.conn is None:
                    self.conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=self.timeout)
                self.conn.request(method, path, body=body, headers=headers)
                r = self.conn.getresponse()
                data = r.read()
                break
            except (http.client.RemoteDisconnected, BrokenPipeError, ConnectionResetError, http.client.CannotSendRequest,
                    http.client.BadStatusLine) as exc:
                last = exc
                self.close()
            except Exception as exc:
                self.close()
                return Resp(False, error=f"transport: {exc!r}")
        else:
            return Resp(False, error=f"transport: {last!r}")
        st = r.status
        if 200 <= st < 300:
            try:
                return Resp(True, result=json.loads(data), http_status=st)
            except ValueError:
                return Resp(False, error="internal: non-JSON response", http_status=st)
        if st == 422:
            return Resp(False, error="validation: " + data.decode("utf-8", "replace")[:600], http_status=st)
        if st in (404, 405):
            return Resp(False, error=f"unknown op: {op}", http_status=st)
        return Resp(False, error=f"internal: HTTP {st}: " + data.decode("utf-8", "replace")[:300], http_status=st)

    def close(self):
        if self.conn:
            try:
                self.conn.close()
            except Exception:
                pass
            self.conn = None


class RpcTarget:
    def __init__(self, popen, timeout=120.0):
        self.p, self.timeout, self.next_id, self.buf = popen, timeout, 1, b""
        self.fd = popen.stdout.fileno()

    def _line(self, deadline):
        while b"\n" not in self.buf:
            left = deadline - time.monotonic()
            if left <= 0:
                return None
            r, _, _ = select.select([self.fd], [], [], min(left, 1.0))
            if r:
                chunk = os.read(self.fd, 65536)
                if not chunk:
                    return None
                self.buf += chunk
            elif self.p.poll() is not None and b"\n" not in self.buf:
                return None
        line, _, self.buf = self.buf.partition(b"\n")
        return line

    def call(self, op, text, timeout=None) -> Resp:
        rid = self.next_id
        self.next_id += 1
        msg = '{"id":%d,"op":%s,"args":%s}\n' % (rid, json.dumps(op), text)
        try:
            self.p.stdin.write(msg.encode("utf-8", "surrogatepass"))
            self.p.stdin.flush()
        except Exception as exc:
            return Resp(False, error=f"transport: {exc!r}")
        deadline = time.monotonic() + (timeout or self.timeout)
        while True:
            line = self._line(deadline)
            if line is None:
                return Resp(False, error="transport: no response (timeout or process exited)")
            try:
                obj = json.loads(line)
            except ValueError:
                continue
            if obj.get("id") != rid and not (obj.get("id") is None and obj.get("ok") is False):
                continue   # (an id-less error line is the reply to an unparsable request line: attribute it to this request)
            if obj.get("ok"):
                return Resp(True, result=obj.get("result"))
            return Resp(False, error=str(obj.get("error", "")))

    def close(self):
        pass


_port_lock = threading.Lock()
_claimed_ports = set()


def pick_port() -> int:
    with _port_lock:
        for _ in range(50):
            p = random.SystemRandom().randint(20000, 45000)
            if p in _claimed_ports:
                continue
            with socket.socket() as s:
                try:
                    s.bind(("127.0.0.1", p))
                except OSError:
                    continue
            _claimed_ports.add(p)
            return p
    return bproc.free_port()


# =====================================================================================================
# Running one target
# =====================================================================================================
class Config:
    bin = None
    legacy_src = None
    legacy_python = None
    isolation = "pidns"      # or "sequential"
    keep = False
    work = None
    startup_timeout = 40.0
    op_timeout = 25.0        # a port op that takes longer than this is reported as a hang ("crash" class)
    steam_timeout = 120.0


def wrap_cmd(cmd):
    if Config.isolation == "pidns":
        return [UNSHARE, "--pid", "--fork", "--mount-proc", "--kill-child", SETPRIV, "--reuid", str(RUN_AS[0]),
                "--regid", str(RUN_AS[1]), "--clear-groups", "--"] + cmd
    return cmd


LIVE = set()          # sides whose processes are running (killed by PID on SIGINT/SIGTERM)
STOP = threading.Event()


def kill_all_live():
    for sd in list(LIVE):
        try:
            sd.kill()
        except Exception:
            pass


class Side:
    """One target (legacy or port) bound to one sandbox."""

    def __init__(self, name, sb, case_dir):
        self.name, self.sb, self.dir = name, sb, case_dir
        self.mp = None
        self.client = None
        self.err_path = os.path.join(case_dir, f"{name}.stderr.log")
        self.out_path = os.path.join(case_dir, f"{name}.stdout.log")
        self.err = None
        self.port = None
        self.snaps = None
        self.texts = {}
        self.init_resp = None

    def start(self):
        LIVE.add(self)
        env = self.sb.env()
        os.makedirs(self.dir, exist_ok=True)
        self.err = open(self.err_path, "wb")
        if self.name == "legacy":
            for attempt in range(3):
                self.port = pick_port()
                env["PATCHOPSIII_BACKEND_PORT"] = str(self.port)
                cmd = wrap_cmd([Config.legacy_python, "-m", "uvicorn", "backend.api:app", "--host", "127.0.0.1",
                                "--port", str(self.port), "--log-level", "warning"])
                self.mp = bproc.ManagedProcess(cmd, cwd=Config.legacy_src, env=env, tag=self.sb.tag, run_as=None,
                                               stdout=open(self.out_path, "wb"), stderr=self.err)
                self.client = HttpTarget(self.port, Config.op_timeout)
                t0 = time.monotonic()
                ok = False
                while time.monotonic() - t0 < Config.startup_timeout:
                    if not self.mp.alive():
                        break
                    try:
                        c = http.client.HTTPConnection("127.0.0.1", self.port, timeout=1.0)
                        c.request("GET", "/api/health")
                        r = c.getresponse()
                        r.read()
                        c.close()
                        if r.status == 200:
                            ok = True
                            break
                    except Exception:
                        time.sleep(0.03)
                if ok:
                    break
                self.kill()
                self.err = open(self.err_path, "ab")
            else:
                raise RuntimeError("legacy backend did not start: " + proc_tail(self.err_path))
        else:
            cmd = wrap_cmd([Config.bin, "--rpc-stdio"])
            self.mp = bproc.ManagedProcess(cmd, cwd=os.path.dirname(Config.bin), env=env, tag=self.sb.tag, run_as=None,
                                           stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.err)
            self.client = RpcTarget(self.mp.popen, Config.op_timeout)

    def alive(self):
        return self.mp is not None and self.mp.alive()

    def kill(self):
        LIVE.discard(self)
        _claimed_ports.discard(self.port)
        if self.mp is not None:
            try:
                bproc.kill_tree(self.mp.pid)
                self.mp.popen.wait(timeout=5)
            except Exception:
                pass
            for f in (self.mp.popen.stdin, self.mp.popen.stdout):
                try:
                    if f:
                        f.close()
                except Exception:
                    pass
            procfs.sweep_tag(self.sb.tag)
        if self.client:
            self.client.close()
        if self.err:
            try:
                self.err.close()
            except Exception:
                pass

    def call(self, op, text):
        to = Config.steam_timeout if op in STEAM_OPS else Config.op_timeout
        if isinstance(self.client, RpcTarget):
            return self.client.call(op, text, timeout=to)
        self.client.conn and setattr(self.client.conn, "timeout", to)
        return self.client.call(op, text)

    def snapshot(self):
        self.texts = {}
        self.snaps = snap.snapshot(self.sb.root, self.texts)
        for rel, t in list(self.texts.items()):          # apply TEXT_NORMALISERS (re-hash only files they change)
            t2 = extra_norm(t)
            if t2 != t:
                self.texts[rel] = t2
                ent = dict(self.snaps[rel])
                ent["sha256"] = "txt:" + hashlib.sha256(t2.encode("utf-8")).hexdigest()
                self.snaps[rel] = ent
        # Files that are not valid UTF-8 (a pre-existing log with a stray byte that the op then appends to) are hashed raw by the
        # snapshot, so the sandbox path / timestamps inside them would differ between the two sandboxes: normalise them bytewise
        # (latin-1 is a bijection between bytes and code points).
        for rel, ent in list(self.snaps.items()):
            if str(ent.get("sha256", "")).startswith("bin:") and ent.get("t") == "f" and ent.get("size", 1 << 30) <= (1 << 20):
                try:
                    with open(os.path.join(self.sb.root, rel), "rb") as fh:
                        raw = fh.read()
                except OSError:
                    continue
                norm = snap.normalize_text(raw.decode("latin-1"), self.sb.root).encode("latin-1", "replace")
                if norm != raw:
                    ent = dict(ent)
                    ent["sha256"] = "bin:" + hashlib.sha256(norm).hexdigest()
                    self.snaps[rel] = ent
        return self.snaps


def proc_tail(path, n=12):
    return bproc.tail(path, n)


# =====================================================================================================
# fx primitives
# =====================================================================================================
def dec(spec, ph):
    """data spec -> bytes. str | {"b64"} | {"cat":[spec..]} | {"repeat":[spec,n]}"""
    if spec is None:
        return b""
    if isinstance(spec, str):
        return workload.expand(spec, ph).encode("utf-8")
    if "b64" in spec:
        return base64.b64decode(spec["b64"])
    if "cat" in spec:
        return b"".join(dec(s, ph) for s in spec["cat"])
    if "repeat" in spec:
        return dec(spec["repeat"][0], ph) * int(spec["repeat"][1])
    raise ValueError(spec)


def enc_data(b: bytes):
    try:
        s = b.decode("utf-8")
        json.dumps(s)
        return s
    except (UnicodeDecodeError, ValueError):
        return {"b64": base64.b64encode(b).decode()}


def roots(sb) -> dict:
    return {"game": sb.game_dir, "home": sb.home, "steam": sb.steam_root, "userdata": sb.userdata,
            "appdata": sb.app_data, "mod": sb.mod_files, "lib2": sb.library2, "alt": sb.alt_dir, "dumps": sb.dumps,
            "sb": sb.root, "common": os.path.dirname(sb.game_dir), "fakebin": sb.fakebin, "tmp": sb.tmp,
            "userdata_root": os.path.dirname(sb.userdata)}


def ph_for(sb):
    ph = workload.placeholders(sb)
    ph.update({"steam_root": sb.steam_root, "userdata": sb.userdata, "appdata": sb.app_data, "lib2": sb.library2,
               "dumps": sb.dumps})
    return ph


def mkzip(members, mode=None):
    bio = io.BytesIO()
    with zipfile.ZipFile(bio, "w", zipfile.ZIP_DEFLATED) as z:
        for m in members:
            zi = zipfile.ZipInfo(m["name"], date_time=(2020, 1, 1, 0, 0, 0))
            zi.compress_type = zipfile.ZIP_DEFLATED
            if m.get("dir"):
                zi.external_attr = 0o40755 << 16
                z.writestr(zi, b"")
            else:
                zi.external_attr = 0o100644 << 16
                z.writestr(zi, m.get("data", "").encode("utf-8") if isinstance(m.get("data", ""), str) else b"")
    data = bio.getvalue()
    if mode == "truncate":
        data = data[: max(4, len(data) // 2)]
    elif mode == "garbage":
        data = b"PK\x03\x04" + data[4:40] + b"\xff" * 64
    elif mode == "notzip":
        data = b"this is not a zip file\n"
    return data


def apply_fx(sb, fx):
    try:
        _apply_fx(sb, fx)
    except Exception:
        pass  # a perturbation that cannot be applied (e.g. a named fixture edit on a directory) is skipped identically on both sides


def _apply_fx(sb, fx):
    r = roots(sb)
    ph = ph_for(sb)
    kind = fx["fx"]

    def path():
        return os.path.join(r[fx.get("root", "game")], workload.expand(fx["path"], ph)) if fx.get("path") else r[fx.get("root", "game")]

    if kind == "named":
        sb.fx(fx["name"])
    elif kind == "write":
        p = path()
        if os.path.islink(p) or os.path.isdir(p):
            sb.rm(p)
        sb.write(p, dec(fx.get("data"), ph), fx.get("mode", 0o644))
    elif kind == "rm":
        sb.rm(path())
    elif kind == "mkdir":
        sb.mkdir(path())
    elif kind == "chmod":
        os.chmod(path(), fx["mode"])
    elif kind == "symlink":
        p = path()
        sb._mkparents(p)
        sb.rm(p)
        os.symlink(workload.expand(fx["to"], ph), p)
        sb._chown(p)
    elif kind == "hardlink":
        p = path()
        sb._mkparents(p)
        sb.rm(p)
        os.link(workload.expand(fx["to"], ph), p)
    elif kind == "zip":
        p = path()
        sb.write(p, mkzip(fx.get("members", []), fx.get("corrupt")), 0o644)
    elif kind == "many":
        base = path()
        for i in range(fx["count"]):
            sb.mkdir(os.path.join(base, fx.get("prefix", "d") + "%04d" % i))
    elif kind == "touchmany":
        base = path()
        for i in range(fx["count"]):
            sb.write(os.path.join(base, fx.get("prefix", "f") + "%04d" % i + fx.get("suffix", "")), b"x")
    else:
        raise ValueError("unknown fx " + kind)
    # new dirs/files created by root must belong to the unprivileged uid
    try:
        sb.chown_tree(path() if fx.get("path") or kind in ("many", "touchmany") else r[fx.get("root", "game")])
    except Exception:
        pass


# =====================================================================================================
# Comparison
# =====================================================================================================
def walk_diff(a, b, path="", out=None, limit=30):
    out = [] if out is None else out
    if len(out) >= limit:
        return out
    if isinstance(a, dict) and isinstance(b, dict):
        for k in sorted(set(a) | set(b)):
            if k not in a:
                out.append(f"  + {path}/{k}: {json.dumps(b[k])[:300]}")
            elif k not in b:
                out.append(f"  - {path}/{k}: {json.dumps(a[k])[:300]}")
            else:
                walk_diff(a[k], b[k], f"{path}/{k}", out, limit)
    elif isinstance(a, list) and isinstance(b, list) and len(a) == len(b):
        for i, (x, y) in enumerate(zip(a, b)):
            walk_diff(x, y, f"{path}[{i}]", out, limit)
    elif a != b:
        out.append(f"  ~ {path}: legacy={json.dumps(a)[:300]} port={json.dumps(b)[:300]}")
    return out


def map_strings(o, fn):
    if isinstance(o, str):
        return fn(o)
    if isinstance(o, list):
        return [map_strings(x, fn) for x in o]
    if isinstance(o, dict):
        return {k: map_strings(v, fn) for k, v in o.items()}
    return o


def norm_body(resp, sb, op):
    body = snap.normalize_json(resp.body(), sb.root)
    # the two sandboxes live in sibling directories (.../l/sb and .../r/sb): normalise the parent as well
    body = json.loads(json.dumps(body).replace(os.path.dirname(sb.root).replace("\\", "\\\\"), "<SBPARENT>"))
    body = map_strings(body, extra_norm)
    if op == "browse":
        workload._filter_browse(body)
    return body


def fs_diff(l, r):
    """snapshot diff legacy -> port: (only_legacy, only_port, changed)"""
    only_l = sorted(k for k in l if k not in r)
    only_r = sorted(k for k in r if k not in l)
    ch = sorted(k for k in l if k in r and l[k] != r[k])
    return only_l, only_r, ch


def compare_step(label, op, rl, rr, side_l, side_r, prev):
    """returns None or a divergence dict"""
    bl, br = norm_body(rl, side_l.sb, op), norm_body(rr, side_r.sb, op)
    sl, sr = side_l.snapshot(), side_r.snapshot()
    only_l, only_r, ch = fs_diff(sl, sr)
    if bl == br and not (only_l or only_r or ch):
        return None
    d = {"label": label, "op": op, "resp_equal": bl == br, "resp_diff": [] if bl == br else walk_diff(bl, br) or ["  ~ (body differs)"],
         "only_legacy": only_l, "only_port": only_r, "changed": ch,
         "legacy_error": (rl.error or "")[:400] if not rl.envelope_ok else None,
         "port_error": (rr.error or "")[:400] if not rr.envelope_ok else None,
         "legacy_body": bl, "port_body": br, "files": {}}
    for p in ch + only_l + only_r:
        if p in side_l.texts or p in side_r.texts:
            d["files"][p] = {"legacy": clip(side_l.texts.get(p)), "port": clip(side_r.texts.get(p)),
                             "legacy_meta": sl.get(p), "port_meta": sr.get(p)}
        else:
            d["files"][p] = {"legacy_meta": sl.get(p), "port_meta": sr.get(p)}
    return d


def clip(t, n=1500):
    if t is None:
        return None
    return t if len(t) <= n else t[:n] + f"...[{len(t)} chars]"


def short_path(p):
    p = re.sub(r"^games/SteamLibrary/steamapps/common/Call of Duty Black Ops III", "GAME", p)
    p = re.sub(r"^home/\.local/share/PatchOpsIII", "APPDATA", p)
    p = re.sub(r"^home/\.steam/steam", "STEAM", p)
    return re.sub(r"\d+", "N", p)


def signature(div):
    """coarse root-cause key used to de-duplicate / keep minimisation faithful"""
    if div is None:
        return None
    if div.get("kind") == "crash":
        return (div["op"], "crash", div.get("who"))
    kinds = []
    if not div["resp_equal"]:
        first = div["resp_diff"][0] if div["resp_diff"] else ""
        m = re.match(r"\s*([+~-]) (\S+?):", first)
        pth = re.sub(r"\[\d+\]", "[]", m.group(2)) if m else "?"
        pth = re.sub(r"^/result(/state)?", "", pth)
        pth = re.sub(r"/(launchProfiles|logs)\b.*", r"/\1", pth)
        kinds.append("resp:" + pth)
    if div["only_legacy"] or div["only_port"] or div["changed"]:
        paths = div["only_legacy"] + div["only_port"] + div["changed"]
        paths = sorted({short_path(p) for p in paths})
        kinds.append("fs:" + ",".join(paths[:3]))
    return (div["op"], "|".join(kinds))


# =====================================================================================================
# Allowlist: known, intentional divergences. Narrow matchers only. ctx = SimpleNamespace(case, step_idx, step, div, steps_before)
# =====================================================================================================
def _case_has_fx(ctx, pred):
    return any("fx" in s and pred(s) for s in ctx.steps_before)


def _vdf_key_clash(ctx):
    # a vdf file in the case was written with a key used both as string and as section
    return _case_has_fx(ctx, lambda f: isinstance(f, dict) and f.get("tag") == "vdf-string-section-clash")


def _legacy_500(ctx):
    """the legacy answered HTTP 500 (envelope `internal:`) while the port answered with a normal envelope"""
    d = ctx.div
    return (d.get("legacy_error") or "").startswith("internal:") and not d.get("port_error")


def _fx_writes(ctx, *suffixes):
    return [st.get("data") for st in ctx.case["steps"] if st.get("fx") == "write" and str(st.get("path", "")).endswith(suffixes) and isinstance(st.get("data"), str)]


def _resp_paths_only(ctx, rx):
    """every response difference line is about a path matching `rx`, and no file differs"""
    d = ctx.div
    return bool(d["resp_diff"]) and all(re.search(rx, ln) for ln in d["resp_diff"]) and not (d["only_legacy"] or d["only_port"] or d["changed"])


# Differences that are each explained by a different allowlisted feature of the same case (e.g. a lenient-vdf localconfig AND a hand-edited
# settings file in one status): every response line must be explained by an explainer whose case feature is present, no file may differ.
def _explainers():
    return [
        ("vdf-lenient-localconfig", lambda ctx: any(t.lstrip("\ufeff \t\r\n").startswith("{ this is not vdf }") or re.search(r"(?m)^[ \t]*\}\}[ \t]*$", t)
                                                    for t in _fx_writes(ctx, "localconfig.vdf", "lc.real")), r"/(currentLaunchOptions|activeLaunchProfile|launchProfiles)"),
        ("settings-hand-edited-types", lambda ctx: any("enhanced_dump_source" in (t or "") for t in _fx_writes(ctx, "electron-settings.json")), r"/enhanced/dumpSource"),
    ]


def _combined_explained(ctx):
    d = ctx.div
    if not d["resp_diff"] or d["only_legacy"] or d["only_port"] or d["changed"]:
        return False
    ex = _explainers()
    used = set()
    for ln in d["resp_diff"]:
        hit = [eid for eid, pred, rx in ex if re.search(rx, ln) and pred(ctx)]
        if not hit:
            return False
        used.update(hit)
    return len(used) > 1


ALLOWLIST = [
    ("vdf-string-and-section", "port rejects a VDF key used as both string and section (documented in steam/vdf.rs); "
     "only for Steam-writing ops after a perturbation tagged vdf-string-section-clash",
     lambda ctx: ctx.step.get("op") in STEAM_OPS and _vdf_key_clash(ctx)),
    # ---- legacy crashes (HTTP 500): the port answers sensibly, nothing to replicate -------------------------------------------------
    ("legacy-500-numeric-config", "legacy: int(float('inf'|'nan'|'1e999')) in the state builder raises OverflowError/ValueError (HTTP 500); the port keeps "
     "its tolerant defaults (crates/patchops-core/README.md, 'Intentional differences')",
     lambda ctx: _legacy_500(ctx) and (KNOWN["legacy500_inf"][1](ctx.case) or any(re.search(r"[eE][+-]?\d{3,}", n) for n in _raw_nums(ctx.case))
                                       or any(re.search(r"(?i)inf|nan|e\+?\d{3,}", t) for t in _fx_writes(ctx, "config.real")))),
    ("legacy-500-t7-conf", "legacy: t7patch.conf that is a directory or not valid UTF-8 makes check_t7_patch_status raise (HTTP 500); the port reads what it can",
     lambda ctx: _legacy_500(ctx) and (KNOWN["t7_nonfile"][1](ctx.case) or any(
         st.get("fx") == "write" and str(st.get("path", "")).endswith("t7real.conf") and isinstance(st.get("data"), dict) for st in ctx.case["steps"]))),
    ("legacy-500-launchoptions-section", "legacy: LaunchOptions stored as a VDF section makes the state builder raise (HTTP 500); the port treats it as 'no launch options'",
     lambda ctx: _legacy_500(ctx) and any(re.search(r'"?LaunchOptions"?\s*\n\s*\{', t) for t in _fx_writes(ctx, "localconfig.vdf", "lc.real"))),
    ("legacy-500-browse-root", "legacy: browse of '/' raises (HTTP 500); the port lists the root (what the directory browser's root button needs)",
     lambda ctx: _legacy_500(ctx) and ctx.step.get("op") == "browse" and KNOWN["browse_root"][1](ctx.case)),
    ("legacy-500-game-directory", "legacy: game-directory with a path beyond PATH_MAX / a >255-byte component (Path.is_dir -> ENAMETOOLONG) or below a directory "
     "without search permission (EACCES) raises (HTTP 500); the port answers ok:false with the reason",
     lambda ctx: ctx.step.get("op") == "game-directory" and _legacy_500(ctx) and (KNOWN["longpath"][1](ctx.case) or any(
         st.get("fx") == "chmod" and isinstance(st.get("mode"), int) and st["mode"] & 0o111 == 0 for st in ctx.case["steps"]))),
    ("legacy-500-settings-save", "legacy: an unwritable app data dir / a settings path that is a directory makes the settings writers (game-directory, "
     "enhanced-validate, release-channel, ...) raise (HTTP 500); the port answers ok:false with the errno text",
     lambda ctx: _legacy_500(ctx) and KNOWN["settings_unwritable"][1](ctx.case)),
    # ---- deliberate differences -------------------------------------------------------------------------------------------------------
    ("vdf-lenient-localconfig", "python-vdf is line-oriented and lenient (a '{ garbage }' file parses to {}, a line '}}' closes one level); the port refuses to "
     "parse such a localconfig.vdf and therefore never rewrites it (launch-options/workshop-install log 'the file was left unchanged'); state building stays tolerant",
     lambda ctx: any(t.lstrip("\ufeff \t\r\n").startswith("{ this is not vdf }") or re.search(r"(?m)^[ \t]*\}\}[ \t]*$", t) for t in _fx_writes(ctx, "localconfig.vdf", "lc.real"))
     and _resp_paths_only(ctx, r"/(currentLaunchOptions|activeLaunchProfile|launchProfiles)")),
    ("rmtree-symlinked-dir", "legacy: shutil.rmtree refuses a symbolic link to a directory (mod-files/clear, t7-uninstall's cache dirs: '[Errno None] None: "
     "PosixPath(...)') and keeps it; the port unlinks the link itself and never touches the target (unit tests "
     "mod_files_clear_unlinks_symlinks_and_never_touches_their_targets, uninstall_unlinks_a_symlinked_cache_dir_and_keeps_its_target)",
     lambda ctx: ctx.step.get("op") in ("mod-files/clear", "t7-uninstall", "reset-stock") and "[Errno None] None: PosixPath(" in json.dumps(ctx.div.get("legacy_body"))),
    ("settings-hand-edited-types", "electron-settings.json edited by hand with a non-string enhanced_dump_source: legacy echoes the raw JSON value in "
     "state.enhanced.dumpSource, the port shows ''",
     lambda ctx: any("enhanced_dump_source" in (t or "") for t in _fx_writes(ctx, "electron-settings.json")) and _resp_paths_only(ctx, r"/enhanced/dumpSource")),
    ("combined-explained-differences", "one response whose differences are each explained by a different allowlisted feature present in the case "
     "(vdf-lenient-localconfig + settings-hand-edited-types)", _combined_explained),
    ("null-args-body-op", "args:null for an op whose body model has only optional fields: legacy answers 422, the port treats null as {} (bench/OPS.md section 1: absent args = {})",
     lambda ctx: (ctx.div.get("legacy_error") or "").startswith("validation:") and not ctx.div.get("port_error")
     and isinstance(ctx.step.get("args"), dict) and "$rawargs" in ctx.step["args"]),
]


# Text normalisers applied (to BOTH sides, identically) to every compared string / small text file before comparing. Each is a
# documented, narrow rewrite of a known intentional port-only addition; anything else in the text is still compared.
TEXT_NORMALISERS = [
    ("first-seen-copy-note",
     "the port appends a note about its write-once first-seen copy (kept outside the snapshot roots) to some Steam-vdf error log lines",
     re.compile(r" The earliest copy PatchOpsIII saw is kept at \S+?\.vdf\."), ""),
]


def extra_norm(text):
    for _id, _why, rx, rep in TEXT_NORMALISERS:
        text = rx.sub(rep, text)
    return text


def allowlisted(ctx):
    for aid, _reason, fn in ALLOWLIST:
        try:
            if fn(ctx):
                return aid
        except Exception:
            pass
    return None


# =====================================================================================================
# Running a case
# =====================================================================================================
def rmtree(path):
    def onerr(fn, p, exc):
        try:
            os.chmod(os.path.dirname(p), 0o755)
            os.chmod(p, 0o755)
            fn(p)
        except Exception:
            pass
    shutil.rmtree(path, onerror=onerr)


def run_case(case, tag, run_dir, steps_limit=None):
    """Run `case` once. Returns dict(status=pass|diverge|allowed|error, ...). run_dir is a fresh per-case directory."""
    t0 = time.monotonic()
    steps = case["steps"] if steps_limit is None else case["steps"][:steps_limit]
    os.makedirs(run_dir, exist_ok=True)
    os.chmod(run_dir, 0o755)
    sbs = {}
    for name, d in (("legacy", "l"), ("port", "r")):
        sbs[name] = Sandbox(os.path.join(run_dir, d, "sb"), RUN_AS if IS_ROOT else None, tag=f"dfz{tag}{d}").build()
        os.chmod(os.path.join(run_dir, d), 0o755)
    sides = {n: Side(n, sbs[n], os.path.join(run_dir, "logs")) for n in sbs}
    res = {"status": "pass", "ops": [], "timing": {}}
    L, R = sides["legacy"], sides["port"]
    try:
        # leading fx before start
        i = 0
        while i < len(steps) and "fx" in steps[i]:
            for s in sides.values():
                apply_fx(s.sb, steps[i])
            i += 1
        t_start = time.monotonic()
        try:
            if Config.isolation == "pidns":
                with cf.ThreadPoolExecutor(2) as ex:
                    fl, fr = ex.submit(L.start), ex.submit(R.start)
                    fl.result(), fr.result()
            else:
                L.start()
                R.start()
        except Exception as exc:
            return {"status": "error", "error": f"start failed: {exc}", "ops": []}
        res["timing"]["start_s"] = round(time.monotonic() - t_start, 2)
        # init: readiness status on both
        pre = [s for s in steps[:i]]
        init = step_pair(L, R, "status", "{}")
        for sd, rsp in zip((L, R), init):
            # identity check: a backend answering for another sandbox (port clash with another run) invalidates the case
            lp = (rsp.result or {}).get("logPath") if rsp.envelope_ok and isinstance(rsp.result, dict) else None
            if lp is not None and not str(lp).startswith(sd.sb.root):
                return {"status": "error", "error": f"{sd.name} backend answered for another sandbox ({lp})", "ops": []}
        prev = None
        dv = compare_step("init-status", "status", init[0], init[1], L, R, prev)
        if dv:
            return finish_div(res, case, steps, -1, {"op": "status", "args": {}}, dv, pre, L, R, "init")
        ph_l = ph_for(L.sb)
        ph_r = ph_for(R.sb)
        n_ops = 0
        idx = i
        while idx < len(steps):
            st = steps[idx]
            if "fx" in st:
                for s in sides.values():
                    apply_fx(s.sb, st)
                L.snapshot()
                R.snapshot()
                idx += 1
                continue
            op = st["op"]
            a_l = workload.expand(st.get("args", {}), ph_l)
            a_r = workload.expand(st.get("args", {}), ph_r)
            t_text_l, t_text_r = args_text(a_l), args_text(a_r)
            ta = time.monotonic()
            rl, rr = step_pair(L, R, op, t_text_l, t_text_r)
            if op in SETTLE:
                time.sleep(SETTLE[op])
            res["ops"].append((op, round(time.monotonic() - ta, 2)))
            n_ops += 1
            crash = crash_check(L, R, op, rl, rr)
            if crash:
                return finish_div(res, case, steps, idx, st, crash, steps[:idx], L, R, "crash")
            dv = compare_step(f"{n_ops}:{op}", op, rl, rr, L, R, None)
            if dv:
                return finish_div(res, case, steps, idx, st, dv, steps[:idx], L, R, "div")
            idx += 1
        # implicit final status
        rl, rr = step_pair(L, R, "status", "{}")
        dv = compare_step("final-status", "status", rl, rr, L, R, None)
        if dv:
            return finish_div(res, case, steps, len(steps), {"op": "status", "args": {}}, dv, steps, L, R, "final")
        return res
    finally:
        for s in sides.values():
            s.kill()
        res["timing"]["total_s"] = round(time.monotonic() - t0, 2)
        res["stderr"] = {n: proc_tail(sides[n].err_path, 6) for n in sides} if res.get("status") != "pass" else {}
        if res.get("status") in ("diverge",) and Config.keep:
            res["kept"] = run_dir
        else:
            rmtree(run_dir)


def step_pair(L, R, op, text_l, text_r=None):
    text_r = text_l if text_r is None else text_r
    if Config.isolation == "pidns":
        with cf.ThreadPoolExecutor(2) as ex:
            a, b = ex.submit(L.call, op, text_l), ex.submit(R.call, op, text_r)
            return a.result(), b.result()
    return L.call(op, text_l), R.call(op, text_r)


def crash_check(L, R, op, rl, rr):
    dead = [s.name for s in (L, R) if not s.alive()]
    tr = [n for n, r in (("legacy", rl), ("port", rr)) if not r.envelope_ok and (r.error or "").startswith("transport")]
    if dead or tr:
        who = sorted(set(dead + tr))
        return {"kind": "crash", "op": op, "who": ",".join(who), "label": op, "resp_equal": False,
                "resp_diff": [f"  process died / no response: {who}"], "only_legacy": [], "only_port": [], "changed": [],
                "legacy_error": rl.error, "port_error": rr.error, "legacy_body": None, "port_body": None, "files": {}}
    return None


def finish_div(res, case, steps, idx, step, dv, before, L, R, where):
    ctx = SimpleNamespace(case=case, step_idx=idx, step=step, div=dv, steps_before=before)
    aid = allowlisted(ctx) if where != "crash" else None
    res["status"] = "allowed" if aid else "diverge"
    res["allow_id"] = aid
    res["step_idx"] = idx
    res["step"] = step
    res["div"] = dv
    res["sig"] = signature(dv)
    res["where"] = where
    return res


# =====================================================================================================
# Minimiser
# =====================================================================================================
def minimise(case, sig, tag, base_dir, max_runs=60, log=lambda m: None):
    """Delta-debugging: truncate after the diverging step, drop chunks / single steps, shrink fx data, drop args.
    A candidate counts as 'still failing' when it diverges with the same field/file signature (the op name may change)."""
    best = copy.deepcopy(case)
    runs = [0]
    key = sig[1:]

    def still(c):
        if runs[0] >= max_runs:
            return None
        runs[0] += 1
        r = run_case(c, f"{tag}m{runs[0]}", os.path.join(base_dir, f"min{runs[0]}"))
        if r["status"] == "diverge" and r["sig"][1:] == key:
            return r
        return False

    last = None

    def adopt(c, r):
        nonlocal best, last
        best, last = c, r
        if r["where"] in ("div", "crash") and r.get("step_idx", -1) >= 0:
            best["steps"] = best["steps"][: r["step_idx"] + 1]

    # 1. chunked removal (ddmin-style), then single steps, until fixpoint
    chunk = max(1, len(best["steps"]) // 2)
    while runs[0] < max_runs and best["steps"]:
        removed = False
        i = 0
        while i < len(best["steps"]) and runs[0] < max_runs:
            cand = copy.deepcopy(best)
            del cand["steps"][i:i + chunk]
            r = still(cand)
            if r:
                adopt(cand, r)
                removed = True
            else:
                i += chunk
        if chunk == 1 and not removed:
            break
        if chunk > 1:
            chunk = max(1, chunk // 2)
    # 2. shrink data of fx writes (by lines) and drop optional args
    for si, s in enumerate(list(best["steps"])):
        if runs[0] >= max_runs or si >= len(best["steps"]):
            break
        s = best["steps"][si]
        if s.get("fx") == "write" and isinstance(s.get("data"), str) and len(s["data"]) > 1:
            lines = s["data"].splitlines(True)
            while len(lines) > 1 and runs[0] < max_runs:
                for half in (lines[: len(lines) // 2], lines[len(lines) // 2:]):
                    cand = copy.deepcopy(best)
                    cand["steps"][si]["data"] = "".join(half)
                    r = still(cand)
                    if r:
                        adopt(cand, r)
                        lines = half
                        break
                else:
                    break
        if "op" in s and isinstance(s.get("args"), dict):
            for k in list(s["args"]):
                if runs[0] >= max_runs:
                    break
                cand = copy.deepcopy(best)
                cand["steps"][si]["args"].pop(k, None)
                r = still(cand)
                if r:
                    adopt(cand, r)
    return best, last, runs[0]


# =====================================================================================================
# GENERATORS
# =====================================================================================================
MANAGED = ["MaxFPS", "FOV", "FullScreenMode", "WindowSize", "RefreshRate", "ResolutionPercent", "Vsync", "DrawFPS",
           "SmoothFramerate", "RestrictGraphicsOptions", "SerializeRender", "MaxFrameLatency", "VideoMemory",
           "StreamMinResident"]
UI_KEYS = list(MANAGED[:12])
GAME_FILES_VIDEO = ["BO3_Global_Logo_LogoSequence.mkv", "BO3_Global_Logo_IW.mkv", "BO3_Global_Logo_Treyarch.mkv",
                    "loadingscreen_zm.mkv"]
T7_FILES = ["t7patch.dll", "t7patch.conf", "discord_game_sdk.dll", "dsound.dll", "t7patchloader.dll", "zbr2.dll"]
PRESET_NAMES = list(json.load(open(os.path.join(REPO, "presets.json"))))
# whitespace that Python's re `\s` / str.strip() know: ASCII, \x1c-\x1f, \x85, nbsp, U+1680, U+2000-200a, U+2028/9, U+202f, U+205f, U+3000
EXOTIC_WS = ["\x0b", "\x0c", "\x1c", "\x1d", "\x1e", "\x1f", "\x85", "\u00a0", "\u1680", "\u2003", "\u2028", "\u2029", "\u202f",
             "\u205f", "\u3000", "\u200b", "\ufeff"]
INVALID_UTF8 = [b"\xe9", b"\xff", b"\xc3", b"\xc0\xaf", b"\xed\xa0\x80", b"\xf0\x9f\x98", b"\x80", b"\xfe\xff"]
UNICODE_SAMPLES = ["Ключ", "日本語", "🙂", "e\u0301", "ǅ", "İstanbul", "ß", "\u202eRTL", "naïve", "𝔘𝔫𝔦"]


class Gen:
    def __init__(self, rng, profile="fast", ops=None):
        self.rng = rng
        self.profile = profile
        self.ops = ops

    # ---- small helpers
    def chance(self, p):
        return self.rng.random() < p

    def pick(self, seq):
        return self.rng.choice(list(seq))

    def wpick(self, table):
        total = sum(w for _, w in table)
        x = self.rng.random() * total
        for item, w in table:
            x -= w
            if x <= 0:
                return item
        return table[-1][0]

    def ws(self, allow_exotic=True):
        if allow_exotic and self.chance(0.15):
            return self.pick(EXOTIC_WS)
        return self.pick([" ", " ", "  ", "\t", " \t ", ""])

    def word(self, n=None):
        n = n or self.rng.randint(1, 8)
        return "".join(self.rng.choice("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_") for _ in range(n))


# ---------- value material -------------------------------------------------------------------------
NUMERIC_STRINGS = ["0", "1", "2", "-1", "+5", "5.", ".5", "1e3", "1E3", "1e999", "-1e999", "inf", "-inf", "nan", "NaN", "Infinity",
                   "0x10", "1_000", "1__0", " 12 ", "12abc", "١٢", "１２", "00012", "-0", "-0.0", "0.29", "0.75", "0.1", "1.0",
                   "100", "240", "9999999999999999999999", "0.30000000000000004", "1e-7", "٠٫٧٥", "", "  ", "true", "True"]
CFG_VALUES = ["1", "0", "2", "3", "Off", "On", "adaptive", "1920x1080", "2560x1440", "165", "59.940", "0.75", "1.0", "8", "High", "x y z"]


def cfg_value_text(g):
    return g.pick(NUMERIC_STRINGS) if g.chance(0.5) else g.pick(CFG_VALUES)


def cfg_line(g, key=None):
    key = key or g.pick(MANAGED)
    val = cfg_value_text(g)
    form = g.wpick([("std", 6), ("nospace", 1), ("tabs", 1), ("noquote", 1), ("emptyq", 1), ("trail", 1), ("innerq", 1),
                    ("eqinval", 1), ("nocomment", 2), ("squote", 0.5), ("bigspace", 1)])
    ind = g.ws() if g.chance(0.25) else ""
    if form == "std":
        return f'{ind}{key} = "{val}" // {g.word()}'
    if form == "nospace":
        return f'{ind}{key}="{val}"'
    if form == "tabs":
        return f'{ind}{key}\t=\t"{val}"\t//c'
    if form == "noquote":
        return f'{ind}{key} = {val}'
    if form == "emptyq":
        return f'{ind}{key} = ""'
    if form == "trail":
        return f'{ind}{key} = "{val}"   \t '
    if form == "innerq":
        return f'{ind}{key} = "a"{val}"b" // x'
    if form == "eqinval":
        return f'{ind}{key} = "a = b {val}" // = ='
    if form == "nocomment":
        return f'{ind}{key} = "{val}"'
    if form == "squote":
        return f"{ind}{key} = '{val}'"
    return f'{ind}{key}  =    "{val}"  //  {g.ws()}'


META_KEYS = ["a.b", "a+b", "[x]", "(a|b)", "\\d", "a*", "^caret", "dollar$", "a{2}", "a\\b", "?q", "x|y", "Foo.Bar", "."]


def build_config_text(g):
    base = [l for l, _ in CONFIG_INI_LINES]
    mode = g.wpick([("base", 5), ("minimal", 2), ("empty", 1), ("blank", 1), ("random", 2)])
    if mode == "empty":
        lines = []
    elif mode == "blank":
        lines = [g.pick(["", " ", "\t", "\n", "   "]) for _ in range(g.rng.randint(1, 5))]
    elif mode == "minimal":
        lines = [cfg_line(g) for _ in range(g.rng.randint(1, 4))]
    elif mode == "random":
        lines = [cfg_line(g) for _ in range(g.rng.randint(3, 30))]
    else:
        lines = list(base)
    # mutations
    for _ in range(g.wpick([(0, 3), (1, 3), (2, 2), (4, 1)])):
        m = g.wpick([("dup", 4), ("prefix", 3), ("comment", 3), ("meta", 2), ("unicode", 2), ("huge", 1), ("exotic", 3),
                     ("ctrl", 1), ("lonecr", 1), ("blank", 2), ("keyalias", 2), ("indent", 2), ("hash", 1)])
        pos = g.rng.randint(0, len(lines)) if lines else 0
        if m == "dup":
            lines.insert(pos, cfg_line(g))
        elif m == "prefix":
            k = g.pick(MANAGED)
            lines.insert(pos, cfg_line(g, k + g.pick(["Mode", "2", "_x", "X", " Y"])))
            if g.chance(0.5):
                lines.insert(pos, cfg_line(g, g.pick(["x", "Pre", "my"]) + k))
        elif m == "comment":
            k = g.pick(MANAGED)
            lines.insert(pos, g.pick(["// ", "# ", "; ", "/* ", "//"]) + cfg_line(g, k).lstrip())
        elif m == "meta":
            lines.insert(pos, f'{g.pick(META_KEYS)} = "{g.pick(CFG_VALUES)}" // meta')
        elif m == "unicode":
            lines.insert(pos, f'{g.pick(UNICODE_SAMPLES)} = "{g.pick(UNICODE_SAMPLES)}" // {g.pick(UNICODE_SAMPLES)}')
        elif m == "huge":
            lines.insert(pos, f'HugeKey = "{"x" * g.rng.randint(20000, 300000)}" // big')
        elif m == "exotic":
            lines.insert(pos, g.pick(EXOTIC_WS) + cfg_line(g).lstrip())
            if g.chance(0.4):
                lines.insert(pos, g.pick(["MaxFPS", "FOV", "Vsync"]) + g.pick(EXOTIC_WS) + '= "' + g.pick(["1", "99"]) + '" // ws')
        elif m == "ctrl":
            lines.insert(pos, cfg_line(g).replace("=", "=\x00", 1) if g.chance(0.5) else "\x1a" + cfg_line(g))
        elif m == "lonecr":
            lines.insert(pos, cfg_line(g) + "\r" + cfg_line(g))
        elif m == "blank":
            lines[pos:pos] = [""] * g.rng.randint(1, 4)
        elif m == "keyalias":
            k = g.pick(UI_KEYS)
            lines.insert(pos, f'{k.lower()} = "7" // case differs')
        elif m == "indent":
            lines.insert(pos, g.pick(["  ", "\t", "    ", " \t"]) + cfg_line(g).lstrip())
        elif m == "hash":
            lines.insert(pos, f'#{g.pick(UI_KEYS)} = "1"')
    # joining
    sep = g.wpick([("\r\n", 5), ("\n", 3), ("\r", 1), ("mixed", 2)])
    out = ""
    for i, l in enumerate(lines):
        s = sep if sep != "mixed" else g.pick(["\r\n", "\n", "\r"])
        out += l + (s if (i < len(lines) - 1 or g.chance(0.8)) else "")
    raw = out.encode("utf-8", "surrogatepass")
    if g.chance(0.12):
        raw = b"\xef\xbb\xbf" + raw
    if g.chance(0.15) and raw:
        for _ in range(g.rng.randint(1, 3)):
            i = g.rng.randint(0, len(raw))
            raw = raw[:i] + g.pick(INVALID_UTF8) + raw[i:]
    return raw


def p_config_ini(g):
    fx = [{"fx": "write", "root": "game", "path": "players/config.ini", "data": enc_data(build_config_text(g))}]
    r = g.wpick([("none", 8), ("ro", 3), ("wo", 1), ("missing", 2), ("dir", 1), ("symlink", 1), ("dangling", 1),
                 ("hardlink", 1), ("dir_ro", 1), ("nodir", 1), ("empty0", 1)])
    p = "players/config.ini"
    if r == "ro":
        fx.append({"fx": "chmod", "root": "game", "path": p, "mode": g.pick([0o444, 0o400])})
    elif r == "wo":
        fx.append({"fx": "chmod", "root": "game", "path": p, "mode": 0o200})
    elif r == "missing":
        fx = [{"fx": "rm", "root": "game", "path": p}]
    elif r == "dir":
        fx = [{"fx": "rm", "root": "game", "path": p}, {"fx": "mkdir", "root": "game", "path": p}]
    elif r == "symlink":
        fx.insert(0, {"fx": "write", "root": "game", "path": "players/config.real", "data": fx[0]["data"]})
        fx = [fx[0], {"fx": "symlink", "root": "game", "path": p, "to": "config.real"}] + fx[2:]
    elif r == "dangling":
        fx = [{"fx": "symlink", "root": "game", "path": p, "to": "nowhere.ini"}]
    elif r == "hardlink":
        fx.append({"fx": "hardlink", "root": "game", "path": "players/config.link", "to": "{game_dir}/players/config.ini"})
    elif r == "dir_ro":
        fx.append({"fx": "chmod", "root": "game", "path": "players", "mode": 0o555})
    elif r == "nodir":
        fx = [{"fx": "rm", "root": "game", "path": "players"}]
    elif r == "empty0":
        fx = [{"fx": "write", "root": "game", "path": p, "data": ""}]
    return fx


def p_config_mid(g):
    return [{"fx": "write", "root": "game", "path": "players/config.ini", "data": enc_data(build_config_text(g))}]


# ---------- game dir layout --------------------------------------------------------------------------
def p_exe(g):
    r = g.wpick([("rm", 2), ("both", 2), ("alt_only", 2), ("dir", 1), ("dangling", 1), ("empty", 1), ("noperm", 1),
                 ("variant", 2), ("hash_seed", 2), ("enhanced_files", 2), ("depot", 2), ("exe_backups", 3), ("install_script", 1)])
    exe = "BlackOps3.exe"
    if r == "rm":
        return [{"fx": "rm", "root": "game", "path": exe}]
    if r == "both":
        return [{"fx": "write", "root": "game", "path": "BlackOpsIII.exe", "data": {"b64": base64.b64encode(b"MZ-second").decode()}}]
    if r == "alt_only":
        return [{"fx": "rm", "root": "game", "path": exe},
                {"fx": "write", "root": "game", "path": "BlackOpsIII.exe", "data": "MZ-only-alt"}]
    if r == "dir":
        return [{"fx": "rm", "root": "game", "path": exe}, {"fx": "mkdir", "root": "game", "path": exe}]
    if r == "dangling":
        return [{"fx": "rm", "root": "game", "path": exe}, {"fx": "symlink", "root": "game", "path": exe, "to": "missing.exe"}]
    if r == "empty":
        return [{"fx": "write", "root": "game", "path": exe, "data": ""}]
    if r == "noperm":
        return [{"fx": "chmod", "root": "game", "path": exe, "mode": 0}]
    if r == "variant":
        return [{"fx": "write", "root": "game", "path": ".patchops_exe_variant.json",
                 "data": g.pick(['{"variant":"default"}', '{"variant":"compatible"}', '{"variant":"enhanced"}', '{"variant":"current"}',
                                 '{"variant":5}', '[]', 'nope', '', '{"variant":"x"}', '{"variant":"default"'])}]
    if r == "hash_seed":
        return [{"fx": "named", "name": "seed_enhanced_hash"}]
    if r == "enhanced_files":
        return [{"fx": "named", "name": "install_fake_enhanced"}] + ([{"fx": "named", "name": "seed_enhanced_hash"}] if g.chance(0.5) else [])
    if r == "depot":
        out = [{"fx": "named", "name": "place_fake_depot"}]
        if g.chance(0.4):
            out.append({"fx": "write", "root": "lib2", "path": "steamapps/content/app_311210/depot_311211/BlackOps3.exe", "data": ""})
        return out
    if r == "exe_backups":
        out = []
        if g.chance(0.5):
            out.append({"fx": "named", "name": "seed_enhanced_hash"})
        for nm in g.rng.sample(["BlackOps3.enhanced.bak", "BlackOps3.enhanced-1.bak", "BlackOps3.current.bak", "BlackOps3.24784313.bak",
                                "BlackOps3.10650222.bak", "BlackOps3.compatible-10650222.exe", "BlackOps3.exe.patchops.bak",
                                "BlackOps3.exe.bak", "BlackOps3.enhanced-2.bak"], g.rng.randint(1, 3)):
            out.append({"fx": "write", "root": "game", "path": nm, "data": g.pick(["MZ-bak", "", "MZ" + "x" * 50])})
        return out
    return [{"fx": "write", "root": "game", "path": "installscript_311210.vdf", "data": g.pick(["", "garbage", '"InstallScript"\n{\n}\n'])}]


BAK_STATES = ["orig", "orig+bak", "orig+pbak", "orig+both", "bak", "pbak", "both", "none"]


def _state_files(g, st, rel, content="x"):
    out = []
    if "orig" in st:
        out.append({"fx": "write", "root": "game", "path": rel, "data": content + "-orig"})
    elif st != "none":
        out.append({"fx": "rm", "root": "game", "path": rel})
    else:
        out.append({"fx": "rm", "root": "game", "path": rel})
    if st in ("orig+bak", "bak", "both") or "+both" in st:
        out.append({"fx": "write", "root": "game", "path": rel + ".bak", "data": content + "-bak"})
    if st in ("orig+pbak", "pbak", "both") or "+both" in st:
        out.append({"fx": "write", "root": "game", "path": rel + ".patchops.bak", "data": content + "-pbak"})
    return out


def p_video(g):
    r = g.wpick([("remove", 1), ("empty", 1), ("states", 8), ("odd", 3), ("asfile", 1), ("symlink", 1), ("ro", 1)])
    v = "video"
    if r == "remove":
        return [{"fx": "rm", "root": "game", "path": v}]
    if r == "empty":
        return [{"fx": "rm", "root": "game", "path": v}, {"fx": "mkdir", "root": "game", "path": v}]
    if r == "asfile":
        return [{"fx": "rm", "root": "game", "path": v}, {"fx": "write", "root": "game", "path": v, "data": "i am a file"}]
    if r == "symlink":
        return [
            {"fx": "mkdir", "root": "game", "path": "video_real"},
            {"fx": "write", "root": "game", "path": "video_real/BO3_Global_Logo_LogoSequence.mkv", "data": "real"},
            {"fx": "rm", "root": "game", "path": v}, {"fx": "symlink", "root": "game", "path": v, "to": "video_real"}]
    out = []
    if r == "states":
        for n in g.rng.sample(GAME_FILES_VIDEO, g.rng.randint(1, 4)):
            out += _state_files(g, g.pick(BAK_STATES), f"video/{n}", n)
        if g.chance(0.4):
            out += _state_files(g, g.pick(BAK_STATES), "video/BO3_Global_Logo_LogoSequence.mkv", "intro")
    elif r == "odd":
        for nm in g.rng.sample(["x.MKV", "x.mkv.patchops.bak.patchops.bak", "a.mkv.bak.bak", ".mkv", "noext", "x.mkv.BAK", "é.mkv",
                                "sp ace.mkv", "a.mkv.patchops.bak", "b.mkv.bak", "keepme.mkv.txt", "n.mkv\n"], g.rng.randint(1, 4)):
            out.append({"fx": "write", "root": "game", "path": "video/" + nm, "data": nm})
        if g.chance(0.5):
            out.append({"fx": "mkdir", "root": "game", "path": "video/dir.mkv"})
        if g.chance(0.3):
            out.append({"fx": "mkdir", "root": "game", "path": "video/dir2.mkv.patchops.bak"})
    elif r == "ro":
        out.append({"fx": "chmod", "root": "game", "path": v, "mode": 0o555})
    return out


def p_d3d(g):
    r = g.wpick([("states", 8), ("dir", 1), ("symlink", 1), ("gamero", 1)])
    rel = "d3dcompiler_46.dll"
    if r == "dir":
        return [{"fx": "rm", "root": "game", "path": rel}, {"fx": "mkdir", "root": "game", "path": rel}]
    if r == "symlink":
        return [{"fx": "rm", "root": "game", "path": rel}, {"fx": "symlink", "root": "game", "path": rel, "to": "steam_api64.dll"}]
    if r == "gamero":
        return [{"fx": "chmod", "root": "game", "path": ".", "mode": 0o555}]
    return _state_files(g, g.pick(BAK_STATES), rel, "d3d")


def p_lpc(g):
    r = g.wpick([("states", 6), ("remove", 1), ("asfile", 1), ("fake_t7", 2)])
    if r == "remove":
        return [{"fx": "rm", "root": "game", "path": "LPC"}]
    if r == "asfile":
        return [{"fx": "rm", "root": "game", "path": "LPC"}, {"fx": "write", "root": "game", "path": "LPC", "data": "x"}]
    if r == "fake_t7":
        return [{"fx": "named", "name": "install_fake_t7"}]
    out = []
    for n in ["lpc_core.ff", "lpc_extra.ff"] + (["lpc_new.ff"] if g.chance(0.4) else []):
        out += _state_files(g, g.pick(BAK_STATES), "LPC/" + n, n)
    return out


def p_dxvk(g):
    r = g.wpick([("partial", 4), ("full", 2), ("conf", 6), ("dirs", 1)])
    out = []
    if r in ("partial", "full"):
        names = ["dxgi.dll", "d3d11.dll"] if r == "full" else g.rng.sample(["dxgi.dll", "d3d11.dll"], g.rng.randint(0, 1))
        for n in names:
            out.append({"fx": "write", "root": "game", "path": n, "data": "MZ" + n})
        if g.chance(0.5):
            out.append({"fx": "write", "root": "game", "path": "dxvk.conf", "data": build_dxvk_conf(g)})
    elif r == "conf":
        out.append({"fx": "write", "root": "game", "path": "dxvk.conf", "data": enc_data(build_dxvk_conf(g, raw=True))})
        m = g.wpick([("none", 8), ("ro", 2), ("dir", 1), ("symlink", 1)])
        if m == "ro":
            out.append({"fx": "chmod", "root": "game", "path": "dxvk.conf", "mode": 0o444})
        elif m == "dir":
            out = [{"fx": "mkdir", "root": "game", "path": "dxvk.conf"}]
        elif m == "symlink":
            out = [{"fx": "write", "root": "game", "path": "dxvk.real", "data": out[0]["data"]},
                   {"fx": "symlink", "root": "game", "path": "dxvk.conf", "to": "dxvk.real"}]
    else:
        out = [{"fx": "mkdir", "root": "game", "path": "dxgi.dll"}, {"fx": "write", "root": "game", "path": "d3d11.dll", "data": "x"}]
    return out


def build_dxvk_conf(g, raw=False):
    keys = ["dxvk.enableAsync", "dxvk.gplAsyncCache", "dxvk.numCompilerThreads", "dxgi.maxFrameRate", "dxgi.maxFrameLatency",
            "dxvk.tearFree", "dxvk.hud"]
    vals = {"dxvk.enableAsync": ["true", "false", "True", "TRUE", " true ", "1", "yes", ""],
            "dxvk.gplAsyncCache": ["true", "false", "True", ""],
            "dxvk.numCompilerThreads": ["4", "0", "abc", "5_0", "+3", "-1", "٣", "0x5", "5.0", " 7 ", "", "99999999999999999999", "1e2"],
            "dxgi.maxFrameRate": ["144", "0", "abc", "-5", "1_0", "60.5", ""],
            "dxgi.maxFrameLatency": ["2", "1", "x", "16", "+1"],
            "dxvk.tearFree": ["Auto", "True", "False", "auto", "", "weird value", "TRUE"],
            "dxvk.hud": ["fps", "", "false", "0", "fps,frametimes", " "]}
    lines = []
    for _ in range(g.rng.randint(0, 9)):
        k = g.pick(keys)
        v = g.pick(vals[k])
        form = g.wpick([("std", 6), ("spaces", 2), ("caseK", 1), ("comment", 1), ("indent", 1), ("tabs", 1), ("multi", 1),
                        ("nokey", 1), ("unknown", 1), ("exotic", 1)]) if raw else "std"
        if form == "std":
            lines.append(f"{k}={v}")
        elif form == "spaces":
            lines.append(f" {k} = {v} ")
        elif form == "caseK":
            lines.append(f"{k.upper()}={v}")
        elif form == "comment":
            lines.append(f"#{k}={v}" if g.chance(0.5) else f"  # {k} = {v}")
        elif form == "indent":
            lines.append(f"\t{k}={v}")
        elif form == "tabs":
            lines.append(f"{k}\t=\t{v}")
        elif form == "multi":
            lines.append(f"{k}={v}={v}")
        elif form == "nokey":
            lines.append(g.pick(["justtext", "=value", "=", "   ", ""]))
        elif form == "unknown":
            lines.append(f"{g.pick(['d3d9.x', 'dxvk.other', 'foo'])}={v}")
        elif form == "exotic":
            lines.append(f"{k}={v}{g.pick(EXOTIC_WS)}{g.pick(['x', '5', ''])}")
    sep = g.pick(["\n", "\r\n", "\n", "\r", "\x0c"]) if raw else "\n"
    text = sep.join(lines) + (sep if g.chance(0.7) else "")
    if raw and g.chance(0.1):
        return text.encode() + g.pick(INVALID_UTF8)
    if raw and g.chance(0.08):
        return b"\xef\xbb\xbf" + text.encode()
    return text.encode() if raw else text


def build_t7_conf(g):
    lines = []
    for _ in range(g.rng.randint(0, 8)):
        k = g.wpick([("playername", 4), ("networkpassword", 3), ("isfriendsonly", 3), ("unrelated", 2), ("PlayerName", 1), ("playername ", 1),
                     (" playername", 1), ("isfriendsonly", 1)])
        v = g.pick(["", "Bench", "^1Bench", "^1", "^", "0", "1", "2", "x=y", "pw", "  sp  ", "name\ttab", "é", "🙂", "a" * 30, "^7^1X",
                    "^", "\x0cx", "1 "])
        lines.append(f"{k}={v}")
    if g.chance(0.4):
        lines.insert(g.rng.randint(0, len(lines)), "# comment")
    sep = g.wpick([("\n", 5), ("\r\n", 3), ("\r", 1), ("mixed", 1)])
    out = ""
    for i, l in enumerate(lines):
        s = sep if sep != "mixed" else g.pick(["\n", "\r\n", "\r"])
        out += l + (s if (i < len(lines) - 1 or g.chance(0.7)) else "")
    raw = out.encode()
    if g.chance(0.1):
        raw += g.pick(INVALID_UTF8)
    if g.chance(0.07):
        raw = b"\xef\xbb\xbf" + raw
    return raw


def p_t7(g):
    r = g.wpick([("conf", 8), ("files", 3), ("fake", 2), ("mod", 2)])
    if r == "fake":
        return [{"fx": "named", "name": "install_fake_t7"}]
    if r == "files":
        out = []
        for n in g.rng.sample(T7_FILES, g.rng.randint(1, 5)):
            out.append({"fx": "write", "root": "game", "path": n, "data": "x" + n})
        if g.chance(0.2):
            out.append({"fx": "mkdir", "root": "game", "path": g.pick(T7_FILES)})
        return out
    if r == "mod":
        out = [{"fx": "named", "name": "install_fake_t7"}]
        for p in g.rng.sample(["linux/x.bin", "Linux.Steamdeck.and.Manual.Windows.Install/a/b.txt", "T7Patch_extracted/sub/t7patch.dll", "T7Patch.zip"], 2):
            out.append({"fx": "write", "root": "mod", "path": p, "data": "m"})
        if g.chance(0.3):
            out.append({"fx": "chmod", "root": "mod", "path": "T7Patch_extracted", "mode": 0o555})
        if g.chance(0.2):
            out.append({"fx": "symlink", "root": "mod", "path": "linux", "to": "{game_dir}"})
        return out
    out = [{"fx": "write", "root": "game", "path": "t7patch.conf", "data": enc_data(build_t7_conf(g))}]
    m = g.wpick([("none", 8), ("ro", 2), ("dir", 1), ("symlink", 1), ("gamero", 1)])
    if m == "ro":
        out.append({"fx": "chmod", "root": "game", "path": "t7patch.conf", "mode": 0o444})
    elif m == "dir":
        out = [{"fx": "mkdir", "root": "game", "path": "t7patch.conf"}]
    elif m == "symlink":
        out = [{"fx": "write", "root": "game", "path": "t7real.conf", "data": out[0]["data"]},
               {"fx": "symlink", "root": "game", "path": "t7patch.conf", "to": "t7real.conf"}]
    elif m == "gamero":
        out.append({"fx": "chmod", "root": "game", "path": ".", "mode": 0o555})
    return out


def p_t7_mid(g):
    return [{"fx": "write", "root": "game", "path": "t7patch.conf", "data": enc_data(build_t7_conf(g))}]


# ---------- Steam files --------------------------------------------------------------------------------
LO_TOKENS = ['%command%', 'WINEDLLOVERRIDES="dsound=n,b"', '+set fs_game 2994481309', '+set fs_game offlinemp', '+set fs_game 2942053577',
             '+set fs_game 3667377161', '-novid', '-windowed', 'PROTON_LOG=1', '+set fs_game', '+set fs_game=x', '+SET fs_game y',
             'XWINEDLLOVERRIDES="dsound=n,b"', '%command%;', '"%command%"', 'WINEDLLOVERRIDES="dsound=n,b";', '+set  fs_game   spaced',
             '+set\tfs_game\ttabbed', '-nosplash +set fs_game 123 -x', "WINEDLLOVERRIDES='dsound=n,b'", 'WINEDLLOVERRIDES="WindowsCodecs=n,b"',
             'wineDLLoverrides="dsound=n,b"', '%COMMAND%', 'MANGOHUD=1', 'DXVK_HUD=fps', 'é', '🙂', '+set fs_game 日本']


def launch_options_text(g):
    n = g.wpick([(0, 2), (1, 3), (2, 3), (3, 2), (5, 1)])
    toks = [g.pick(LO_TOKENS) for _ in range(n)]
    out = ""
    for t in toks:
        out += (g.pick([" ", " ", "  ", "\t", " \t", "\u00a0", "\x0c", "\x1c", "\u3000"]) if out else g.pick(["", "", " ", "\t"])) + t
    return out + g.pick(["", "", " ", "  ", "\t"])


def vesc(s):
    return s.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n").replace("\t", "\\t")


def vdf_localconfig(g):
    """a localconfig.vdf with random structure/format (and, rarely, errors)"""
    lo = launch_options_text(g)
    ind = g.pick(["\t", "\t", "  ", "    ", ""])
    kv = g.pick(['"{k}"\t\t"{v}"', '"{k}" "{v}"', '"{k}"\t"{v}"', '"{k}"    "{v}"'])
    nl = g.pick(["\n", "\n", "\r\n"])
    lines = []
    depth = [0]

    def emit(s):
        lines.append(ind * depth[0] + s)

    def sec(name, body):
        emit(f'"{name}"')
        emit("{")
        depth[0] += 1
        body()
        depth[0] -= 1
        emit("}")

    def kvl(k, v):
        emit(kv.format(k=k, v=v))

    variant = g.wpick([("normal", 10), ("no_app", 3), ("no_lo", 3), ("lo_section", 1), ("no_apps", 1), ("no_software", 1), ("empty_root", 1),
                       ("dupe_app", 2), ("case_key", 2), ("only_other", 1), ("dupe_apps", 1)])
    esc_lo = vesc(lo)

    def app_body(with_lo=True):
        def b():
            kvl("LastPlayed", "1700000000")
            if with_lo:
                if variant == "case_key":
                    kvl(g.pick(["launchoptions", "LAUNCHOPTIONS", "LaunchOption"]), esc_lo)
                elif variant == "lo_section":
                    sec("LaunchOptions", lambda: kvl("a", "b"))
                else:
                    kvl("LaunchOptions", esc_lo)
                    if variant == "dupe_app" or g.chance(0.1):
                        kvl("LaunchOptions", vesc(launch_options_text(g)))
        return b

    def apps_body():
        if variant == "no_app":
            sec("220", app_body())
        elif variant == "only_other":
            sec("220", app_body())
            sec("440", app_body())
        else:
            if g.chance(0.4):
                sec("220", app_body())
            sec("311210", app_body(variant != "no_lo"))
            if variant == "dupe_app":
                sec("311210", app_body())

    def steam_body():
        if variant == "no_apps":
            kvl("x", "y")
        else:
            sec("apps", apps_body)
            if variant == "dupe_apps":
                sec("apps", lambda: sec("311210", app_body()))
        if g.chance(0.2):
            kvl("AutoLoginUser", "x")

    def valve_body():
        sec("Steam", steam_body)

    def sw_body():
        sec("Valve", valve_body)

    def root_body():
        if variant == "empty_root":
            return
        if variant != "no_software":
            sec("Software", sw_body)
        else:
            kvl("a", "b")
        if g.chance(0.5):
            sec("friends", lambda: kvl("PersonaName", "x"))
    sec("UserLocalConfigStore", root_body)
    text = nl.join(lines) + g.pick([nl, "", nl])
    # decorations
    if g.chance(0.12):
        text = "// comment line" + nl + text
    if g.chance(0.1):
        text = text.replace('"apps"', '"apps" [$WIN32]', 1)
    if g.chance(0.08):
        text = text.replace('"311210"', '311210', 1)
    if g.chance(0.05):
        text = text.replace('"LaunchOptions"', 'LaunchOptions', 1)
    if g.chance(0.06):
        text = text.replace('"LastPlayed"', '"LastPlayed" // trailing', 1)
    raw = text.encode("utf-8", "surrogatepass")
    err = g.wpick([("ok", 18), ("bom", 2), ("invalid", 1), ("truncated", 1), ("garbage", 1), ("empty", 1), ("extra_close", 1)])
    if err == "bom":
        raw = b"\xef\xbb\xbf" + raw
    elif err == "invalid":
        i = g.rng.randint(0, len(raw))
        raw = raw[:i] + g.pick(INVALID_UTF8) + raw[i:]
    elif err == "truncated":
        raw = raw[: max(1, len(raw) * g.rng.randint(1, 9) // 10)]
    elif err == "garbage":
        raw = b"{ this is not vdf }\n"
    elif err == "empty":
        raw = b""
    elif err == "extra_close":
        raw += b"}\n"
    return raw


def p_steam_local(g):
    r = g.wpick([("variant", 10), ("missing", 1), ("dir", 1), ("ro", 1), ("clash", 1), ("symlink", 1), ("cfgdir_ro", 1)])
    p = "config/localconfig.vdf"
    if r == "variant":
        return [{"fx": "write", "root": "userdata", "path": p, "data": enc_data(vdf_localconfig(g))}]
    if r == "missing":
        return [{"fx": "rm", "root": "userdata", "path": p}]
    if r == "dir":
        return [{"fx": "rm", "root": "userdata", "path": p}, {"fx": "mkdir", "root": "userdata", "path": p}]
    if r == "ro":
        return [{"fx": "write", "root": "userdata", "path": p, "data": enc_data(vdf_localconfig(g))},
                {"fx": "chmod", "root": "userdata", "path": p, "mode": 0o444}]
    if r == "symlink":
        return [{"fx": "write", "root": "userdata", "path": "config/lc.real", "data": enc_data(vdf_localconfig(g))},
                {"fx": "symlink", "root": "userdata", "path": p, "to": "lc.real"}]
    if r == "cfgdir_ro":
        return [{"fx": "chmod", "root": "userdata", "path": "config", "mode": 0o555}]
    clash = ('"UserLocalConfigStore"\n{\n\t"Software"\n\t{\n\t\t"Valve"\n\t\t{\n\t\t\t"Steam"\n\t\t\t{\n\t\t\t\t"apps"\n\t\t\t\t{\n'
             '\t\t\t\t\t"311210"\n\t\t\t\t\t{\n\t\t\t\t\t\t"LaunchOptions"\t\t"-novid"\n\t\t\t\t\t}\n\t\t\t\t\t"311210"\t\t"string-now"\n'
             '\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n')
    return [{"fx": "write", "root": "userdata", "path": p, "data": clash, "tag": "vdf-string-section-clash"}]


def p_userdata(g):
    r = g.wpick([("multi", 5), ("nonnum", 2), ("filenum", 1), ("none", 1), ("second_root", 1)])
    lc = vdf_localconfig(g)
    if r == "multi":
        names = g.rng.sample(["7", "0", "99999999", "00123", "1", "23456789", "12345677"], g.rng.randint(1, 3))
        return [{"fx": "write", "root": "userdata_root", "path": f"{n}/config/localconfig.vdf", "data": enc_data(lc if g.chance(0.6) else vdf_localconfig(g))} for n in names]
    if r == "nonnum":
        return [{"fx": "write", "root": "userdata_root", "path": f"{g.pick(['abc', '12a', '-5', '١٢٣', ' 5'])}/config/localconfig.vdf", "data": enc_data(lc)}]
    if r == "filenum":
        return [{"fx": "write", "root": "userdata_root", "path": g.pick(["5", "0000"]), "data": "x"}]
    if r == "none":
        return [{"fx": "rm", "root": "userdata_root", "path": "12345678"}]
    return [{"fx": "write", "root": "home", "path": ".local/share/Steam/userdata/42/config/localconfig.vdf", "data": enc_data(lc)},
            {"fx": "mkdir", "root": "home", "path": ".local/share/Steam/steamapps"}]


def p_steam_config(g):
    r = g.wpick([("variant", 6), ("missing", 2), ("garbage", 1), ("bom", 1), ("invalid", 1), ("noroot", 1), ("dir", 1)])
    p = "config/config.vdf"
    base = STEAM_CONFIG_VDF
    if r == "variant":
        tool = g.pick(["proton_9", "BO3 Enhanced", "bo3 enhanced", "GE-Proton9-1", ""])
        extra = g.pick(["", '\t\t\t\t\t"311210"\n\t\t\t\t\t{\n\t\t\t\t\t\t"name"\t\t"BO3 Enhanced"\n\t\t\t\t\t\t"config"\t\t""\n\t\t\t\t\t\t"priority"\t\t"250"\n\t\t\t\t\t}\n',
                        '\t\t\t\t\t"311210"\t\t"oops"\n', '\t\t\t\t\t"311210"\n\t\t\t\t\t{\n\t\t\t\t\t}\n'])
        text = base.replace('"proton_9"', f'"{tool}"').replace('\t\t\t\t\t"0"\n', extra + '\t\t\t\t\t"0"\n', 1)
        return [{"fx": "write", "root": "steam", "path": p, "data": text}]
    if r == "missing":
        return [{"fx": "rm", "root": "steam", "path": p}]
    if r == "garbage":
        return [{"fx": "write", "root": "steam", "path": p, "data": "}{ not vdf"}]
    if r == "bom":
        return [{"fx": "write", "root": "steam", "path": p, "data": {"b64": base64.b64encode(b"\xef\xbb\xbf" + base.encode()).decode()}}]
    if r == "invalid":
        return [{"fx": "write", "root": "steam", "path": p, "data": {"b64": base64.b64encode(base.encode() + b"\xff\n").decode()}}]
    if r == "noroot":
        return [{"fx": "write", "root": "steam", "path": p, "data": '"Other"\n{\n}\n'}]
    return [{"fx": "rm", "root": "steam", "path": p}, {"fx": "mkdir", "root": "steam", "path": p}]


def p_libraries(g):
    r = g.wpick([("missing", 1), ("garbage", 1), ("bad_paths", 2), ("old_format", 1), ("dups", 1), ("only_root", 1), ("second_root_game", 2),
                 ("rm_game", 1), ("trailing", 1), ("relative", 1), ("empty", 1), ("bom", 1)])
    p = "steamapps/libraryfolders.vdf"

    def lib(paths, extra=""):
        s = '"libraryfolders"\n{\n'
        for i, pth in enumerate(paths):
            s += f'\t"{i}"\n\t{{\n\t\t"path"\t\t"{pth}"\n\t\t"label"\t\t""\n\t}}\n'
        return s + extra + "}\n"
    if r == "missing":
        return [{"fx": "rm", "root": "steam", "path": p}]
    if r == "garbage":
        return [{"fx": "write", "root": "steam", "path": p, "data": "{{{ nope"}]
    if r == "bad_paths":
        return [{"fx": "write", "root": "steam", "path": p, "data": lib(["/does/not/exist", "{steam_root}", "{lib2}", "/nope2"])}]
    if r == "old_format":
        return [{"fx": "write", "root": "steam", "path": p, "data": '"LibraryFolders"\n{\n\t"TimeNextStatsReport"\t\t"1"\n\t"1"\t\t"{lib2}"\n}\n'},
                ] if g.chance(0.5) else [{"fx": "write", "root": "steam", "path": p, "data": '"libraryfolders"\n{\n\t"contentstatsid"\t\t"123"\n\t"1"\t\t"{lib2}"\n}\n'}]
    if r == "dups":
        return [{"fx": "write", "root": "steam", "path": p, "data": lib(["{lib2}", "{lib2}", "{lib2}/", "{lib2}/../SteamLibrary", "{steam_root}"])}]
    if r == "only_root":
        return [{"fx": "write", "root": "steam", "path": p, "data": lib(["{steam_root}"])}]
    if r == "second_root_game":
        return [{"fx": "write", "root": "home", "path": ".local/share/Steam/steamapps/common/Call of Duty Black Ops III/BlackOps3.exe", "data": "MZ2"},
                {"fx": "write", "root": "home", "path": ".local/share/Steam/steamapps/common/Call of Duty Black Ops III/players/config.ini", "data": 'FOV = "99" // other\n'}] + \
               ([{"fx": "write", "root": "steam", "path": p, "data": lib(["{steam_root}"])}] if g.chance(0.5) else [])
    if r == "rm_game":
        return [{"fx": "rm", "root": "game", "path": "."}]
    if r == "trailing":
        return [{"fx": "write", "root": "steam", "path": p, "data": lib(["{steam_root}/", "{lib2}//"])}]
    if r == "relative":
        return [{"fx": "write", "root": "steam", "path": p, "data": lib(["../../../games/SteamLibrary", "games"])}]
    if r == "empty":
        return [{"fx": "write", "root": "steam", "path": p, "data": ""}]
    return [{"fx": "write", "root": "steam", "path": p, "data": {"b64": base64.b64encode(b"\xef\xbb\xbf" + lib(["{lib2}"]).encode()).decode()}}]


def p_workshop(g):
    r = g.wpick([("acf", 3), ("content", 3), ("rm", 1)])
    if r == "rm":
        return [{"fx": "rm", "root": "lib2", "path": "steamapps/workshop"}]
    if r == "content":
        wid = g.pick(["2994481309", "2942053577", "3667377161"])
        return [g.pick([{"fx": "write", "root": "lib2", "path": f"steamapps/workshop/content/311210/{wid}/x.bin", "data": "x"},
                        {"fx": "mkdir", "root": "lib2", "path": f"steamapps/workshop/content/311210/{wid}"},
                        {"fx": "rm", "root": "lib2", "path": f"steamapps/workshop/content/311210/{wid}"},
                        {"fx": "write", "root": "lib2", "path": f"steamapps/workshop/content/311210/{wid}", "data": "file"}])]
    ids = g.rng.sample(["2994481309", "2942053577", "3667377161", "1"], g.rng.randint(0, 3))
    body = "".join(f'\t\t"{i}"\n\t\t{{\n\t\t\t"size"\t\t"1"\n\t\t}}\n' for i in ids)
    text = g.pick(['"AppWorkshop"\n{\n\t"appid"\t\t"311210"\n\t"WorkshopItemsInstalled"\n\t{\n' + body + '\t}\n}\n',
                   '"AppWorkshop"\n{\n\t"WorkshopItemDetails"\n\t{\n' + body + '\t}\n\t"x"\t\t"' + (ids[0] if ids else "z") + '"\n}\n', "garbage {", "",
                   '"Other"\n{\n}\n'])
    return [{"fx": "write", "root": "lib2", "path": "steamapps/workshop/appworkshop_311210.acf", "data": text}]


# ---------- settings / appdata / log --------------------------------------------------------------------
def p_settings(g):
    r = g.wpick([("corrupt", 2), ("unknown", 2), ("wrong_types", 4), ("game_alt", 2), ("game_missing", 2), ("rc", 2), ("dir", 1), ("empty", 1),
                 ("nondict", 1), ("hashes", 3), ("bom", 1)])
    p = "electron-settings.json"
    if r == "corrupt":
        return [{"fx": "write", "root": "appdata", "path": p, "data": g.pick(['{"game_dir": ', "not json", "{'a': 1}", '{"a":1}}', '\x00', '{"game_dir":"x"} trailing'])}]
    if r == "unknown":
        return [{"fx": "write", "root": "appdata", "path": p, "data": json.dumps({"zzz": [1, 2], "release_channel": "beta", "x": {"y": None}})}]
    if r == "wrong_types":
        obj = {}
        for k in g.rng.sample(["game_dir", "release_channel", "enhanced_dump_source", "enhanced_exe_hashes", "enhanced_exe_hash"], g.rng.randint(1, 3)):
            obj[k] = g.pick([5, None, True, [], {}, ["a"], 1.5, "", " ", {"a": 1}])
        return [{"fx": "write", "root": "appdata", "path": p, "data": json.dumps(obj)}]
    if r == "game_alt":
        return [{"fx": "write", "root": "appdata", "path": p, "data": json.dumps({"game_dir": "{alt}"})}]
    if r == "game_missing":
        return [{"fx": "write", "root": "appdata", "path": p, "data": json.dumps({"game_dir": g.pick(["/nonexistent/dir", "{game_dir}/players", "{game_dir}/", "", "{common}"])})}]
    if r == "rc":
        return [{"fx": "write", "root": "appdata", "path": p, "data": json.dumps({"release_channel": g.pick(["beta", "BETA", " beta ", "stable", "nightly", 3, None, "Beta\n"])})}]
    if r == "dir":
        return [{"fx": "mkdir", "root": "appdata", "path": p}]
    if r == "empty":
        return [{"fx": "write", "root": "appdata", "path": p, "data": ""}]
    if r == "nondict":
        return [{"fx": "write", "root": "appdata", "path": p, "data": g.pick(["[]", "5", "null", '"x"', "true"])}]
    if r == "bom":
        return [{"fx": "write", "root": "appdata", "path": p, "data": {"b64": base64.b64encode(b'\xef\xbb\xbf{"release_channel":"beta"}').decode()}}]
    return [{"fx": "named", "name": "seed_enhanced_hash"}] + [{"fx": "write", "root": "appdata", "path": "electron-settings.json",
            "data": json.dumps({"enhanced_exe_hashes": g.pick([{"{game_dir}": "ABCDEF"}, {"{game_dir}": ["x", None, ""]}, {"{game_dir}": 5}, [], "s"]),
                                "enhanced_exe_hash": g.pick(["abc", "", 1])})}]


def p_log(g):
    r = g.wpick([("text", 4), ("big", 1), ("crlf", 1), ("invalid", 2), ("dir", 1), ("ro", 1), ("appdata_ro", 1), ("nonl", 1), ("empty", 1), ("backups", 2), ("symlink", 1)])
    p = "PatchOpsIII.log"
    line = "2025-01-02 03:04:05 - Info: old entry"
    if r == "text":
        return [{"fx": "write", "root": "appdata", "path": p, "data": "\n".join(line + str(i) for i in range(g.rng.randint(1, 20))) + "\n"}]
    if r == "big":
        return [{"fx": "write", "root": "appdata", "path": p, "data": {"repeat": [line + "\n", g.rng.randint(2000, 8000)]}}]
    if r == "crlf":
        return [{"fx": "write", "root": "appdata", "path": p, "data": (line + "\r\n") * 3}]
    if r == "invalid":
        return [{"fx": "write", "root": "appdata", "path": p, "data": {"b64": base64.b64encode((line + "\n").encode() + g.pick(INVALID_UTF8) + b"\n").decode()}}]
    if r == "dir":
        return [{"fx": "mkdir", "root": "appdata", "path": p}]
    if r == "ro":
        return [{"fx": "write", "root": "appdata", "path": p, "data": line + "\n"}, {"fx": "chmod", "root": "appdata", "path": p, "mode": 0o444}]
    if r == "appdata_ro":
        return [{"fx": "mkdir", "root": "appdata", "path": "."}, {"fx": "chmod", "root": "appdata", "path": ".", "mode": 0o555}]
    if r == "nonl":
        return [{"fx": "write", "root": "appdata", "path": p, "data": line + "\n  trailing   \n\n\n"}]
    if r == "empty":
        return [{"fx": "write", "root": "appdata", "path": p, "data": ""}]
    if r == "symlink":
        return [{"fx": "write", "root": "appdata", "path": "real.log", "data": line + "\n"}, {"fx": "symlink", "root": "appdata", "path": p, "to": "real.log"}]
    return [{"fx": "write", "root": "appdata", "path": "backups/localconfig_backup.vdf", "data": g.pick(["old", "", LOCALCONFIG_VDF])}]


def p_mod(g):
    r = g.wpick([("junk", 3), ("ro", 1), ("asfile", 1), ("symlink_out", 2), ("symlink_file", 1), ("nodir_perm", 1), ("deep", 1)])
    if r == "junk":
        return [{"fx": "named", "name": "place_mod_junk"}]
    if r == "ro":
        return [{"fx": "named", "name": "place_mod_junk"}, {"fx": "chmod", "root": "mod", "path": "sub/nested", "mode": 0o555}]
    if r == "asfile":
        return [{"fx": "write", "root": "appdata", "path": "BO3 Mod Files", "data": "file"}]
    if r == "symlink_out":
        return [{"fx": "symlink", "root": "mod", "path": "linkdir", "to": "{game_dir}/video"}]
    if r == "symlink_file":
        return [{"fx": "symlink", "root": "mod", "path": "linkfile", "to": "{game_dir}/players/config.ini"}]
    if r == "nodir_perm":
        return [{"fx": "write", "root": "mod", "path": "f.bin", "data": "x"}, {"fx": "chmod", "root": "mod", "path": ".", "mode": 0o555}]
    return [{"fx": "write", "root": "mod", "path": "a/b/c/d/e/f.bin", "data": "x"}, {"fx": "write", "root": "mod", "path": "T7Patch.zip", "data": "z"}]


def p_dumps(g):
    members_valid = [{"name": "BlackOps3.exe", "data": "MZ"}, {"name": "MicrosoftGame.config", "data": "<Game/>"}]
    r = g.wpick([("zip_ok", 3), ("zip_dump", 3), ("zip_nested", 2), ("zip_missing", 2), ("zip_empty", 1), ("zip_corrupt", 2), ("zip_odd", 3),
                 ("dir_ok", 2), ("dir_missing", 2), ("dir_nested", 2), ("dir_case", 1), ("dir_file", 1)])

    def zp(members, corrupt=None, name="d.zip"):
        return {"fx": "zip", "root": "dumps", "path": name, "members": members, "corrupt": corrupt}
    if r == "zip_ok":
        return [zp(members_valid)]
    if r == "zip_dump":
        return [zp([{"name": "DUMP/" + m["name"], "data": m["data"]} for m in members_valid])]
    if r == "zip_nested":
        return [zp([{"name": g.pick(["a/DUMP/", "x/", "DUMP/DUMP/", "dump/", "Dump/"]) + m["name"], "data": m["data"]} for m in members_valid])]
    if r == "zip_missing":
        return [zp(g.rng.sample(members_valid, 1) + ([{"name": "readme.txt", "data": "x"}] if g.chance(0.5) else []))]
    if r == "zip_empty":
        return [zp([])]
    if r == "zip_corrupt":
        return [zp(members_valid, g.pick(["truncate", "garbage", "notzip"]))]
    if r == "zip_odd":
        nm = g.pick(["../BlackOps3.exe", "/abs/BlackOps3.exe", "DUMP\\BlackOps3.exe", "é/BlackOps3.exe", "DUMP/./BlackOps3.exe",
                     "BlackOps3.exe/", "blackops3.exe", "BlackOps3.exe ", "DUMP//BlackOps3.exe", "a\x00b"])
        return [zp([{"name": nm, "data": "x"}, {"name": g.pick(["MicrosoftGame.config", "DUMP/MicrosoftGame.config", "../MicrosoftGame.config"]), "data": "c"}]
                   + ([{"name": "dirmember/", "dir": True}] if g.chance(0.4) else []))]
    if r == "dir_ok":
        return [{"fx": "write", "root": "dumps", "path": "d1/BlackOps3.exe", "data": "MZ"}, {"fx": "write", "root": "dumps", "path": "d1/MicrosoftGame.config", "data": "<G/>"}]
    if r == "dir_missing":
        return [{"fx": "write", "root": "dumps", "path": "d1/BlackOps3.exe", "data": "MZ"}]
    if r == "dir_nested":
        return [{"fx": "write", "root": "dumps", "path": g.pick(["d1/a/DUMP/BlackOps3.exe", "d1/DUMP/sub/BlackOps3.exe"]), "data": "MZ"},
                {"fx": "write", "root": "dumps", "path": "d1/DUMP/MicrosoftGame.config", "data": "<G/>"}]
    if r == "dir_case":
        return [{"fx": "write", "root": "dumps", "path": "d1/blackops3.exe", "data": "MZ"}, {"fx": "write", "root": "dumps", "path": "d1/microsoftgame.config", "data": "x"}]
    return [{"fx": "write", "root": "dumps", "path": "d1", "data": "a file"}]


def p_browse(g):
    r = g.wpick([("many", 1), ("unicode", 2), ("symlinks", 2), ("noperm", 1), ("dotfiles", 1), ("games", 2), ("long", 1), ("spaces", 1)])
    if r == "many":
        return [{"fx": "many", "root": "home", "path": "Downloads/many", "count": 330, "prefix": "d"}]
    if r == "unicode":
        return [{"fx": "mkdir", "root": "home", "path": "Downloads/" + n} for n in g.rng.sample(["Ünï", "日本", "🙂 dir", "a\u0301", "ZZ", "aa", "Bb", "é"], 3)]
    if r == "symlinks":
        return [{"fx": "symlink", "root": "home", "path": "Downloads/linkgame", "to": "{game_dir}"}, {"fx": "symlink", "root": "home", "path": "Downloads/linkfile", "to": "{game_dir}/BlackOps3.exe"},
                {"fx": "symlink", "root": "home", "path": "Downloads/linkloop", "to": "linkloop"}, {"fx": "symlink", "root": "home", "path": "Downloads/linkup", "to": ".."}]
    if r == "noperm":
        return [{"fx": "mkdir", "root": "home", "path": "Downloads/locked"}, {"fx": "chmod", "root": "home", "path": "Downloads/locked", "mode": 0}]
    if r == "dotfiles":
        return [{"fx": "mkdir", "root": "home", "path": "Downloads/.hidden"}, {"fx": "write", "root": "home", "path": "Downloads/.f", "data": "x"}]
    if r == "games":
        return [{"fx": "write", "root": "home", "path": "Downloads/Call of Duty Black Ops III/BlackOpsIII.exe", "data": "MZ"},
                {"fx": "write", "root": "home", "path": "Downloads/other/BlackOps3.exe", "data": "MZ"}]
    if r == "long":
        return [{"fx": "mkdir", "root": "home", "path": "Downloads/" + "/".join(["n" * 100] * 6)}]
    return [{"fx": "mkdir", "root": "home", "path": "Downloads/with space/and  two"}, {"fx": "mkdir", "root": "home", "path": "Desktop/x y"}]


def p_named(g):
    return [{"fx": "named", "name": g.pick(["install_fake_t7", "install_fake_dxvk", "install_fake_enhanced", "place_mod_junk", "place_fake_depot",
                                           "seed_enhanced_hash", "unseed_enhanced_hash", "remove_video_dir", "restore_video_dir", "remove_localconfig",
                                           "restore_localconfig"])}]


def p_desktop(g):
    r = g.pick(["rm", "file", "link", "rmhome"])
    if r == "rm":
        return [{"fx": "rm", "root": "home", "path": "Desktop"}, {"fx": "rm", "root": "home", "path": "Downloads"}]
    if r == "file":
        return [{"fx": "rm", "root": "home", "path": "Desktop"}, {"fx": "write", "root": "home", "path": "Desktop", "data": "x"}]
    if r == "link":
        return [{"fx": "rm", "root": "home", "path": "Downloads"}, {"fx": "symlink", "root": "home", "path": "Downloads", "to": "{game_dir}"}]
    return [{"fx": "rm", "root": "home", "path": ".steam"}]


# perturbation registry: (name, fn, weight, mid_safe)
PERTURBATIONS = [
    ("config_ini", p_config_ini, 14, False), ("exe", p_exe, 5, False), ("video", p_video, 6, True), ("d3d", p_d3d, 5, True),
    ("lpc", p_lpc, 3, True), ("dxvk", p_dxvk, 6, True), ("t7", p_t7, 7, True), ("steam_local", p_steam_local, 5, False),
    ("userdata", p_userdata, 3, False), ("steam_config", p_steam_config, 3, False), ("libraries", p_libraries, 3, False),
    ("workshop", p_workshop, 2, False), ("settings", p_settings, 5, False), ("log", p_log, 3, False), ("mod", p_mod, 3, True),
    ("dumps", p_dumps, 3, True), ("browse", p_browse, 3, True), ("named", p_named, 3, False), ("home", p_desktop, 1, False),
]
MID = [("config_mid", p_config_mid, 4), ("t7_mid", p_t7_mid, 2)] + [(n, f, w) for n, f, w, m in PERTURBATIONS if m]


# =====================================================================================================
# ops
# =====================================================================================================
def junk(g, avoid=()):
    return g.pick([None, 0, 1, 2, -1, 1.5, "", "x", "true", "1", [], {}, [1], {"a": 1}, True, False, "maybe", 2 ** 40])


def val_pool(g):
    k = g.wpick([("str", 6), ("int", 4), ("float", 3), ("bool", 1), ("raw", 5), ("esc", 4), ("uni", 2)])
    if k == "str":
        return g.pick(["", "1", "0", "abc", "a b", "2560x1440", "On", "  padded  ", '"quoted"', "a\"b", "x//y", "tab\there", "line\nbreak", "é", "007", "1e3", "-0", "nan"] +
                      ["x" * g.rng.randint(10, 100000)] * 1)
    if k == "int":
        return g.pick([0, 1, -1, 240, 2 ** 31, 2 ** 53 + 1, 2 ** 63, 2 ** 64, -2 ** 63, 10 ** 30, 99999999999])
    if k == "float":
        return g.pick([0.0, 1.0, -0.0, 0.1, 1e3 + 0.5, 3.14159, 1e16, 1e-7, 1.5e300, 5e-324, 0.30000000000000004, 123456789.123456789, 1e22, 1e21, 100.0, 0.75])
    if k == "bool":
        return g.pick([True, False])
    if k == "raw":
        return {"$raw": g.pick(["1e3", "1E3", "-0", "-0.0", "0.1", "1e400", "-1e400", "1.0e2", "100e-2", "0.1e1", "123456789012345678901234567890", "1.7976931348623157e308",
                                "2.5E-5", "0e0", "1e+2", "12345678901234567890.5", "0.30000000000000004", "9007199254740993", "-9223372036854775809", "1e15", "1e16", "1.5e-5", "0.0001", "0.00001"])}
    if k == "esc":
        return g.pick(["\\1", "\\g<0>", "\\g<1>", "\\q", "\\", "\\\\", "\\n", "\\t", "a\\nb", "\\x41", "\\u00e9", "\\0", "\\10", "\\g<name>", "\\N", "\\Z", "\\a", "\\f",
                       "\\g<0", "\\9", "$1", "&", "\\Ucase", "100\\%", "a\\\\n", "\\r\\n", "\\101", "\\08", "\\g<-1>"])
    return g.pick(UNICODE_SAMPLES) + g.pick(UNICODE_SAMPLES)


def key_pool(g):
    k = g.wpick([("ui", 8), ("fixture", 3), ("meta", 3), ("new", 2), ("prefix", 3), ("uni", 1), ("odd", 3), ("long", 1), ("case", 2)])
    if k == "ui":
        return g.pick(UI_KEYS)
    if k == "fixture":
        return g.pick(["DrawFPS", "Vsync", "VsyncMode", "FOV", "MaxFPS", "SerializeRender", "AATechnique", "Gamma", "WindowSize", "VideoMemory"])
    if k == "meta":
        return g.pick(META_KEYS)
    if k == "new":
        return "Bench" + g.word()
    if k == "prefix":
        return g.pick(["Vsync", "VsyncM", "Vsyn", "Max", "FOV", "FO", "MaxFPSX", "Draw"])
    if k == "uni":
        return g.pick(UNICODE_SAMPLES)
    if k == "odd":
        return g.pick([" Vsync", "Vsync ", "Vs ync", "a\nb", "a=b", '"q"', "\t", " ", "//x", "a\\1", "\\g<0>", "  ", "[", "(", "*", "+", "?", "\\", "{"])
    if k == "long":
        return "K" * g.rng.randint(500, 5000)
    return g.pick(UI_KEYS).lower() if g.chance(0.5) else g.pick(UI_KEYS).upper()


def a_config(g):
    a = {"key": key_pool(g), "value": val_pool(g)}
    if g.chance(0.4):
        a["comment"] = g.pick(["", "c", "0 or 1", "\\1", "\\g<0>", "with // slashes", "multi\nline", "é", "x" * 2000, "\\q", " ", "back\\slash", "\r", "\\n"])
    return a


def a_vram(g):
    t = g.wpick([("ok", 6), ("edge", 4), ("bad", 4), ("omit", 2)])
    a = {"limited": g.pick([True, False])}
    if t == "ok":
        a["target"] = g.rng.randint(75, 100)
    elif t == "edge":
        a["target"] = g.pick([75, 100, 74, 101, 0, -1, 29, 80, 99])
    elif t == "bad":
        a["target"] = g.pick([2 ** 31, 2 ** 63, "80", 80.0, 80.5, True, None, "x", [], 1e2, {"$raw": "80.0"}, {"$raw": "8e1"}, {"$raw": "1e2"}, {"$raw": "75.0000001"}])
    return a


def a_preset(g):
    return {"name": g.pick(PRESET_NAMES + PRESET_NAMES + ["Nope", "", "smooth preset", " Smooth Preset", "Smooth Preset ", "Smooth Preset\n", "Низкий", "__proto__", "ReduceStutter",
                                                          "a" * 3000])}


def a_enabled(g):
    return {"enabled": g.pick([True, False])}


def a_dxvk(g):
    a = {}
    if g.chance(0.8):
        a["enableAsync"] = g.pick([True, False])
    if g.chance(0.8):
        a["gplAsyncCache"] = g.pick([True, False])
    if g.chance(0.8):
        a["numCompilerThreads"] = g.pick([0, 1, 64, 65, -1, 32, 4])
    if g.chance(0.8):
        a["maxFrameRate"] = g.pick([0, 1, 360, 361, -1, 144, 60])
    if g.chance(0.8):
        a["maxFrameLatency"] = g.pick([0, 1, 16, 17, -1, 3, 2])
    if g.chance(0.8):
        a["tearFree"] = g.pick(["Auto", "True", "False", "auto", "true", "TRUE", "", " ", "None", "Auto\n", "é", "x" * 500, "Auto ", "False "])
    if g.chance(0.8):
        a["hudEnabled"] = g.pick([True, False])
    return a


GT_SAMPLES = ["Bench", " Bench ", "", " ", "\t", "\n", "a" * 20, "a" * 21, "a" * 19, "é" * 20, "é" * 21, "🙂" * 10, "🙂" * 11, "🙂" * 20, "🙂" * 21, "日本語" * 7, "日本語" * 6 + "ab",
              "^1Name", "^^", "a b", "a\nb", "a\rb", "a=b", "x\x0c", "\x1cname\x1c", "\u00a0name\u00a0", "\u3000", "\u200b", "a\u0301" * 20, "a\u0301" * 21, "AAAAAAAAAAAAAAAAAAAA ",
              " AAAAAAAAAAAAAAAAAAAA", "AAAAAAAAAAAAAAAAAAAAA", "∑" * 21, "\U0001d11e" * 10 + "abcdefghij", "\U0001d11e" * 11, "A" * 100]


def a_t7(g):
    a = {}
    t = g.wpick([("gt", 6), ("pw", 3), ("fo", 3), ("all", 3), ("color", 2), ("none", 1)])
    if t in ("gt", "all"):
        a["gamertag"] = g.pick(GT_SAMPLES)
    if t in ("pw", "all"):
        a["networkPassword"] = g.pick(["", " ", "pw", "  pw  ", "p=w", "é", "a\nb", "x" * 300, "\x1cpw", "🙂", "pw\r\n"])
    if t in ("fo", "all"):
        a["friendsOnly"] = g.pick([True, False, None, 0, 1, "true"])
    if t in ("color", "gt", "all") and g.chance(0.6):
        a["colorCode"] = g.pick(["", "^1", "^7", "^", "^12", "x", "^1^2", "\n", " ", "é", "^" * 30, "^\n"])
    if g.chance(0.15):
        a[g.pick(["gamertag", "networkPassword", "friendsOnly"])] = None
    return a


def a_empty(g):
    return {}


RC = ["stable", "beta", "BETA", "Beta", " beta ", "\tstable\n", "", "alpha", "stable\n", "ß", "İ", "STABLE", "\x1cbeta", "\u00a0beta", "\u3000stable\u3000",
      "beta\u200b", "bеta", "\x0bbeta", "\x85beta", "beta\x00", "ſtable", "Ｂeta", "betA", "b e t a", "null", "𝔰table"]


def a_rc(g):
    return {"channel": g.pick(RC)}


def path_pool(g, gd=False):
    k = g.wpick([("game", 5), ("trailing", 3), ("dotdot", 3), ("symlink", 2), ("missing", 2), ("file", 2), ("uni", 1), ("spaces", 1), ("long", 1), ("dl", 2), ("misc", 2), ("alt", 2), ("slashes", 2)])
    if k == "game":
        return g.pick(["{game_dir}", "{alt}", "{common}", "{lib2}", "{steam_root}", "{home}", "{sb}"])
    if k == "trailing":
        return g.pick(["{game_dir}/", "{game_dir}//", "{alt}/", "{common}/"])
    if k == "dotdot":
        return g.pick(["{game_dir}/..", "{game_dir}/../Call of Duty Black Ops III", "{game_dir}/./", "{game_dir}/players/..", "{common}/../..", "{home}/../.."])
    if k == "symlink":
        return g.pick(["{home}/Downloads/linkgame", "{home}/Downloads/linkfile", "{home}/Downloads/linkloop", "{home}/Downloads/linkup"])
    if k == "missing":
        return g.pick(["{game_dir}/nope", "{sb}/does/not/exist", "/nonexistent", "{common}/Call Of Duty Black Ops III"])
    if k == "file":
        return g.pick(["{game_dir}/BlackOps3.exe", "{game_dir}/players/config.ini", "{sb}/fakebin/steam"])
    if k == "uni":
        return g.pick(["{home}/Downloads/Ünï", "{home}/Downloads/日本", "{home}/Downloads/🙂 dir", "{home}/Downloads/é"])
    if k == "spaces":
        return g.pick(["{home}/Downloads/with space/and  two", " {game_dir}", "{game_dir} ", "{game_dir}\n"])
    if k == "long":
        return "{home}/Downloads/" + "/".join(["n" * 100] * 6) if g.chance(0.5) else "{sb}/" + "x" * g.rng.randint(250, 5000)
    if k == "dl":
        return g.pick(["{home}/Downloads", "{home}/Desktop", "{home}/Downloads/many", "{home}/Downloads/locked", "{home}/.steam", "{home}/Downloads/.hidden", "{home}/.local/share"])
    if k == "alt":
        return g.pick(["{home}/Downloads/Call of Duty Black Ops III", "{home}/Downloads/other", "{home}/.local/share/Steam/steamapps/common/Call of Duty Black Ops III"])
    if k == "slashes":
        return g.pick(["/", "//", "{game_dir}///players", "/proc/self/..", "{home}/./Downloads/../Desktop", "~", "{game_dir}/\\"])
    return g.pick(["", " ", "%00", "?", "#frag", "a?b=c"])


def a_browse(g):
    a = {}
    if g.chance(0.85):
        a["path"] = path_pool(g)
    if g.chance(0.1):
        a["extra"] = "x"
    return a


def a_gamedir(g):
    return {"path": path_pool(g, True)}


def a_dump(g):
    return {"dumpSource": g.pick(["{valid_dump}", "{invalid_dump}", "{valid_dump}/DUMP", "{valid_dump}/", "{dumps}/d.zip", "{dumps}/d1", "{dumps}/d1/", "{dumps}/missing",
                                  "", " ", "\t", "  {valid_dump}  ", "{valid_dump}\n", "{dumps}/d1/BlackOps3.exe", "{sb}", "{game_dir}", "{dumps}/../dumps/d.zip", "{dumps}/D.ZIP", "é", "x" * 400])}


def a_launch_opts(g):
    a = {"options": g.pick(["", "", "+set fs_game offlinemp", "+set fs_game 2994481309", "+set fs_game 2942053577", "+set fs_game 3667377161",
                            "+set fs_game x", " +set fs_game offlinemp", "+set fs_game offlinemp ", "+SET fs_game offlinemp", "+set  fs_game offlinemp", "-novid", "%command%",
                            "+set fs_game offlinemp\n", "+set fs_game 2994481309 +set fs_game 2942053577"])}
    if g.chance(0.7):
        a["preserve_fs_game"] = g.pick([True, False])
    return a


def a_workshop(g):
    return {"profileId": g.pick(["all_around", "ultimate", "reforged", "default", "offline", "", "ALL_AROUND", "x", " all_around"])}


OP_TABLE = {
    # op: (fast weight, steam weight, arg generator)
    "status": (6, 1, a_empty), "health": (1, 0, a_empty),
    "config": (22, 3, a_config), "vram-target": (5, 1, a_vram), "presets/apply": (4, 1, a_preset), "config-readonly": (3, 1, a_enabled),
    "intro-skip": (4, 1, a_enabled), "all-intros-skip": (4, 1, a_enabled), "d3dcompiler": (4, 1, a_enabled),
    "dxvk-config": (5, 1, a_dxvk), "dxvk-uninstall": (2, 0.5, a_empty),
    "t7-config": (8, 1, a_t7), "t7-uninstall": (2, 0.5, a_empty), "mod-files/clear": (2, 0.5, a_empty),
    "logs/clear": (2, 0.5, a_empty), "logs/payload": (2, 0.5, a_empty),
    "browse": (4, 0.5, a_browse), "game-directory": (4, 1, a_gamedir), "release-channel": (3, 0.5, a_rc),
    "enhanced-validate": (3, 0.5, a_dump), "exe-swap/compatible": (1.5, 0.5, a_empty), "exe-swap/compatible-depot": (1.5, 0.5, a_empty),
    "exe-swap/current": (1.5, 0.5, a_empty), "exe-swap/enhanced": (1.5, 0.5, a_empty), "launch": (1, 1, a_empty),
    # Steam-dependent: legacy sleeps ~7 s per cycle
    "launch-options": (0.2, 6, a_launch_opts), "workshop-install": (0.2, 3, a_workshop), "enhanced-uninstall": (0.1, 2, a_empty),
    "reset-stock": (0.15, 3, a_empty),
}

SCHEMA = {  # op -> [(field, kind, required)]
    "config": [("key", "str", True), ("value", "scalar", True), ("comment", "str", False)],
    "vram-target": [("limited", "bool", True), ("target", "int", False)],
    "presets/apply": [("name", "str", True)], "config-readonly": [("enabled", "bool", True)], "intro-skip": [("enabled", "bool", True)],
    "all-intros-skip": [("enabled", "bool", True)], "d3dcompiler": [("enabled", "bool", True)],
    "dxvk-config": [("enableAsync", "bool", False), ("gplAsyncCache", "bool", False), ("numCompilerThreads", "int", False), ("maxFrameRate", "int", False),
                    ("maxFrameLatency", "int", False), ("tearFree", "str", False), ("hudEnabled", "bool", False)],
    "t7-config": [("gamertag", "str", False), ("colorCode", "str", False), ("networkPassword", "str", False), ("friendsOnly", "bool", False)],
    "release-channel": [("channel", "str", True)], "game-directory": [("path", "str", True)], "enhanced-validate": [("dumpSource", "str", True)],
    "launch-options": [("options", "str", False), ("preserve_fs_game", "bool", False)], "workshop-install": [("profileId", "str", True)],
    "browse": [("path", "str", False)],
}
JUNK = {"str": [5, 0, 1.5, True, False, None, [], ["a"], {}, {"a": "b"}, "", 10 ** 20, {"$raw": "1e3"}],
        "bool": [0, 1, 2, -1, "true", "false", "True", "yes", "no", "on", "off", "1", "0", "t", "f", "y", "n", "maybe", "", None, [], {}, 1.0, 0.0, 0.5, "TRUE", " true", {"$raw": "1.0"}, {"$raw": "1e0"}],
        "int": ["80", " 80 ", "80.0", 80.0, 80.5, True, False, None, [], {}, "x", "", "0x50", "1e2", "8_0", 2 ** 70, {"$raw": "80.00"}, {"$raw": "8e1"}, -0.0, "٨٠"],
        "scalar": [None, [], {}, [1], {"a": 1}, {"$raw": "[]"}]}


def mutate_args(g, op, args):
    spec = SCHEMA.get(op)
    a = dict(args)
    m = g.wpick([("junk", 6), ("drop", 4), ("extra", 2), ("case", 1), ("top", 1), ("null_body", 1)])
    if op in GET_OPS and m in ("junk", "top", "null_body", "case"):
        m = "extra"
    if m == "top":
        return {"$rawargs": g.pick(["null", "[]", "[1]", "\"x\"", "5", "true", "{\"a\":"[:5] + "1}"])}
    if m == "null_body":
        return {"$rawargs": "null"}
    if m == "extra":
        if op in GET_OPS:   # GET args become query-string parameters: only strings make sense there
            a[g.pick(["extra", "unknown", "options", "enabled", "key"])] = g.pick(["1", "x", ""])
        else:
            a[g.pick(["extra", "unknown", "options", "enabled", "path", "key"])] = g.pick([1, "x", None, [], {"a": 1}])
        return a
    if not spec:
        a["extra"] = 1
        return a
    f, kind, _req = g.pick(spec)
    if m == "drop":
        a.pop(f, None)
    elif m == "case":
        if f in a:
            a[f.upper() if g.chance(0.5) else f.lower()] = a.pop(f)
    else:
        a[f] = g.pick(JUNK[kind])
    return a


# =====================================================================================================
# Known-divergence feature detectors. Used (a) by --avoid to steer generation away from already-triaged root causes so that
# a campaign can look deeper, and (b) to label divergences in reports. They look only at the case JSON.
# =====================================================================================================
def _walk(o):
    if isinstance(o, dict):
        for k, v in o.items():
            yield k
            yield from _walk(v)
    elif isinstance(o, list):
        for v in o:
            yield from _walk(v)
    else:
        yield o


def _strings(case):
    out = []
    for st in case["steps"]:
        for x in _walk(st):
            if isinstance(x, str):
                out.append(x)
            elif isinstance(x, dict):
                pass
    return out


def _raw_nums(case):
    out = []
    for st in case["steps"]:
        for x in _walk(st.get("args", {})):
            if isinstance(x, (int, float)) and not isinstance(x, bool):
                out.append(repr(x))
            if isinstance(x, str) and re.fullmatch(r"-?[0-9][0-9.eE+-]*", x):
                out.append("S" + x)
        a = st.get("args")
        if isinstance(a, dict):
            for v in a.values():
                if isinstance(v, dict) and "$raw" in v:
                    out.append(v["$raw"])
    return out


def _cfg_value_nums(case):
    out = []
    for st in case["steps"]:
        if st.get("op") == "config" and isinstance(st.get("args"), dict):
            v = st["args"].get("value")
            if isinstance(v, dict) and "$raw" in v:
                out.append(v["$raw"])
            elif isinstance(v, (int, float)) and not isinstance(v, bool):
                out.append(repr(v))
    return out


def _fx_data(case, path_suffix):
    for st in case["steps"]:
        if st.get("fx") == "write" and str(st.get("path", "")).endswith(path_suffix):
            d = st.get("data")
            yield d if isinstance(d, str) else (base64.b64decode(d["b64"]).decode("latin-1") if isinstance(d, dict) and "b64" in d else "")


KNOWN = {
    "bigint": ("config value integer beyond i64 / 2**53 is formatted as float by the port",
               lambda c: any(re.fullmatch(r"-?\d{16,}", n) for n in _cfg_value_nums(c))),
    "negzero": ("JSON -0 (int) is written as '-0.0' instead of '0'", lambda c: any(n in ("-0", "-0.0") for n in _cfg_value_nums(c))),
    "floatrepr": ("config float value formatting (shortest repr / exponent forms)",
                  lambda c: any(re.search(r"[.eE]", n) and n not in ("-0.0",) and not re.fullmatch(r"-?\d{1,3}\.\d{1,3}", n) for n in _cfg_value_nums(c))),
    "json_overflow": ("1e400 JSON literal: the port rejects the whole request line", lambda c: any("e400" in n or "E400" in n for n in _raw_nums(c))),
    "ctrl_ws": ("U+001C..U+001F whitespace (Python strip/\\s) is not whitespace for Rust", lambda c: any(re.search("[\x1c-\x1f]", t) for t in _strings(c))),
    "splitlines": ("dxvk.conf with Python-only line separators (\\x0b \\x0c \\x1c-\\x1e \\x85 U+2028/9)",
                   lambda c: any(re.search("[\x0b\x0c\x1c-\x1e\x85\u2028\u2029]", t) for t in _fx_data(c, "dxvk.conf")) or any(re.search("[\x0b\x0c\x1c-\x1e\x85\u2028\u2029]", t) for t in _fx_data(c, "dxvk.real"))),
    "lone_cr": ("lone CR line endings in t7patch.conf / log (Python universal newlines)",
                lambda c: any(re.search(r"\r(?!\n)", t) for t in list(_fx_data(c, "t7patch.conf")) + list(_fx_data(c, "t7real.conf")) + list(_fx_data(c, "PatchOpsIII.log")))),
    "inplace": ("config/t7/dxvk written through symlink / hardlink / read-only parent dir (port replaces the file)",
                lambda c: any((st.get("fx") in ("symlink", "hardlink") and any(k in st.get("path", "") for k in ("config.", "t7patch", "dxvk.conf"))) or
                              (st.get("fx") == "chmod" and st.get("path") in ("players", ".") and st.get("root") == "game") for st in c["steps"])),
    "log_utf8": ("invalid UTF-8 / CRLF in a pre-existing log file", lambda c: any(st.get("fx") == "write" and st.get("path") == "PatchOpsIII.log" and (isinstance(st.get("data"), dict) or "\r" in str(st.get("data"))) for st in c["steps"])),
    "settings_types": ("wrong JSON types in electron-settings.json enhanced_dump_source", lambda c: any(st.get("fx") == "write" and st.get("path") == "electron-settings.json" and "enhanced_dump_source" in str(st.get("data")) for st in c["steps"])),
    "null_body": ("null / non-object request args", lambda c: any(isinstance(st.get("args"), dict) and "$rawargs" in st["args"] for st in c["steps"] if "op" in st)),
    "video_file": ("video path is a file / symlink", lambda c: any(st.get("fx") in ("write", "symlink") and st.get("path") == "video" for st in c["steps"])),
    "modclear": ("mod-files/clear with symlinks or unwritable subdirs", lambda c: any(st.get("root") == "mod" and st.get("fx") in ("symlink", "chmod") for st in c["steps"])),
    "nosteamuser": ("missing Steam userdata (log-warning multiplicity)", lambda c: any(st.get("fx") == "rm" and st.get("path") in ("12345678", ".steam") for st in c["steps"])),
    "legacy500_inf": ("legacy answers HTTP 500 when a numeric config key holds inf/1e999 (int(float('inf')) OverflowError); the port tolerates it",
                      lambda c: any(re.search(r"(?i)inf|nan|e\+?\d{3,}", t) for t in _fx_data(c, "config.ini")) or
                      any(st.get("op") == "config" and isinstance(st.get("args"), dict) and isinstance(st["args"].get("value"), str) and re.search(r"(?i)inf|nan|e\+?\d{3,}", st["args"]["value"]) for st in c["steps"])),
    "t7_nonfile": ("t7patch.conf that is a directory / invalid UTF-8: legacy status answers HTTP 500, the port tolerates it",
                   lambda c: any(st.get("path") == "t7patch.conf" and (st.get("fx") == "mkdir" or (st.get("fx") == "write" and isinstance(st.get("data"), dict))) for st in c["steps"])
                   ),
    "t7_multiname": ("several playername= lines in t7patch.conf (port keeps a stale colorCode)",
                     lambda c: any(len(re.findall(r"(?mi)^\s*playername", d)) > 1 for d in _fx_data(c, "t7patch.conf"))),
    "dxvk_dir": ("dxvk.conf is a directory (port logs no read warning)", lambda c: any(st.get("fx") == "mkdir" and st.get("path") == "dxvk.conf" for st in c["steps"])),
    "browse_root": ("browse of / makes the legacy answer HTTP 500", lambda c: any(st.get("op") == "browse" and isinstance(st.get("args"), dict) and st["args"].get("path") in ("/", "//", "/proc/self/..") for st in c["steps"])),
    "nonprint": ("non-printable unicode inside a Python repr() in dxvk.conf error text",
                 lambda c: any(any(unicodedata.category(ch) in ("Cf", "Zs", "Cc", "Zl", "Zp", "Co", "Cn") and ch not in " \n\r\t" for ch in d) for d in list(_fx_data(c, "dxvk.conf")) + list(_fx_data(c, "dxvk.real")))),
    "longpath": ("path longer than PATH_MAX: legacy answers HTTP 500 (ENAMETOOLONG from Path.is_dir)",
                 lambda c: any(isinstance(st.get("args"), dict) and any(isinstance(v, str) and "/" in v and (len(v) > 3000 or max(len(x) for x in v.split("/")) > 200) for v in st["args"].values()) for st in c["steps"] if "op" in st)),
    "settings_unwritable": ("app data dir read-only / settings path is a directory: legacy settings writers answer HTTP 500",
                            lambda c: any((st.get("fx") == "chmod" and st.get("root") == "appdata" and st.get("path") in (".", "")) or
                                          (st.get("fx") == "mkdir" and st.get("root") == "appdata" and st.get("path") in ("electron-settings.json",)) for st in c["steps"])),
    "t7_args_newline": ("CR/LF inside a t7-config gamertag/password/colorCode (breaks the conf line structure; universal-newline reads differ)",
                        lambda c: any(st.get("op") == "t7-config" and isinstance(st.get("args"), dict) and any(isinstance(v, str) and re.search(r"[\r\n]", v) for v in st["args"].values()) for st in c["steps"])),
    "bool_strings": ("pydantic bool string parsing vs the port's (surrounding whitespace / case)",
                     lambda c: any(isinstance(st.get("args"), dict) and any(k in ("enabled", "limited", "friendsOnly", "preserve_fs_game", "hudEnabled", "enableAsync", "gplAsyncCache") and isinstance(v, str) for k, v in st["args"].items()) for st in c["steps"] if "op" in st)),
    "int_strings": ("pydantic lax int parsing of strings/floats ('80.0', ' 80 ', 80.0) vs the port's",
                    lambda c: any(isinstance(st.get("args"), dict) and any(k in ("target", "numCompilerThreads", "maxFrameRate", "maxFrameLatency") and not (isinstance(v, int) and not isinstance(v, bool)) for k, v in st["args"].items()) for st in c["steps"] if "op" in st)),
    "vdf_garbage": ("localconfig.vdf that python-vdf parses leniently but the port rejects / LaunchOptions as a section",
                    lambda c: any(st.get("fx") == "write" and str(st.get("path", "")).endswith("localconfig.vdf") and isinstance(st.get("data"), str) and
                                  (not st["data"].lstrip("\ufeff\r\n \t/").startswith('"UserLocalConfigStore"') or re.search(r'"LaunchOptions"\s*\n\s*\{', st["data"]) is not None) for st in c["steps"])),
    "dxvk_ints": ("Python int() syntax in dxvk.conf numbers (underscores, unicode digits, big, whitespace)",
                  lambda c: any(re.search(r"(numCompilerThreads|maxFrameRate|maxFrameLatency)\s*=\s*[^\n\r]*[_٣ ]|=\s*\d{19,}", t) for t in list(_fx_data(c, "dxvk.conf")) + list(_fx_data(c, "dxvk.real")))),
}


def avoided(case, avoid):
    return any(KNOWN[a][1](case) for a in avoid)


class CaseBuilder:
    @staticmethod
    def build(seed, idx, profile, ops_filter, max_ops=12, avoid=()):
        for attempt in range(40):
            case = CaseBuilder._build(seed, idx, profile, ops_filter, max_ops, attempt)
            if not avoid or not avoided(case, avoid):
                return case
        return case

    @staticmethod
    def _build(seed, idx, profile, ops_filter, max_ops=12, attempt=0):
        rng = random.Random(f"{seed}:{profile}:{idx}:{','.join(ops_filter or [])}" + (f":{attempt}" if attempt else ""))
        g = Gen(rng, profile, ops_filter)
        steps = []
        # perturbations
        npert = g.wpick([(0, 2), (1, 5), (2, 4), (3, 3), (4, 2), (6, 1)])
        names = [(n, f, w) for n, f, w, m in PERTURBATIONS]
        focus = None
        if ops_filter:
            focus = {"config": ["config_ini", "settings"], "vram-target": ["config_ini"], "presets/apply": ["config_ini", "d3d"], "intro-skip": ["video"],
                     "all-intros-skip": ["video"], "d3dcompiler": ["d3d"], "dxvk-config": ["dxvk"], "dxvk-uninstall": ["dxvk"], "t7-config": ["t7"],
                     "t7-uninstall": ["t7", "lpc", "mod"], "mod-files/clear": ["mod"], "logs/clear": ["log"], "logs/payload": ["log"],
                     "launch-options": ["steam_local", "userdata"], "workshop-install": ["steam_local", "workshop"], "enhanced-validate": ["dumps"],
                     "browse": ["browse"], "game-directory": ["browse", "exe"], "release-channel": ["settings"], "reset-stock": ["steam_local", "config_ini", "t7", "dxvk"],
                     "enhanced-uninstall": ["steam_config", "steam_local", "exe"], "exe-swap/compatible": ["exe"], "exe-swap/current": ["exe"],
                     "exe-swap/enhanced": ["exe"], "exe-swap/compatible-depot": ["exe", "libraries"]}
        for _ in range(npert):
            pool = names
            if ops_filter and g.chance(0.6):
                want = set(sum((focus.get(o, []) for o in ops_filter), []))
                sub = [x for x in names if x[0] in want]
                pool = sub or names
            n, f, w = g.wpick([((n, f, w), w) for n, f, w in pool])
            steps += f(g)
        # ops
        table = {o: v for o, v in OP_TABLE.items() if o in (ops_filter or ALL_OPS)}
        if ops_filter:
            for o in ops_filter:
                table.setdefault(o, OP_TABLE.get(o, (1, 1, a_empty)))
        wi = 0 if profile == "fast" else 1
        weighted = [((o, v[2]), v[wi]) for o, v in table.items() if v[wi] > 0] or [((o, v[2]), 1) for o, v in table.items()]
        nops = g.wpick([(1, 2), (2, 3), (3, 4), (4, 4), (5, 3), (6, 3), (8, 2), (10, 1), (12, 1)])
        nops = min(nops, max_ops)
        for i in range(nops):
            op, argfn = g.wpick(weighted)
            if profile == "steam" and i == 0 and not (ops_filter):
                op = g.pick(sorted(STEAM_OPS))
                argfn = OP_TABLE[op][2]
            args = argfn(g)
            if g.chance(0.10):
                args = mutate_args(g, op, args)
            if steps and i > 0 and g.chance(0.18):
                n, f, w = g.wpick([((n, f, w), w) for n, f, w in MID])
                steps += f(g)
            steps.append({"op": op, "args": args})
        return {"version": 1, "seed": seed, "index": idx, "profile": profile, "steps": steps}


# =====================================================================================================
# Reporting
# =====================================================================================================
def fmt_div(res, indent=""):
    d = res["div"]
    out = []
    st = res.get("step") or {}
    out.append(f"{indent}step #{res.get('step_idx')} ({res.get('where')}): op={d['op']} args={enc(st.get('args', {}))[:300]}")
    if d.get("legacy_error") or d.get("port_error"):
        out.append(f"{indent}  legacy envelope error: {d.get('legacy_error')}")
        out.append(f"{indent}  port   envelope error: {d.get('port_error')}")
    if d["resp_diff"]:
        out.append(f"{indent}  response diff (legacy vs port):")
        out += [indent + x for x in d["resp_diff"][:14]]
    for k in ("only_legacy", "only_port", "changed"):
        if d[k]:
            out.append(f"{indent}  fs {k.replace('_', ' ')}: {d[k][:8]}")
    for p, f in list(d["files"].items())[:4]:
        if "legacy" in f:
            out.append(f"{indent}  file {p}:\n{indent}    legacy: {json.dumps(f['legacy'])[:500]}\n{indent}    port:   {json.dumps(f['port'])[:500]}")
        else:
            out.append(f"{indent}  file {p}: legacy={f.get('legacy_meta')} port={f.get('port_meta')}")
    for n, t in (res.get("stderr") or {}).items():
        if t.strip() and n == "port":
            out.append(f"{indent}  port stderr tail: {t.strip()[-300:]}")
    return "\n".join(out)


def case_summary(case):
    out = []
    for s in case["steps"]:
        if "fx" in s:
            f = s
            d = f.get("data")
            ds = ""
            if f["fx"] in ("write",) and d is not None:
                ds = " data=" + json.dumps(d if isinstance(d, dict) else d[:80] + ("..." if len(d) > 80 else ""))
            out.append(f"  fx {f['fx']} {f.get('root', '')}/{f.get('path', f.get('name', ''))}{ds}")
        else:
            out.append(f"  op {s['op']} {enc(s.get('args', {}))[:200]}")
    return "\n".join(out)


def save_case(path, case, res=None, note=None):
    obj = {"case": case}
    if note:
        obj.update(note)
    if res and res.get("div"):
        d = res["div"]
        obj["observed"] = {"step_idx": res.get("step_idx"), "op": d["op"], "resp_diff": d["resp_diff"][:20], "only_legacy": d["only_legacy"], "only_port": d["only_port"],
                           "changed": d["changed"], "legacy_error": d.get("legacy_error"), "port_error": d.get("port_error"),
                           "files": {p: f for p, f in list(d["files"].items())[:6]}}
    with open(path, "w") as fh:
        json.dump(obj, fh, indent=1, ensure_ascii=False, sort_keys=False)
        fh.write("\n")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0], formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--bin", default=os.path.join(REPO, "target", "debug", "patchops-rpc"), help="patchops-rpc binary (must be reachable by uid 65534)")
    dl = os.path.join(REPO, "bench", ".work", "legacy")
    ap.add_argument("--legacy-src", default=dl, help="directory holding the legacy backend/ + utils.py (cwd of uvicorn)")
    ap.add_argument("--legacy-python", default=os.path.join(dl, ".venv", "bin", "python"))
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--cases", type=int, default=50)
    ap.add_argument("--first", type=int, default=0, help="index of the first case (to split a campaign)")
    ap.add_argument("--ops", default="", help="comma list of ops to focus on")
    ap.add_argument("--avoid", default="", help="comma list of KNOWN feature ids to steer generation away from (see KNOWN); 'list' prints them")
    ap.add_argument("--steam", action="store_true", help="Steam profile: Steam-dependent ops at high probability (slow, ~7 s per legacy cycle)")
    ap.add_argument("--time-budget", type=float, default=0, help="stop starting new cases after N seconds")
    ap.add_argument("--jobs", type=int, default=3)
    ap.add_argument("--keep", action="store_true", help="keep sandboxes + process logs of failing cases")
    ap.add_argument("--replay", default="", help="re-run a saved case (JSON file with a 'case' key, or a bare case)")
    ap.add_argument("--minimise", action="store_true", help="with --replay: also minimise")
    ap.add_argument("--no-minimise", action="store_true")
    ap.add_argument("--min-per-sig", type=int, default=2, help="minimise at most N divergences per signature (default 2)")
    ap.add_argument("--min-runs", type=int, default=60, help="max re-runs per minimisation")
    ap.add_argument("--save-dir", default="", help="write JSON for each divergence (raw + minimised) here")
    ap.add_argument("--work-dir", default="", help="parent for sandboxes (must be world-traversable); default a fresh dir under /tmp")
    ap.add_argument("--stats", default="", help="write per-op counters / timings JSON here")
    ap.add_argument("-v", "--verbose", action="store_true")
    a = ap.parse_args()

    if a.avoid == "list":
        for k, (d, _f) in KNOWN.items():
            print(f"{k:14s} {d}")
        return
    avoid = [x for x in a.avoid.split(",") if x]
    for x in avoid:
        if x not in KNOWN:
            sys.exit(f"unknown --avoid id {x}")
    Config.bin = os.path.abspath(a.bin)
    Config.legacy_src = os.path.abspath(a.legacy_src)
    Config.legacy_python = a.legacy_python
    Config.keep = a.keep
    for p, what in ((Config.bin, "port binary"), (Config.legacy_python, "legacy python"), (os.path.join(Config.legacy_src, "backend", "api.py"), "legacy backend")):
        if not os.path.exists(p):
            sys.exit(f"{what} not found: {p} (see the module docstring for the setup)")
    if not (IS_ROOT and UNSHARE and SETPRIV):
        Config.isolation = "sequential"
        if a.jobs > 1:
            print("WARNING: no root/unshare/setpriv: pid-namespace isolation unavailable, forcing --jobs 1 and running targets as the current uid", file=sys.stderr)
            a.jobs = 1
    else:
        try:
            subprocess.run([UNSHARE, "--pid", "--fork", "--mount-proc", "true"], check=True, capture_output=True, timeout=10)
        except Exception as exc:
            sys.exit(f"unshare --pid unusable here: {exc}")
    work = a.work_dir or tempfile.mkdtemp(prefix="dfz-")
    os.makedirs(work, exist_ok=True)
    os.chmod(work, 0o755)
    if a.save_dir:
        os.makedirs(a.save_dir, exist_ok=True)
    profile = "steam" if a.steam else "fast"
    ops_filter = [o.strip() for o in a.ops.split(",") if o.strip()] or None
    if ops_filter:
        bad = [o for o in ops_filter if o not in LEGACY_ROUTES or o in NET_OPS]
        if bad:
            sys.exit(f"unknown or network ops: {bad}")

    lock = threading.Lock()
    stats = {"cases": 0, "pass": 0, "diverge": 0, "allowed": 0, "error": 0, "ops": {}, "op_seconds": {}, "case_seconds": 0.0, "start_seconds": 0.0,
             "by_sig": {}, "allowed_by": {}, "ops_total": 0}
    t_begin = time.monotonic()

    def count_ops(res):
        for op, sec in res.get("ops", []):
            stats["ops"][op] = stats["ops"].get(op, 0) + 1
            stats["op_seconds"][op] = round(stats["op_seconds"].get(op, 0) + sec, 2)
            stats["ops_total"] += 1

    def do_case(case, label, minimise_ok):
        tag = hashlib.md5(label.encode()).hexdigest()[:6]
        rd = os.path.join(work, "c" + tag)
        for attempt in range(3):
            res = run_case(case, tag, rd)
            if not (res["status"] == "error" and "another sandbox" in res.get("error", "")):
                break
        with lock:
            stats["cases"] += 1
            stats[res["status"]] = stats.get(res["status"], 0) + 1
            stats["case_seconds"] += res["timing"].get("total_s", 0)
            stats["start_seconds"] += res["timing"].get("start_s", 0)
            count_ops(res)
        return res, tag

    def report(label, case, res, tag):
        st = res["status"]
        if st == "pass":
            if a.verbose:
                print(f"[{label}] pass ({res['timing'].get('total_s')}s)", flush=True)
            return
        if st == "error":
            print(f"[{label}] ERROR {res.get('error')}", flush=True)
            return
        if st == "allowed":
            with lock:
                stats["allowed_by"][res["allow_id"]] = stats["allowed_by"].get(res["allow_id"], 0) + 1
            print(f"[{label}] allowlisted divergence: {res['allow_id']} (op {res['div']['op']})", flush=True)
            return
        sig = res["sig"]
        with lock:
            ent = stats["by_sig"].setdefault(json.dumps(sig), {"count": 0, "labels": [], "minimised": 0})
            ent["count"] += 1
            ent["labels"].append(label)
            do_min = (not a.no_minimise) and ent["minimised"] < a.min_per_sig
            if do_min:
                ent["minimised"] += 1
        print(f"[{label}] DIVERGENCE sig={sig}  ({res['timing'].get('total_s')}s)", flush=True)
        trunc = copy.deepcopy(case)
        trunc["steps"] = trunc["steps"][: res["step_idx"] + 1] if res["where"] not in ("final", "init") else trunc["steps"]
        mincase, minres, nr = trunc, res, 0
        if do_min:
            mincase, mr, nr = minimise(trunc, sig, tag, os.path.join(work, "m" + tag), a.min_runs)
            if mr:
                minres = mr
        with lock:
            print(f"--- repro for [{label}] (seed={case.get('seed')} index={case.get('index')} profile={case.get('profile')}; minimisation re-runs: {nr})", flush=True)
            print(case_summary(mincase), flush=True)
            print(fmt_div(minres, "  "), flush=True)
            if minres.get("kept"):
                print(f"  sandboxes kept at {minres['kept']}", flush=True)
            if a.save_dir:
                fn = os.path.join(a.save_dir, f"div-{case.get('profile')}-{case.get('seed')}-{case.get('index')}.json")
                save_case(fn, mincase, minres, {"signature": list(sig), "original_steps": len(case["steps"]), "raw_case": case})
            print("", flush=True)

    # ------------------------------------------------------------------ replay
    if a.replay:
        obj = json.load(open(a.replay))
        case = obj["case"] if "case" in obj else obj
        res, tag = do_case(case, "replay", False)
        if res["status"] == "pass":
            print("replay: NO divergence (case passes)")
            sys.exit(0)
        if res["status"] == "error":
            print("replay: error", res.get("error"))
            sys.exit(2)
        if res["status"] == "allowed":
            print("replay: allowlisted divergence", res["allow_id"])
            sys.exit(0)
        mincase = case
        if a.minimise:
            mincase, mr, nr = minimise(case, res["sig"], tag, os.path.join(work, "mrep"), a.min_runs)
            if mr:
                res = mr
            print(case_summary(mincase))
            print(json.dumps(mincase))
        print(f"replay: DIVERGENCE sig={res['sig']}")
        print(fmt_div(res, "  "))
        if not a.keep:
            shutil.rmtree(work, ignore_errors=True)
        sys.exit(1)

    # ------------------------------------------------------------------ campaign
    def worker(i):
        if STOP.is_set() or (a.time_budget and time.monotonic() - t_begin > a.time_budget):
            return None
        idx = a.first + i
        case = CaseBuilder.build(a.seed, idx, profile, ops_filter, avoid=avoid)
        label = f"{profile}:{a.seed}:{idx}"
        res, tag = do_case(case, label, True)
        report(label, case, res, tag)
        return res["status"]

    def on_signal(signum, _frm):
        STOP.set()
        print(f"signal {signum}: finishing in-flight cases and stopping", file=sys.stderr, flush=True)
        if signum == signal.SIGTERM or STOP_COUNT[0] >= 1:
            kill_all_live()
        STOP_COUNT[0] += 1
    STOP_COUNT = [0]
    signal.signal(signal.SIGINT, on_signal)
    signal.signal(signal.SIGTERM, on_signal)
    found = 0
    try:
        with cf.ThreadPoolExecutor(a.jobs) as ex:
            futs = [ex.submit(worker, i) for i in range(a.cases)]
            done = 0
            for f in cf.as_completed(futs):
                try:
                    r = f.result()
                except Exception as exc:
                    import traceback
                    traceback.print_exc()
                    r = "error"
                done += 1
                if done % 25 == 0:
                    el = time.monotonic() - t_begin
                    print(f"... {done}/{a.cases} cases, {done / el * 60:.1f} cases/min, pass={stats.get('pass', 0)} diverge={stats['diverge']} allowed={stats['allowed']} error={stats['error']}", flush=True)
    except KeyboardInterrupt:
        print("interrupted", file=sys.stderr)
    el = time.monotonic() - t_begin
    print("\n===== summary =====")
    print(f"cases run: {stats['cases']} in {el:.0f}s ({stats['cases'] / max(el, 1) * 60:.1f} cases/min, jobs={a.jobs}, profile={profile}, seed={a.seed}, first={a.first})")
    print(f"pass={stats.get('pass', 0)} diverge={stats['diverge']} allowlisted={stats['allowed']} error={stats['error']}; ops executed: {stats['ops_total']}")
    print("per-op counts:", json.dumps(dict(sorted(stats["ops"].items()))))
    if stats["allowed_by"]:
        print("allowlisted:", stats["allowed_by"])
    for sig, ent in sorted(stats["by_sig"].items(), key=lambda kv: -kv[1]["count"]):
        print(f"  {ent['count']:4d} x {sig}  e.g. {ent['labels'][:3]}")
    if a.stats:
        json.dump(stats, open(a.stats, "w"), indent=1)
    shutil.rmtree(work, ignore_errors=True) if not a.keep else None
    sys.exit(1 if stats["diverge"] else 0)


if __name__ == "__main__":
    main()
