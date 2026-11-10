"""The capability workload: a deterministic parity pass (with golden snapshots) and a latency pass.

Both targets execute exactly the same steps through the same op names (see OPS.md).
Args may contain placeholders: {sb} {home} {game_dir} {common} {alt} {valid_dump} {invalid_dump}.
"""
from __future__ import annotations

import json
import os
import statistics
import time
from dataclasses import dataclass, field

from . import procfs, snapshot as snap

STEAM = "steam"      # op sleeps/polls for the (fake) Steam client in the legacy implementation (~5-20 s)
QUIRK = "quirk"      # legacy behaviour that is surprising; port must decide whether to replicate
NETWORK = "network"  # needs the internet; excluded from headline numbers


@dataclass
class Step:
    label: str
    op: str
    args: dict = field(default_factory=dict)
    fx: tuple = ()
    tags: tuple = ()
    note: str = ""


def S(label, op, args=None, fx=(), tags=(), note=""):
    return Step(label, op, args or {}, tuple(fx), tuple(tags), note)


def placeholders(sb) -> dict:
    return {
        "sb": sb.root,
        "home": sb.home,
        "game_dir": sb.game_dir,
        "common": os.path.dirname(sb.game_dir),
        "alt": sb.alt_dir,
        "valid_dump": os.path.join(sb.dumps, "valid-dump"),
        "invalid_dump": os.path.join(sb.dumps, "invalid-dump"),
    }


def expand(obj, ph: dict):
    if isinstance(obj, str):
        out = obj
        for k, v in ph.items():
            out = out.replace("{" + k + "}", v)
        return out
    if isinstance(obj, list):
        return [expand(x, ph) for x in obj]
    if isinstance(obj, dict):
        return {k: expand(v, ph) for k, v in obj.items()}
    return obj


def _config_steps():
    st = []
    ui = [
        ("MaxFPS", 240, "Maximum FPS cap"),
        ("FOV", 100, "Field of view"),
        ("FullScreenMode", 2, "0=Windowed,1=Fullscreen,2=Fullscreen Windowed"),
        ("WindowSize", "2560x1440", "any text"),
        ("RefreshRate", 144, "1 to 240"),
        ("ResolutionPercent", 125, "50 to 200"),
        ("Vsync", "0", "Vertical sync"),
        ("DrawFPS", "1", "FPS counter"),
        ("SmoothFramerate", "1", "Frame smoothing"),
        ("RestrictGraphicsOptions", "0", "Expose all graphics options"),
        ("SerializeRender", "2", "Reduce CPU pressure"),
        ("MaxFrameLatency", 3, "Maximum frame latency"),
    ]
    for key, val, comment in ui:
        st.append(S(f"config-{key}", "config", {"key": key, "value": val, "comment": comment}))
    st += [
        S("config-append-new-key", "config", {"key": "BenchNewKey", "value": "abc", "comment": "appended"}),
        S("config-bool-value", "config", {"key": "BenchBool", "value": True}, note="JSON true is written as 'True'"),
        S("config-float-value", "config", {"key": "BenchFloat", "value": 1.0}, note="JSON 1.0 is written as '1.0'"),
        S("config-default-comment", "config", {"key": "BenchDefault", "value": 7}),
        S("config-backslash-n", "config", {"key": "BenchEsc", "value": "a\\nb", "comment": "c"}, tags=(QUIRK,),
          note="value is used as a re.sub replacement template: a backslash-n becomes a real newline when the key already exists; first call appends"),
        S("config-backslash-n-existing", "config", {"key": "BenchEsc", "value": "x\\ny", "comment": "c"}, tags=(QUIRK,)),
        S("config-bad-escape", "config", {"key": "BenchEsc", "value": "\\q", "comment": "c"}, tags=(QUIRK,),
          note="re.error 'bad escape' text is returned verbatim in error"),
        S("config-empty-key", "config", {"key": "", "value": 1}),
        S("config-missing-value", "config", {"key": "X"}),
        S("config-null-value", "config", {"key": "X", "value": None}),
    ]
    return st


def parity_steps() -> list[Step]:
    s: list[Step] = []
    a = s.append
    a(S("health", "health"))
    a(S("status-initial", "status"))
    a(S("browse-default", "browse"))
    a(S("browse-common-dir", "browse", {"path": "{common}"}))
    a(S("browse-game-dir", "browse", {"path": "{game_dir}"}))
    a(S("browse-home", "browse", {"path": "{home}"}))
    a(S("browse-missing-dir", "browse", {"path": "{sb}/does/not/exist"}))
    a(S("game-directory-not-a-game", "game-directory", {"path": "{home}"}))
    a(S("game-directory-missing-arg", "game-directory", {}))
    a(S("game-directory-empty", "game-directory", {"path": ""}))
    a(S("game-directory-alt-install", "game-directory", {"path": "{alt}"}))
    a(S("status-alt-install", "status"))
    a(S("config-without-config-ini", "config", {"key": "MaxFPS", "value": 60}))
    a(S("config-readonly-without-config-ini", "config-readonly", {"enabled": True}))
    a(S("browse-after-saved-dir", "browse"))
    a(S("game-directory-main", "game-directory", {"path": "{game_dir}"}))
    a(S("status-main", "status"))
    a(S("release-channel-beta", "release-channel", {"channel": "beta"}))
    a(S("release-channel-invalid", "release-channel", {"channel": "nightly"}))
    a(S("release-channel-missing", "release-channel", {}))
    a(S("release-channel-stable", "release-channel", {"channel": " STABLE "}))
    s += _config_steps()
    a(S("vram-target-limited-80", "vram-target", {"limited": True, "target": 80}))
    a(S("vram-target-limited-100", "vram-target", {"limited": True, "target": 100}))
    a(S("vram-target-limited-default", "vram-target", {"limited": True}))
    a(S("vram-target-full", "vram-target", {"limited": False, "target": 75}))
    a(S("vram-target-too-low", "vram-target", {"limited": True, "target": 50}))
    a(S("vram-target-too-high", "vram-target", {"limited": True, "target": 101}))
    a(S("vram-target-missing-limited", "vram-target", {}))
    a(S("preset-smooth", "presets/apply", {"name": "Smooth Preset"}))
    a(S("preset-low-latency", "presets/apply", {"name": "Low-Latency Preset"}))
    a(S("preset-unknown", "presets/apply", {"name": "Nope"}, note="KeyError text is returned as \"'Nope'\""))
    a(S("preset-missing-name", "presets/apply", {}))
    a(S("intro-skip-on", "intro-skip", {"enabled": True}))
    a(S("intro-skip-on-again", "intro-skip", {"enabled": True}))
    a(S("intro-skip-off", "intro-skip", {"enabled": False}))
    a(S("intro-skip-off-again", "intro-skip", {"enabled": False}))
    a(S("intro-skip-bad-arg", "intro-skip", {"enabled": "maybe"}))
    a(S("intro-skip-on-then-all", "intro-skip", {"enabled": True}))
    a(S("all-intros-on", "all-intros-skip", {"enabled": True}))
    a(S("all-intros-on-again", "all-intros-skip", {"enabled": True}))
    a(S("status-all-intros", "status"))
    a(S("all-intros-off", "all-intros-skip", {"enabled": False}))
    a(S("all-intros-off-again", "all-intros-skip", {"enabled": False}))
    a(S("all-intros-no-video-dir", "all-intros-skip", {"enabled": True}, fx=("remove_video_dir",)))
    a(S("intro-skip-no-video-dir", "intro-skip", {"enabled": True}))
    a(S("restore-video-dir", "status", fx=("restore_video_dir",)))
    a(S("d3dcompiler-on", "d3dcompiler", {"enabled": True}))
    a(S("d3dcompiler-on-again", "d3dcompiler", {"enabled": True}))
    a(S("d3dcompiler-off", "d3dcompiler", {"enabled": False}))
    a(S("d3dcompiler-off-again", "d3dcompiler", {"enabled": False}))
    a(S("config-readonly-on", "config-readonly", {"enabled": True}))
    a(S("status-readonly", "status", note="configReadonly uses os.access(W_OK): always false when running as root"))
    a(S("config-write-while-readonly", "config", {"key": "MaxFPS", "value": 99}, tags=(QUIRK,),
        note="PermissionError text returned as error (non-root)"))
    a(S("preset-while-readonly", "presets/apply", {"name": "Smooth Preset"}, tags=(QUIRK,)))
    a(S("vram-while-readonly", "vram-target", {"limited": False}, tags=(QUIRK,)))
    a(S("config-readonly-off", "config-readonly", {"enabled": False}))
    a(S("config-after-readonly-off", "config", {"key": "MaxFPS", "value": 165, "comment": "Maximum FPS cap"}))
    # --- launch options / Steam (fake client on PATH) ---
    a(S("launch-options-offline", "launch-options", {"options": "+set fs_game offlinemp", "preserve_fs_game": False}, tags=(STEAM,)))
    a(S("launch-options-unsupported", "launch-options", {"options": "+set fs_game 123"}))
    a(S("launch-options-workshop-all-around", "launch-options", {"options": "+set fs_game 2994481309"}, tags=(STEAM,)))
    a(S("launch-options-preserve-fs-game", "launch-options", {"options": "", "preserve_fs_game": True}, tags=(STEAM,)))
    a(S("launch-options-default", "launch-options", {"options": ""}, tags=(STEAM,)))
    a(S("workshop-install-unknown", "workshop-install", {"profileId": "nope"}))
    a(S("workshop-install-empty-id", "workshop-install", {"profileId": ""}))
    a(S("workshop-install-ultimate", "workshop-install", {"profileId": "ultimate"}, tags=(STEAM,)))
    a(S("status-launch-profiles", "status"))
    a(S("launch", "launch"))
    # --- T7 patch (config editing only; installs need the network) ---
    a(S("t7-config-without-conf", "t7-config", {"gamertag": "Bench", "colorCode": "^5"}))
    a(S("t7-password-without-conf", "t7-config", {"networkPassword": "pw"}))
    a(S("t7-friends-without-conf", "t7-config", {"friendsOnly": True}))
    a(S("status-fake-t7", "status", fx=("install_fake_t7",)))
    a(S("t7-gamertag", "t7-config", {"gamertag": "Bench", "colorCode": "^5"}))
    a(S("t7-gamertag-trim", "t7-config", {"gamertag": "  Spaced Name  ", "colorCode": ""}))
    a(S("t7-gamertag-empty", "t7-config", {"gamertag": "   ", "colorCode": ""}))
    a(S("t7-gamertag-too-long", "t7-config", {"gamertag": "A" * 21, "colorCode": "^1"}))
    a(S("t7-password-set", "t7-config", {"networkPassword": "hunter2"}))
    a(S("t7-password-trim", "t7-config", {"networkPassword": "  padded  "}))
    a(S("t7-password-clear", "t7-config", {"networkPassword": ""}))
    a(S("t7-friends-on", "t7-config", {"friendsOnly": True}))
    a(S("t7-friends-off", "t7-config", {"friendsOnly": False}))
    a(S("t7-all-fields", "t7-config", {"gamertag": "Combo", "colorCode": "^2", "networkPassword": "pw2", "friendsOnly": True}))
    a(S("t7-no-fields", "t7-config", {}))
    a(S("t7-colorcode-only", "t7-config", {"colorCode": "^3"}))
    a(S("t7-uninstall", "t7-uninstall"))
    a(S("t7-uninstall-again", "t7-uninstall"))
    a(S("t7-config-after-uninstall", "t7-config", {"gamertag": "Gone"}))
    # --- DXVK (config editing / uninstall; install needs the network) ---
    a(S("dxvk-config-without-install", "dxvk-config", {}, note="writes dxvk.conf even if DXVK is not installed"))
    a(S("dxvk-uninstall-conf-only", "dxvk-uninstall"))
    a(S("status-fake-dxvk", "status", fx=("install_fake_dxvk",)))
    a(S("dxvk-config-recommended", "dxvk-config", {"enableAsync": True, "gplAsyncCache": True, "numCompilerThreads": 0,
                                                    "maxFrameRate": 0, "maxFrameLatency": 1, "tearFree": "True", "hudEnabled": False}))
    a(S("dxvk-config-tuned", "dxvk-config", {"enableAsync": False, "gplAsyncCache": False, "numCompilerThreads": 8,
                                              "maxFrameRate": 144, "maxFrameLatency": 3, "tearFree": "False", "hudEnabled": True}))
    a(S("status-dxvk-tuned", "status"))
    a(S("dxvk-config-bogus-tearfree", "dxvk-config", {"tearFree": "Sometimes"}, note="unknown tearFree falls back to 'Auto'"))
    a(S("dxvk-config-threads-too-high", "dxvk-config", {"numCompilerThreads": 65}))
    a(S("dxvk-config-fps-too-high", "dxvk-config", {"maxFrameRate": 361}))
    a(S("dxvk-config-latency-too-high", "dxvk-config", {"maxFrameLatency": 17}))
    a(S("dxvk-config-negative", "dxvk-config", {"numCompilerThreads": -1}))
    a(S("dxvk-uninstall", "dxvk-uninstall"))
    a(S("dxvk-uninstall-again", "dxvk-uninstall"))
    # --- Enhanced + EXE swap (no downloads) ---
    a(S("enhanced-validate-empty", "enhanced-validate", {"dumpSource": "   "}))
    a(S("enhanced-validate-valid", "enhanced-validate", {"dumpSource": "{valid_dump}"}))
    a(S("enhanced-validate-invalid", "enhanced-validate", {"dumpSource": "{invalid_dump}"}))
    a(S("enhanced-validate-missing-path", "enhanced-validate", {"dumpSource": "{sb}/nope"}))
    a(S("enhanced-validate-missing-arg", "enhanced-validate", {}))
    a(S("enhanced-install-bad-source", "enhanced-install", {"dumpSource": "{sb}/nope"}))
    a(S("enhanced-install-invalid-source", "enhanced-install", {"dumpSource": "{invalid_dump}"}))
    a(S("exe-swap-compatible-unverified", "exe-swap/compatible"))
    a(S("exe-swap-depot-absent", "exe-swap/compatible-depot"))
    a(S("exe-swap-current-unverified", "exe-swap/current"))
    a(S("exe-swap-enhanced-unverified", "exe-swap/enhanced"))
    a(S("status-fake-enhanced", "status", fx=("install_fake_enhanced",)))
    a(S("exe-swap-compatible-enhanced-active", "exe-swap/compatible"))
    a(S("exe-swap-current-enhanced-active", "exe-swap/current"))
    a(S("exe-swap-enhanced-already-active", "exe-swap/enhanced"))
    a(S("enhanced-uninstall", "enhanced-uninstall", tags=(STEAM,)))
    a(S("enhanced-uninstall-again", "enhanced-uninstall", tags=(STEAM,)))
    a(S("status-enhanced-removed", "status"))
    a(S("exe-swap-depot-fake-present", "exe-swap/compatible-depot", fx=("place_fake_depot",)))
    a(S("exe-swap-compatible-depot-bad-hash", "exe-swap/compatible"))
    a(S("exe-swap-compatible-known-hash", "exe-swap/compatible", fx=("seed_enhanced_hash",)))
    a(S("exe-swap-current-known-hash", "exe-swap/current"))
    a(S("exe-swap-enhanced-known-hash", "exe-swap/enhanced"))
    a(S("status-known-enhanced-hash", "status"))
    a(S("unseed-hash", "status", fx=("unseed_enhanced_hash",)))
    # --- maintenance ---
    a(S("logs-payload", "logs/payload"))
    a(S("logs-clear", "logs/clear"))
    a(S("logs-payload-after-clear", "logs/payload"))
    a(S("mod-files-clear", "mod-files/clear", fx=("place_mod_junk",)))
    a(S("mod-files-clear-empty", "mod-files/clear"))
    # --- reset to stock: everything modded at once ---
    a(S("status-everything-modded", "status", fx=("install_fake_t7", "install_fake_dxvk", "install_fake_enhanced", "place_mod_junk")))
    a(S("pre-reset-intros", "all-intros-skip", {"enabled": True}))
    a(S("pre-reset-d3d", "d3dcompiler", {"enabled": True}))
    a(S("pre-reset-preset", "presets/apply", {"name": "Low-Latency Preset"}))
    a(S("pre-reset-vram", "vram-target", {"limited": True, "target": 85}))
    a(S("pre-reset-launch-options", "launch-options", {"options": "+set fs_game 2942053577"}, tags=(STEAM,)))
    a(S("reset-stock", "reset-stock", tags=(STEAM,)))
    a(S("status-after-reset", "status"))
    a(S("reset-stock-again", "reset-stock", tags=(STEAM,)))
    a(S("status-final", "status"))
    return s


def network_steps() -> list[Step]:
    n = (NETWORK,)
    return [
        S("net-update-check-stable", "update-check", tags=n),
        S("net-release-channel-beta", "release-channel", {"channel": "beta"}),
        S("net-update-check-beta", "update-check", tags=n),
        S("net-release-channel-stable", "release-channel", {"channel": "stable"}),
        S("net-dxvk-install", "dxvk-install", {"tearFree": "True"}, tags=n),
        S("net-dxvk-uninstall", "dxvk-uninstall", tags=n),
        S("net-t7-install", "t7-install", tags=n, fx=("seed_enhanced_hash",)),
        S("net-t7-uninstall", "t7-uninstall", tags=n),
        S("net-enhanced-install", "enhanced-install", {"dumpSource": "{valid_dump}"}, tags=n),
        S("net-enhanced-uninstall", "enhanced-uninstall", tags=n + (STEAM,)),
    ]


# ---------------------------------------------------------------- latency groups
@dataclass
class Group:
    name: str
    n: int
    calls: list            # [(label, op, args)]
    pre: tuple = ()        # fixture mutations once before the group
    setup: tuple = ()      # before every iteration (untimed)
    post: tuple = ()
    tags: tuple = ()


def latency_groups() -> list[Group]:
    g: list[Group] = []
    c = lambda op, args=None, label=None: (label or op, op, args or {})
    g.append(Group("health", 100, [c("health")]))
    g.append(Group("status", 60, [c("status")]))
    g.append(Group("status-modded", 40, [c("status", label="status(modded)")],
                   pre=("install_fake_t7", "install_fake_dxvk", "install_fake_enhanced"), post=("reset_fixture",)))
    g.append(Group("browse", 40, [c("browse"), c("browse", {"path": "{game_dir}"}, "browse(game)"),
                                  c("browse", {"path": "{common}"}, "browse(common)")]))
    g.append(Group("game-directory", 40, [c("game-directory", {"path": "{game_dir}"})]))
    g.append(Group("release-channel", 30, [c("release-channel", {"channel": "beta"}, "release-channel(beta)"),
                                           c("release-channel", {"channel": "stable"}, "release-channel(stable)")]))
    keys = [("MaxFPS", 240), ("FOV", 100), ("FullScreenMode", 2), ("WindowSize", "2560x1440"), ("RefreshRate", 144),
            ("ResolutionPercent", 125), ("Vsync", "0"), ("DrawFPS", "1"), ("SmoothFramerate", "1"),
            ("RestrictGraphicsOptions", "0"), ("SerializeRender", "2"), ("MaxFrameLatency", 3)]
    g.append(Group("config", 30, [c("config", {"key": k, "value": v, "comment": "bench"}, f"config({k})") for k, v in keys]))
    g.append(Group("vram-target", 30, [c("vram-target", {"limited": True, "target": 80}, "vram-target(limited)"),
                                       c("vram-target", {"limited": False, "target": 75}, "vram-target(full)")]))
    g.append(Group("presets", 30, [c("presets/apply", {"name": "Smooth Preset"}, "presets/apply(Smooth)"),
                                   c("presets/apply", {"name": "Low-Latency Preset"}, "presets/apply(Low-Latency)")]))
    g.append(Group("intro-skip", 30, [c("intro-skip", {"enabled": True}, "intro-skip(on)"), c("intro-skip", {"enabled": False}, "intro-skip(off)")]))
    g.append(Group("all-intros-skip", 30, [c("all-intros-skip", {"enabled": True}, "all-intros-skip(on)"),
                                           c("all-intros-skip", {"enabled": False}, "all-intros-skip(off)")]))
    g.append(Group("d3dcompiler", 30, [c("d3dcompiler", {"enabled": True}, "d3dcompiler(on)"), c("d3dcompiler", {"enabled": False}, "d3dcompiler(off)")]))
    g.append(Group("config-readonly", 30, [c("config-readonly", {"enabled": True}, "config-readonly(on)"),
                                           c("config-readonly", {"enabled": False}, "config-readonly(off)")]))
    g.append(Group("t7-config", 30, [c("t7-config", {"gamertag": "Bench", "colorCode": "^5"}, "t7-config(gamertag)"),
                                     c("t7-config", {"networkPassword": "hunter2"}, "t7-config(password)"),
                                     c("t7-config", {"friendsOnly": True}, "t7-config(friends)")], pre=("install_fake_t7",)))
    g.append(Group("dxvk-config", 30, [c("dxvk-config", {"tearFree": "True"}, "dxvk-config(default)"),
                                       c("dxvk-config", {"enableAsync": False, "numCompilerThreads": 8, "maxFrameRate": 144, "tearFree": "False", "hudEnabled": True}, "dxvk-config(tuned)")],
                   pre=("install_fake_dxvk",)))
    g.append(Group("enhanced-validate", 30, [c("enhanced-validate", {"dumpSource": "{valid_dump}"}, "enhanced-validate(valid)"),
                                             c("enhanced-validate", {"dumpSource": "{invalid_dump}"}, "enhanced-validate(invalid)")]))
    g.append(Group("exe-swap", 30, [c("exe-swap/compatible"), c("exe-swap/compatible-depot"), c("exe-swap/current"), c("exe-swap/enhanced")]))
    g.append(Group("logs-payload", 30, [c("logs/payload")]))
    g.append(Group("logs-clear", 30, [c("logs/clear")]))
    g.append(Group("mod-files-clear", 30, [c("mod-files/clear")], setup=("place_mod_junk",)))
    g.append(Group("t7-uninstall", 20, [c("t7-uninstall")], setup=("install_fake_t7",)))
    g.append(Group("dxvk-uninstall", 20, [c("dxvk-uninstall")], setup=("install_fake_dxvk",)))
    g.append(Group("launch", 15, [c("launch")]))
    # steam-dependent (slow in the legacy implementation: close_steam sleeps 5 s, open_steam polls >= 2 s)
    st = (STEAM,)
    g.append(Group("launch-options", 3, [c("launch-options", {"options": "+set fs_game offlinemp"}, "launch-options(offline)"),
                                         c("launch-options", {"options": ""}, "launch-options(default)")], tags=st))
    g.append(Group("workshop-install", 2, [c("workshop-install", {"profileId": "reforged"})], tags=st))
    g.append(Group("enhanced-uninstall", 2, [c("enhanced-uninstall")], setup=("install_fake_enhanced",), tags=st))
    g.append(Group("reset-stock", 2, [c("reset-stock")], setup=("install_fake_t7", "install_fake_dxvk", "install_fake_enhanced"), tags=st))
    return g


# ---------------------------------------------------------------- runners
def stats_ms(samples_ms: list[float]) -> dict:
    if not samples_ms:
        return {"n": 0}
    s = sorted(samples_ms)

    def pct(p):
        if len(s) == 1:
            return s[0]
        k = (len(s) - 1) * p
        lo = int(k)
        hi = min(lo + 1, len(s) - 1)
        return s[lo] + (s[hi] - s[lo]) * (k - lo)

    return {"n": len(s), "min": round(s[0], 3), "median": round(statistics.median(s), 3), "mean": round(statistics.fmean(s), 3),
            "p95": round(pct(0.95), 3), "p99": round(pct(0.99), 3), "max": round(s[-1], 3)}


def _app_ok(resp):
    if resp.envelope_ok and isinstance(resp.result, dict):
        return resp.result.get("ok")
    return None


FIXTURE_DOTDIRS = {".cache", ".config", ".local", ".steam"}


def _filter_browse(body: dict):
    """Chromium creates extra dot-directories (e.g. ~/.pki) in HOME in GUI runs; ignore dot-dirs the fixture did not create."""
    res = body.get("result") if isinstance(body, dict) else None
    if isinstance(res, dict) and isinstance(res.get("entries"), list):
        res["entries"] = [e for e in res["entries"] if not (e["name"].startswith(".") and e["name"] not in FIXTURE_DOTDIRS)]


def _golden_name(i: int, label: str) -> str:
    return f"{i:03d}-{label}.json"


def run_parity(client, sb, golden_dir: str, mode: str, steps: list[Step] | None = None, log=print, group="core") -> dict:
    """mode: 'compare' (default), 'update' (write golden), 'none'."""
    steps = steps if steps is not None else parity_steps()
    ph = placeholders(sb)
    os.makedirs(golden_dir, exist_ok=True)
    prev_texts: dict = {}
    prev = snap.snapshot(sb.root, prev_texts)
    initial_digest = snap.digest(prev)
    gold_initial = os.path.join(golden_dir, "000-initial-fs.json")
    results = []
    mism_total = 0
    t_pass = time.perf_counter()
    lat_sum = lat_steam = 0.0
    if mode == "update":
        for f in os.listdir(golden_dir):
            if f[:3].isdigit() and f.endswith(".json"):
                os.unlink(os.path.join(golden_dir, f))
        json.dump({"fs": prev, "digest": initial_digest, "texts": prev_texts}, open(gold_initial, "w"), indent=1, sort_keys=True)
    gold_prev_ok = True
    if mode == "compare":
        try:
            g0 = json.load(open(gold_initial))
            if g0["digest"] != initial_digest:
                results.append({"i": 0, "label": "initial-fs", "match": False, "mismatch": ["initial fixture differs from golden"]})
                mism_total += 1
        except OSError:
            gold_prev_ok = False
    for i, step in enumerate(steps, start=1):
        for fx in step.fx:
            sb.fx(fx)
        if step.fx:
            prev_texts = {}
            prev = snap.snapshot(sb.root, prev_texts)
        args = expand(step.args, ph)
        resp = client.call(step.op, args)
        after_texts: dict = {}
        after = snap.snapshot(sb.root, after_texts)
        d = snap.diff(prev, after)
        entry = {
            "i": i, "label": step.label, "op": step.op, "args": step.args, "fx": list(step.fx), "tags": list(step.tags),
            "latency_ms": round(resp.latency_s * 1000, 3), "envelope_ok": resp.envelope_ok, "app_ok": _app_ok(resp),
        }
        if resp.envelope_ok and isinstance(resp.result, dict) and resp.result.get("error"):
            entry["error"] = str(resp.result["error"])[:200]
        lat_sum += resp.latency_s
        if STEAM in step.tags:
            lat_steam += resp.latency_s
        body = snap.normalize_json(resp.body(), sb.root)
        if step.op == "browse":
            _filter_browse(body)
        if not resp.envelope_ok:
            entry["error"] = (resp.error or "")[:200]
        record = {
            "step": i, "label": step.label, "op": step.op, "args": step.args, "fx": list(step.fx), "tags": list(step.tags),
            "note": step.note, "response": body, "fs_diff": d, "fs_digest": snap.digest(after),
            "fs_text": snap.text_delta(prev_texts, after_texts, d),
        }
        path = os.path.join(golden_dir, _golden_name(i, step.label))
        if mode == "update":
            json.dump(record, open(path, "w"), indent=1, sort_keys=True)
        elif mode == "compare":
            mism = []
            try:
                gold = json.load(open(path))
            except OSError:
                gold = None
                mism.append("no golden file")
            if gold:
                if gold["op"] != step.op or gold["args"] != step.args:
                    mism.append("golden step definition differs (re-generate golden)")
                if gold["response"] != body:
                    mism.append("response: " + _short_diff(gold["response"], body))
                if gold["fs_diff"] != d:
                    mism.append("fs_diff: " + _short_diff(gold["fs_diff"], d))
                    mism.extend(_text_diffs(gold.get("fs_text") or {}, record["fs_text"]))
                elif gold["fs_digest"] != record["fs_digest"]:
                    mism.append("fs_digest differs (state diverged earlier)")
            entry["match"] = not mism
            if mism:
                entry["mismatch"] = mism
                mism_total += 1
        results.append(entry)
        prev, prev_texts = after, after_texts
        log(f"  [{i:3d}/{len(steps)}] {step.label:<44} {resp.latency_s*1000:9.1f} ms  envelope={resp.envelope_ok} ok={_app_ok(resp)}" +
            ("" if mode != "compare" or entry.get("match", True) else "  MISMATCH"))
    wall = time.perf_counter() - t_pass
    out = {
        "group": group, "mode": mode, "steps": len(steps), "wall_s": round(wall, 3), "sum_latency_s": round(lat_sum, 3),
        "steam_wait_latency_s": round(lat_steam, 3), "sum_latency_excl_steam_s": round(lat_sum - lat_steam, 3),
        "mismatches": mism_total if mode == "compare" else None, "golden_present": gold_prev_ok if mode == "compare" else None,
        "results": results,
    }
    if mode == "compare":
        quirk_m = sum(1 for r in results if not r.get("match", True) and QUIRK in r.get("tags", []))
        out["quirk_mismatches"] = quirk_m
    return out


def _text_diffs(gold_text: dict, new_text: dict, max_files=3, max_lines=12) -> list[str]:
    import difflib
    out = []
    for path in sorted(set(gold_text) | set(new_text)):
        g, n = gold_text.get(path), new_text.get(path)
        if g == n:
            continue
        def lines(v):
            if isinstance(v, dict):
                return v.get("appended", [])
            return (v or "").splitlines()
        d = list(difflib.unified_diff(lines(g), lines(n), "golden", "got", lineterm="", n=0))[2:max_lines]
        out.append(f"text diff {os.path.basename(path)}: " + " | ".join(d))
        if len(out) >= max_files:
            break
    return out


def _short_diff(a, b, limit=3) -> str:
    out = []

    def walk(x, y, path):
        if len(out) >= limit:
            return
        if isinstance(x, dict) and isinstance(y, dict):
            for k in sorted(set(x) | set(y)):
                if k not in x:
                    out.append(f"{path}/{k}: <absent> != {str(y[k])[:60]}")
                elif k not in y:
                    out.append(f"{path}/{k}: {str(x[k])[:60]} != <absent>")
                else:
                    walk(x[k], y[k], f"{path}/{k}")
        elif x != y:
            out.append(f"{path}: golden={str(x)[:70]} got={str(y)[:70]}")

    walk(a, b, "")
    return "; ".join(out) or "differs"


def run_latency(client, sb, scale: float = 1.0, groups: list[Group] | None = None, log=print, only: set | None = None, quiet=None, root_pid: int | None = None) -> dict:
    groups = groups if groups is not None else latency_groups()
    ph = placeholders(sb)
    labels: dict[str, dict] = {}
    gsum = []
    t_all = time.perf_counter()
    for grp in groups:
        if only and grp.name not in only:
            continue
        n = max(1, int(round(grp.n * scale)))
        for fx in grp.pre:
            sb.fx(fx)
        waited = quiet(grp.name) if quiet else 0.0
        meter = procfs.LoadMeter(root_pid)
        t0 = time.perf_counter()
        for _ in range(n):
            for fx in grp.setup:
                sb.fx(fx)
            for label, op, args in grp.calls:
                resp = client.call(op, expand(args, ph))
                rec = labels.setdefault(label, {"op": op, "group": grp.name, "tags": list(grp.tags), "samples_ms": [],
                                                "errors": 0, "app_not_ok": 0})
                rec["samples_ms"].append(round(resp.latency_s * 1000, 3))
                if not resp.envelope_ok:
                    rec["errors"] += 1
                elif _app_ok(resp) is False:
                    rec["app_not_ok"] += 1
        for fx in grp.post:
            sb.fx(fx)
        gsum.append({"group": grp.name, "iterations": n, "wall_s": round(time.perf_counter() - t0, 3), "tags": list(grp.tags),
                     "quiet_wait_s": round(waited, 1), **{k: v for k, v in meter.read().items()}})
        log(f"  [latency] {grp.name:<20} x{n:<4} {time.perf_counter()-t0:7.2f}s")
    for rec in labels.values():
        rec["stats"] = stats_ms(rec["samples_ms"])
    # aggregate per op
    per_op: dict[str, list[float]] = {}
    for label, rec in labels.items():
        per_op.setdefault(rec["op"] if rec["group"] != "status-modded" else "status(modded)", []).extend(rec["samples_ms"])
    return {"scale": scale, "wall_s": round(time.perf_counter() - t_all, 3), "groups": gsum, "labels": labels,
            "per_op": {k: stats_ms(v) for k, v in per_op.items()}}
