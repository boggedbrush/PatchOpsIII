//! Group `enhanced`: `enhanced-validate`, `enhanced-install`, `enhanced-uninstall`,
//! `exe-swap/compatible`, `exe-swap/compatible-depot`, `exe-swap/current`, `exe-swap/enhanced`
//! plus the `enhanced` and `exeSwap` state builders.
//!
//! TODO(enhanced agent) - port from `backend/api.py` + `bo3_enhanced.py` + `utils.py`:
//! `validate_enhanced`, `install_enhanced`/`_install_enhanced`, `uninstall_enhanced`/`_uninstall_enhanced`
//! (+ `cleanup_bo3_enhanced_linux`, uses Steam control from `ops::steam_ops`),
//! `install_compatible_exe`, `compatible_depot_status`, `restore_current_exe`, `restore_enhanced_exe`
//! and everything they call (`_executable_integrity_status`, `_exe_swap_status`, `_enhanced_status`,
//! `_known_enhanced_hashes`, `_record_enhanced_hash`, depot helpers, `detect_enhanced_install`,
//! `status_summary`, `load_state`). Hashing: `crate::hash`. `exe_swap_state` is implemented except the
//! backup-availability flags; `enhanced_state` is still a stub.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{not_implemented, Args, Group};
use crate::context::Core;
use crate::error::OpError;
use crate::state::StateCx;

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
pub const COMPATIBLE_DEPOT_COMMAND: &str = "download_depot 311210 311211 9084453472036406216";
pub const T7_PATCH_LABEL_CURRENT: &str = "T7 Patch Scroptss/T7Patch";

fn call(core: &Core, op: &str, args: &Args) -> Result<Value, OpError> {
    // TODO(enhanced agent)
    not_implemented(core, op, args)
}

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

/// TODO(enhanced agent): real implementation (`status_summary`, `load_state`). Stub = no-game defaults.
pub fn enhanced_state(cx: &StateCx) -> EnhancedState {
    EnhancedState {
        installed: false,
        detected_at: None,
        acknowledged_at: None,
        launch_options_active: false,
        dump_source: cx.core.settings().enhanced_dump_source().to_string(),
        files_installed: 0,
        backup_status: "Not created".into(),
    }
}

pub const EXPECTED_ENHANCED_FILES: [&str; 4] = ["T7WSBootstrapper.dll", "T7InternalWS.dll", "steam_api65.dll", "WindowsCodecs.dll"];

/// Legacy `detect_enhanced_install`: all four Enhanced DLLs are present.
pub fn detect_enhanced_install(game_dir: &std::path::Path) -> bool {
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

pub fn executable_integrity(exe_hash: &str, exists: bool, enhanced_active: bool, enhanced_hash_known: bool) -> Integrity {
    let h = exe_hash.to_lowercase();
    if !exists {
        return Integrity { trusted: false, status: "missing", profile: "", message: "BlackOps3.exe or BlackOpsIII.exe was not found.".into() };
    }
    if h == DEFAULT_STEAM_EXE_SHA256.to_lowercase() {
        return Integrity { trusted: true, status: "trusted", profile: "current", message: format!("Executable matches latest Steam BuildID {CURRENT_STEAM_BUILD_ID}.") };
    }
    if COMPATIBLE_BUILD_SHA256.contains(&h.as_str()) {
        return Integrity { trusted: true, status: "trusted", profile: "compatible", message: format!("Executable matches compatible BuildID {COMPATIBLE_STEAM_BUILD_ID}.") };
    }
    if enhanced_active || enhanced_hash_known {
        return Integrity { trusted: true, status: "enhanced", profile: "enhanced", message: "Executable is managed by BO3 Enhanced.".into() };
    }
    let short: String = if h.is_empty() { "unreadable".into() } else { h.chars().take(12).collect() };
    Integrity {
        trusted: false,
        status: "unverified",
        profile: "",
        message: format!(
            "Active executable hash {short} does not match latest BuildID {CURRENT_STEAM_BUILD_ID}, compatible BuildID {COMPATIBLE_STEAM_BUILD_ID}, or a PatchOpsIII-preserved Enhanced EXE."
        ),
    }
}

/// Legacy `_exe_swap_status`.
///
/// TODO(enhanced agent): `backup_available`/`latest_available`/`compatible_available`/
/// `enhanced_available` need the validated-backup helpers (`_validated_*_backup_path`); they are
/// `false` here (correct for a fresh install without preserved executables).
pub fn exe_swap_state(cx: &StateCx) -> ExeSwapState {
    let game = cx.game_dir.as_deref().map(std::path::Path::new);
    let exe = game.and_then(crate::game::find_executable);
    let hash = exe.as_deref().and_then(crate::hash::file_sha256).unwrap_or_default().to_lowercase();
    let enhanced_active = game.is_some_and(detect_enhanced_install);
    let known = known_enhanced_hashes(cx.core, cx.game_dir.as_deref());
    let integ = executable_integrity(&hash, exe.is_some(), enhanced_active, known.contains(&hash));
    let mut profile = integ.profile.to_string();
    if profile.is_empty() {
        profile = game.and_then(crate::game::read_exe_variant).unwrap_or_default();
    }
    let (active_id, active_date) = if exe.is_none() {
        ("Unknown", "")
    } else if profile == "enhanced" {
        ("Enhanced", "")
    } else if profile == "compatible" {
        (COMPATIBLE_STEAM_BUILD_ID, COMPATIBLE_STEAM_BUILD_DATE)
    } else if profile == "current" || profile == "default" {
        profile = "current".into();
        (CURRENT_STEAM_BUILD_ID, CURRENT_STEAM_BUILD_DATE)
    } else {
        ("Unverified", "")
    };
    let patch_label = if profile == "compatible" { "T7 Patch 2.04" } else { T7_PATCH_LABEL_CURRENT };
    ExeSwapState {
        mode_label: match profile.as_str() {
            "compatible" => "Compatible EXE",
            "current" => "Current EXE",
            "enhanced" => "Enhanced",
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
        backup_available: false,
        latest_available: false,
        compatible_available: false,
        enhanced_available: profile == "enhanced",
        compatible_active: profile == "compatible",
        enhanced_exe_active: profile == "enhanced",
        enhanced_active,
        profile,
    }
}
