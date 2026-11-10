//! The `state` object returned by `status` and embedded in most mutating responses
//! (`bench/OPS.md` section 4).
//!
//! `State` is only an assembly point: every sub-struct and its builder lives in the op-group
//! module that owns that area, so groups can be ported in parallel without touching this file
//! (unless they add a new top-level field).

use serde::{Deserialize, Serialize};

use crate::context::Core;
use crate::log::{LogEntry, STATE_TAIL};
use crate::ops;

/// Everything a state builder may need, computed once per `State::build`.
pub struct StateCx<'a> {
    pub core: &'a Core,
    /// Resolved game directory.
    pub game_dir: Option<String>,
    /// `players/config.ini` content (invalid UTF-8 bytes dropped); empty when missing.
    pub config: String,
    /// Raw launch options of BO3 from `localconfig.vdf` (`None`: no Steam user / no file).
    pub launch_options: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub app_version: String,
    pub platform: String,
    pub game_dir: Option<String>,
    pub game_detected: bool,
    pub config_exists: bool,
    pub steam_user_id: Option<String>,
    pub log_path: String,
    pub presets: Vec<String>,
    pub current_launch_options: Option<String>,
    /// `default|offline|all_around|ultimate|reforged|custom`
    pub active_launch_profile: String,
    /// `stable|beta`
    pub release_channel: String,
    pub launch_profiles: Vec<ops::steam_ops::LaunchProfile>,
    pub enhanced: ops::enhanced::EnhancedState,
    pub exe_swap: ops::enhanced::ExeSwapState,
    pub t7: ops::t7::T7State,
    pub dxvk: ops::dxvk::DxvkState,
    pub qol: ops::qol::QolState,
    pub graphics: ops::config::GraphicsState,
    pub advanced: ops::config::AdvancedState,
    pub maintenance: ops::basics::MaintenanceState,
    pub mods: ModsState,
    /// Last <= 80 entries of the in-memory ring.
    pub logs: Vec<LogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModsState {
    pub t7_patch: bool,
    pub dxvk: bool,
    pub enhanced: bool,
}

impl State {
    /// Assemble the full state (reads config.ini, Steam files, settings; takes a few ms).
    pub fn build(core: &Core) -> State {
        let game_dir = core.game_dir();
        let config = game_dir
            .as_deref()
            .map(|g| crate::config_ini::read(&crate::game::config_path(std::path::Path::new(g))))
            .unwrap_or_default();
        let steam_user_id = crate::steam::find_user_id(core);
        let launch_options = steam_user_id
            .as_deref()
            .and_then(|u| crate::steam::read_launch_options(core, u, crate::steam::BO3_APP_ID));
        let cx = StateCx { core, game_dir, config, launch_options };

        let enhanced = ops::enhanced::enhanced_state(&cx);
        let launch_profiles = ops::steam_ops::launch_profiles_state(&cx);
        let active_launch_profile = ops::steam_ops::active_profile_id(&launch_profiles, cx.launch_options.as_deref());

        State {
            app_version: core.version.clone(),
            platform: crate::paths::system_name().to_string(),
            game_detected: cx.game_dir.is_some(),
            config_exists: !cx.config.is_empty(),
            steam_user_id,
            log_path: core.paths.log.to_string_lossy().into_owned(),
            presets: crate::presets::names(core),
            current_launch_options: cx.launch_options.clone(),
            active_launch_profile,
            release_channel: core.settings().release_channel().to_string(),
            launch_profiles,
            mods: ModsState {
                t7_patch: ops::t7::installed_marker(&cx),
                dxvk: ops::dxvk::files_installed(&cx),
                enhanced: enhanced.installed,
            },
            enhanced,
            exe_swap: ops::enhanced::exe_swap_state(&cx),
            t7: ops::t7::t7_state(&cx),
            dxvk: ops::dxvk::dxvk_state(&cx),
            qol: ops::qol::qol_state(&cx),
            graphics: ops::config::graphics_state(&cx),
            advanced: ops::config::advanced_state(&cx),
            maintenance: ops::basics::maintenance_state(&cx),
            logs: core.log.recent(STATE_TAIL),
            game_dir: cx.game_dir,
        }
    }
}
