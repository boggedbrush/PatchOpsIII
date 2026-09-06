use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    app::{AppState, Settings},
    fs_ops,
    models::ExeSwapState,
    steam,
};

pub const CURRENT_EXE_ID: &str = "current";
pub const COMPATIBLE_EXE_ID: &str = "compatible";
pub const ENHANCED_EXE_ID: &str = "enhanced";
pub const DEFAULT_STEAM_EXE_SHA256: &str =
    "9ba98dba41e18ef47de6c63937340f8eae7cb251f8fbc2e78d70047b64aa15b5";
pub const COMPATIBLE_BUILD_SHA256: &[&str] =
    &["66b95eb4667bd5b3b3d230e7bed1d29ccd261d48ca2699f01216c863be24ff44"];
pub const COMPATIBLE_DEPOT_MANIFEST_ID: &str = "9084453472036406216";
pub const COMPATIBLE_DEPOT_COMMAND: &str = "download_depot 311210 311211 9084453472036406216";
pub const COMPATIBLE_DEPOT_REQUIRED_MESSAGE: &str = "Compatible Steam depot was not found. Copy the depot command, open Steam Console, run it, then continue once the download finishes.";
pub const CURRENT_STEAM_BUILD_ID: &str = "21201493";
pub const CURRENT_STEAM_BUILD_DATE: &str = "Feb 19, 2026";
pub const COMPATIBLE_STEAM_BUILD_ID: &str = "10650222";
pub const COMPATIBLE_STEAM_BUILD_DATE: &str = "Mar 3, 2023";

const GAME_EXECUTABLE_NAMES: [&str; 2] = ["BlackOpsIII.exe", "BlackOps3.exe"];
const VARIANT_STATE_FILENAME: &str = ".patchops_exe_variant.json";

#[derive(Clone, Debug)]
struct Integrity {
    trusted: bool,
    status: &'static str,
    profile: &'static str,
    message: String,
}

#[derive(Serialize, Deserialize)]
struct VariantState {
    variant: String,
}

pub fn find_executable(game_dir: &Path) -> Option<PathBuf> {
    GAME_EXECUTABLE_NAMES
        .iter()
        .map(|name| game_dir.join(name))
        .find(|candidate| candidate.is_file())
}

pub fn read_variant(game_dir: &Path) -> Option<String> {
    let body = fs::read(game_dir.join(VARIANT_STATE_FILENAME)).ok()?;
    serde_json::from_slice::<VariantState>(&body)
        .ok()
        .map(|state| state.variant)
}

pub fn write_variant(game_dir: &Path, variant: &str) -> Result<(), String> {
    let body = serde_json::to_vec_pretty(&VariantState {
        variant: variant.to_owned(),
    })
    .map_err(|error| error.to_string())?;
    fs_ops::atomic_write(&game_dir.join(VARIANT_STATE_FILENAME), &body)
}

fn known_enhanced_hashes(settings: &Settings, game_dir: Option<&Path>) -> HashSet<String> {
    let mut known = HashSet::new();
    if let Some(game_dir) = game_dir {
        if let Some(value) = settings
            .enhanced_exe_hashes
            .get(&game_dir.display().to_string())
        {
            match value {
                Value::String(hash) if !hash.is_empty() => {
                    known.insert(hash.to_ascii_lowercase());
                }
                Value::Array(hashes) => {
                    known.extend(
                        hashes
                            .iter()
                            .filter_map(Value::as_str)
                            .filter(|hash| !hash.is_empty())
                            .map(str::to_ascii_lowercase),
                    );
                }
                _ => {}
            }
        }
    }
    if let Some(hash) = settings
        .enhanced_exe_hash
        .as_deref()
        .filter(|hash| !hash.is_empty())
    {
        known.insert(hash.to_ascii_lowercase());
    }
    known
}

pub fn record_enhanced_hash(
    state: &AppState,
    game_dir: &Path,
    exe_hash: &str,
) -> Result<(), String> {
    let normalized = exe_hash.to_ascii_lowercase();
    if normalized.is_empty() {
        return Ok(());
    }

    let mut settings = state.load_settings();
    let key = game_dir.display().to_string();
    let mut values = match settings.enhanced_exe_hashes.remove(&key) {
        Some(Value::String(value)) => vec![Value::String(value)],
        Some(Value::Array(values)) => values,
        _ => Vec::new(),
    };
    if !values
        .iter()
        .filter_map(Value::as_str)
        .any(|value| value.eq_ignore_ascii_case(&normalized))
    {
        values.push(Value::String(normalized));
    }
    if values.len() > 5 {
        values.drain(..values.len() - 5);
    }
    settings
        .enhanced_exe_hashes
        .insert(key, Value::Array(values));
    state.save_settings(&settings)
}

fn integrity_for_hash(
    exe_hash: &str,
    executable_exists: bool,
    enhanced_active: bool,
    enhanced_hash_known: bool,
) -> Integrity {
    let normalized = exe_hash.to_ascii_lowercase();
    if !executable_exists {
        return Integrity {
            trusted: false,
            status: "missing",
            profile: "",
            message: "BlackOps3.exe or BlackOpsIII.exe was not found.".into(),
        };
    }
    if normalized == DEFAULT_STEAM_EXE_SHA256 {
        return Integrity {
            trusted: true,
            status: "trusted",
            profile: CURRENT_EXE_ID,
            message: format!("Executable matches latest Steam BuildID {CURRENT_STEAM_BUILD_ID}."),
        };
    }
    if COMPATIBLE_BUILD_SHA256.contains(&normalized.as_str()) {
        return Integrity {
            trusted: true,
            status: "trusted",
            profile: COMPATIBLE_EXE_ID,
            message: format!("Executable matches compatible BuildID {COMPATIBLE_STEAM_BUILD_ID}."),
        };
    }
    if enhanced_active || enhanced_hash_known {
        return Integrity {
            trusted: true,
            status: "enhanced",
            profile: ENHANCED_EXE_ID,
            message: "Executable is managed by BO3 Enhanced.".into(),
        };
    }
    let short_hash = if normalized.is_empty() {
        "unreadable"
    } else {
        &normalized[..normalized.len().min(12)]
    };
    Integrity {
        trusted: false,
        status: "unverified",
        profile: "",
        message: format!(
            "Active executable hash {short_hash} does not match latest BuildID {CURRENT_STEAM_BUILD_ID}, compatible BuildID {COMPATIBLE_STEAM_BUILD_ID}, or a PatchOpsIII-preserved Enhanced EXE."
        ),
    }
}

fn active_integrity(state: &AppState, game_dir: &Path, target: &Path) -> Integrity {
    let hash = fs_ops::sha256_file(target)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let settings = state.load_settings();
    integrity_for_hash(
        &hash,
        target.is_file(),
        crate::enhanced::detect_install(game_dir),
        known_enhanced_hashes(&settings, Some(game_dir)).contains(&hash),
    )
}

fn inactive_build_path(target: &Path, build_id: &str) -> PathBuf {
    target.with_file_name(format!(
        "{}.{}.bak",
        target
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("BlackOps3"),
        build_id
    ))
}

fn enhanced_backup_path(target: &Path) -> PathBuf {
    target.with_file_name(format!(
        "{}.enhanced.bak",
        target
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("BlackOps3")
    ))
}

fn sorted_matching_files(parent: &Path, prefix: &str, suffix: &str) -> Vec<PathBuf> {
    let mut matches: Vec<_> = fs::read_dir(parent)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            (path.is_file() && name.starts_with(prefix) && name.ends_with(suffix)).then_some(path)
        })
        .collect();
    matches.sort_by_key(|path| {
        std::cmp::Reverse(
            path.metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH),
        )
    });
    matches
}

fn build_backup_candidates(target: &Path, build_id: &str, role: &str) -> Vec<PathBuf> {
    let stem = target
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("BlackOps3");
    let extension = target
        .extension()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let exact = inactive_build_path(target, build_id);
    let old_exact = target.with_file_name(format!("{stem}.{role}-{build_id}.{extension}"));
    let mut candidates = Vec::new();
    if exact.is_file() {
        candidates.push(exact);
    }
    if old_exact.is_file() {
        candidates.push(old_exact);
    }
    candidates.extend(sorted_matching_files(
        target.parent().unwrap_or_else(|| Path::new(".")),
        &format!("{stem}.{role}-{build_id}-"),
        &format!(".{extension}"),
    ));
    candidates
}

fn first_matching_hash(candidates: Vec<PathBuf>, hashes: &[&str]) -> Option<PathBuf> {
    candidates.into_iter().find(|candidate| {
        fs_ops::sha256_file(candidate)
            .map(|hash| {
                hashes
                    .iter()
                    .any(|expected| hash.eq_ignore_ascii_case(expected))
            })
            .unwrap_or(false)
    })
}

fn validated_preserved_compatible(target: &Path) -> Option<PathBuf> {
    first_matching_hash(
        build_backup_candidates(target, COMPATIBLE_STEAM_BUILD_ID, "compatible"),
        COMPATIBLE_BUILD_SHA256,
    )
}

fn validated_latest_backup(target: &Path) -> Option<PathBuf> {
    first_matching_hash(
        build_backup_candidates(target, CURRENT_STEAM_BUILD_ID, "current"),
        &[DEFAULT_STEAM_EXE_SHA256],
    )
    .or_else(|| {
        fs_ops::existing_backup(target).filter(|candidate| {
            fs_ops::sha256_file(candidate)
                .map(|hash| hash.eq_ignore_ascii_case(DEFAULT_STEAM_EXE_SHA256))
                .unwrap_or(false)
        })
    })
}

fn validated_enhanced_backup(
    settings: &Settings,
    game_dir: Option<&Path>,
    target: &Path,
) -> Option<PathBuf> {
    let known = known_enhanced_hashes(settings, game_dir);
    if known.is_empty() {
        return None;
    }
    let stem = target
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("BlackOps3");
    let exact = enhanced_backup_path(target);
    let mut candidates = if exact.is_file() {
        vec![exact]
    } else {
        Vec::new()
    };
    candidates.extend(sorted_matching_files(
        target.parent().unwrap_or_else(|| Path::new(".")),
        &format!("{stem}.enhanced-"),
        ".bak",
    ));
    candidates.into_iter().find(|candidate| {
        fs_ops::sha256_file(candidate)
            .map(|hash| known.contains(&hash.to_ascii_lowercase()))
            .unwrap_or(false)
    })
}

pub fn status(settings: &Settings, game_dir: Option<&Path>) -> ExeSwapState {
    let executable = game_dir.and_then(find_executable);
    let exe_hash = executable
        .as_deref()
        .and_then(|path| fs_ops::sha256_file(path).ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let enhanced_active = game_dir.is_some_and(crate::enhanced::detect_install);
    let integrity = integrity_for_hash(
        &exe_hash,
        executable.is_some(),
        enhanced_active,
        known_enhanced_hashes(settings, game_dir).contains(&exe_hash),
    );
    let mut profile = if integrity.profile.is_empty() {
        game_dir.and_then(read_variant).unwrap_or_default()
    } else {
        integrity.profile.to_owned()
    };
    let (active_build_id, active_build_date) = if executable.is_none() {
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

    let latest_available = executable
        .as_deref()
        .is_some_and(|path| validated_latest_backup(path).is_some());
    let compatible_available = executable
        .as_deref()
        .is_some_and(|path| validated_preserved_compatible(path).is_some());
    let enhanced_available = executable
        .as_deref()
        .is_some_and(|path| validated_enhanced_backup(settings, game_dir, path).is_some());
    let patch_label = if profile == COMPATIBLE_EXE_ID {
        "T7 Patch 2.04"
    } else {
        "T7 Patch Scroptss/T7Patch"
    };
    let mode_label = match profile.as_str() {
        COMPATIBLE_EXE_ID => "Compatible EXE",
        CURRENT_EXE_ID => "Current EXE",
        ENHANCED_EXE_ID => "Enhanced",
        _ => "Unknown",
    };

    ExeSwapState {
        profile: profile.clone(),
        mode_label: mode_label.into(),
        patch_label: patch_label.into(),
        display_label: format!("{} T7 installs will use {patch_label}.", integrity.message),
        state: integrity.status.into(),
        active_build_id: active_build_id.into(),
        active_build_date: active_build_date.into(),
        current_build_id: CURRENT_STEAM_BUILD_ID.into(),
        current_build_date: CURRENT_STEAM_BUILD_DATE.into(),
        compatible_build_id: COMPATIBLE_STEAM_BUILD_ID.into(),
        compatible_build_date: COMPATIBLE_STEAM_BUILD_DATE.into(),
        enhanced_build_id: "Enhanced".into(),
        enhanced_build_date: String::new(),
        executable: executable
            .as_deref()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        executable_name: executable
            .as_deref()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        executable_hash: exe_hash,
        trusted_executable: integrity.trusted,
        integrity_status: integrity.status.into(),
        integrity_message: integrity.message,
        backup_available: latest_available,
        latest_available,
        compatible_available,
        enhanced_available: enhanced_available || profile == ENHANCED_EXE_ID,
        compatible_active: profile == COMPATIBLE_EXE_ID,
        enhanced_exe_active: profile == ENHANCED_EXE_ID,
        enhanced_active,
    }
}

fn available_sibling_path(path: &Path) -> Result<PathBuf, String> {
    if !path.exists() {
        return Ok(path.to_owned());
    }
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Invalid executable backup filename.".to_string())?;
    let extension = path
        .extension()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    for index in 2..1000 {
        let candidate = path.with_file_name(if extension.is_empty() {
            format!("{stem}-{index}")
        } else {
            format!("{stem}-{index}.{extension}")
        });
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "No available filename found near {}",
        path.display()
    ))
}

fn unique_stage_path(target: &Path) -> Result<PathBuf, String> {
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Invalid executable filename.".to_string())?;
    for index in 1..1000 {
        let candidate = target.with_file_name(format!(
            ".{name}.patchops-stage-{}-{index}",
            std::process::id()
        ));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("Could not allocate an executable staging path.".into())
}

fn activate_source(
    target: &Path,
    source: &Path,
    expected_hashes: &[&str],
    preserved_active: &Path,
    consume_source: bool,
) -> Result<(), String> {
    let source_hash = fs_ops::sha256_file(source)?;
    if !expected_hashes
        .iter()
        .any(|expected| source_hash.eq_ignore_ascii_case(expected))
    {
        return Err(format!(
            "Refusing executable with untrusted SHA-256: {}",
            source.display()
        ));
    }

    let stage = unique_stage_path(target)?;
    fs::copy(source, &stage)
        .map_err(|error| format!("Failed to stage {}: {error}", source.display()))?;
    let staged_hash = fs_ops::sha256_file(&stage);
    if !staged_hash.is_ok_and(|hash| hash.eq_ignore_ascii_case(&source_hash)) {
        let _ = fs::remove_file(&stage);
        return Err("Staged executable failed SHA-256 verification.".into());
    }

    let mut active_preserved = false;
    let result = (|| {
        if target.exists() {
            fs::rename(target, preserved_active)
                .map_err(|error| format!("Failed to preserve active executable: {error}"))?;
            active_preserved = true;
        }
        fs::rename(&stage, target)
            .map_err(|error| format!("Failed to activate staged executable: {error}"))?;
        let activated_hash = fs_ops::sha256_file(target)?;
        if !activated_hash.eq_ignore_ascii_case(&source_hash)
            || !expected_hashes
                .iter()
                .any(|expected| activated_hash.eq_ignore_ascii_case(expected))
        {
            return Err("Activated executable failed SHA-256 verification.".into());
        }
        Ok(())
    })();

    if let Err(error) = result {
        let _ = fs::remove_file(&stage);
        let _ = fs::remove_file(target);
        let rollback = if active_preserved {
            fs::rename(preserved_active, target).map_err(|rollback| rollback.to_string())
        } else {
            Ok(())
        };
        return match rollback {
            Ok(()) => Err(error),
            Err(rollback) => Err(format!("{error} Rollback also failed: {rollback}")),
        };
    }

    if consume_source {
        let _ = fs::remove_file(source);
    }
    Ok(())
}

fn preserved_path_for_profile(target: &Path, profile: &str) -> Result<PathBuf, String> {
    let preferred = match profile {
        CURRENT_EXE_ID => inactive_build_path(target, CURRENT_STEAM_BUILD_ID),
        COMPATIBLE_EXE_ID => inactive_build_path(target, COMPATIBLE_STEAM_BUILD_ID),
        ENHANCED_EXE_ID => enhanced_backup_path(target),
        _ => return Err("Active executable is not a recognized PatchOpsIII build.".into()),
    };
    available_sibling_path(&preferred)
}

fn prepare_activation(
    state: &AppState,
    game_dir: &Path,
    target: &Path,
    desired_profile: &str,
) -> Result<Option<PathBuf>, String> {
    if !target.is_file() {
        return Ok(Some(available_sibling_path(
            &target.with_file_name(".patchops-missing-exe.rollback"),
        )?));
    }
    let integrity = active_integrity(state, game_dir, target);
    if integrity.profile == desired_profile {
        return Ok(None);
    }
    if !integrity.trusted || integrity.profile.is_empty() {
        return Err(integrity.message);
    }
    if integrity.profile == ENHANCED_EXE_ID {
        let hash = fs_ops::sha256_file(target)?;
        record_enhanced_hash(state, game_dir, &hash)?;
    }
    preserved_path_for_profile(target, integrity.profile).map(Some)
}

fn copy_depot_installscript(state: &AppState, depot: &Path, game_dir: &Path) {
    let source = depot.join("installscript_311210.vdf");
    if !source.is_file() {
        return;
    }
    let target = game_dir.join("installscript_311210.vdf");
    let backup = fs_ops::backup_path(&target);
    let result = (|| {
        if target.is_file() && !backup.exists() {
            fs::copy(&target, &backup).map_err(|error| error.to_string())?;
        }
        let body = fs::read(&source).map_err(|error| error.to_string())?;
        fs_ops::atomic_write(&target, &body)
    })();
    if let Err(error) = result {
        state.log("Warning", format!("Compatible executable is active, but installscript_311210.vdf could not be updated: {error}"));
    }
}

fn restore_installscript_backup(state: &AppState, game_dir: &Path) {
    let target = game_dir.join("installscript_311210.vdf");
    let backup = fs_ops::backup_path(&target);
    if !backup.is_file() {
        return;
    }
    let result = fs::read(&backup)
        .and_then(|body| fs_ops::atomic_write(&target, &body).map_err(std::io::Error::other));
    if let Err(error) = result {
        state.log("Warning", format!("Current executable is active, but installscript_311210.vdf could not be restored: {error}"));
    }
}

fn remember_variant(state: &AppState, game_dir: &Path, variant: &str) {
    if let Err(error) = write_variant(game_dir, variant) {
        state.log(
            "Warning",
            format!("Executable was activated, but its variant marker could not be saved: {error}"),
        );
    }
}

fn activate_compatible_from_depot(
    state: &AppState,
    game_dir: &Path,
    depot: Option<PathBuf>,
) -> Result<(), String> {
    let target = find_executable(game_dir)
        .ok_or_else(|| "BlackOps3.exe or BlackOpsIII.exe was not found.".to_string())?;
    let Some(preserved_active) = prepare_activation(state, game_dir, &target, COMPATIBLE_EXE_ID)?
    else {
        state.log("Info", "Compatible executable is already active.");
        return Ok(());
    };

    if let Some(source) = validated_preserved_compatible(&target) {
        activate_source(
            &target,
            &source,
            COMPATIBLE_BUILD_SHA256,
            &preserved_active,
            true,
        )?;
        remember_variant(state, game_dir, COMPATIBLE_EXE_ID);
        state.log(
            "Success",
            format!(
                "Reused preserved compatible executable from {}.",
                source.file_name().unwrap_or_default().to_string_lossy()
            ),
        );
        return Ok(());
    }

    let depot = depot.ok_or_else(|| COMPATIBLE_DEPOT_REQUIRED_MESSAGE.to_string())?;
    activate_source(
        &target,
        &depot.join("BlackOps3.exe"),
        COMPATIBLE_BUILD_SHA256,
        &preserved_active,
        false,
    )?;
    copy_depot_installscript(state, &depot, game_dir);
    remember_variant(state, game_dir, COMPATIBLE_EXE_ID);
    state.log(
        "Success",
        format!("Compatible executable installed from Steam depot {COMPATIBLE_DEPOT_MANIFEST_ID}."),
    );
    Ok(())
}

pub fn activate_current(state: &AppState, game_dir: &Path) -> Result<(), String> {
    if !game_dir.is_dir() {
        return Err("Game directory is not set.".into());
    }
    let target = find_executable(game_dir).unwrap_or_else(|| game_dir.join("BlackOps3.exe"));
    let Some(preserved_active) = prepare_activation(state, game_dir, &target, CURRENT_EXE_ID)?
    else {
        state.log("Info", "Latest executable is already active.");
        return Ok(());
    };
    let source = validated_latest_backup(&target)
        .ok_or_else(|| "No backup executable found. Cannot restore the current EXE.".to_string())?;
    activate_source(
        &target,
        &source,
        &[DEFAULT_STEAM_EXE_SHA256],
        &preserved_active,
        true,
    )?;
    restore_installscript_backup(state, game_dir);
    remember_variant(state, game_dir, "default");
    state.log("Success", "Current executable restored from backup.");
    Ok(())
}

pub fn activate_enhanced(state: &AppState, game_dir: &Path) -> Result<(), String> {
    if !game_dir.is_dir() {
        return Err("Game directory is not set.".into());
    }
    let target = find_executable(game_dir).unwrap_or_else(|| game_dir.join("BlackOps3.exe"));
    let Some(preserved_active) = prepare_activation(state, game_dir, &target, ENHANCED_EXE_ID)?
    else {
        state.log("Info", "Enhanced executable is already active.");
        return Ok(());
    };
    let settings = state.load_settings();
    let source = validated_enhanced_backup(&settings, Some(game_dir), &target)
        .ok_or_else(|| "No PatchOpsIII-preserved Enhanced executable was found.".to_string())?;
    let hash = fs_ops::sha256_file(&source)?;
    let expected = [hash.as_str()];
    activate_source(&target, &source, &expected, &preserved_active, true)?;
    record_enhanced_hash(state, game_dir, &hash)?;
    remember_variant(state, game_dir, ENHANCED_EXE_ID);
    state.log(
        "Success",
        "Enhanced executable restored from PatchOpsIII backup.",
    );
    Ok(())
}

pub fn compatible_depot_available() -> bool {
    steam::compatible_depot_download_exists()
}

pub fn activate_compatible(state: &AppState, game_dir: &Path) -> Result<(), String> {
    activate_compatible_from_depot(state, game_dir, steam::find_valid_compatible_depot())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "patchops-exe-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn classifies_exact_supported_hashes() {
        let current = integrity_for_hash(DEFAULT_STEAM_EXE_SHA256, true, false, false);
        assert!(current.trusted);
        assert_eq!(current.profile, CURRENT_EXE_ID);

        let compatible = integrity_for_hash(COMPATIBLE_BUILD_SHA256[0], true, false, false);
        assert!(compatible.trusted);
        assert_eq!(compatible.profile, COMPATIBLE_EXE_ID);

        let unknown = integrity_for_hash(&"a".repeat(64), true, false, false);
        assert!(!unknown.trusted);
        assert_eq!(unknown.status, "unverified");
    }

    #[test]
    fn staged_activation_verifies_before_touching_active_exe() {
        let root = temp_dir("staged");
        let target = root.join("BlackOps3.exe");
        let source = root.join("source.exe");
        let preserved = root.join("BlackOps3.21201493.bak");
        fs::write(&target, b"current").unwrap();
        fs::write(&source, b"compatible").unwrap();
        let expected = fs_ops::sha256_file(&source).unwrap();

        activate_source(&target, &source, &[&expected], &preserved, false).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"compatible");
        assert_eq!(fs::read(&preserved).unwrap(), b"current");

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn hash_mismatch_leaves_active_exe_untouched() {
        let root = temp_dir("mismatch");
        let target = root.join("BlackOps3.exe");
        let source = root.join("source.exe");
        let preserved = root.join("BlackOps3.21201493.bak");
        fs::write(&target, b"current").unwrap();
        fs::write(&source, b"tampered").unwrap();

        assert!(activate_source(&target, &source, &[&"0".repeat(64)], &preserved, false).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"current");
        assert!(!preserved.exists());

        fs::remove_dir_all(root).unwrap();
    }
}
