//! Group `dxvk`: `dxvk-config`, `dxvk-uninstall` plus the `dxvk` state builder. `dxvk-install`
//! (network download) is still the not-implemented stub; it should reuse [`write_conf`] for the
//! config step.
//!
//! Legacy reference: `backend/api.py` (`dxvk_config`, `uninstall_dxvk`, `_read_dxvk_settings`,
//! `_dxvk_payload_to_settings`, `_write_dxvk_conf`, `_uninstall_dxvk`) and `dxvk_manager.py`
//! (`_build_dxvk_conf`, `_preset_settings`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{not_implemented, state_reply, Args, Group};
use crate::context::Core;
use crate::error::{AppError, AppResult, OpError};
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
        // Network installer: owned by the network agents, see the module doc.
        "dxvk-install" => not_implemented(core, op, a),
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
    let game = game_dir(core)?;
    let settings = settings.normalized();
    write_conf(core, &game, &settings, true).map_err(|e| fail(core, "Failed to update DXVK settings", e))?;
    Ok(State::build(core))
}

/// Legacy `_uninstall_dxvk`: remove the DLLs and `dxvk.conf` that exist, logging each removal.
/// Shared with reset-to-stock.
pub fn uninstall(core: &Core, game_dir: &Path) -> Result<(), String> {
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
    let game = game_dir(core)?;
    uninstall(core, &game).map_err(|e| fail(core, "DXVK-GPLAsync uninstall failed", e))?;
    Ok(State::build(core))
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
