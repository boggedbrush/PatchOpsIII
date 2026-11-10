"""Target `legacy`: Electron + React renderer + Python FastAPI backend (PyInstaller-packaged or from source)."""
from __future__ import annotations

import glob
import http.client
import json
import os
import shutil
import stat
import time

from .. import fsutil, proc, sysinfo
from ..clients import LegacyHttpClient
from .common import Instance

NAME = "legacy"


class LegacyTarget:
    name = NAME

    def __init__(self, src: str, work: str, mode: str = "auto", run_as=None, log=print):
        self.src = os.path.abspath(src)
        self.work = os.path.abspath(work)
        self.w = os.path.join(self.work, "legacy")          # copy of the sources + build outputs
        self.cache = os.path.join(self.work, "legacy-cache")  # package-manager caches (wiped for cold measurements)
        self.logs = os.path.join(self.work, "logs", "legacy")
        self.mode_req = mode
        self.run_as = run_as
        self.log = log
        self.mode = None

    # ------------------------------------------------------------ info
    def info(self) -> dict:
        pkg = {}
        try:
            pkg = json.load(open(os.path.join(self.src, "package.json")))
        except Exception:
            pass
        return {"target": NAME, "source": sysinfo.git_info(self.src), "app_version": pkg.get("version"),
                "work_dir": self.w, "mode": self.mode}

    # ------------------------------------------------------------ build
    def _env(self, extra=None):
        env = dict(os.environ)
        env.update({
            "XDG_CACHE_HOME": os.path.join(self.cache, "xdg"),
            "BUN_INSTALL_CACHE_DIR": os.path.join(self.cache, "bun"),
            "ELECTRON_CACHE": os.path.join(self.cache, "electron"),
            "electron_config_cache": os.path.join(self.cache, "electron"),
            "ELECTRON_BUILDER_CACHE": os.path.join(self.cache, "electron-builder"),
            "PIP_CACHE_DIR": os.path.join(self.cache, "pip"),
            "PIP_DISABLE_PIP_VERSION_CHECK": "1",
            "CI": "1",
        })
        if extra:
            env.update(extra)
        return env

    def build(self) -> dict:
        res: dict = {"steps": {}, "sizes": {}, "notes": []}
        steps = res["steps"]
        log = self.log

        def run(name, cmd, **kw):
            log(f"[build:legacy] {name} ...")
            r = proc.run_measured(cmd, cwd=kw.pop("cwd", self.w), env=kw.pop("env", self._env()),
                                  log_path=os.path.join(self.logs, f"build-{name}.log"), shell=isinstance(cmd, str),
                                  timeout=kw.pop("timeout", 2400))
            steps[name] = r
            log(f"[build:legacy] {name}: exit={r['exit_code']} wall={r['wall_s']}s cpu={r['cpu_total_s']}s peak_rss_sum={r['peak_rss_sum_kb']//1024}MB")
            return r

        os.makedirs(self.logs, exist_ok=True)
        shutil.rmtree(self.cache, ignore_errors=True)
        os.makedirs(self.cache, exist_ok=True)
        fsutil.copy_source(self.src, self.w)
        t_total = time.monotonic()

        # --- node dependencies: cold cache (download + link), then warm cache (link only) ---
        r = run("bun-install-cold", "bun install --frozen-lockfile")
        res["sizes"]["bun_cache"] = fsutil.du(os.path.join(self.cache, "bun"))
        # bun runs electron's postinstall itself (electron is on its default trusted list); only fall back to the explicit
        # download when the binary is missing so the step is measured whenever it actually does work.
        electron_bin = os.path.join(self.w, "node_modules", "electron", "dist", "electron")
        r2 = None
        if not os.path.exists(electron_bin) and os.path.exists(os.path.join(self.w, "node_modules", "electron", "install.js")):
            r2 = run("electron-binary-download-cold", "node node_modules/electron/install.js")
        else:
            res["notes"].append("Electron binary was downloaded by bun's postinstall as part of bun-install-cold (and unpacked from the "
                                "local electron cache in bun-install-warm)")
        res["sizes"]["electron_cache"] = fsutil.du(os.path.join(self.cache, "electron"))
        shutil.rmtree(os.path.join(self.w, "node_modules"), ignore_errors=True)
        run("bun-install-warm", "bun install --frozen-lockfile")
        if r2 is not None:
            run("electron-binary-extract-warm", "node node_modules/electron/install.js")
        res["sizes"]["node_modules"] = fsutil.du(os.path.join(self.w, "node_modules"))

        # --- python: download wheels (network), then install from the local wheelhouse (no network) ---
        wh = os.path.join(self.cache, "wheelhouse")
        req = os.path.join(self.w, "requirements.txt")
        py = "python3"
        run("venv-create", f"{py} -m venv .venv")
        pip = os.path.join(self.w, ".venv", "bin", "pip")
        run("pip-download-cold", f"{pip} download -r requirements.txt pyinstaller -d {wh}")
        res["sizes"]["wheelhouse"] = fsutil.du(wh)
        shutil.rmtree(os.path.join(self.w, ".venv"), ignore_errors=True)
        run("venv-create-2", f"{py} -m venv .venv")
        run("pip-install-from-wheelhouse", f"{pip} install --no-index --find-links {wh} -r requirements.txt pyinstaller")
        res["sizes"]["venv"] = fsutil.du(os.path.join(self.w, ".venv"))

        # --- app build (clean outputs) ---
        for d in ("dist", "build"):
            shutil.rmtree(os.path.join(self.w, d), ignore_errors=True)
        rb = run("bun-run-build", "bun run build")
        res["sizes"]["dist_main"] = fsutil.du(os.path.join(self.w, "dist", "main"))
        res["sizes"]["dist_renderer"] = fsutil.du(os.path.join(self.w, "dist", "renderer"))
        rp = run("pyinstaller-backend", "bun run build:backend:linux")
        res["sizes"]["backend_binary"] = fsutil.du(os.path.join(self.w, "dist", "backend", "patchops-backend"))

        # --- electron-builder: unpacked linux app dir ---
        pk = run("electron-builder-linux-dir", "bun x electron-builder@26.8.1 --linux dir --publish never", timeout=2400)
        unpacked = os.path.join(self.w, "dist", "packages", "linux-unpacked")
        res["sizes"]["packaged_app_dir"] = fsutil.du(unpacked)
        if pk["exit_code"] != 0:
            res["notes"].append("electron-builder --linux dir failed; see steps.electron-builder-linux-dir.log_tail. "
                                "Startup/memory/workload fall back to dev mode (electron . + venv python) if possible.")
        res["total_wall_s"] = round(time.monotonic() - t_total, 1)
        res["total_cpu_s"] = round(sum(s["cpu_total_s"] for s in steps.values()), 1)
        res["total_peak_rss_sum_kb"] = max((s["peak_rss_sum_kb"] for s in steps.values()), default=0)
        # derived: network-bound vs local time
        derived = {}
        if "bun-install-cold" in steps and "bun-install-warm" in steps:
            derived["bun_download_estimate_s"] = round(max(0.0, steps["bun-install-cold"]["wall_s"] - steps["bun-install-warm"]["wall_s"]), 2)
        if "electron-binary-download-cold" in steps and "electron-binary-extract-warm" in steps:
            derived["electron_download_estimate_s"] = round(max(0.0, steps["electron-binary-download-cold"]["wall_s"] - steps["electron-binary-extract-warm"]["wall_s"]), 2)
        if "pip-download-cold" in steps:
            derived["pip_download_s"] = steps["pip-download-cold"]["wall_s"]
        derived["network_bound_s_estimate"] = round(sum(v for k, v in derived.items() if k.endswith("_s") or k.endswith("_estimate_s")) , 2) if derived else None
        local_keys = ["bun-install-warm", "electron-binary-extract-warm", "venv-create-2", "pip-install-from-wheelhouse", "bun-run-build",
                      "pyinstaller-backend", "electron-builder-linux-dir"]
        derived["local_install_and_compile_s"] = round(sum(steps[k]["wall_s"] for k in local_keys if k in steps), 2)
        res["derived"] = derived
        res["footprint"] = {
            "distributable_unpacked_app_dir_bytes": res["sizes"]["packaged_app_dir"]["apparent_bytes"],
            "backend_binary_bytes": res["sizes"]["backend_binary"]["apparent_bytes"],
            "dependency_footprint_bytes": res["sizes"]["node_modules"]["apparent_bytes"] + res["sizes"]["venv"]["apparent_bytes"],
        }
        fsutil.chmod_readable(self.w)
        res["ok"] = all(steps[k]["exit_code"] == 0 for k in steps if k not in ("electron-builder-linux-dir",))
        res["packaging_ok"] = pk["exit_code"] == 0
        return res

    # ------------------------------------------------------------ run
    def _packaged_exe(self) -> str | None:
        d = os.path.join(self.w, "dist", "packages", "linux-unpacked")
        try:
            name = json.load(open(os.path.join(self.w, "package.json")))["name"]
        except Exception:
            name = "patchopsiii-electron"
        for cand in (os.path.join(d, name), os.path.join(d, "PatchOpsIII"), os.path.join(d, "patchopsiii")):
            if os.path.isfile(cand) and os.access(cand, os.X_OK):
                return cand
        return None

    def _dev_electron(self) -> str | None:
        p = os.path.join(self.w, "node_modules", "electron", "dist", "electron")
        main = os.path.join(self.w, "dist", "main", "main.js")
        py = os.path.join(self.w, ".venv", "bin", "python")
        return p if os.path.isfile(p) and os.path.isfile(main) and os.path.isfile(py) else None

    def runnable(self) -> str | None:
        """None if runnable, else a reason."""
        mode = self.resolve_mode()
        if mode is None:
            return ("target not built: neither a packaged app (dist/packages/linux-unpacked) nor a dev build "
                    f"(node_modules/electron + dist/main + .venv) exists under {self.w}; run the build phase first")
        return None

    def resolve_mode(self) -> str | None:
        if self.mode_req in ("packaged", "auto") and self._packaged_exe():
            self.mode = "packaged"
        elif self.mode_req in ("dev", "auto") and self._dev_electron():
            self.mode = "dev"
        else:
            self.mode = None
        return self.mode

    def start(self, sb, kind: str, display: str | None) -> Instance:
        mode = self.mode or self.resolve_mode()
        port = proc.free_port()
        env = sb.env({"PATCHOPSIII_BACKEND_PORT": str(port)})
        os.makedirs(os.path.join(self.logs, "run"), exist_ok=True)
        tag = sb.tag
        out = open(os.path.join(self.logs, "run", f"{kind}-{tag}.out.log"), "wb")
        err = open(os.path.join(self.logs, "run", f"{kind}-{tag}.err.log"), "wb")
        if kind == "gui":
            env["DISPLAY"] = display
            if mode == "packaged":
                exe = self._packaged_exe()
                cmd = [exe, "--no-sandbox", "--disable-gpu"]
                cwd = os.path.dirname(exe)
            else:
                cmd = [self._dev_electron(), self.w, "--no-sandbox", "--disable-gpu"]
                cwd = self.w
        else:
            if mode == "packaged":
                exe = os.path.join(self.w, "dist", "packages", "linux-unpacked", "resources", "backend-bin", "patchops-backend")
                cmd, cwd = [exe], os.path.dirname(exe)
            else:
                cmd = [os.path.join(self.w, ".venv", "bin", "python"), "-m", "uvicorn", "backend.api:app", "--host", "127.0.0.1",
                       "--port", str(port)]
                cwd = self.w
        mp = proc.ManagedProcess(cmd, cwd=cwd, env=env, tag=tag, run_as=self.run_as, stdout=out, stderr=err)
        client = LegacyHttpClient("127.0.0.1", port)
        inst = Instance(mp, client, kind, display, backend_port=port, log_files=(out, err))
        inst.cmd = cmd
        inst.mode = mode
        return inst

    @staticmethod
    def ready_probe(inst: Instance, res: dict, t0: float) -> bool:
        try:
            c = http.client.HTTPConnection("127.0.0.1", inst.backend_port, timeout=1.0)
            c.request("GET", "/api/health")
            r = c.getresponse()
            r.read()
            if r.status == 200:
                if res["health_ms"] is None:
                    res["health_ms"] = round((time.monotonic() - t0) * 1000, 1)
                c.request("GET", "/api/status")
                r2 = c.getresponse()
                r2.read()
                c.close()
                if r2.status == 200:
                    res["ready_ms"] = round((time.monotonic() - t0) * 1000, 1)
                    return True
            c.close()
        except Exception:
            pass
        return False
