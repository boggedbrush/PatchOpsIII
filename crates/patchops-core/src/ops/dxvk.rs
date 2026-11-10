//! Group `dxvk`: `dxvk-config`, `dxvk-install`, `dxvk-uninstall` plus the `dxvk` state builder.
//!
//! Legacy reference: `backend/api.py` (`dxvk_config`, `install_dxvk`, `_install_dxvk`,
//! `uninstall_dxvk`, `_read_dxvk_settings`, `_dxvk_payload_to_settings`, `_write_dxvk_conf`,
//! `_uninstall_dxvk`) and `dxvk_manager.py` (`_build_dxvk_conf`, `_preset_settings`,
//! `_supports_gpl_async_cache`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{state_reply, Args, Group};
use crate::context::Core;
use crate::error::{AppError, AppResult, OpError};
use crate::net::{no_progress, DownloadOptions, Net, ProgressFn};
use crate::pyfmt::io_error_text;
use crate::state::{State, StateCx};

pub const OPS: &[&str] = &["dxvk-config", "dxvk-install", "dxvk-uninstall"];

pub const GROUP: Group = Group { name: "dxvk", ops: OPS, call };

pub const DXVK_ASYNC_FILES: [&str; 2] = ["dxgi.dll", "d3d11.dll"];

const CONF_FILE: &str = "dxvk.conf";
const TEAR_FREE_VALUES: [&str; 3] = ["Auto", "True", "False"];

fn call(core: &Core, op: &str, a: &Args) -> Result<Value, OpError> {
    match op {
        "dxvk-config" => {
            let settings = settings_from_args(a)?;
            state_reply(set_dxvk_config(core, settings))
        }
        "dxvk-uninstall" => state_reply(dxvk_uninstall(core)),
        "dxvk-install" => {
            let settings = settings_from_args(a)?;
            state_reply(dxvk_install(core, settings))
        }
        _ => Err(OpError::UnknownOp(op.to_string())),
    }
}

/// Body of `dxvk-config` (`DxvkConfigPayload`): defaults and ranges as in OPS.md section 3.
fn settings_from_args(a: &Args) -> Result<DxvkSettings, OpError> {
    Ok(DxvkSettings {
        enable_async: a.bool_or("enableAsync", true)?,
        gpl_async_cache: a.bool_or("gplAsyncCache", true)?,
        num_compiler_threads: a.int_range("numCompilerThreads", 0, 0, 64)?,
        max_frame_rate: a.int_range("maxFrameRate", 0, 0, 360)?,
        max_frame_latency: a.int_range("maxFrameLatency", 1, 0, 16)?,
        tear_free: a.str_or("tearFree", "True")?,
        hud_enabled: a.bool_or("hudEnabled", false)?,
    })
}

// ---- settings / conf file -------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DxvkSettings {
    pub enable_async: bool,
    pub gpl_async_cache: bool,
    pub num_compiler_threads: i64,
    pub max_frame_rate: i64,
    pub max_frame_latency: i64,
    pub tear_free: String,
    pub hud_enabled: bool,
}

impl Default for DxvkSettings {
    /// Legacy `_preset_settings("recommended")`.
    fn default() -> Self {
        DxvkSettings {
            enable_async: true,
            gpl_async_cache: true,
            num_compiler_threads: 0,
            max_frame_rate: 0,
            max_frame_latency: 1,
            tear_free: "True".into(),
            hud_enabled: false,
        }
    }
}

impl DxvkSettings {
    /// Legacy `_dxvk_payload_to_settings`: `tearFree` outside `Auto|True|False` becomes `Auto`.
    /// Apply before writing a conf file (the config and install paths both do).
    pub fn normalized(mut self) -> DxvkSettings {
        if !TEAR_FREE_VALUES.contains(&self.tear_free.as_str()) {
            self.tear_free = "Auto".into();
        }
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DxvkState {
    pub installed: bool,
    pub conf_exists: bool,
    pub settings: DxvkSettings,
}

/// Both DXVK DLLs exist in the game directory (legacy `is_dxvk_async_installed`).
pub fn files_installed(cx: &StateCx) -> bool {
    cx.game_dir.as_deref().is_some_and(|g| DXVK_ASYNC_FILES.iter().all(|f| std::path::Path::new(g).join(f).exists()))
}

/// Legacy `_build_dxvk_conf`: the text of `dxvk.conf` (always ends with `\n`).
/// `include_gpl_async_cache` is false for GPLAsync releases >= 2.7, which dropped the key.
pub fn build_conf(settings: &DxvkSettings, include_gpl_async_cache: bool) -> String {
    let py_bool = |b: bool| if b { "true" } else { "false" };
    let mut lines = vec![format!("dxvk.enableAsync={}", py_bool(settings.enable_async))];
    if include_gpl_async_cache && settings.gpl_async_cache {
        lines.push("dxvk.gplAsyncCache=true".into());
    }
    lines.push(format!("dxvk.numCompilerThreads={}", settings.num_compiler_threads));
    lines.push(format!("dxgi.maxFrameRate={}", settings.max_frame_rate));
    lines.push(format!("dxgi.maxFrameLatency={}", settings.max_frame_latency));
    lines.push(format!("dxvk.tearFree={}", settings.tear_free));
    if settings.hud_enabled {
        lines.push("dxvk.hud=fps,frametimes,gpuload".into());
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Legacy `_write_dxvk_conf`: (re)write `<game>/dxvk.conf` and log `Updated dxvk.conf ...`.
/// Errors are the Python exception text. Shared with `dxvk-install`.
pub fn write_conf(core: &Core, game_dir: &Path, settings: &DxvkSettings, include_gpl_async_cache: bool) -> Result<(), String> {
    let conf = game_dir.join(CONF_FILE);
    std::fs::write(&conf, build_conf(settings, include_gpl_async_cache)).map_err(|e| io_error_text(&e, Some(&conf)))?;
    core.log.success("Updated dxvk.conf from DXVK settings.");
    Ok(())
}

/// Legacy `_read_dxvk_settings`: defaults overlaid with `dxvk.conf`; a malformed integer stops
/// parsing (earlier lines stay applied) and logs a warning.
pub fn read_settings(core: &Core, game_dir: Option<&str>) -> DxvkSettings {
    let mut s = DxvkSettings::default();
    let Some(game) = game_dir else { return s };
    let conf = std::path::Path::new(game).join(CONF_FILE);
    let Ok(bytes) = std::fs::read(&conf) else { return s };
    let text = crate::config_ini::decode_ignore(&bytes);
    for line in text.split(['\n', '\r']) {
        if !line.contains('=') || line.trim().starts_with('#') {
            continue;
        }
        let (k, v) = line.split_once('=').unwrap_or((line, ""));
        let (key, value) = (k.trim(), v.trim());
        let lowered = value.to_lowercase();
        let int = |v: &str| v.parse::<i64>().map_err(|_| format!("invalid literal for int() with base 10: {}", crate::pyfmt::repr_str(v)));
        let r: Result<(), String> = (|| {
            match key {
                "dxvk.enableAsync" => s.enable_async = lowered == "true",
                "dxvk.gplAsyncCache" => s.gpl_async_cache = lowered == "true",
                "dxvk.numCompilerThreads" => s.num_compiler_threads = int(value)?,
                "dxgi.maxFrameRate" => s.max_frame_rate = int(value)?,
                "dxgi.maxFrameLatency" => s.max_frame_latency = int(value)?,
                "dxvk.tearFree" => s.tear_free = value.to_string(),
                "dxvk.hud" => s.hud_enabled = !value.is_empty(),
                _ => {}
            }
            Ok(())
        })();
        if let Err(e) = r {
            core.log.warning(format!("Failed to read dxvk.conf: {e}"));
            break;
        }
    }
    s
}

pub fn dxvk_state(cx: &StateCx) -> DxvkState {
    DxvkState {
        installed: files_installed(cx),
        conf_exists: cx.game_dir.as_deref().is_some_and(|g| std::path::Path::new(g).join(CONF_FILE).exists()),
        settings: read_settings(cx.core, cx.game_dir.as_deref()),
    }
}

// ---- typed handlers -------------------------------------------------------------------------

fn game_dir(core: &Core) -> AppResult<PathBuf> {
    core.game_dir().map(PathBuf::from).ok_or_else(|| AppError::new("Game directory is not set."))
}

/// Log an unexpected failure (legacy `write_log(f"...: {exc}", "Error")`) and return it as the app error.
fn fail(core: &Core, prefix: &str, msg: String) -> AppError {
    core.log.error(format!("{prefix}: {msg}"));
    AppError::new(msg)
}

/// Legacy `dxvk_config`: writes `dxvk.conf` even when DXVK is not installed.
pub fn set_dxvk_config(core: &Core, settings: DxvkSettings) -> AppResult<State> {
    let _op = core.lock_ops();
    let game = game_dir(core)?;
    let settings = settings.normalized();
    write_conf(core, &game, &settings, true).map_err(|e| fail(core, "Failed to update DXVK settings", e))?;
    Ok(State::build(core))
}

/// Legacy `_uninstall_dxvk`: remove the DLLs and `dxvk.conf` that exist, logging each removal.
/// Shared with reset-to-stock.
pub fn uninstall(core: &Core, game_dir: &Path) -> Result<(), String> {
    let _op = core.lock_ops();
    let mut changed = false;
    for file in DXVK_ASYNC_FILES {
        let target = game_dir.join(file);
        if target.exists() {
            remove(&target)?;
            changed = true;
            core.log.success(format!("Removed {file}."));
        }
    }
    let conf = game_dir.join(CONF_FILE);
    if conf.exists() {
        remove(&conf)?;
        changed = true;
        core.log.success("Removed dxvk.conf.");
    }
    if changed {
        core.log.success("DXVK-GPLAsync has been uninstalled.");
    } else {
        core.log.info("DXVK-GPLAsync was not installed.");
    }
    Ok(())
}

fn remove(path: &Path) -> Result<(), String> {
    std::fs::remove_file(path).map_err(|e| io_error_text(&e, Some(path)))
}

/// Legacy `uninstall_dxvk` endpoint.
pub fn dxvk_uninstall(core: &Core) -> AppResult<State> {
    let _op = core.lock_ops();
    let game = game_dir(core)?;
    uninstall(core, &game).map_err(|e| fail(core, "DXVK-GPLAsync uninstall failed", e))?;
    Ok(State::build(core))
}

// ---- dxvk-install ---------------------------------------------------------------------------

/// Pinned release of the binary tarball (legacy `DXVK_VERSION` / `DXVK_DOWNLOAD_URL`).
pub const DXVK_VERSION: &str = "v3.0-1";
const DXVK_COMMIT: &str = "40b31c3cee687e9bc01124e26032a31e9d9bb7c9";
/// SHA-256 of the pinned tarball. The legacy code does not verify the download; the URL is pinned
/// to a commit, so the bytes cannot legitimately change and we verify them (new check).
pub const DXVK_SHA256: &str = "f3d13925baa8124a3b0a6b7cd46ff57e9204426beb42f243dcc17267f7c2c0dc";

pub fn download_url(net: &Net) -> String {
    format!(
        "{}/Ph42oN/dxvk-gplasync/-/raw/{DXVK_COMMIT}/releases/dxvk-gplasync-{DXVK_VERSION}.tar.gz",
        net.endpoints.gitlab_web
    )
}

/// Legacy `_supports_gpl_async_cache`: `dxvk.gplAsyncCache` was removed in GPLAsync 2.7+. The first
/// `<digits>.<digits>` of the tag decides; no match means supported.
pub fn supports_gpl_async_cache(tag: &str) -> bool {
    let b = tag.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i + 1 < b.len() && b[i] == b'.' && b[i + 1].is_ascii_digit() {
            let minor_start = i + 1;
            let mut j = minor_start;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let num = |s: &str| s.parse::<u64>().unwrap_or(u64::MAX);
            return (num(&tag[start..i]), num(&tag[minor_start..j])) < (2, 7);
        }
    }
    true
}

/// First directory (pre-order, sorted; `x64` preferred) holding both DXVK DLLs. The legacy
/// `os.walk` took whichever of `x32`/`x64` the filesystem listed first; BO3 is 64-bit.
fn find_dll_dir(root: &Path) -> Option<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut subs = Vec::new();
        let mut files = Vec::new();
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            match e.file_type() {
                Ok(t) if t.is_dir() => subs.push(e.path()),
                Ok(_) => files.push(name),
                Err(_) => {}
            }
        }
        if DXVK_ASYNC_FILES.iter().all(|f| files.iter().any(|n| n == f)) {
            out.push(dir.to_path_buf());
        }
        subs.sort();
        for s in subs {
            walk(&s, out);
        }
    }
    let mut found = Vec::new();
    walk(root, &mut found);
    let x64 = found.iter().find(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("x64"))).cloned();
    x64.or_else(|| found.into_iter().next())
}

/// Legacy `install_dxvk` route against the real endpoints, without progress reporting.
pub fn dxvk_install(core: &Core, settings: DxvkSettings) -> AppResult<State> {
    dxvk_install_with(core, &Net::production(), settings, &mut no_progress)
}

/// Legacy `install_dxvk`: download + unpack the pinned DXVK-GPLAsync, copy the two DLLs, write
/// `dxvk.conf`. `progress` receives the download progress.
pub fn dxvk_install_with(core: &Core, net: &Net, settings: DxvkSettings, progress: ProgressFn<'_>) -> AppResult<State> {
    let _op = core.lock_ops();
    let game = game_dir(core)?;
    install_inner(core, net, &game, settings, progress).map_err(|e| fail(core, "DXVK-GPLAsync install failed", e))?;
    Ok(State::build(core))
}

fn install_inner(core: &Core, net: &Net, game: &Path, settings: DxvkSettings, progress: ProgressFn<'_>) -> Result<(), String> {
    let mod_files = &core.paths.mod_files;
    let extract_dir = mod_files.join("dxvk_extracted");
    let url = download_url(net);
    let archive = mod_files.join(url.rsplit('/').next().unwrap_or("dxvk-gplasync").split('?').next().unwrap_or("dxvk-gplasync"));
    std::fs::create_dir_all(mod_files).map_err(|e| io_error_text(&e, Some(mod_files)))?;
    if extract_dir.exists() {
        std::fs::remove_dir_all(&extract_dir).map_err(|e| io_error_text(&e, Some(&extract_dir)))?;
    }
    std::fs::create_dir_all(&extract_dir).map_err(|e| io_error_text(&e, Some(&extract_dir)))?;

    let result = (|| {
        core.log.info("Downloading DXVK-GPLAsync...");
        // The hash pin only applies to the real endpoint (tests serve their own bytes).
        let pins = if net.endpoints.gitlab_web == crate::net::Endpoints::production().gitlab_web { vec![DXVK_SHA256.to_string()] } else { Vec::new() };
        let opts = DownloadOptions::new(30).verify(pins);
        net.client.download(&url, &archive, &opts, progress).map_err(|e| e.to_string())?;
        core.log.success("Downloaded DXVK-GPLAsync successfully.");
        crate::archive::extract_archive(&archive, &extract_dir, Default::default())?;
        core.log.success("Extracted DXVK-GPLAsync successfully.");

        let source = find_dll_dir(&extract_dir).ok_or("Required DXVK files were not found in the archive.")?;
        for file in DXVK_ASYNC_FILES {
            let dst = game.join(file);
            crate::fsutil::copy2(&source.join(file), &dst).map_err(|e| io_error_text(&e, Some(&dst)))?;
            core.log.success(format!("Installed {file}."));
        }
        write_conf(core, game, &settings.normalized(), supports_gpl_async_cache(DXVK_VERSION))?;
        core.log.success("DXVK-GPLAsync installed successfully.");
        Ok(())
    })();
    let _ = std::fs::remove_file(&archive);
    let _ = std::fs::remove_dir_all(&extract_dir);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;
    use serde_json::json;

    fn setup(name: &str) -> (Core, PathBuf) {
        let root = std::env::temp_dir().join(format!("patchops-dxvk-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let game = root.join("game");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::create_dir_all(root.join("home")).unwrap();
        std::fs::write(game.join("BlackOps3.exe"), b"exe").unwrap();
        let core = Core::with_paths(Paths::with_app_data(root.join("home"), root.join("appdata")), "test".into());
        let game_s = game.to_string_lossy().into_owned();
        core.update_settings(|s| s.set_str("game_dir", game_s)).unwrap();
        (core, game)
    }

    #[test]
    fn conf_text_matches_legacy() {
        assert_eq!(
            build_conf(&DxvkSettings::default(), true),
            "dxvk.enableAsync=true\ndxvk.gplAsyncCache=true\ndxvk.numCompilerThreads=0\ndxgi.maxFrameRate=0\ndxgi.maxFrameLatency=1\ndxvk.tearFree=True\n"
        );
        let tuned = DxvkSettings {
            enable_async: false,
            gpl_async_cache: false,
            num_compiler_threads: 8,
            max_frame_rate: 144,
            max_frame_latency: 3,
            tear_free: "False".into(),
            hud_enabled: true,
        };
        assert_eq!(
            build_conf(&tuned, true),
            "dxvk.enableAsync=false\ndxvk.numCompilerThreads=8\ndxgi.maxFrameRate=144\ndxgi.maxFrameLatency=3\ndxvk.tearFree=False\ndxvk.hud=fps,frametimes,gpuload\n"
        );
        // Newer GPLAsync releases drop the key even when requested.
        assert!(!build_conf(&DxvkSettings::default(), false).contains("gplAsyncCache"));
    }

    #[test]
    fn tear_free_is_normalised() {
        let s = DxvkSettings { tear_free: "Sometimes".into(), ..DxvkSettings::default() }.normalized();
        assert_eq!(s.tear_free, "Auto");
        assert_eq!(DxvkSettings { tear_free: "False".into(), ..DxvkSettings::default() }.normalized().tear_free, "False");
    }

    #[test]
    fn args_defaults_and_ranges() {
        let v = json!({});
        let s = settings_from_args(&Args::new(&v).unwrap()).unwrap();
        assert_eq!(s, DxvkSettings::default());
        let bad = json!({"numCompilerThreads": 65});
        assert!(matches!(settings_from_args(&Args::new(&bad).unwrap()), Err(OpError::Validation(_))));
        let bad = json!({"maxFrameLatency": -1});
        assert!(matches!(settings_from_args(&Args::new(&bad).unwrap()), Err(OpError::Validation(_))));
    }

    #[test]
    fn config_round_trips_through_state_parser() {
        let (core, game) = setup("roundtrip");
        let tuned = DxvkSettings { max_frame_rate: 144, hud_enabled: true, tear_free: "False".into(), ..DxvkSettings::default() };
        let st = set_dxvk_config(&core, tuned.clone()).unwrap();
        assert!(st.dxvk.conf_exists && !st.dxvk.installed);
        assert_eq!(read_settings(&core, Some(game.to_str().unwrap())), tuned);
        assert_eq!(core.log.recent(1)[0].message, "Updated dxvk.conf from DXVK settings.");
        let _ = std::fs::remove_dir_all(core.paths.home.parent().unwrap());
    }

    #[test]
    fn uninstall_removes_files_and_logs() {
        let (core, game) = setup("uninstall");
        std::fs::write(game.join("dxgi.dll"), b"x").unwrap();
        std::fs::write(game.join(CONF_FILE), b"x").unwrap();
        let st = dxvk_uninstall(&core).unwrap();
        assert!(!st.dxvk.conf_exists && !st.dxvk.installed);
        let msgs: Vec<String> = core.log.recent(10).into_iter().map(|e| e.message).collect();
        assert_eq!(msgs, ["Removed dxgi.dll.", "Removed dxvk.conf.", "DXVK-GPLAsync has been uninstalled."]);
        dxvk_uninstall(&core).unwrap();
        assert_eq!(core.log.recent(1)[0].message, "DXVK-GPLAsync was not installed.");
        let _ = std::fs::remove_dir_all(core.paths.home.parent().unwrap());
    }

    #[test]
    fn gpl_async_cache_version_gate() {
        assert!(supports_gpl_async_cache("v2.6-1"));
        assert!(supports_gpl_async_cache("gplasync-2.6.9"));
        assert!(!supports_gpl_async_cache("v2.7-1"));
        assert!(!supports_gpl_async_cache("v3.0-1"));
        assert!(!supports_gpl_async_cache("release 10.1"));
        assert!(supports_gpl_async_cache("nightly"));
        assert!(supports_gpl_async_cache(""));
        assert!(supports_gpl_async_cache("v2"));
    }

    fn dxvk_tarball() -> Vec<u8> {
        use crate::archive::testutil::*;
        tar_gz(&[
            TarMember::dir("dxvk-gplasync-v3.0-1/"),
            TarMember::file("dxvk-gplasync-v3.0-1/x32/dxgi.dll", b"32-dxgi"),
            TarMember::file("dxvk-gplasync-v3.0-1/x32/d3d11.dll", b"32-d3d11"),
            TarMember::file("dxvk-gplasync-v3.0-1/x64/dxgi.dll", b"64-dxgi"),
            TarMember::file("dxvk-gplasync-v3.0-1/x64/d3d11.dll", b"64-d3d11"),
        ])
    }

    #[test]
    fn install_downloads_extracts_and_writes_conf() {
        use crate::net::testserver::{serve, Route};
        let (core, game) = setup("install");
        let mut routes = std::collections::HashMap::new();
        let net0 = Net::local("http://x");
        let path = download_url(&net0).replacen("http://x", "", 1);
        routes.insert(path, Route::bytes(dxvk_tarball()));
        let srv = serve(routes);
        let net = Net::local(&srv.base);
        let mut last = None;
        let st = dxvk_install_with(&core, &net, DxvkSettings { max_frame_rate: 90, ..Default::default() }, &mut |t| last = Some(*t)).unwrap();
        assert!(st.dxvk.installed && st.dxvk.conf_exists);
        assert_eq!(std::fs::read(game.join("dxgi.dll")).unwrap(), b"64-dxgi");
        assert_eq!(std::fs::read(game.join("d3d11.dll")).unwrap(), b"64-d3d11");
        // GPLAsync >= 2.7: no gplAsyncCache line even though it was requested.
        let conf = std::fs::read_to_string(game.join("dxvk.conf")).unwrap();
        assert!(conf.contains("dxgi.maxFrameRate=90") && !conf.contains("gplAsyncCache"));
        assert!(last.unwrap().done > 0);
        let msgs: Vec<String> = core.log.recent(20).into_iter().map(|e| e.message).collect();
        assert_eq!(
            msgs,
            [
                "Downloading DXVK-GPLAsync...",
                "Downloaded DXVK-GPLAsync successfully.",
                "Extracted DXVK-GPLAsync successfully.",
                "Installed dxgi.dll.",
                "Installed d3d11.dll.",
                "Updated dxvk.conf from DXVK settings.",
                "DXVK-GPLAsync installed successfully."
            ]
        );
        // Temporary files are cleaned up.
        assert_eq!(std::fs::read_dir(&core.paths.mod_files).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(core.paths.home.parent().unwrap());
    }

    #[test]
    fn install_failures_are_logged_and_reported() {
        use crate::net::testserver::{serve, Route};
        let (core, _game) = setup("install-fail");
        let srv = serve(std::collections::HashMap::new());
        let net = Net::local(&srv.base);
        let err = dxvk_install_with(&core, &net, DxvkSettings::default(), &mut no_progress).unwrap_err();
        assert!(err.message.starts_with("404 Client Error: Not Found for url: "), "{}", err.message);
        assert_eq!(core.log.recent(1)[0].message, format!("DXVK-GPLAsync install failed: {}", err.message));

        // Archive without the DLLs.
        let mut routes = std::collections::HashMap::new();
        routes.insert(
            download_url(&Net::local("http://x")).replacen("http://x", "", 1),
            Route::bytes(crate::archive::testutil::tar_gz(&[crate::archive::testutil::TarMember::file("a/readme", b"x")])),
        );
        let srv = serve(routes);
        let err = dxvk_install_with(&core, &Net::local(&srv.base), DxvkSettings::default(), &mut no_progress).unwrap_err();
        assert_eq!(err.message, "Required DXVK files were not found in the archive.");
        let _ = std::fs::remove_dir_all(core.paths.home.parent().unwrap());
    }

    #[test]
    fn missing_game_dir_is_app_error() {
        let root = std::env::temp_dir().join(format!("patchops-dxvk-nogame-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("home")).unwrap();
        let core = Core::with_paths(Paths::with_app_data(root.join("home"), root.join("appdata")), "test".into());
        assert_eq!(dxvk_uninstall(&core).unwrap_err().message, "Game directory is not set.");
        assert_eq!(set_dxvk_config(&core, DxvkSettings::default()).unwrap_err().message, "Game directory is not set.");
        let _ = std::fs::remove_dir_all(root);
    }
}
