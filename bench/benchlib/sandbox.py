"""Synthetic fixture: fake HOME, fake Steam, fake Black Ops III install, fake `steam`/`xdg-open`.

The sandbox is rebuilt from scratch for every GUI/backend run, so every run starts from byte-identical state.
Everything (except mutations performed on purpose by workload steps) is created here.
"""
from __future__ import annotations

import hashlib
import json
import os
import shutil
import stat
import uuid

FIXTURE_VERSION = "1"
APP_ID = "311210"
GAME_DIR_NAME = "Call of Duty Black Ops III"

# env vars removed from the child environment unless the network group is requested
PROXY_VARS = ("HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy", "ALL_PROXY", "all_proxy")

FAKE_STEAM = r"""#!/bin/bash
# Fake Steam client used by the PatchOpsIII benchmark. Not the real thing.
#   steam -silent      -> detach a resident process whose comm is "steam" (so pgrep/pkill -x steam see it)
#   steam -applaunch N -> record the call and exit
LOG="${PATCHOPS_BENCH_FAKESTEAM_LOG:-/dev/null}"
case "$1" in
  --resident)
    trap 'exit 0' TERM INT
    while :; do sleep 0.5; done
    ;;
  -silent)
    echo "steam $*" >> "$LOG"
    setsid -f "$0" --resident </dev/null >/dev/null 2>&1
    exit 0
    ;;
  *)
    echo "steam $*" >> "$LOG"
    exit 0
    ;;
esac
"""

FAKE_XDG_OPEN = r"""#!/bin/bash
LOG="${PATCHOPS_BENCH_FAKESTEAM_LOG:-/dev/null}"
echo "xdg-open $*" >> "$LOG"
exit 0
"""


def det_bytes(label: str, size: int) -> bytes:
    """Deterministic pseudo-random bytes."""
    out = bytearray()
    counter = 0
    while len(out) < size:
        out += hashlib.sha256(f"{label}:{counter}".encode()).digest()
        counter += 1
    return bytes(out[:size])


CONFIG_INI_LINES = [
    ("// Black Ops III configuration (bench fixture)", None),
    ('AATechnique = "SMAA 1x" // anti-aliasing technique', None),
    ('ActorShadows = "1" // actor shadows', None),
    ('BackbufferCount = "2" // back buffers', None),
    ('CorpseCount = "32" // corpses', None),
    ('DisableDynamicLightShadows = "0" // dynamic light shadows', None),
    ('DisableDynamicSunShadows = "0" // dynamic sun shadows', None),
    ('DrawFPS = "0" // FPS counter', None),
    ('FOV = "80" // field of view', None),
    ('// FOV = "70" (commented out, must not be matched)', None),
    ('FullScreenMode = "1" // 0=Windowed,1=Fullscreen,2=Fullscreen Windowed', None),
    ('Gamma = "1.0" // gamma (not managed by PatchOpsIII)', None),
    ("", None),
    ("", None),
    ('MaxFPS = "165" // Maximum FPS cap', None),
    ('MaxFrameLatency = "1" // frame latency', None),
    ('MeshQuality = "0" // mesh quality', None),
    ('MotionBlur = "On" // motion blur', None),
    ('MotionBlurQuality = "High" // motion blur quality', None),
    ('OIT = "0" // order independent transparency', None),
    ('RefreshRate = "59.940" // refresh rate', None),
    ('RestrictGraphicsOptions = "1" // restrict graphics options', None),
    ('ResolutionPercent = "100" // render resolution percent', None),
    ('  SerializeRender = "0" // 0 to 2 (leading spaces)', None),
    ('ShadowFiltering = "0" // shadow filtering', None),
    ('SmoothFramerate = "0" // smooth framerate', None),
    ('SpotShadowTextureSize = "2048" // spot shadows', None),
    ('StreamMinResident = "0" // 0 or 1', None),
    ('TextureFilter = "1" // aniso', None),
    ('TextureQuality = "0" // texture quality', None),
    ('VideoMemory = "0.75" // 0.75 to 1', None),
    ('Vsync = "1" // Vertical sync', None),
    ('VsyncMode = "adaptive" // different key sharing the prefix "Vsync"', None),
    ('WindowSize = "1920x1080" // any text', None),
    ('DrawFPS = "0" // duplicate key line, both occurrences get rewritten', None),
    ("// café latin-1 byte in a comment \x00MARK", None),
]


def build_config_ini() -> bytes:
    text = "\r\n".join(l for l, _ in CONFIG_INI_LINES) + "\r\n"
    raw = text.encode("utf-8")
    # replace the placeholder with a lone 0xE9 byte (invalid UTF-8) to exercise errors="ignore"
    raw = raw.replace("café".encode("utf-8"), b"caf\xe9").replace(b"\x00MARK", b"MARK")
    return raw


LOCALCONFIG_VDF = '''"UserLocalConfigStore"
{
\t"Software"
\t{
\t\t"Valve"
\t\t{
\t\t\t"Steam"
\t\t\t{
\t\t\t\t"apps"
\t\t\t\t{
\t\t\t\t\t"220"
\t\t\t\t\t{
\t\t\t\t\t\t"LaunchOptions"\t\t"-novid"
\t\t\t\t\t}
\t\t\t\t\t"311210"
\t\t\t\t\t{
\t\t\t\t\t\t"LastPlayed"\t\t"1700000000"
\t\t\t\t\t\t"LaunchOptions"\t\t"WINEDLLOVERRIDES=\\"dsound=n,b\\" %command%"
\t\t\t\t\t}
\t\t\t\t}
\t\t\t}
\t\t}
\t}
\t"friends"
\t{
\t\t"PersonaName"\t\t"BenchUser"
\t}
}
'''

STEAM_CONFIG_VDF = '''"InstallConfigStore"
{
\t"Software"
\t{
\t\t"Valve"
\t\t{
\t\t\t"Steam"
\t\t\t{
\t\t\t\t"CompatToolMapping"
\t\t\t\t{
\t\t\t\t\t"0"
\t\t\t\t\t{
\t\t\t\t\t\t"name"\t\t"proton_9"
\t\t\t\t\t\t"config"\t\t""
\t\t\t\t\t\t"priority"\t\t"75"
\t\t\t\t\t}
\t\t\t\t}
\t\t\t}
\t\t}
\t}
}
'''

APPWORKSHOP_ACF = '''"AppWorkshop"
{
\t"appid"\t\t"311210"
\t"WorkshopItemsInstalled"
\t{
\t\t"2942053577"
\t\t{
\t\t\t"size"\t\t"1024"
\t\t\t"timeupdated"\t\t"1700000000"
\t\t}
\t}
}
'''


class Sandbox:
    def __init__(self, root: str, run_as: tuple[int, int] | None, tag: str | None = None, network: bool = False):
        self.root = os.path.realpath(root)
        self.run_as = run_as if (run_as is not None and os.geteuid() == 0) else None
        self.tag = tag or uuid.uuid4().hex[:12]
        self.network = network
        j = os.path.join
        self.home = j(self.root, "home")
        self.data_home = j(self.home, ".local", "share")
        self.app_data = j(self.data_home, "PatchOpsIII")
        self.mod_files = j(self.app_data, "BO3 Mod Files")
        self.steam_root = j(self.home, ".steam", "steam")
        self.userdata = j(self.steam_root, "userdata", "12345678")
        self.localconfig = j(self.userdata, "config", "localconfig.vdf")
        self.library2 = j(self.root, "games", "SteamLibrary")
        self.game_dir = j(self.library2, "steamapps", "common", GAME_DIR_NAME)
        self.alt_dir = j(self.root, "games", "alt", GAME_DIR_NAME)
        self.fakebin = j(self.root, "fakebin")
        self.fakebin_log = j(self.root, "fakebin-log", "steam.calls")
        self.dumps = j(self.root, "dumps")
        self.tmp = j(self.root, "tmp")
        self.run_dir = j(self.root, "run")

    # ---- environment -------------------------------------------------
    def env(self, extra: dict | None = None) -> dict:
        base = os.environ
        env = {
            "PATH": f"{self.fakebin}:{base.get('PATH', '/usr/local/bin:/usr/bin:/bin')}",
            "HOME": self.home,
            "USER": "bench",
            "LOGNAME": "bench",
            "XDG_DATA_HOME": self.data_home,
            "XDG_CONFIG_HOME": os.path.join(self.home, ".config"),
            "XDG_CACHE_HOME": os.path.join(self.home, ".cache"),
            "XDG_STATE_HOME": os.path.join(self.home, ".local", "state"),
            "XDG_RUNTIME_DIR": self.run_dir,
            "TMPDIR": self.tmp,
            "LANG": "C.UTF-8",
            "LC_ALL": "C.UTF-8",
            "TZ": "UTC",
            "PATCHOPSIII_VERSION_OVERRIDE": "bench",
            "PATCHOPS_BENCH_FAKESTEAM_LOG": self.fakebin_log,
            "PATCHOPS_BENCH_TAG": self.tag,
            "PATCHOPS_BENCH_SANDBOX": self.root,
            "PYTHONDONTWRITEBYTECODE": "1",
        }
        if self.network:
            ca = self._ca_copy()
            for k in PROXY_VARS + ("NO_PROXY", "no_proxy", "SSL_CERT_FILE", "REQUESTS_CA_BUNDLE", "CURL_CA_BUNDLE",
                                   "NODE_EXTRA_CA_CERTS", "SSL_CERT_DIR"):
                if k in base:
                    env[k] = base[k]
            if ca:
                for k in ("SSL_CERT_FILE", "REQUESTS_CA_BUNDLE", "CURL_CA_BUNDLE", "NODE_EXTRA_CA_CERTS"):
                    env[k] = ca
        else:
            env["NO_PROXY"] = "*"
            env["no_proxy"] = "*"
        for k, v in base.items():
            if k.startswith("BENCH_"):      # harness-private passthrough (used by tools/mock-patchops)
                env[k] = v
        if extra:
            env.update(extra)
        return env

    def _ca_copy(self) -> str | None:
        """The proxy CA bundle may live under /root (unreadable for the unprivileged uid): copy it into the sandbox."""
        src = next((os.environ[k] for k in ("REQUESTS_CA_BUNDLE", "SSL_CERT_FILE", "CURL_CA_BUNDLE") if os.environ.get(k)), None)
        if not src or not os.path.isfile(src):
            return None
        dst = os.path.join(self.root, "ca-bundle.pem")
        if not os.path.exists(dst):
            shutil.copyfile(src, dst)
            os.chmod(dst, 0o644)
            self._chown(dst)
        return dst

    # ---- file helpers --------------------------------------------------
    def _chown(self, path: str):
        if self.run_as is not None:
            try:
                os.chown(path, self.run_as[0], self.run_as[1], follow_symlinks=False)
            except OSError:
                pass

    def chown_tree(self, path: str):
        if self.run_as is None:
            return
        self._chown(path)
        for dp, dns, fns in os.walk(path):
            for n in dns + fns:
                self._chown(os.path.join(dp, n))

    def _mkparents(self, path: str):
        d = os.path.dirname(path)
        missing = []
        while d and not os.path.exists(d):
            missing.append(d)
            d = os.path.dirname(d)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        for m in reversed(missing):
            self._chown(m)

    def write(self, path: str, data: bytes | str, mode: int = 0o644):
        if isinstance(data, str):
            data = data.encode("utf-8")
        self._mkparents(path)
        with open(path, "wb") as fh:
            fh.write(data)
        os.chmod(path, mode)
        self._chown(path)

    def mkdir(self, path: str):
        self._mkparents(os.path.join(path, "x"))
        os.makedirs(path, exist_ok=True)
        self._chown(path)

    def rm(self, path: str):
        if os.path.isdir(path) and not os.path.islink(path):
            shutil.rmtree(path, ignore_errors=True)
        elif os.path.lexists(path):
            os.unlink(path)

    # ---- build ---------------------------------------------------------
    FIXTURE_DIRS = ("home/.steam", "home/.local/share/PatchOpsIII", "games", "dumps", "fakebin-log", "fakebin")

    def build(self) -> "Sandbox":
        if os.path.exists(self.root):
            shutil.rmtree(self.root)
        os.makedirs(self.root)
        return self._populate()

    def reset_fixture(self) -> "Sandbox":
        """Restore the pristine fixture while processes are running (keeps tmp/run/.config/.cache untouched)."""
        from . import procfs
        for pid in procfs.tagged_pids(self.tag):
            st = procfs.read_stat(pid)
            if st and st[1] == "steam":
                try:
                    os.kill(pid, 9)
                except OSError:
                    pass
        for rel in self.FIXTURE_DIRS:
            shutil.rmtree(os.path.join(self.root, rel), ignore_errors=True)
        return self._populate()

    def _populate(self) -> "Sandbox":
        for d in (self.home, self.tmp, self.run_dir, self.data_home, os.path.join(self.home, ".config"),
                  os.path.join(self.home, ".cache"), os.path.join(self.home, "Desktop"),
                  os.path.join(self.home, "Downloads"), os.path.dirname(self.fakebin_log), self.fakebin, self.dumps):
            os.makedirs(d, exist_ok=True)
        os.chmod(self.run_dir, 0o700)
        # fake binaries
        self.write(os.path.join(self.fakebin, "steam"), FAKE_STEAM, 0o755)
        self.write(os.path.join(self.fakebin, "xdg-open"), FAKE_XDG_OPEN, 0o755)
        # steam root
        self.write(os.path.join(self.steam_root, "config", "config.vdf"), STEAM_CONFIG_VDF)
        self.write(self.localconfig, LOCALCONFIG_VDF)
        self.write(os.path.join(self.steam_root, "steamapps", "libraryfolders.vdf"),
                   '"libraryfolders"\n{\n\t"0"\n\t{\n\t\t"path"\t\t"%s"\n\t\t"label"\t\t""\n\t}\n'
                   '\t"1"\n\t{\n\t\t"path"\t\t"%s"\n\t\t"label"\t\t"Games"\n\t}\n}\n' % (self.steam_root, self.library2))
        # workshop state in library 2
        self.write(os.path.join(self.library2, "steamapps", "workshop", "content", APP_ID, "2994481309", "workshop.json"),
                   '{"Title":"All-around Enhancement Lite"}\n')
        self.write(os.path.join(self.library2, "steamapps", "workshop", f"appworkshop_{APP_ID}.acf"), APPWORKSHOP_ACF)
        # game install
        g = self.game_dir
        self.write(os.path.join(g, "BlackOps3.exe"), b"MZ" + det_bytes("bo3-exe", 16 * 1024))
        self.write(os.path.join(g, "d3dcompiler_46.dll"), b"MZ" + det_bytes("d3dcompiler_46", 4096))
        self.write(os.path.join(g, "steam_api64.dll"), b"MZ" + det_bytes("steam_api64", 2048))
        self.write(os.path.join(g, "installscript_311210.vdf"), '"InstallScript"\n{\n}\n')
        self.write(os.path.join(g, "players", "config.ini"), build_config_ini())
        for name in ("BO3_Global_Logo_LogoSequence.mkv", "BO3_Global_Logo_IW.mkv", "BO3_Global_Logo_Treyarch.mkv",
                     "loadingscreen_zm.mkv"):
            self.write(os.path.join(g, "video", name), det_bytes("mkv:" + name, 3000))
        self.write(os.path.join(g, "video", "keepme.bik"), det_bytes("bik", 500))
        self.write(os.path.join(g, "LPC", "lpc_core.ff"), det_bytes("lpc_core", 1500))
        self.write(os.path.join(g, "LPC", "lpc_extra.ff"), det_bytes("lpc_extra", 1500))
        # secondary game copy (browse/set-directory scenarios) and dump sources for Enhanced validation
        self.write(os.path.join(self.alt_dir, "BlackOpsIII.exe"), b"MZ" + det_bytes("alt-exe", 1024))
        self.write(os.path.join(self.dumps, "valid-dump", "DUMP", "BlackOps3.exe"), b"MZ" + det_bytes("dump-exe", 512))
        self.write(os.path.join(self.dumps, "valid-dump", "DUMP", "MicrosoftGame.config"), "<Game/>\n")
        self.write(os.path.join(self.dumps, "invalid-dump", "readme.md"), "not a dump\n")
        # a pre-existing log line? No: PatchOpsIII creates its own app-data dir on first write.
        self.chown_tree(self.root)
        return self

    # ---- workload mutations (simulate state that normally comes from downloads) ----
    def fx(self, name: str, **kw):
        getattr(self, "fx_" + name)(**kw)

    def fx_install_fake_t7(self):
        g = self.game_dir
        self.write(os.path.join(g, "t7patch.dll"), b"MZ" + det_bytes("t7patch.dll", 1024))
        self.write(os.path.join(g, "t7patchloader.dll"), b"MZ" + det_bytes("t7patchloader.dll", 1024))
        self.write(os.path.join(g, "dsound.dll"), b"MZ" + det_bytes("dsound.dll", 512))
        self.write(os.path.join(g, "t7patch.conf"), "playername=^1BenchPlayer\nnetworkpassword=\nisfriendsonly=0\nunrelated=keep\n")
        # LPC: originals backed up by the "installer", replacement installed
        for n in ("lpc_core.ff", "lpc_extra.ff"):
            p = os.path.join(g, "LPC", n)
            if os.path.exists(p):
                os.rename(p, p + ".patchops.bak")
            self.write(p, det_bytes("t7-lpc:" + n, 1200))
        self.write(os.path.join(self.mod_files, "T7Patch.zip"), det_bytes("zip", 256))
        self.write(os.path.join(self.mod_files, "T7Patch_extracted", "t7patch.dll"), "x")
        self.chown_tree(g)

    def fx_install_fake_dxvk(self):
        g = self.game_dir
        self.write(os.path.join(g, "dxgi.dll"), b"MZ" + det_bytes("dxgi", 800))
        self.write(os.path.join(g, "d3d11.dll"), b"MZ" + det_bytes("d3d11", 800))
        self.write(os.path.join(g, "dxvk.conf"),
                   "dxvk.enableAsync=true\ndxvk.gplAsyncCache=true\ndxvk.numCompilerThreads=4\n"
                   "dxgi.maxFrameRate=144\ndxgi.maxFrameLatency=2\ndxvk.tearFree=Auto\ndxvk.hud=fps\n")

    def fx_install_fake_enhanced(self):
        g = self.game_dir
        for n in ("T7WSBootstrapper.dll", "T7InternalWS.dll", "steam_api65.dll", "WindowsCodecs.dll"):
            self.write(os.path.join(g, n), b"MZ" + det_bytes("enh:" + n, 600))
        # Enhanced replaced a stock file and kept the original as a PatchOps backup
        self.write(os.path.join(g, "steam_api64.dll.patchops.bak"), b"MZ" + det_bytes("steam_api64-orig", 2048))

    def fx_place_mod_junk(self):
        self.write(os.path.join(self.mod_files, "junk.zip"), det_bytes("junk", 100))
        self.write(os.path.join(self.mod_files, "sub", "nested", "file.bin"), det_bytes("nested", 100))
        self.mkdir(os.path.join(self.mod_files, "emptydir"))

    def fx_seed_enhanced_hash(self):
        """Mark the fixture executable's SHA-256 as a known PatchOps-preserved Enhanced EXE (settings file edit)."""
        exe = os.path.join(self.game_dir, "BlackOps3.exe")
        digest = hashlib.sha256(open(exe, "rb").read()).hexdigest()
        path = os.path.join(self.app_data, "electron-settings.json")
        try:
            settings = json.load(open(path))
        except Exception:
            settings = {}
        hashes = settings.get("enhanced_exe_hashes") or {}
        hashes[self.game_dir] = [digest]
        settings["enhanced_exe_hashes"] = hashes
        self.write(path, json.dumps(settings, indent=2))

    def fx_unseed_enhanced_hash(self):
        path = os.path.join(self.app_data, "electron-settings.json")
        try:
            settings = json.load(open(path))
        except Exception:
            return
        settings.pop("enhanced_exe_hashes", None)
        self.write(path, json.dumps(settings, indent=2))

    def fx_remove_video_dir(self):
        self.rm(os.path.join(self.game_dir, "video"))

    def fx_restore_video_dir(self):
        for name in ("BO3_Global_Logo_LogoSequence.mkv", "BO3_Global_Logo_IW.mkv", "BO3_Global_Logo_Treyarch.mkv",
                     "loadingscreen_zm.mkv"):
            self.write(os.path.join(self.game_dir, "video", name), det_bytes("mkv:" + name, 3000))
        self.write(os.path.join(self.game_dir, "video", "keepme.bik"), det_bytes("bik", 500))

    def fx_remove_localconfig(self):
        self.rm(self.localconfig)

    def fx_restore_localconfig(self):
        self.write(self.localconfig, LOCALCONFIG_VDF)

    def fx_noop(self):
        pass

    def fx_reset_fixture(self):
        self.reset_fixture()

    def read_steam_calls(self) -> list[str]:
        try:
            return open(self.fakebin_log).read().splitlines()
        except OSError:
            return []

    def destroy(self):
        shutil.rmtree(self.root, ignore_errors=True)


def _fx_place_fake_depot(self):
    d = os.path.join(self.library2, "steamapps", "content", "app_311210", "depot_311211")
    self.write(os.path.join(d, "BlackOps3.exe"), b"MZ" + det_bytes("fake-depot-exe", 2048))
    self.write(os.path.join(d, "installscript_311210.vdf"), '"InstallScript"\n{\n}\n')


Sandbox.fx_place_fake_depot = _fx_place_fake_depot
