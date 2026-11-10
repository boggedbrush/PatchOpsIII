"""Target `gpui`: the Rust + GPUI port (does not exist yet; this driver implements the contract in OPS.md).

Binary: <gpui-src>/target/release/patchops (override --gpui-bin).
  patchops --rpc-stdio                 headless JSON-lines RPC on stdin/stdout
  PATCHOPS_BENCH_RPC=1 patchops        GUI process that also serves the same RPC on stdin/stdout
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import time

from .. import fsutil, proc, sysinfo
from ..clients import RpcClient
from .common import Instance

NAME = "gpui"


class GpuiTarget:
    name = NAME

    def __init__(self, src: str, work: str, bin_override: str | None = None, run_as=None, log=print):
        self.src = os.path.abspath(src)
        self.work = os.path.abspath(work)
        self.w = os.path.join(self.work, "gpui")
        self.cache = os.path.join(self.work, "gpui-cache")
        self.logs = os.path.join(self.work, "logs", "gpui")
        self.bin_override = os.path.abspath(bin_override) if bin_override else None
        self.run_as = run_as
        self.log = log
        self.mode = "gui+rpc"

    def info(self) -> dict:
        return {"target": NAME, "source": sysinfo.git_info(self.src), "work_dir": self.w, "binary": self.binary_path()}

    def binary_path(self) -> str:
        return self.bin_override or os.path.join(self.w, "target", "release", "patchops")

    def runnable(self) -> str | None:
        b = self.binary_path()
        if not os.path.isfile(b) or not os.access(b, os.X_OK):
            return f"target not built: {b} does not exist (gpui binary 'patchops' is not implemented/built yet)"
        return None

    # ------------------------------------------------------------ build
    def _env(self, extra=None):
        env = dict(os.environ)
        env.update({"CARGO_HOME": os.path.join(self.cache, "cargo-home"), "CARGO_TARGET_DIR": os.path.join(self.w, "target"),
                    "CARGO_TERM_COLOR": "never", "CARGO_INCREMENTAL": os.environ.get("CARGO_INCREMENTAL", "")})
        if not env["CARGO_INCREMENTAL"]:
            env.pop("CARGO_INCREMENTAL")
        if extra:
            env.update(extra)
        return env

    def _find_touch_file(self) -> str | None:
        try:
            out = subprocess.run(["cargo", "metadata", "--no-deps", "--format-version", "1", "--offline"], cwd=self.w,
                                 env=self._env(), capture_output=True, text=True, timeout=120)
            meta = json.loads(out.stdout)
        except Exception:
            return None
        best = None
        for pkg in meta.get("packages", []):
            for t in pkg.get("targets", []):
                if "bin" in t.get("kind", []):
                    if t["name"] == "patchops":
                        return t["src_path"]
                    best = best or t["src_path"]
        return best

    def build(self) -> dict:
        res: dict = {"steps": {}, "sizes": {}, "notes": []}
        steps = res["steps"]
        log = self.log
        os.makedirs(self.logs, exist_ok=True)
        if not os.path.isfile(os.path.join(self.src, "Cargo.toml")):
            res.update(ok=False, skipped="target not built: no Cargo.toml in gpui source " + self.src)
            return res
        shutil.rmtree(self.cache, ignore_errors=True)
        os.makedirs(self.cache, exist_ok=True)
        fsutil.copy_source(self.src, self.w, extra_exclude={"crates_target"})
        t_total = time.monotonic()

        def run(name, cmd, **kw):
            log(f"[build:gpui] {name} ...")
            r = proc.run_measured(cmd, cwd=self.w, env=kw.pop("env", self._env()), log_path=os.path.join(self.logs, f"build-{name}.log"),
                                  shell=isinstance(cmd, str), timeout=kw.pop("timeout", 5400))
            steps[name] = r
            log(f"[build:gpui] {name}: exit={r['exit_code']} wall={r['wall_s']}s cpu={r['cpu_total_s']}s peak_rss_sum={r['peak_rss_sum_kb']//1024}MB")
            return r

        locked = "--locked" if os.path.isfile(os.path.join(self.w, "Cargo.lock")) else ""
        f = run("cargo-fetch-cold", f"cargo fetch {locked}")
        res["sizes"]["cargo_home"] = fsutil.du(os.path.join(self.cache, "cargo-home"))
        shutil.rmtree(os.path.join(self.w, "target"), ignore_errors=True)
        if f["exit_code"] == 0:
            b = run("cargo-build-release-clean", f"cargo build --release --offline {locked}")
        else:
            b = run("cargo-build-release-clean", f"cargo build --release {locked}")
            res["notes"].append("cargo fetch failed; clean build ran online so compile time includes downloads")
        binp = os.path.join(self.w, "target", "release", "patchops")
        res["sizes"]["target_dir"] = fsutil.du(os.path.join(self.w, "target"))
        if b["exit_code"] == 0 and os.path.isfile(binp):
            res["sizes"]["binary"] = fsutil.du(binp)
            touch = self._find_touch_file()
            if touch and os.path.isfile(touch):
                os.utime(touch, None)
                res["incremental_touched_file"] = os.path.relpath(touch, self.w)
                run("cargo-build-release-incremental", f"cargo build --release --offline {locked}")
            else:
                res["notes"].append("could not determine a source file to touch for the incremental rebuild")
            run("cargo-build-release-noop", f"cargo build --release --offline {locked}")
            fsutil.chmod_readable(self.w)
        elif b["exit_code"] == 0:
            res["notes"].append("cargo build succeeded but target/release/patchops was not produced (no bin target named 'patchops')")
        res["total_wall_s"] = round(time.monotonic() - t_total, 1)
        res["total_cpu_s"] = round(sum(s["cpu_total_s"] for s in steps.values()), 1)
        res["total_peak_rss_sum_kb"] = max((s["peak_rss_sum_kb"] for s in steps.values()), default=0)
        res["derived"] = {"network_bound_s": steps["cargo-fetch-cold"]["wall_s"],
                          "compile_clean_s": steps.get("cargo-build-release-clean", {}).get("wall_s"),
                          "compile_incremental_s": steps.get("cargo-build-release-incremental", {}).get("wall_s")}
        res["footprint"] = {"binary_bytes": res["sizes"].get("binary", {}).get("apparent_bytes"),
                            "dependency_footprint_bytes": res["sizes"]["cargo_home"]["apparent_bytes"]}
        res["ok"] = bool(b["exit_code"] == 0 and os.path.isfile(binp))
        return res

    # ------------------------------------------------------------ run
    def start(self, sb, kind: str, display: str | None) -> Instance:
        binp = self.binary_path()
        env = sb.env()
        os.makedirs(os.path.join(self.logs, "run"), exist_ok=True)
        err = open(os.path.join(self.logs, "run", f"{kind}-{sb.tag}.err.log"), "wb")
        if kind == "gui":
            env["DISPLAY"] = display
            env["PATCHOPS_BENCH_RPC"] = "1"
            cmd = [binp]
        else:
            cmd = [binp, "--rpc-stdio"]
        mp = proc.ManagedProcess(cmd, cwd=os.path.dirname(binp), env=env, tag=sb.tag, run_as=self.run_as,
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=err)
        client = RpcClient(mp.popen)
        inst = Instance(mp, client, kind, display, backend_port=None, log_files=(err,))
        inst.cmd = cmd
        inst.mode = "gui+rpc" if kind == "gui" else "rpc-stdio"
        return inst

    @staticmethod
    def ready_probe(inst: Instance, res: dict, t0: float) -> bool:
        # One request is enough: the reply to `status` arrives only once the RPC server is serving.
        if res.get("_probe_sent"):
            return False
        res["_probe_sent"] = True
        r = inst.client.call("status", {}, timeout=60.0)
        if r.envelope_ok and isinstance(r.result, dict):
            now = round((time.monotonic() - t0) * 1000, 1)
            res["health_ms"] = now
            res["ready_ms"] = now
            return True
        now = round((time.monotonic() - t0) * 1000, 1)
        if r.envelope_ok or (r.error or "").startswith(("unknown op", "validation", "internal")):
            # the RPC server is alive but the contract's `status` op does not work: fail fast and say why
            res["health_ms"] = now
            res["failed"] = f"RPC answered but `status` is not usable: {r.error or r.result}"
            return True
        res["failed_probe"] = r.error
        return False
