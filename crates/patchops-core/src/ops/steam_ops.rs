//! Group `steam_ops`: `launch`, `launch-options`, `workshop-install` plus the launch profile
//! state (read side implemented; `steamUserId`/`currentLaunchOptions` are assembled in
//! `State::build` from `crate::steam`).
//!
//! Legacy reference: `backend/api.py` (`launch_game`, `launch_options`, `workshop_install`,
//! `_open_external_url`) and `utils.py`. The process/VDF machinery is in [`crate::steam`]
//! (`client`, `launch_options`); the shared helpers for other groups are re-exported there
//! (`close_steam`, `open_steam`, `apply_launch_options`, `clear_launch_options`, ...).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{state_reply, to_json, Args, Group};
use crate::context::Core;
use crate::error::{AppError, AppResult, OpError};
use crate::state::{State, StateCx};
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

fn call(core: &Core, op: &str, a: &Args) -> Result<Value, OpError> {
    match op {
        "launch" => {
            launch(core);
            Ok(json!({"ok": true}))
        }
        "launch-options" => {
            let options = a.str_or("options", "")?;
            let preserve = a.bool_or("preserve_fs_game", false)?;
            match set_launch_options(core, &options, preserve) {
                Ok(r) => to_json(&r),
                // The legacy route has no handler for the missing-user exception: HTTP 500.
                Err(e) if e.state.is_none() => Err(OpError::Internal(e.message)),
                Err(e) => Ok(e.to_json()),
            }
        }
        "workshop-install" => {
            let profile = a.str_req("profileId", 1)?;
            state_reply(workshop_install(core, &profile))
        }
        _ => Err(OpError::UnknownOp(op.to_string())),
    }
}

/// Launch Black Ops III through Steam; failures are only logged (`{"ok": true}` always).
pub fn launch(core: &Core) {
    steam::launch_game(core, steam::BO3_APP_ID);
}

/// `launch-options` result: `ok` is whether `localconfig.vdf` was written (no `error` key).
#[derive(Debug, Clone, Serialize)]
pub struct LaunchOptionsResult {
    pub ok: bool,
    pub state: State,
}

/// Apply one of the supported launch option strings (full Steam close/edit/open cycle).
///
/// Errors: `Unsupported launch option.` carries the current state; `Steam user ID not found!`
/// has no state (the op layer maps it to an `internal:` envelope error).
pub fn set_launch_options(core: &Core, options: &str, preserve_fs_game: bool) -> AppResult<LaunchOptionsResult> {
    if !is_supported_option(options) {
        core.log.warning("Unsupported launch option.");
        return Err(AppError::new("Unsupported launch option.").with_state(State::build(core)));
    }
    let ok = steam::apply_launch_options(core, options, preserve_fs_game).map_err(|e| with_state_if_steam_busy(core, e))?;
    Ok(LaunchOptionsResult { ok, state: State::build(core) })
}

/// "Steam did not close in time" is an ordinary `{ok:false,error,state}` reply (nothing was
/// changed), unlike the missing-user exception which the legacy route turned into a 500.
fn with_state_if_steam_busy(core: &Core, e: AppError) -> AppError {
    if e.state.is_none() && e.message == steam::client::STEAM_CLOSE_TIMEOUT_MSG {
        e.with_state(State::build(core))
    } else {
        e
    }
}

fn is_supported_option(options: &str) -> bool {
    options.is_empty() || options == OFFLINE_OPTION || WORKSHOP_PROFILES.iter().any(|p| p.3 == options)
}

/// Apply a Workshop profile's launch option, then open its Workshop page in Steam.
pub fn workshop_install(core: &Core, profile_id: &str) -> AppResult<State> {
    let Some((_, name, workshop_id, option)) = WORKSHOP_PROFILES.iter().find(|p| p.0 == profile_id) else {
        return Err(AppError::new("Select an installable Workshop mod."));
    };
    let ok = match steam::apply_launch_options(core, option, false) {
        Ok(ok) => ok,
        Err(e) => {
            core.log.error(format!("Workshop install flow failed: {}", e.message));
            return Err(e);
        }
    };
    if !ok {
        return Err(AppError::new("Failed to apply launch options."));
    }
    let url = format!("steam://openurl/https://steamcommunity.com/sharedfiles/filedetails/?id={workshop_id}");
    if let Err(e) = steam::open_external_url(&url) {
        core.log.error(format!("Workshop install flow failed: {e}"));
        return Err(AppError::new(e));
    }
    core.log.info(format!("Opened {name} Workshop page in Steam."));
    Ok(State::build(core))
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
