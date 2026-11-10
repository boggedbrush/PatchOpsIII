//! Group `t7`: `t7-config`, `t7-install`, `t7-uninstall` plus the `t7` state builder.
//!
//! TODO(t7 agent) - port from `backend/api.py` + `t7_patch.py`: `set_t7_config`
//! (`update_t7patch_conf`), `uninstall_t7_patch` (`_uninstall_t7_patch`, `restore_lpc_backups`),
//! `install_t7_patch` (`_install_t7_patch`; network + elevation seams on Windows).
//! The `t7` state builder below is already complete (conf parsing, `mode`); reuse its
//! `check_t7_patch_status`-style parsing for `t7-config` if useful.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{not_implemented, Args, Group};
use crate::context::Core;
use crate::error::OpError;
use crate::state::StateCx;

pub const OPS: &[&str] = &["t7-config", "t7-install", "t7-uninstall"];

pub const GROUP: Group = Group { name: "t7", ops: OPS, call };

/// Files removed by `t7-uninstall` (legacy `T7_GAME_FILES`).
pub const T7_GAME_FILES: [&str; 6] =
    ["t7patch.dll", "t7patch.conf", "discord_game_sdk.dll", "dsound.dll", "t7patchloader.dll", "zbr2.dll"];

fn call(core: &Core, op: &str, args: &Args) -> Result<Value, OpError> {
    // TODO(t7 agent)
    not_implemented(core, op, args)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct T7State {
    pub installed: bool,
    pub conf_exists: bool,
    pub gamertag: String,
    pub plain_name: String,
    pub color_code: String,
    pub network_password: String,
    pub friends_only: bool,
    /// `Unknown|Default|Custom|Enhanced`
    pub mode: String,
}

/// Markers whose presence means T7 Patch is installed (legacy `T7_INSTALL_MARKERS`).
pub const T7_INSTALL_MARKERS: [&str; 2] = ["t7patchloader.dll", "t7patch.dll"];

/// Legacy `check_t7_patch_status` + `_t7_status` + `_t7_mode` (state side; complete).
/// `mode` relies on `enhanced::detect_enhanced_install` (complete) and the exe hash.
pub fn t7_state(cx: &StateCx) -> T7State {
    let mut st = T7State {
        installed: false,
        conf_exists: false,
        gamertag: String::new(),
        plain_name: String::new(),
        color_code: String::new(),
        network_password: String::new(),
        friends_only: false,
        mode: "Unknown".into(),
    };
    let Some(game) = cx.game_dir.as_deref().map(std::path::Path::new) else { return st };
    st.installed = game.is_dir() && T7_INSTALL_MARKERS.iter().any(|m| game.join(m).exists());
    let conf = game.join("t7patch.conf");
    st.conf_exists = conf.exists();
    if let Ok(bytes) = std::fs::read(&conf) {
        for line in String::from_utf8_lossy(&bytes).split_inclusive('\n') {
            let value = |l: &str| l.trim().split_once('=').map(|(_, v)| v.to_string()).unwrap_or_default();
            if line.starts_with("playername=") {
                st.gamertag = value(line);
                if st.gamertag.starts_with('^') && st.gamertag.chars().count() > 2 {
                    st.color_code = st.gamertag.chars().take(2).collect();
                    st.plain_name = st.gamertag.chars().skip(2).collect();
                } else {
                    st.plain_name = st.gamertag.clone();
                }
            } else if line.starts_with("networkpassword=") {
                st.network_password = value(line);
            } else if line.starts_with("isfriendsonly=") {
                st.friends_only = value(line) == "1";
            }
        }
    }
    st.mode = t7_mode(game);
    st
}

fn t7_mode(game: &std::path::Path) -> String {
    if super::enhanced::detect_enhanced_install(game) {
        return "Enhanced".into();
    }
    let exe = crate::game::find_executable(game);
    let hash = exe.as_deref().and_then(crate::hash::file_sha256);
    if hash.as_deref() == Some(super::enhanced::DEFAULT_STEAM_EXE_SHA256) || crate::game::read_exe_variant(game).as_deref() == Some("default") {
        "Default".into()
    } else if exe.is_some() {
        "Custom".into()
    } else {
        "Unknown".into()
    }
}

/// `state.mods.t7Patch`: `t7patch.exe` or `t7patch.dll` exists.
pub fn installed_marker(cx: &StateCx) -> bool {
    cx.game_dir.as_deref().is_some_and(|g| {
        let g = std::path::Path::new(g);
        g.join("t7patch.exe").exists() || g.join("t7patch.dll").exists()
    })
}
