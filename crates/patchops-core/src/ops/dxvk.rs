//! Group `dxvk`: `dxvk-config`, `dxvk-install`, `dxvk-uninstall` plus the `dxvk` state builder
//! (read side implemented).
//!
//! TODO(dxvk agent) - port from `backend/api.py` + `dxvk_manager.py`: `dxvk_config`
//! (`_write_dxvk_conf`, `_build_dxvk_conf`, tearFree normalisation), `dxvk_uninstall`
//! (`_uninstall_dxvk`), `dxvk_install` (`_install_dxvk`; network, needs an HTTP client + tar.gz
//! extraction - coordinate with the network agents).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{not_implemented, Args, Group};
use crate::context::Core;
use crate::error::OpError;
use crate::state::StateCx;

pub const OPS: &[&str] = &["dxvk-config", "dxvk-install", "dxvk-uninstall"];

pub const GROUP: Group = Group { name: "dxvk", ops: OPS, call };

pub const DXVK_ASYNC_FILES: [&str; 2] = ["dxgi.dll", "d3d11.dll"];

fn call(core: &Core, op: &str, args: &Args) -> Result<Value, OpError> {
    // TODO(dxvk agent)
    not_implemented(core, op, args)
}

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

/// Legacy `_read_dxvk_settings`: defaults overlaid with `dxvk.conf`; a malformed integer stops
/// parsing (earlier lines stay applied) and logs a warning.
pub fn read_settings(core: &Core, game_dir: Option<&str>) -> DxvkSettings {
    let mut s = DxvkSettings::default();
    let Some(game) = game_dir else { return s };
    let conf = std::path::Path::new(game).join("dxvk.conf");
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
        conf_exists: cx.game_dir.as_deref().is_some_and(|g| std::path::Path::new(g).join("dxvk.conf").exists()),
        settings: read_settings(cx.core, cx.game_dir.as_deref()),
    }
}
