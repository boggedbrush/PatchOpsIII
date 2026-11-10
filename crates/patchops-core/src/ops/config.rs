//! Group `config`: `config`, `vram-target`, `presets/apply`, `config-readonly`
//! plus the `graphics` and `advanced` state builders.
//!
//! Legacy reference: `backend/api.py` (`set_config`, `_write_config_value(s)`, `vram_target`,
//! `_apply_preset_values`, `apply_preset`, `config_readonly`, `_config_is_readonly`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{state_reply, Args, Group};
use crate::backup;
use crate::config_ini::{self as ini, ConfigValue};
use crate::context::Core;
use crate::error::{AppError, AppResult, OpError};
use crate::game;
use crate::presets;
use crate::pyfmt::{io_error_text, path_str, repr_str};
use crate::state::{State, StateCx};

pub const OPS: &[&str] = &["config", "vram-target", "presets/apply", "config-readonly"];

pub const GROUP: Group = Group { name: "config", ops: OPS, call };

fn call(core: &Core, op: &str, a: &Args) -> Result<Value, OpError> {
    match op {
        "config" => {
            let key = a.str_req("key", 1)?;
            let value = a.config_value("value")?;
            let comment = a.str_or("comment", "Managed by PatchOpsIII")?;
            state_reply(set_config(core, &key, &value, &comment))
        }
        "vram-target" => {
            let limited = a.bool_req("limited")?;
            let target = a.int_range("target", 75, 75, 100)?;
            state_reply(set_vram_target(core, limited, target))
        }
        "presets/apply" => {
            let name = a.str_req("name", 0)?;
            state_reply(apply_preset(core, &name))
        }
        "config-readonly" => {
            let enabled = a.bool_req("enabled")?;
            state_reply(set_config_readonly(core, enabled))
        }
        _ => Err(OpError::UnknownOp(op.to_string())),
    }
}

// ---- config writes --------------------------------------------------------------------------

type Update = (String, ConfigValue, String);

fn update(key: &str, value: ConfigValue, comment: &str) -> Update {
    (key.to_string(), value, comment.to_string())
}

fn game_dir_or_err(core: &Core) -> AppResult<String> {
    core.game_dir().ok_or_else(|| AppError::new("Game directory is not set."))
}

/// `config`: set one key (legacy `set_config` + `_write_config_value`).
pub fn set_config(core: &Core, key: &str, value: &ConfigValue, comment: &str) -> AppResult<State> {
    let _op = core.lock_ops();
    let game_dir = game_dir_or_err(core)?;
    let updates = [update(key, value.clone(), comment)];
    if let Err(e) = ini::write_values(Path::new(&game_dir), &updates) {
        core.log.error(&e.message);
        return Err(e);
    }
    core.log.success(format!("Set {key} to {}.", value.py_str()));
    Ok(State::build(core))
}

/// `vram-target`: `limited` writes `VideoMemory = target/100` (a Python float) and
/// `StreamMinResident = "1"`; otherwise `VideoMemory = "1"` and `StreamMinResident = "0"`.
pub fn set_vram_target(core: &Core, limited: bool, target: i64) -> AppResult<State> {
    let _op = core.lock_ops();
    let game_dir = game_dir_or_err(core)?;
    let updates = if limited {
        vec![
            update("VideoMemory", ConfigValue::Float(target as f64 / 100.0), "0.75 to 1"),
            update("StreamMinResident", ConfigValue::Str("1".into()), "0 or 1"),
        ]
    } else {
        vec![
            update("VideoMemory", ConfigValue::Str("1".into()), "0.75 to 1"),
            update("StreamMinResident", ConfigValue::Str("0".into()), "0 or 1"),
        ]
    };
    match ini::write_values(Path::new(&game_dir), &updates) {
        Ok(()) => {
            if limited {
                core.log.success(format!("Limited VRAM usage set to {target}%."));
            } else {
                core.log.success("Enabled full VRAM usage.");
            }
            Ok(State::build(core))
        }
        Err(e) => {
            core.log.error(format!("Failed to update VRAM target: {}", e.message));
            Err(e)
        }
    }
}

/// `presets/apply` (legacy `apply_preset` + `_apply_preset_values`).
pub fn apply_preset(core: &Core, name: &str) -> AppResult<State> {
    let _op = core.lock_ops();
    let game_dir = game_dir_or_err(core)?;
    match apply_preset_values(core, Path::new(&game_dir), name) {
        Ok(()) => {
            core.log.success(format!("Applied preset '{name}'."));
            Ok(State::build(core))
        }
        Err(e) => {
            core.log.error(format!("Failed to apply preset: {}", e.message));
            Err(e)
        }
    }
}

fn apply_preset_values(core: &Core, game_dir: &Path, name: &str) -> AppResult<()> {
    let all = presets::load(core);
    // A missing name is Python's `KeyError`, whose text is the repr of the key.
    let preset = match all.get(name) {
        None => return Err(AppError::new(repr_str(name))),
        Some(Value::Object(m)) => m,
        Some(_) => return Err(AppError::new("preset is not a mapping")),
    };
    let mut updates = Vec::new();
    for (key, item) in preset {
        let items = item.as_array().ok_or_else(|| AppError::new("preset entry is not a [value, comment] list"))?;
        if key == "ReduceStutter" {
            // Not a config key: rename the stutter-reducing DLL to its backup when its value is "1".
            let first = items.first().ok_or_else(|| AppError::new("list index out of range"))?;
            let dll = game_dir.join("d3dcompiler_46.dll");
            let backup = backup::patchops_path(&dll);
            if scalar_text(first) == "1" && dll.exists() && !backup.exists() {
                std::fs::rename(&dll, &backup).map_err(|e| AppError::new(rename_error_text(&e, &dll, &backup)))?;
            }
            continue;
        }
        let (value, comment) = match items.as_slice() {
            [value, comment] => (value, comment),
            more if more.len() > 2 => return Err(AppError::new("too many values to unpack (expected 2)")),
            less => return Err(AppError::new(format!("not enough values to unpack (expected 2, got {})", less.len()))),
        };
        updates.push((key.clone(), json_config_value(value), scalar_text(comment)));
    }
    if !updates.is_empty() {
        ini::write_values(game_dir, &updates)?;
    }
    Ok(())
}

/// Python `str()` of a preset JSON scalar.
fn scalar_text(v: &Value) -> String {
    json_config_value(v).py_str()
}

/// A preset JSON scalar as the config value it becomes. Non-scalars (never shipped) fall back to
/// their JSON text, which is not Python's `str(list)`.
fn json_config_value(v: &Value) -> ConfigValue {
    ConfigValue::from_json(v).unwrap_or_else(|| ConfigValue::Str(v.to_string()))
}

/// `[Errno N] msg: 'from' -> 'to'`, the text Python's `Path.rename` raises.
fn rename_error_text(e: &std::io::Error, from: &Path, to: &Path) -> String {
    format!("{}: {} -> {}", io_error_text(e, None), repr_str(&path_str(from)), repr_str(&path_str(to)))
}

// ---- config-readonly ------------------------------------------------------------------------

/// `config-readonly`: chmod `players/config.ini` to `0400` (read-only) or `0600` (legacy
/// `config_readonly`). Missing config is reported with its own message (no game-dir check).
pub fn set_config_readonly(core: &Core, enabled: bool) -> AppResult<State> {
    let _op = core.lock_ops();
    let path: Option<PathBuf> =
        core.game_dir().map(|g| game::config_path(Path::new(&g))).filter(|p| p.exists());
    let Some(path) = path else {
        return Err(AppError::new("config.ini was not found."));
    };
    let mode = if enabled { 0o400 } else { 0o600 };
    match set_mode(&path, mode) {
        Ok(()) => {
            if enabled {
                core.log.success("config.ini set to read-only.");
            } else {
                core.log.success("config.ini set to writable.");
            }
            Ok(State::build(core))
        }
        Err(e) => {
            let msg = io_error_text(&e, Some(path.as_path()));
            core.log.error(format!("Failed to change config.ini permissions: {msg}"));
            Err(AppError::new(msg))
        }
    }
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_readonly(mode & 0o200 == 0);
    std::fs::set_permissions(path, perms)
}

// ---- state ----------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphicsState {
    pub max_fps: i64,
    pub fov: i64,
    pub display_mode: i64,
    pub resolution: String,
    pub refresh_rate: f64,
    pub render_resolution: i64,
    pub vsync: bool,
    pub draw_fps: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdvancedState {
    pub smooth_framerate: bool,
    pub unlock_options: bool,
    pub reduce_cpu: bool,
    pub max_frame_latency: i64,
    pub vram_limited: bool,
    pub vram_target: i64,
    pub config_readonly: bool,
}

pub fn graphics_state(cx: &StateCx) -> GraphicsState {
    let c = &cx.config;
    GraphicsState {
        max_fps: ini::get_int(c, "MaxFPS", 165),
        fov: ini::get_int(c, "FOV", 80),
        display_mode: ini::get_int(c, "FullScreenMode", 1),
        resolution: ini::extract_or(c, "WindowSize", "1920x1080"),
        refresh_rate: ini::get_float(c, "RefreshRate", 60.0),
        render_resolution: ini::get_int(c, "ResolutionPercent", 100),
        vsync: ini::get_flag(c, "Vsync", "1", "1"),
        draw_fps: ini::get_flag(c, "DrawFPS", "1", "0"),
    }
}

pub fn advanced_state(cx: &StateCx) -> AdvancedState {
    let c = &cx.config;
    AdvancedState {
        smooth_framerate: ini::get_flag(c, "SmoothFramerate", "1", "0"),
        unlock_options: ini::get_flag(c, "RestrictGraphicsOptions", "0", "1"),
        reduce_cpu: ini::get_flag(c, "SerializeRender", "2", "0"),
        max_frame_latency: ini::get_int(c, "MaxFrameLatency", 1),
        vram_limited: !(ini::extract_or(c, "VideoMemory", "1") == "1" && ini::extract_or(c, "StreamMinResident", "0") == "0"),
        vram_target: (ini::get_float(c, "VideoMemory", 0.75) * 100.0) as i64,
        config_readonly: cx
            .game_dir
            .as_deref()
            .map(|g| ini::is_readonly(&game::config_path(Path::new(g))))
            .unwrap_or(false),
    }
}
