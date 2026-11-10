//! Group `config`: `config`, `vram-target`, `presets/apply`, `config-readonly`
//! plus the `graphics` and `advanced` state builders (already implemented, read-only).
//!
//! TODO(config agent) - port from `backend/api.py`:
//!   `set_config` (+ `_write_config_value(s)`, regex-sub semantics incl. `\q` bad-escape text),
//!   `vram_target`, `apply_preset` (+ `_apply_preset_values`, `ReduceStutter` rename),
//!   `config_readonly` (chmod 0400 / 0600), `_config_is_readonly`.
//! Shared pieces to use/extend: `crate::config_ini` (`write_values` is the stub to fill),
//! `crate::presets`, `crate::backup`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{not_implemented, Args, Group};
use crate::config_ini as ini;
use crate::context::Core;
use crate::error::OpError;
use crate::state::StateCx;

pub const OPS: &[&str] = &["config", "vram-target", "presets/apply", "config-readonly"];

pub const GROUP: Group = Group { name: "config", ops: OPS, call };

fn call(core: &Core, op: &str, args: &Args) -> Result<Value, OpError> {
    // TODO(config agent): validate with `args` and call typed handlers; see ops/mod.rs docs.
    not_implemented(core, op, args)
}

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
            .map(|g| ini::is_readonly(&crate::game::config_path(std::path::Path::new(g))))
            .unwrap_or(false),
    }
}
