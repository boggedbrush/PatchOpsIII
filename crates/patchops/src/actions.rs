//! Every backend operation the GUI can trigger, as one enum, plus the blocking executor that runs
//! them against [`patchops_core`].
//!
//! Views never call core directly: they hand an [`Action`] to
//! [`Controller::run`](crate::controller::Controller::run), which executes [`exec`] on the
//! background executor and applies the resulting [`Reply`] on the UI thread.
//!
//! Ops call the typed core functions in-process. The few whose typed function is private
//! (`t7-uninstall`) go through `ops::dispatch` with the documented JSON argument shape and parse
//! the reply back (see [`via_json`]); switching one to a typed call later only touches [`exec`].

use std::panic::AssertUnwindSafe;

use patchops_core::config_ini::ConfigValue;
use patchops_core::net::{Net, Transfer};
use patchops_core::ops::dxvk::DxvkSettings;
use patchops_core::ops::enhanced::COMPATIBLE_DEPOT_COMMAND;
use patchops_core::ops::{self, basics, config, dxvk, enhanced, maintenance, qol, steam_ops, t7};
use patchops_core::{AppResult, Core, State};
use serde_json::Value;

/// A value written to `config.ini` (`SetConfig`).
#[derive(Clone, Debug, PartialEq)]
pub enum ConfigInput {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
}

impl ConfigInput {
    fn to_value(&self) -> ConfigValue {
        match self {
            ConfigInput::Bool(b) => ConfigValue::Bool(*b),
            ConfigInput::Int(i) => ConfigValue::Int(*i),
            ConfigInput::Float(f) => ConfigValue::Float(*f),
            ConfigInput::Text(s) => ConfigValue::Str(s.clone()),
        }
    }
}

/// The managed `config.ini` settings (legacy `configMap` in main.tsx). The booleans are stored
/// the way the legacy `setConfig` normalised them (see [`ConfigId::normalize`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigId {
    MaxFps,
    Fov,
    DisplayMode,
    Resolution,
    RefreshRate,
    RenderResolution,
    Vsync,
    DrawFps,
    SmoothFramerate,
    UnlockOptions,
    ReduceCpu,
    MaxFrameLatency,
}

impl ConfigId {
    /// `(config.ini key, comment)` exactly as the legacy UI sent them.
    pub fn key_comment(self) -> (&'static str, &'static str) {
        match self {
            ConfigId::MaxFps => ("MaxFPS", "Maximum FPS cap"),
            ConfigId::Fov => ("FOV", "Field of view"),
            ConfigId::DisplayMode => ("FullScreenMode", "0=Windowed,1=Fullscreen,2=Fullscreen Windowed"),
            ConfigId::Resolution => ("WindowSize", "any text"),
            ConfigId::RefreshRate => ("RefreshRate", "1 to 240"),
            ConfigId::RenderResolution => ("ResolutionPercent", "50 to 200"),
            ConfigId::Vsync => ("Vsync", "Vertical sync"),
            ConfigId::DrawFps => ("DrawFPS", "FPS counter"),
            ConfigId::SmoothFramerate => ("SmoothFramerate", "Frame smoothing"),
            ConfigId::UnlockOptions => ("RestrictGraphicsOptions", "Expose all graphics options"),
            ConfigId::ReduceCpu => ("SerializeRender", "Reduce CPU pressure"),
            ConfigId::MaxFrameLatency => ("MaxFrameLatency", "Maximum frame latency"),
        }
    }

    /// Busy key (the legacy id).
    pub fn busy_key(self) -> &'static str {
        match self {
            ConfigId::MaxFps => "maxFps",
            ConfigId::Fov => "fov",
            ConfigId::DisplayMode => "displayMode",
            ConfigId::Resolution => "resolution",
            ConfigId::RefreshRate => "refreshRate",
            ConfigId::RenderResolution => "renderResolution",
            ConfigId::Vsync => "vsync",
            ConfigId::DrawFps => "drawFps",
            ConfigId::SmoothFramerate => "smoothFramerate",
            ConfigId::UnlockOptions => "unlockOptions",
            ConfigId::ReduceCpu => "reduceCpu",
            ConfigId::MaxFrameLatency => "maxFrameLatency",
        }
    }

    /// Legacy value normalisation: vsync / drawFps / smoothFramerate bool -> "1"/"0",
    /// unlockOptions true -> "0" (RestrictGraphicsOptions inverted), reduceCpu true -> "2".
    pub fn normalize(self, v: &ConfigInput) -> ConfigInput {
        let flag = |on: &str, off: &str| match v {
            ConfigInput::Bool(b) => ConfigInput::Text(if *b { on } else { off }.into()),
            other => other.clone(),
        };
        match self {
            ConfigId::Vsync | ConfigId::DrawFps | ConfigId::SmoothFramerate => flag("1", "0"),
            ConfigId::UnlockOptions => flag("0", "1"),
            ConfigId::ReduceCpu => flag("2", "0"),
            _ => v.clone(),
        }
    }
}

/// `stable` / `beta`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Beta,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Beta => "beta",
        }
    }
}

/// One backend operation. Variants map 1:1 to the op catalogue (`bench/OPS.md` section 3), plus a
/// few UI-level helpers (`Refresh`, `OpenUrl`, `CopyLogs`, `QolAll`).
///
/// Confirmation prompts are NOT part of the action; use
/// [`Controller::confirm`](crate::controller::Controller::confirm).
#[derive(Clone, Debug)]
pub enum Action {
    /// Re-read the whole state (legacy "Refresh" button).
    Refresh,
    /// Launch Black Ops III through Steam.
    Launch,
    /// Open a URL (`steam://...`, `https://...`) with the system handler.
    OpenUrl(String),
    SetGameDirectory(String),
    SetReleaseChannel(Channel),
    /// Check for a new PatchOpsIII release (network).
    UpdateCheck,
    /// Steam launch options (full Steam close/edit/open cycle).
    LaunchOptions { options: String, preserve_fs_game: bool },
    /// Apply the Workshop profile's option, then open its Workshop page.
    WorkshopInstall(String),
    /// A managed `config.ini` value (graphics / advanced tab).
    SetConfig(ConfigId, ConfigInput),
    VramTarget { limited: bool, target: i64 },
    ApplyPreset(String),
    ConfigReadonly(bool),
    IntroSkip(bool),
    AllIntrosSkip(bool),
    D3dCompiler(bool),
    /// "Apply All": d3dcompiler, intro and all-intros in sequence (legacy `toggleAllQol`).
    QolAll(bool),
    DxvkConfig(DxvkSettings),
    /// Download + install DXVK-GPLAsync with these settings (network).
    DxvkInstall(DxvkSettings),
    DxvkUninstall,
    /// T7 gamertag: `name` is trimmed by core, `color_code` e.g. `"^1"` or `""`.
    T7Gamertag { name: String, color_code: String },
    /// Set the network password (empty clears it).
    T7Password(String),
    /// Switching the network password off (clears it); separate busy key like the legacy UI.
    T7PasswordOff,
    T7FriendsOnly(bool),
    /// Download + install T7 Patch (network).
    T7Install,
    T7Uninstall,
    /// Validate a dump zip/folder; result arrives as [`ReplyData::Validated`].
    EnhancedValidate(String),
    /// Install BO3 Enhanced from a dump (network).
    EnhancedInstall(String),
    EnhancedUninstall,
    /// Swap in the compatible build. If the Steam depot is missing the controller opens the
    /// "Copy this" depot prompt automatically.
    ExeCompatible,
    ExeCurrent,
    ExeEnhanced,
    /// Poll whether the compatible depot is downloaded (used by the depot prompt; not for views).
    ExeDepotCheck,
    /// Copy the log payload to the clipboard.
    CopyLogs,
    ClearLogs,
    ClearModFiles,
    ResetStock,
}

impl Action {
    /// The key for [`Controller::is_busy`](crate::controller::Controller::is_busy); equals the
    /// legacy `busy` id of the same button.
    pub fn busy_key(&self) -> &'static str {
        match self {
            Action::Refresh => "refresh",
            Action::Launch => "launch",
            Action::OpenUrl(_) => "open-url",
            Action::SetGameDirectory(_) => "directory",
            Action::SetReleaseChannel(_) => "release-channel",
            Action::UpdateCheck => "update-check",
            Action::LaunchOptions { .. } => "launch-options",
            Action::WorkshopInstall(_) => "workshop-install",
            Action::SetConfig(id, _) => id.busy_key(),
            Action::VramTarget { .. } => "vram-target",
            Action::ApplyPreset(_) => "preset",
            Action::ConfigReadonly(_) => "config-readonly",
            Action::IntroSkip(_) => "intro",
            Action::AllIntrosSkip(_) => "all-intros",
            Action::D3dCompiler(_) => "d3dcompiler",
            Action::QolAll(_) => "qol-all",
            Action::DxvkConfig(_) => "dxvk-config",
            Action::DxvkInstall(_) => "dxvk-install",
            Action::DxvkUninstall => "dxvk-uninstall",
            Action::T7Gamertag { .. } => "t7-gamertag",
            Action::T7Password(_) => "t7-password",
            Action::T7PasswordOff => "t7-password-toggle",
            Action::T7FriendsOnly(_) => "t7-friends",
            Action::T7Install => "t7-install",
            Action::T7Uninstall => "t7-uninstall",
            Action::EnhancedValidate(_) => "enhanced-validate",
            Action::EnhancedInstall(_) => "enhanced-install",
            Action::EnhancedUninstall => "enhanced-uninstall",
            Action::ExeCompatible => "exe-compatible",
            Action::ExeCurrent => "exe-current",
            Action::ExeEnhanced => "exe-enhanced",
            Action::ExeDepotCheck => "exe-depot",
            Action::CopyLogs => "copy-logs",
            Action::ClearLogs => "clear-logs",
            Action::ClearModFiles => "clear-mod-files",
            Action::ResetStock => "reset-stock",
        }
    }
}

/// Extra data an op returns next to the state.
#[derive(Clone, Debug, Default)]
pub enum ReplyData {
    #[default]
    None,
    /// `EnhancedValidate`.
    Validated { valid: bool, message: String },
    /// `ExeCompatible` failed because the Steam depot has to be downloaded first.
    DepotRequired { command: String },
    /// `ExeDepotCheck`.
    DepotAvailable(bool),
    /// `CopyLogs`.
    Payload(String),
    /// Anything else the op returned (e.g. the `update-check` body).
    Json(Value),
}

/// Result of [`exec`].
#[derive(Debug, Default)]
pub struct Reply {
    pub ok: bool,
    pub error: Option<String>,
    pub state: Option<State>,
    pub data: ReplyData,
}

impl Reply {
    fn ok_state(state: State) -> Reply {
        Reply { ok: true, state: Some(state), ..Reply::default() }
    }
    fn ok_empty() -> Reply {
        Reply { ok: true, ..Reply::default() }
    }
    pub fn err(msg: impl Into<String>) -> Reply {
        Reply { ok: false, error: Some(msg.into()), ..Reply::default() }
    }
    fn from_app(r: AppResult<State>) -> Reply {
        match r {
            Ok(s) => Reply::ok_state(s),
            Err(e) => Reply::from_err(e),
        }
    }
    fn from_err(e: patchops_core::AppError) -> Reply {
        Reply { ok: false, error: Some(e.message), state: e.state.map(|s| *s), data: ReplyData::None }
    }
}

/// Call an op through the JSON dispatcher and parse the legacy `{ok, error?, state?, ...}` body.
fn via_json(core: &Core, op: &str, args: Value) -> Reply {
    let v = match ops::dispatch(core, op, &args) {
        Ok(v) => v,
        Err(e) => return Reply::err(e.to_string()),
    };
    let ok = v.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let error = v.get("error").and_then(Value::as_str).map(str::to_string);
    let state = v.get("state").and_then(|s| serde_json::from_value::<State>(s.clone()).ok());
    let data = if v.get("depotRequired").and_then(Value::as_bool) == Some(true) {
        match v.get("depotCommand").and_then(Value::as_str) {
            Some(c) => ReplyData::DepotRequired { command: c.to_string() },
            None => ReplyData::None,
        }
    } else if let Some(valid) = v.get("valid").and_then(Value::as_bool) {
        ReplyData::Validated {
            valid,
            message: v.get("message").and_then(Value::as_str).unwrap_or("").to_string(),
        }
    } else if let Some(av) = v.get("available").and_then(Value::as_bool) {
        ReplyData::DepotAvailable(av)
    } else if let Some(u) = v.get("update") {
        ReplyData::Json(u.clone())
    } else {
        ReplyData::None
    };
    Reply { ok, error, state, data }
}

/// Forward a transfer as a percent; only when the whole percent changed would be nicer, but the
/// receiver dedupes (cheap channel send).
fn report(progress: &mut dyn FnMut(u8), t: &Transfer) {
    if let Some(p) = t.percent() {
        progress(p);
    }
}

fn t7_config(core: &Core, gamertag: Option<&str>, color: &str, password: Option<&str>, friends: Option<bool>) -> Reply {
    Reply::from_app(t7::set_t7_config(core, gamertag, color, password, friends))
}

/// Download progress sink: whole percent (0..=100) of the running network install.
pub type Progress<'a> = &'a mut dyn FnMut(u8);

/// Run one action to completion. Blocking (some ops take seconds); call from a background thread.
/// A panic inside an op becomes an error reply instead of taking the UI down. `progress` receives
/// the download percent of the network installers (`T7Install`, `DxvkInstall`, `EnhancedInstall`).
pub fn exec(core: &Core, action: &Action, progress: Progress<'_>) -> Reply {
    let r = std::panic::catch_unwind(AssertUnwindSafe(|| exec_inner(core, action, progress)));
    let mut reply = r.unwrap_or_else(|p| {
        let msg = p
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| p.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".into());
        Reply::err(format!("Internal error: {msg}"))
    });
    // Legacy `runAction`: a successful result without a state is followed by a refresh.
    if reply.ok && reply.state.is_none() && !matches!(action, Action::OpenUrl(_) | Action::CopyLogs) {
        reply.state = std::panic::catch_unwind(AssertUnwindSafe(|| basics::status(core))).ok();
    }
    reply
}

fn exec_inner(core: &Core, action: &Action, progress: Progress<'_>) -> Reply {
    match action {
        Action::Refresh => Reply::ok_state(basics::status(core)),
        Action::Launch => {
            steam_ops::launch(core);
            Reply::ok_empty()
        }
        Action::OpenUrl(url) => match patchops_core::steam::open_external_url(url) {
            Ok(()) => Reply::ok_empty(),
            Err(e) => Reply::err(e),
        },
        Action::SetGameDirectory(p) => Reply::from_app(basics::set_game_directory(core, p)),
        Action::SetReleaseChannel(c) => Reply::from_app(basics::set_release_channel(core, c.as_str())),
        Action::UpdateCheck => match maintenance::update_check(core) {
            Ok(r) => Reply {
                ok: true,
                state: Some(r.state),
                data: ReplyData::Json(serde_json::to_value(&r.update).unwrap_or(Value::Null)),
                error: None,
            },
            Err(e) => Reply::from_err(e),
        },
        Action::LaunchOptions { options, preserve_fs_game } => {
            match steam_ops::set_launch_options(core, options, *preserve_fs_game) {
                Ok(r) if r.ok => Reply::ok_state(r.state),
                // `ok: false` = localconfig.vdf was not written (legacy: "Action failed").
                Ok(r) => Reply {
                    ok: false,
                    error: Some("Failed to apply launch options.".into()),
                    state: Some(r.state),
                    data: ReplyData::None,
                },
                Err(e) => Reply::from_err(e),
            }
        }
        Action::WorkshopInstall(id) => Reply::from_app(steam_ops::workshop_install(core, id)),
        Action::SetConfig(id, v) => {
            let (key, comment) = id.key_comment();
            Reply::from_app(config::set_config(core, key, &id.normalize(v).to_value(), comment))
        }
        Action::VramTarget { limited, target } => {
            Reply::from_app(config::set_vram_target(core, *limited, (*target).clamp(75, 100)))
        }
        Action::ApplyPreset(name) => Reply::from_app(config::apply_preset(core, name)),
        Action::ConfigReadonly(on) => Reply::from_app(config::set_config_readonly(core, *on)),
        Action::IntroSkip(on) => Reply::from_app(qol::set_intro_skip(core, *on)),
        Action::AllIntrosSkip(on) => Reply::from_app(qol::set_all_intros_skip(core, *on)),
        Action::D3dCompiler(on) => Reply::from_app(qol::set_d3dcompiler(core, *on)),
        Action::QolAll(on) => {
            // Legacy: three requests in order; only the last result is looked at.
            let _ = qol::set_d3dcompiler(core, *on);
            let _ = qol::set_intro_skip(core, *on);
            Reply::from_app(qol::set_all_intros_skip(core, *on))
        }
        Action::DxvkConfig(s) => Reply::from_app(dxvk::set_dxvk_config(core, s.clone())),
        Action::DxvkInstall(s) => {
            let mut p = |t: &Transfer| report(progress, t);
            Reply::from_app(dxvk::dxvk_install_with(core, &Net::production(), s.clone(), &mut p))
        }
        Action::DxvkUninstall => Reply::from_app(dxvk::dxvk_uninstall(core)),
        Action::T7Gamertag { name, color_code } => t7_config(core, Some(name), color_code, None, None),
        Action::T7Password(p) => t7_config(core, None, "", Some(p), None),
        Action::T7PasswordOff => t7_config(core, None, "", Some(""), None),
        Action::T7FriendsOnly(on) => t7_config(core, None, "", None, Some(*on)),
        Action::T7Install => {
            let mut p = |t: &Transfer| report(progress, t);
            Reply::from_app(t7::t7_install_with(core, &Net::production(), &mut p))
        }
        Action::T7Uninstall => via_json(core, "t7-uninstall", Value::Null),
        Action::EnhancedValidate(src) => match enhanced::validate_enhanced(core, src) {
            Ok(r) => Reply {
                ok: r.ok,
                error: None,
                state: Some(r.state),
                data: ReplyData::Validated { valid: r.valid, message: r.message },
            },
            Err(e) => Reply::from_err(e),
        },
        Action::EnhancedInstall(src) => {
            let mut p = |t: &Transfer| report(progress, t);
            Reply::from_app(enhanced::install_enhanced_with(core, &Net::production(), src, &mut p))
        }
        Action::EnhancedUninstall => Reply::from_app(enhanced::uninstall_enhanced_op(core)),
        Action::ExeCompatible => match enhanced::install_compatible_exe_op(core) {
            Err(e) if e.extra.get("depotRequired").and_then(Value::as_bool) == Some(true) => {
                let command = e
                    .extra
                    .get("depotCommand")
                    .and_then(Value::as_str)
                    .unwrap_or(COMPATIBLE_DEPOT_COMMAND)
                    .to_string();
                let mut r = Reply::from_err(e);
                r.data = ReplyData::DepotRequired { command };
                r
            }
            other => Reply::from_app(other),
        },
        Action::ExeCurrent => Reply::from_app(enhanced::restore_current_exe_op(core)),
        Action::ExeEnhanced => Reply::from_app(enhanced::restore_enhanced_exe_op(core)),
        Action::ExeDepotCheck => match enhanced::compatible_depot_status(core) {
            Ok(d) => Reply { ok: d.ok, error: None, state: Some(d.state), data: ReplyData::DepotAvailable(d.available) },
            Err(e) => Reply::from_err(e),
        },
        Action::CopyLogs => Reply { ok: true, data: ReplyData::Payload(basics::log_payload(core)), ..Reply::default() },
        Action::ClearLogs => Reply::from_app(basics::clear_logs(core)),
        Action::ClearModFiles => Reply::from_app(basics::clear_mod_files(core)),
        Action::ResetStock => Reply::from_app(maintenance::reset_stock(core)),
    }
}
