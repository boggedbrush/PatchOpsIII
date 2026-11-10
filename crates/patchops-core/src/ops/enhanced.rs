//! Group `enhanced`: `enhanced-validate`, `enhanced-install`, `enhanced-uninstall`,
//! `exe-swap/compatible`, `exe-swap/compatible-depot`, `exe-swap/current`, `exe-swap/enhanced`
//! plus the `enhanced` and `exeSwap` state builders.
//!
//! Legacy reference: `backend/api.py` (`_enhanced_status`, `_exe_swap_status`, `_install_enhanced`,
//! `_uninstall_enhanced`, `_install_compatible_exe`, `_restore_*_exe`, endpoints) and
//! `bo3_enhanced.py`. The machinery lives in [`crate::enhanced`] (zip reader, hash cache, file
//! placement, exe swapping, Linux Proton/Steam side); this file is the op layer, the state
//! structs/builders and the helpers the reset-to-stock port needs.
//!
//! `enhanced-install` downloads through [`crate::enhanced_net`] (release zip, Linux Proton tool) and
//! then runs the local pipeline: `prepare_install`, `install_enhanced_files`,
//! `configure_bo3_enhanced_linux`, `finish_install`.
//!
//! Helpers for reset-to-stock: [`uninstall_enhanced`], [`restore_current_exe`] /
//! [`active_exe_profile`] and [`detect_enhanced_install`] (the "Enhanced files present" check).

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{reply, state_reply, Args, Group};
use crate::context::Core;
use crate::enhanced::exe::{KnownBuilds, Swap, SwapError, COMPATIBLE_EXE_ID, CURRENT_EXE_ID, ENHANCED_EXE_ID};
use crate::enhanced::{files, hashcache, linux, state_file};
use crate::error::{AppError, AppResult, OpError};
use crate::net::{no_progress, Net, ProgressFn};
use crate::state::{State, StateCx};

pub const OPS: &[&str] = &[
    "enhanced-validate",
    "enhanced-install",
    "enhanced-uninstall",
    "exe-swap/compatible",
    "exe-swap/compatible-depot",
    "exe-swap/current",
    "exe-swap/enhanced",
];

pub const GROUP: Group = Group { name: "enhanced", ops: OPS, call };

// Legacy constants (api.py / t7_patch.py).
pub const CURRENT_STEAM_BUILD_ID: &str = "24784313";
pub const CURRENT_STEAM_BUILD_DATE: &str = "Sep 10, 2026";
pub const COMPATIBLE_STEAM_BUILD_ID: &str = "10650222";
pub const COMPATIBLE_STEAM_BUILD_DATE: &str = "Mar 3, 2023";
pub const DEFAULT_STEAM_EXE_SHA256: &str = "51ca63bbc660e0826943c60da67606f6bcb4b3b519528b5e0548c68c9423a323";
pub const COMPATIBLE_BUILD_SHA256: [&str; 1] = ["66b95eb4667bd5b3b3d230e7bed1d29ccd261d48ca2699f01216c863be24ff44"];
pub const COMPATIBLE_DEPOT_APP_ID: &str = "311210";
pub const COMPATIBLE_DEPOT_ID: &str = "311211";
pub const COMPATIBLE_DEPOT_MANIFEST_ID: &str = "9084453472036406216";
pub const COMPATIBLE_DEPOT_COMMAND: &str = "download_depot 311210 311211 9084453472036406216";
pub const T7_PATCH_LABEL_CURRENT: &str = "T7 Patch Scroptss/T7Patch";

const NO_GAME_DIR: &str = "Game directory is not set.";

fn call(core: &Core, op: &str, a: &Args) -> Result<Value, OpError> {
    match op {
        "enhanced-validate" => {
            let dump_source = a.str_req("dumpSource", 1)?;
            reply(validate_enhanced(core, &dump_source))
        }
        "enhanced-install" => {
            let dump_source = a.str_req("dumpSource", 1)?;
            state_reply(install_enhanced(core, &dump_source))
        }
        "enhanced-uninstall" => state_reply(uninstall_enhanced_op(core)),
        "exe-swap/compatible" => state_reply(install_compatible_exe_op(core)),
        "exe-swap/compatible-depot" => reply(compatible_depot_status(core)),
        "exe-swap/current" => state_reply(restore_current_exe_op(core)),
        "exe-swap/enhanced" => state_reply(restore_enhanced_exe_op(core)),
        _ => Err(OpError::UnknownOp(op.to_string())),
    }
}

// ---- enhanced-validate ----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidateReply {
    pub ok: bool,
    pub valid: bool,
    pub message: String,
    pub state: State,
}

/// `enhanced-validate`: blank source -> `valid:false` without saving; otherwise remember the
/// source in the settings and validate it (directory or zip).
pub fn validate_enhanced(core: &Core, dump_source: &str) -> AppResult<ValidateReply> {
    let _op = core.lock_ops();
    let source = dump_source.trim();
    if source.is_empty() {
        return Ok(ValidateReply { ok: true, valid: false, message: "Select a dump source first.".into(), state: State::build(core) });
    }
    core.update_settings(|s| s.set_str("enhanced_dump_source", source))
        .map_err(|e| AppError::new(crate::pyfmt::io_error_text(&e, Some(&core.paths.settings))))?;
    let valid = files::validate_dump_source(core, Path::new(source));
    Ok(ValidateReply {
        ok: true,
        valid,
        message: if valid { "Source looks ready." } else { "Source is missing required dump files." }.into(),
        state: State::build(core),
    })
}

// ---- enhanced-install -----------------------------------------------------------------------

/// Legacy `install_enhanced` route against the real endpoints, without progress reporting.
pub fn install_enhanced(core: &Core, dump_source: &str) -> AppResult<State> {
    install_enhanced_with(core, &Net::production(), dump_source, &mut no_progress)
}

/// Legacy `install_enhanced` / `_install_enhanced`: validate the dump, download the latest BO3
/// Enhanced release (SHA-256 from the release metadata), place the files, on Linux fetch the
/// GDK-Proton tool and wire Steam up, then record the variant. `progress` receives the download
/// progress of the Enhanced zip and (Linux, first install) the Proton archive.
pub fn install_enhanced_with(core: &Core, net: &Net, dump_source: &str, progress: ProgressFn<'_>) -> AppResult<State> {
    let _op = core.lock_ops();
    let game_dir = core.game_dir().ok_or_else(|| AppError::new(NO_GAME_DIR))?;
    match install_enhanced_inner(core, net, Path::new(&game_dir), dump_source, progress) {
        Ok(()) => Ok(State::build(core)),
        Err(e) => {
            core.log.error(format!("BO3 Enhanced install failed: {e}"));
            Err(AppError::new(e))
        }
    }
}

fn install_enhanced_inner(core: &Core, net: &Net, game_dir: &Path, dump_source: &str, progress: ProgressFn<'_>) -> Result<(), String> {
    let source = files::prepare_install(core, dump_source)?;
    let mod_files = &core.paths.mod_files;
    std::fs::create_dir_all(mod_files).map_err(|e| crate::pyfmt::io_error_text(&e, Some(mod_files)))?;
    core.log.info("Fetching latest BO3 Enhanced release...");
    crate::enhanced_net::download_latest_enhanced(core, net, mod_files, &core.paths.app_data, progress)
        .ok_or("Failed to download BO3 Enhanced.")?;

    core.log.info("Installing BO3 Enhanced files...");
    if !files::install_enhanced_files(core, game_dir, mod_files, Path::new(&source)) {
        return Err("BO3 Enhanced installation failed.".into());
    }
    if crate::paths::system_name() == "Linux" {
        // Legacy order: the Steam user check comes before the tool is resolved / downloaded.
        let tool = if crate::steam::find_user_id(core).is_some() { crate::enhanced_net::resolve_tool_source(core, net, progress) } else { None };
        match linux::configure_bo3_enhanced_linux(core, tool.as_deref()) {
            Ok(()) => {}
            Err(linux::FlowError::SteamNotClosed) => {
                return Err(format!("Installed files, but {}", crate::steam::client::STEAM_CLOSE_TIMEOUT_MSG));
            }
            Err(linux::FlowError::Failed) => return Err("Installed files, but Linux compatibility setup failed.".into()),
        }
    }
    files::finish_install(core, game_dir);
    Ok(())
}

// ---- enhanced-uninstall ---------------------------------------------------------------------

/// Legacy `_uninstall_enhanced`: remove/restore the Enhanced files, run the Linux Steam cleanup
/// and mark the exe variant `default`. `Err` carries the legacy exception text. Also what
/// reset-to-stock calls when Enhanced files are present.
pub fn uninstall_enhanced(core: &Core, game_dir: &Path) -> Result<(), String> {
    let _op = core.lock_ops();
    if !files::uninstall_enhanced_files(core, game_dir, &core.paths.mod_files) {
        return Err("BO3 Enhanced uninstall failed.".into());
    }
    if crate::paths::system_name() == "Linux" {
        match linux::cleanup_bo3_enhanced_linux(core) {
            Ok(()) => {}
            Err(linux::FlowError::SteamNotClosed) => {
                return Err(format!("BO3 Enhanced files were removed, but {}", crate::steam::client::STEAM_CLOSE_TIMEOUT_MSG));
            }
            Err(linux::FlowError::Failed) => {
                return Err("BO3 Enhanced files were removed, but Linux compatibility cleanup was incomplete.".into());
            }
        }
    }
    crate::game::write_exe_variant(game_dir, "default");
    core.log.success("Uninstalled BO3 Enhanced successfully.");
    Ok(())
}

pub fn uninstall_enhanced_op(core: &Core) -> AppResult<State> {
    let _op = core.lock_ops();
    let game_dir = core.game_dir().ok_or_else(|| AppError::new(NO_GAME_DIR))?;
    if let Err(e) = uninstall_enhanced(core, Path::new(&game_dir)) {
        core.log.error(format!("BO3 Enhanced uninstall failed: {e}"));
        return Err(AppError::new(e));
    }
    Ok(State::build(core))
}

// ---- exe swap -------------------------------------------------------------------------------

/// Shared endpoint skeleton: game dir check, run, log `<prefix> failed: <err>`.
fn swap_op(core: &Core, failure_prefix: &str, run: impl FnOnce(&Swap, &Path) -> Result<(), SwapError>) -> AppResult<State> {
    let _op = core.lock_ops();
    let game_dir = core.game_dir().ok_or_else(|| AppError::new(NO_GAME_DIR))?;
    let builds = KnownBuilds::production();
    let swap = Swap::for_op(core, &builds);
    match run(&swap, Path::new(&game_dir)) {
        Ok(()) => Ok(State::build(core)),
        Err(SwapError::DepotRequired) => {
            let msg = SwapError::DepotRequired.to_string();
            core.log.info(&msg);
            Err(AppError::new(msg)
                .with_extra("depotRequired", Value::Bool(true))
                .with_extra("depotCommand", Value::String(COMPATIBLE_DEPOT_COMMAND.into()))
                .with_state(State::build(core)))
        }
        Err(SwapError::Msg(m)) => {
            core.log.error(format!("{failure_prefix} failed: {m}"));
            Err(AppError::new(m))
        }
    }
}

/// `exe-swap/compatible`: `depotRequired` responses carry the depot command and the state.
pub fn install_compatible_exe_op(core: &Core) -> AppResult<State> {
    swap_op(core, "Compatible EXE swap", |s, g| s.install_compatible_exe(g))
}

pub fn restore_current_exe_op(core: &Core) -> AppResult<State> {
    swap_op(core, "Current EXE restore", |s, g| s.restore_current_exe(g))
}

pub fn restore_enhanced_exe_op(core: &Core) -> AppResult<State> {
    swap_op(core, "Enhanced EXE restore", |s, g| s.restore_enhanced_exe(g))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepotStatus {
    pub ok: bool,
    pub available: bool,
    pub state: State,
}

/// `exe-swap/compatible-depot`: is the compatible depot downloaded (exe non-empty + install script)?
pub fn compatible_depot_status(core: &Core) -> AppResult<DepotStatus> {
    let available = crate::enhanced::exe::compatible_depot_download_exists(core);
    Ok(DepotStatus { ok: true, available, state: State::build(core) })
}

/// Reset-to-stock helper: the legacy `_restore_current_exe` (`Err` = exception text; the caller
/// logs `Executable reset step failed: ...`).
pub fn restore_current_exe(core: &Core, game_dir: &Path) -> Result<(), String> {
    let _op = core.lock_ops();
    crate::enhanced::exe::restore_current_exe(core, game_dir)
}

/// Reset-to-stock helper: profile (`current|compatible|enhanced|""`) of the active executable.
pub fn active_exe_profile(core: &Core, game_dir: &Path) -> &'static str {
    crate::enhanced::exe::active_exe_profile(core, game_dir)
}

// ---- state ----------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnhancedState {
    pub installed: bool,
    pub detected_at: Option<String>,
    pub acknowledged_at: Option<String>,
    pub launch_options_active: bool,
    pub dump_source: String,
    pub files_installed: i64,
    /// `Created|Not created`
    pub backup_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExeSwapState {
    pub profile: String,
    pub mode_label: String,
    pub patch_label: String,
    pub display_label: String,
    pub state: String,
    pub active_build_id: String,
    pub active_build_date: String,
    pub current_build_id: String,
    pub current_build_date: String,
    pub compatible_build_id: String,
    pub compatible_build_date: String,
    pub enhanced_build_id: String,
    pub enhanced_build_date: String,
    pub executable: String,
    pub executable_name: String,
    pub executable_hash: String,
    pub trusted_executable: bool,
    pub integrity_status: String,
    pub integrity_message: String,
    pub backup_available: bool,
    pub latest_available: bool,
    pub compatible_available: bool,
    pub enhanced_available: bool,
    pub compatible_active: bool,
    pub enhanced_exe_active: bool,
    pub enhanced_active: bool,
}

/// Legacy `_enhanced_status`. Like the original it may rewrite `bo3_enhanced_state.json`
/// (stale `installed` flag, see [`state_file::status_summary`]).
pub fn enhanced_state(cx: &StateCx) -> EnhancedState {
    let core = cx.core;
    let tracked = state_file::installed_files_len(&state_file::load_state(core));
    let dump_source = core.settings().enhanced_dump_source().to_string();
    let Some(game_dir) = cx.game_dir.as_deref() else {
        return EnhancedState {
            installed: false,
            detected_at: None,
            acknowledged_at: None,
            launch_options_active: false,
            dump_source,
            files_installed: tracked as i64,
            backup_status: "Not created".into(),
        };
    };
    let summary = state_file::status_summary(core, Path::new(game_dir));
    let options = cx.launch_options.as_deref().unwrap_or("");
    EnhancedState {
        installed: summary.installed,
        detected_at: summary.detected_at,
        acknowledged_at: summary.acknowledged_at,
        launch_options_active: options.to_lowercase().contains("windowscodecs=n,b"),
        dump_source,
        files_installed: tracked as i64,
        backup_status: if summary.installed && tracked > 0 { "Created" } else { "Not created" }.into(),
    }
}

pub const EXPECTED_ENHANCED_FILES: [&str; 4] = ["T7WSBootstrapper.dll", "T7InternalWS.dll", "steam_api65.dll", "WindowsCodecs.dll"];

/// Legacy `detect_enhanced_install`: all four Enhanced DLLs are present ("Enhanced files present").
pub fn detect_enhanced_install(game_dir: &Path) -> bool {
    EXPECTED_ENHANCED_FILES.iter().all(|f| game_dir.join(f).exists())
}

/// Lower-cased exe hashes recorded for Enhanced (settings `enhanced_exe_hashes[game_dir]`, plus the
/// legacy single `enhanced_exe_hash`).
pub fn known_enhanced_hashes(core: &Core, game_dir: Option<&str>) -> Vec<String> {
    let settings = core.settings();
    let mut known: Vec<String> = Vec::new();
    if let (Some(Value::Object(map)), Some(g)) = (settings.0.get("enhanced_exe_hashes"), game_dir) {
        match map.get(&crate::pyfmt::path_str(g)) {
            Some(Value::String(s)) if !s.is_empty() => known.push(s.to_lowercase()),
            Some(Value::Array(a)) => known.extend(a.iter().filter_map(Value::as_str).filter(|s| !s.is_empty()).map(str::to_lowercase)),
            _ => {}
        }
    }
    if let Some(s) = settings.get_str("enhanced_exe_hash").filter(|s| !s.is_empty()) {
        known.push(s.to_lowercase());
    }
    known
}

/// Result of the legacy `_executable_integrity_status`.
#[derive(Debug, Clone)]
pub struct Integrity {
    pub trusted: bool,
    pub status: &'static str,
    pub profile: &'static str,
    pub message: String,
}

/// `_executable_integrity_status` against the production hash table.
pub fn executable_integrity(exe_hash: &str, exists: bool, enhanced_active: bool, enhanced_hash_known: bool) -> Integrity {
    crate::enhanced::exe::integrity(&KnownBuilds::production(), exe_hash, exists, enhanced_active, enhanced_hash_known)
}

/// Legacy `_exe_swap_status` (production hash table). The exe digest comes from a
/// signature-validated cache, so `status` does not re-read a 100+ MB executable.
pub fn exe_swap_state(cx: &StateCx) -> ExeSwapState {
    exe_swap_state_with(cx, &KnownBuilds::production())
}

/// [`exe_swap_state`] with an explicit known-hash table (tests).
pub fn exe_swap_state_with(cx: &StateCx, builds: &KnownBuilds) -> ExeSwapState {
    let game = cx.game_dir.as_deref().map(Path::new);
    let exe = game.and_then(crate::game::find_executable);
    let hash = exe.as_deref().and_then(hashcache::file_sha256_cached).unwrap_or_default().to_lowercase();
    let enhanced_active = game.is_some_and(detect_enhanced_install);
    let known = known_enhanced_hashes(cx.core, cx.game_dir.as_deref());
    let integ = crate::enhanced::exe::integrity(builds, &hash, exe.is_some(), enhanced_active, known.contains(&hash));
    let mut profile = integ.profile.to_string();
    if profile.is_empty() {
        profile = game.and_then(crate::game::read_exe_variant).unwrap_or_default();
    }
    let (active_id, active_date) = if exe.is_none() {
        ("Unknown", "")
    } else if profile == ENHANCED_EXE_ID {
        ("Enhanced", "")
    } else if profile == COMPATIBLE_EXE_ID {
        (COMPATIBLE_STEAM_BUILD_ID, COMPATIBLE_STEAM_BUILD_DATE)
    } else if profile == CURRENT_EXE_ID || profile == "default" {
        profile = CURRENT_EXE_ID.into();
        (CURRENT_STEAM_BUILD_ID, CURRENT_STEAM_BUILD_DATE)
    } else {
        ("Unverified", "")
    };

    let swap = Swap::for_state(cx.core, builds);
    let latest_available = exe.as_deref().is_some_and(|e| swap.validated_latest_build_backup_path(e, false).is_some());
    let compatible_available = exe.as_deref().is_some_and(|e| swap.validated_preserved_compatible_exe(e, false).is_some());
    let enhanced_backup = exe.as_deref().is_some_and(|e| swap.validated_enhanced_backup_path(game, e, false).is_some());

    let patch_label = if profile == COMPATIBLE_EXE_ID { "T7 Patch 2.04" } else { T7_PATCH_LABEL_CURRENT };
    ExeSwapState {
        mode_label: match profile.as_str() {
            COMPATIBLE_EXE_ID => "Compatible EXE",
            CURRENT_EXE_ID => "Current EXE",
            ENHANCED_EXE_ID => "Enhanced",
            _ => "Unknown",
        }
        .into(),
        patch_label: patch_label.into(),
        display_label: format!("{} T7 installs will use {patch_label}.", integ.message),
        state: integ.status.into(),
        active_build_id: active_id.into(),
        active_build_date: active_date.into(),
        current_build_id: CURRENT_STEAM_BUILD_ID.into(),
        current_build_date: CURRENT_STEAM_BUILD_DATE.into(),
        compatible_build_id: COMPATIBLE_STEAM_BUILD_ID.into(),
        compatible_build_date: COMPATIBLE_STEAM_BUILD_DATE.into(),
        enhanced_build_id: "Enhanced".into(),
        enhanced_build_date: String::new(),
        executable: exe.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
        executable_name: exe.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        executable_hash: hash,
        trusted_executable: integ.trusted,
        integrity_status: integ.status.into(),
        integrity_message: integ.message,
        backup_available: latest_available,
        latest_available,
        compatible_available,
        enhanced_available: enhanced_backup || profile == ENHANCED_EXE_ID,
        compatible_active: profile == COMPATIBLE_EXE_ID,
        enhanced_exe_active: profile == ENHANCED_EXE_ID,
        enhanced_active,
        profile,
    }
}
