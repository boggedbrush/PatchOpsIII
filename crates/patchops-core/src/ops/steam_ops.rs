//! Group `steam_ops`: `launch`, `launch-options`, `workshop-install` plus the launch profile
//! state (read side implemented; `steamUserId`/`currentLaunchOptions` are assembled in
//! `State::build` from `crate::steam`).
//!
//! TODO(steam agent) - port from `utils.py` + `backend/api.py`: `launch_game_via_steam`,
//! `apply_launch_options`/`set_launch_options` (merge algorithm in OPS.md), `close_steam`,
//! `open_steam` (process control: `pgrep`/`pkill -x steam`, PATH lookup of `steam`),
//! `workshop_install`, `_open_external_url`, plus the localconfig VDF *write* (use
//! `crate::steam::vdf`; `dump_pretty` is a first cut whose separator/escaping must be verified
//! against the launch-options goldens - the `vdf` Python version recorded them with
//! `"key"\t\t"value"`). Steam control helpers (`close_steam`/`open_steam`, config.vdf
//! compat-tool mapping) are also needed by `enhanced` and `maintenance`: expose them as `pub fn` here.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{not_implemented, Args, Group};
use crate::context::Core;
use crate::error::OpError;
use crate::state::StateCx;
use crate::steam;

pub const OPS: &[&str] = &["launch", "launch-options", "workshop-install"];

pub const GROUP: Group = Group { name: "steam_ops", ops: OPS, call };

pub const OFFLINE_OPTION: &str = "+set fs_game offlinemp";

/// `(id, display name, workshop id, launch option)` in legacy order.
pub const WORKSHOP_PROFILES: [(&str, &str, &str, &str); 3] = [
    ("all_around", "All-around Enhancement Lite", "2994481309", "+set fs_game 2994481309"),
    ("ultimate", "Ultimate Experience Mod", "2942053577", "+set fs_game 2942053577"),
    ("reforged", "BO3 Reforged", "3667377161", "+set fs_game 3667377161"),
];

fn call(core: &Core, op: &str, args: &Args) -> Result<Value, OpError> {
    // TODO(steam agent)
    not_implemented(core, op, args)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchProfile {
    pub id: String,
    pub label: String,
    pub option: String,
    pub active: bool,
    pub installed: bool,
    pub subscribed: bool,
    pub state: String,
    /// Workshop profiles only: key present, `null` when not installed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<Option<String>>,
}

/// Legacy `_launch_profiles(current_options)`.
pub fn launch_profiles_state(cx: &StateCx) -> Vec<LaunchProfile> {
    let current = cx.launch_options.as_deref().unwrap_or("");
    let mut v = vec![
        LaunchProfile {
            id: "default".into(),
            label: "Default (None)".into(),
            option: String::new(),
            active: current.trim().is_empty(),
            installed: true,
            subscribed: true,
            state: "Ready".into(),
            path: None,
        },
        LaunchProfile {
            id: "offline".into(),
            label: "Play Offline".into(),
            option: OFFLINE_OPTION.into(),
            active: current.contains(OFFLINE_OPTION),
            installed: true,
            subscribed: true,
            state: "Ready".into(),
            path: None,
        },
    ];
    for (id, name, workshop_id, option) in WORKSHOP_PROFILES {
        let ws = steam::workshop_item_state(cx.core, steam::BO3_APP_ID, workshop_id);
        v.push(LaunchProfile {
            id: id.into(),
            label: name.into(),
            option: option.into(),
            active: current.contains(option),
            installed: ws.installed,
            subscribed: ws.subscribed,
            state: ws.state.into(),
            path: Some(ws.path),
        });
    }
    v
}

/// First active profile id, else `custom` if options are non-empty, else `default`.
pub fn active_profile_id(profiles: &[LaunchProfile], options: Option<&str>) -> String {
    profiles
        .iter()
        .find(|p| p.active)
        .map(|p| p.id.clone())
        .unwrap_or_else(|| if options.is_some_and(|o| !o.is_empty()) { "custom".into() } else { "default".into() })
}
