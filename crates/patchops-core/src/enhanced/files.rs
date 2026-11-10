//! Dump validation and file placement / removal for BO3 Enhanced (legacy `bo3_enhanced.py`).
//!
//! Everything here is local: the network part of an install (downloading the Enhanced release
//! into `BO3 Mod Files/BO3Enhanced_latest.zip`) is somebody else's; once that archive exists,
//! [`prepare_install`] -> [`install_enhanced_files`] -> [`finish_install`] is the whole local
//! pipeline (Linux Proton-tool configuration is Steam/network territory and goes in between).

use crate::fsutil::copy2;
use crate::pyfmt::rename_error_text;
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::state_file::{self, installed_files, load_state, save_state, utc_timestamp};
use super::zip::{ZipArchive, ZipError};
use crate::backup;
use crate::context::Core;
use crate::log::Category;
use crate::ops::enhanced::EXPECTED_ENHANCED_FILES;
use crate::pyfmt::{abspath, io_error_text, normpath};

pub const ENHANCED_ARCHIVE_NAME: &str = "BO3Enhanced_latest.zip";
pub const DUMP_ARCHIVE_NAME: &str = "DUMP.zip";
pub const EXPECTED_DUMP_FILES: [&str; 2] = ["BlackOps3.exe", "MicrosoftGame.config"];

/// Only these dump members are ever copied into the game directory.
pub const UWP_DUMP_WHITELIST: [&str; 8] = [
    "BlackOps3.exe",
    "MicrosoftGame.config",
    "GameChat2.dll",
    "Party.dll",
    "PartyXboxLive.dll",
    "PlayFabMultiplayerGDK.dll",
    "libScePad.dll",
    "XCurl.dll",
];

fn zip_err_text(e: &ZipError, path: &Path) -> String {
    match e {
        ZipError::Bad(m) => m.clone(),
        ZipError::Io(e) => io_error_text(e, Some(path)),
    }
}

/// Legacy `_is_within_root` (lexical, after `abspath`).
pub fn is_within_root(root: &Path, candidate: &Path) -> bool {
    let r = normpath(&abspath(&root.to_string_lossy()));
    let c = normpath(&abspath(&candidate.to_string_lossy()));
    Path::new(&c).starts_with(Path::new(&r))
}

fn basename(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

/// Legacy `_should_copy_dump_member`.
pub fn should_copy_dump_member(rel_path: &str) -> bool {
    let name = basename(rel_path);
    if EXPECTED_ENHANCED_FILES.contains(&name) {
        return false;
    }
    UWP_DUMP_WHITELIST.contains(&name)
}

fn strip_dump_prefix(rel: &str) -> &str {
    rel.strip_prefix("DUMP/").unwrap_or(rel)
}

/// Files below `root` like `os.walk` (symlinks to directories are neither listed nor entered).
fn walk_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(root) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if e.file_type().map(|t| !t.is_symlink()).unwrap_or(false) {
                walk_files(&p, out);
            }
        } else {
            out.push(p);
        }
    }
}

fn rel_slash(root: &Path, p: &Path) -> String {
    let rel = p.strip_prefix(root).unwrap_or(p);
    rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

/// Legacy `validate_dump_source`: a directory or zip holding `BlackOps3.exe` and
/// `MicrosoftGame.config` (optionally below `DUMP/`). Logs file-only like the original.
pub fn validate_dump_source(core: &Core, source: &Path) -> bool {
    let log = |cat: Category, m: String| core.log.log_file_only(cat, m);
    if !source.exists() {
        log(Category::Error, format!("Dump source not found: {}", source.display()));
        return false;
    }
    let mut found: BTreeSet<String> = BTreeSet::new();
    if source.is_dir() {
        let mut files = Vec::new();
        walk_files(source, &mut files);
        for f in files {
            let rel = rel_slash(source, &f);
            found.insert(strip_dump_prefix(&rel).to_string());
        }
    } else {
        if !ZipArchive::is_zipfile(source) {
            log(Category::Error, "Dump source is not a valid zip file or directory".into());
            return false;
        }
        match ZipArchive::open(source) {
            Ok(z) => {
                for e in z.entries() {
                    found.insert(strip_dump_prefix(&e.name).to_string());
                }
            }
            Err(e) => {
                log(Category::Error, format!("Failed to validate dump source: {}", zip_err_text(&e, source)));
                return false;
            }
        }
    }
    // Legacy order is hash-seed dependent; sorted here (OPS.md quirk 4).
    let missing: Vec<&str> = EXPECTED_DUMP_FILES.iter().copied().filter(|f| !found.contains(*f)).collect();
    if !missing.is_empty() {
        log(Category::Error, format!("Dump source missing required files: {}", missing.join(", ")));
        return false;
    }
    true
}

/// Legacy `validate_enhanced_archive` (file-only logging).
pub fn validate_enhanced_archive(core: &Core, path: &Path) -> bool {
    match ZipArchive::open(path) {
        Ok(z) => {
            let names: BTreeSet<&str> = z.entries().iter().map(|e| basename(&e.name)).collect();
            let mut missing: Vec<&str> = EXPECTED_ENHANCED_FILES.iter().copied().filter(|f| !names.contains(*f)).collect();
            if !missing.is_empty() {
                missing.sort_unstable();
                core.log.log_file_only(Category::Error, format!("Enhanced archive missing expected files: {}", missing.join(", ")));
                return false;
            }
            true
        }
        Err(ZipError::Bad(_)) => {
            core.log.log_file_only(Category::Error, "Enhanced archive is not a valid zip");
            false
        }
        Err(ZipError::Io(e)) => {
            core.log.log_file_only(Category::Error, format!("Failed to validate Enhanced archive: {}", io_error_text(&e, Some(path))));
            false
        }
    }
}

// ---- install --------------------------------------------------------------------------------

/// `(target, backup, backup_created_this_run)`
type CreatedBackup = (PathBuf, PathBuf, bool);

#[derive(Default)]
struct Rollback {
    backups: Vec<CreatedBackup>,
    created_files: Vec<PathBuf>,
}

/// Legacy `_backup_with_bak_for_install`. Returns whether `target` existed.
fn backup_for_install(core: &Core, target: &Path, rb: &mut Rollback) -> Result<bool, String> {
    if !target.exists() {
        return Ok(false);
    }
    if let Some(existing) = backup::existing(target) {
        core.log.log_file_only(
            Category::Info,
            format!("Existing backup found for {}; preserving original backup.", target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
        );
        rb.backups.push((target.to_path_buf(), existing, false));
        return Ok(true);
    }
    let bak = backup::patchops_path(target);
    fs::rename(target, &bak).map_err(|e| rename_error_text(&e, target, &bak))?;
    rb.backups.push((target.to_path_buf(), bak, true));
    Ok(true)
}

/// Legacy `_install_enhanced` prelude: expand `~`, require an existing, valid dump source and
/// remember it in the settings. Returns the normalised source path.
pub fn prepare_install(core: &Core, dump_source: &str) -> Result<String, String> {
    let normalized = crate::pyfmt::expanduser(dump_source, &core.paths.home).to_string_lossy().into_owned();
    if !Path::new(&normalized).exists() {
        return Err("The selected dump source does not exist.".into());
    }
    if !validate_dump_source(core, Path::new(&normalized)) {
        return Err("The dump source is missing required files.".into());
    }
    core.update_settings(|s| s.set_str("enhanced_dump_source", normalized.clone()))
        .map_err(|e| io_error_text(&e, Some(&core.paths.settings)))?;
    Ok(normalized)
}

/// Legacy `install_enhanced_files`: place the whitelisted dump members (from a directory or zip)
/// and the four Enhanced DLLs (from `<mod_files_dir>/BO3Enhanced_latest.zip`) into the game
/// directory, backing existing files up as `.patchops.bak`, with full rollback on failure.
pub fn install_enhanced_files(core: &Core, game_dir: &Path, mod_files_dir: &Path, dump_source: &Path) -> bool {
    let log = &core.log;
    if game_dir.as_os_str().is_empty() || !game_dir.is_dir() {
        log.error("Invalid game directory for Enhanced install.");
        return false;
    }
    let enhanced_zip = mod_files_dir.join(ENHANCED_ARCHIVE_NAME);
    if !enhanced_zip.exists() {
        log.error(format!("Enhanced archive not found at {}", enhanced_zip.display()));
        return false;
    }
    if !dump_source.exists() {
        log.error(format!("Dump source not found at {}", dump_source.display()));
        return false;
    }
    if !validate_enhanced_archive(core, &enhanced_zip) {
        log.error("Enhanced archive failed validation; aborting install.");
        return false;
    }
    if !validate_dump_source(core, dump_source) {
        log.error("Dump source failed validation; aborting install.");
        return false;
    }

    let mut rb = Rollback::default();
    match install_inner(core, game_dir, &enhanced_zip, dump_source, &mut rb) {
        Ok(installed) => {
            log.success("Installed BO3 Enhanced files and dump into game directory.");
            let mut state = load_state(core);
            state.insert("installed".into(), Value::Bool(true));
            let detected = state.get("detected_at").filter(|v| state_file::truthy(Some(v))).cloned().unwrap_or_else(|| Value::String(utc_timestamp()));
            state.insert("detected_at".into(), detected);
            let files: BTreeSet<String> = installed.into_iter().collect();
            if files.is_empty() {
                state.insert("installed".into(), Value::Bool(false));
            }
            state.insert("installed_files".into(), Value::Array(files.into_iter().map(Value::String).collect()));
            save_state(core, &state);
            true
        }
        Err(msg) => {
            for f in rb.created_files.iter().rev() {
                if f.exists() {
                    if let Err(e) = fs::remove_file(f) {
                        log.warning(format!("Failed to remove partial file during rollback: {}", io_error_text(&e, Some(f))));
                    }
                }
            }
            for (target, bak, created_now) in rb.backups.iter().rev() {
                let r: io::Result<()> = (|| {
                    if target.exists() {
                        fs::remove_file(target)?;
                    }
                    if bak.exists() {
                        if *created_now {
                            fs::rename(bak, target)?;
                        } else {
                            copy2(bak, target)?;
                        }
                    }
                    Ok(())
                })();
                if let Err(e) = r {
                    log.warning(format!("Failed to restore backup during rollback for {}: {}", target.display(), io_error_text(&e, None)));
                }
            }
            log.error(format!("Failed to install BO3 Enhanced: {msg}"));
            false
        }
    }
}

fn io_msg(e: io::Error, p: &Path) -> String {
    io_error_text(&e, Some(p))
}

fn install_inner(core: &Core, game_dir: &Path, enhanced_zip: &Path, dump_source: &Path, rb: &mut Rollback) -> Result<Vec<String>, String> {
    let mut installed: Vec<String> = Vec::new();
    let target_for = |rel: &str| PathBuf::from(normpath(&game_dir.join(rel).to_string_lossy()));

    // 1) dump contents
    if dump_source.is_dir() {
        let mut files = Vec::new();
        walk_files(dump_source, &mut files);
        for src in files {
            let rel = rel_slash(dump_source, &src);
            let final_rel = strip_dump_prefix(&rel).to_string();
            if final_rel.is_empty() || !should_copy_dump_member(&final_rel) {
                continue;
            }
            let target = target_for(&final_rel);
            if !is_within_root(game_dir, &target) {
                continue;
            }
            let had = backup_for_install(core, &target, rb)?;
            if let Some(dir) = target.parent() {
                fs::create_dir_all(dir).map_err(|e| io_msg(e, dir))?;
            }
            // Deviation from legacy: track the new file *before* writing it, so a half-written file is
            // removed by the rollback (the original leaves it behind).
            if !had {
                rb.created_files.push(target.clone());
            }
            copy2(&src, &target).map_err(|e| io_msg(e, &target))?;
            installed.push(final_rel);
        }
    } else {
        let mut z = ZipArchive::open(dump_source).map_err(|e| zip_err_text(&e, dump_source))?;
        for entry in z.entries().to_vec() {
            let final_rel = strip_dump_prefix(&entry.name).to_string();
            if final_rel.is_empty() || final_rel.ends_with('/') || !should_copy_dump_member(&final_rel) {
                continue;
            }
            let target = target_for(&final_rel);
            if !is_within_root(game_dir, &target) {
                return Err(format!("Unsafe path detected in dump archive: {}", entry.name));
            }
            let had = backup_for_install(core, &target, rb)?;
            if let Some(dir) = target.parent() {
                fs::create_dir_all(dir).map_err(|e| io_msg(e, dir))?;
            }
            if !had {
                rb.created_files.push(target.clone());
            }
            let mut out = File::create(&target).map_err(|e| io_msg(e, &target))?;
            z.extract(&entry, &mut out).map_err(|e| zip_err_text(&e, dump_source))?;
            installed.push(final_rel);
        }
    }

    // 2) Enhanced DLLs on top
    let mut z = ZipArchive::open(enhanced_zip).map_err(|e| zip_err_text(&e, enhanced_zip))?;
    for entry in z.entries().to_vec() {
        let name = basename(&entry.name).to_string();
        if !EXPECTED_ENHANCED_FILES.contains(&name.as_str()) {
            continue;
        }
        let dest = game_dir.join(&name);
        let had = backup_for_install(core, &dest, rb)?;
        // `_safe_extract_member`: always the bare file name inside the game dir.
        let target = PathBuf::from(normpath(&game_dir.join(&name).to_string_lossy()));
        if !target.starts_with(normpath(&game_dir.to_string_lossy())) {
            return Err(format!("Unsafe path detected in archive: {}", entry.name));
        }
        if !had {
            rb.created_files.push(dest);
        }
        let mut out = File::create(&target).map_err(|e| io_msg(e, &target))?;
        z.extract(&entry, &mut out).map_err(|e| zip_err_text(&e, enhanced_zip))?;
        installed.push(name);
    }
    Ok(installed)
}

/// Legacy tail of `_install_enhanced`: remember the (now Enhanced) exe hash, mark the variant
/// and log success. Call after [`install_enhanced_files`] (and the Linux tool setup).
pub fn finish_install(core: &Core, game_dir: &Path) {
    if let Some(exe) = crate::game::find_executable(game_dir) {
        if let Some(h) = super::hashcache::file_sha256_fresh(&exe) {
            super::exe::record_enhanced_hash(core, game_dir, &h);
        }
    }
    crate::game::write_exe_variant(game_dir, "enhanced");
    core.log.success("Installed BO3 Enhanced successfully.");
}

// ---- uninstall ------------------------------------------------------------------------------

fn restore_backup_over(target: &Path, bak: &Path) -> io::Result<()> {
    if target.exists() {
        fs::remove_file(target)?;
    }
    fs::rename(bak, target)
}

/// Legacy `uninstall_dump_only`.
pub fn uninstall_dump_only(core: &Core, game_dir: &Path, mod_files_dir: &Path) -> bool {
    let log = &core.log;
    if game_dir.as_os_str().is_empty() || !game_dir.is_dir() {
        log.error("Invalid game directory for dump uninstall.");
        return false;
    }
    let mut state = load_state(core);
    let mut tracked: Vec<String> = match state.get("dump_only_files") {
        Some(Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
        _ => Vec::new(),
    };
    let dump_zip = mod_files_dir.join(DUMP_ARCHIVE_NAME);
    let (mut restored, mut removed) = (0u32, 0u32);
    let outer: Result<bool, String> = (|| {
        if tracked.is_empty() {
            if !dump_zip.exists() {
                return Ok(true);
            }
            let z = ZipArchive::open(&dump_zip).map_err(|e| zip_err_text(&e, &dump_zip))?;
            for e in z.entries() {
                if !e.name.starts_with("DUMP/") {
                    continue;
                }
                let rel = e.name.split_once('/').map(|x| x.1).unwrap_or(&e.name);
                if rel.is_empty() || rel.ends_with('/') || !should_copy_dump_member(rel) {
                    continue;
                }
                tracked.push(rel.to_string());
            }
        }
        for rel in &tracked {
            let target = PathBuf::from(normpath(&game_dir.join(rel).to_string_lossy()));
            if !is_within_root(game_dir, &target) {
                continue;
            }
            if let Some(bak) = backup::existing(&target) {
                match restore_backup_over(&target, &bak) {
                    Ok(()) => {
                        restored += 1;
                        continue;
                    }
                    Err(e) => log.warning(format!("Failed to restore {rel}: {}", io_error_text(&e, None))),
                }
            }
            if target.exists() {
                let is_protected = target.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase() == "blackops3.exe");
                if is_protected {
                    log.log_file_only(Category::Warning, format!("Backup missing for {rel}; skipping deletion to preserve game executable."));
                } else {
                    match fs::remove_file(&target) {
                        Ok(()) => removed += 1,
                        Err(e) => log.warning(format!("Failed to remove {rel}: {}", io_error_text(&e, Some(&target)))),
                    }
                }
            }
        }
        if restored > 0 || removed > 0 {
            log.success(format!("Dump-only uninstall completed. Restored {restored} files; removed {removed} files."));
            state.insert("dump_only_files".into(), Value::Array(vec![]));
            save_state(core, &state);
            return Ok(true);
        }
        log.warning("No dump files were restored or removed.");
        Ok(false)
    })();
    match outer {
        Ok(v) => v,
        Err(m) => {
            log.error(format!("Failed to uninstall dump: {m}"));
            false
        }
    }
}

/// Legacy `uninstall_enhanced_files`: dump files first, then every tracked (or, failing that, the
/// four known) Enhanced file is restored from its backup or removed. `false` = incomplete.
pub fn uninstall_enhanced_files(core: &Core, game_dir: &Path, mod_files_dir: &Path) -> bool {
    let log = &core.log;
    if game_dir.as_os_str().is_empty() || !game_dir.is_dir() {
        log.error("Invalid game directory for Enhanced uninstall.");
        return false;
    }
    if uninstall_dump_only(core, game_dir, mod_files_dir) {
        log.success("Dump files uninstalled.");
    }

    // Reload: `uninstall_dump_only` may have changed persisted keys.
    let mut state = load_state(core);
    let tracked: Vec<String> = installed_files(&state).into_iter().filter(|r| !should_copy_dump_member(r)).collect();
    let (mut restored, mut removed) = (0u32, 0u32);
    let mut attempted = !tracked.is_empty();
    let mut unresolved: Vec<String> = Vec::new();

    for rel in &tracked {
        let mut resolved = false;
        let target = game_dir.join(rel);
        let backup_path = backup::existing(&target);
        if let Some(bak) = &backup_path {
            let r = (|| {
                if let Some(dir) = target.parent() {
                    fs::create_dir_all(dir)?;
                }
                restore_backup_over(&target, bak)
            })();
            match r {
                Ok(()) => {
                    restored += 1;
                    continue;
                }
                Err(e) => log.warning(format!("Failed to restore backup for {rel}: {}", io_error_text(&e, None))),
            }
        }
        if backup_path.is_none() && !target.exists() {
            resolved = true;
        } else if target.exists() {
            let protected = basename(rel).to_lowercase() == "blackops3.exe";
            if !protected {
                match fs::remove_file(&target) {
                    Ok(()) => {
                        removed += 1;
                        resolved = true;
                    }
                    Err(e) => log.warning(format!("Failed to remove {rel}: {}", io_error_text(&e, Some(&target)))),
                }
            } else {
                log.log_file_only(Category::Warning, format!("Backup missing for {rel}; skipping deletion to preserve game file."));
            }
        }
        if !resolved {
            unresolved.push(rel.clone());
        }
    }

    // Nothing tracked: best-effort cleanup of the four known files.
    if !attempted && restored == 0 && removed == 0 {
        attempted = true;
        for name in EXPECTED_ENHANCED_FILES {
            let target = game_dir.join(name);
            if let Some(bak) = backup::existing(&target) {
                match restore_backup_over(&target, &bak) {
                    Ok(()) => {
                        restored += 1;
                        continue;
                    }
                    Err(e) => log.warning(format!("Failed to restore backup for {name}: {}", io_error_text(&e, None))),
                }
            }
            if target.exists() {
                match fs::remove_file(&target) {
                    Ok(()) => removed += 1,
                    Err(e) => log.warning(format!("Failed to remove {name}: {}", io_error_text(&e, Some(&target)))),
                }
            }
        }
    }

    if unresolved.is_empty() {
        state.insert("installed".into(), Value::Bool(false));
        state.insert("installed_files".into(), Value::Array(vec![]));
    } else {
        let set: BTreeSet<String> = unresolved.iter().cloned().collect();
        state.insert("installed".into(), Value::Bool(true));
        state.insert("installed_files".into(), Value::Array(set.into_iter().map(Value::String).collect()));
    }
    save_state(core, &state);

    if !unresolved.is_empty() {
        log.warning("Enhanced uninstall was incomplete; keeping tracked files so cleanup can be retried.");
        return false;
    }
    if restored > 0 || removed > 0 {
        log.success(format!("Uninstall completed. Restored {restored} files from backup; removed {removed} files."));
        return true;
    }
    log.warning("No BO3 Enhanced files were removed or restored (none tracked).");
    attempted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enhanced::testkit::Sandbox;
    use crate::enhanced::zip::testutil::stored_zip;

    const DEFLATED_DUMP_HEX: &str = "504b03041400000008006b416a5ddd95d72dcf000000900100001200000044554d502f426c61636b4f7073332e6578651590cb0d00410842ef54416b0e2af65fc1b29998cc4f79401e6f9fea09d9b0f5fc3665cd9b2ebf51b7a15e9d569dcbc515ab76d0b7c5391a39df33acccc02b2ed3ef4119c327fb309a01f05e4b14f986e3165cca50bd1a72ea456caaa2b779c9887374f77458a33557762f07da7be5769ac2f3109539203e748fab9f51ec665f6d2ce92e15b87cea6a86ffd251ab092e5691d91fdfafe23a180e0876cdcabe6ecad39c462ccd032e098d2e46d0e04f12874dd6549282f84ad5ebe15ddf24c067d33f5e270061b25ef4da1bf52e7a3e504b03041400000008006b416a5dcea0656008000000960000001900000044554d502f4d6963726f736f667447616d652e636f6e6669674b4e4b4f1e7c0800504b010214031400000008006b416a5ddd95d72dcf0000009001000012000000000000000000000080010000000044554d502f426c61636b4f7073332e657865504b010214031400000008006b416a5dcea0656008000000960000001900000000000000000000008001ff00000044554d502f4d6963726f736f667447616d652e636f6e666967504b05060000000002000200870000003e0100000000";

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    fn enhanced_zip(sb: &Sandbox) -> PathBuf {
        let members: Vec<(String, Vec<u8>)> = EXPECTED_ENHANCED_FILES.iter().map(|n| (format!("BO3Enhanced/{n}"), format!("new-{n}").into_bytes())).collect();
        let refs: Vec<(&str, &[u8])> = members.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
        fs::create_dir_all(&sb.core.paths.mod_files).unwrap();
        let p = sb.core.paths.mod_files.join(ENHANCED_ARCHIVE_NAME);
        fs::write(&p, stored_zip(&refs)).unwrap();
        p
    }

    fn dump_dir(sb: &Sandbox, prefix: &str) -> PathBuf {
        let d = sb.root.join("dump");
        let base = if prefix.is_empty() { d.clone() } else { d.join(prefix) };
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join("BlackOps3.exe"), b"gdk-exe").unwrap();
        fs::write(base.join("MicrosoftGame.config"), b"cfg").unwrap();
        fs::write(base.join("Party.dll"), b"party").unwrap();
        fs::write(base.join("random.dll"), b"nope").unwrap();
        fs::write(base.join("WindowsCodecs.dll"), b"must-not-copy").unwrap();
        d
    }

    #[test]
    fn validates_directories_and_zips() {
        let sb = Sandbox::new();
        assert!(validate_dump_source(&sb.core, &dump_dir(&sb, "")));
        let d2 = sb.root.join("d2");
        fs::create_dir_all(d2.join("DUMP")).unwrap();
        fs::write(d2.join("DUMP/BlackOps3.exe"), b"x").unwrap();
        assert!(!validate_dump_source(&sb.core, &d2));
        assert!(sb.log().contains("Error: Dump source missing required files: MicrosoftGame.config"));
        fs::write(d2.join("DUMP/MicrosoftGame.config"), b"x").unwrap();
        assert!(validate_dump_source(&sb.core, &d2));

        let z = sb.root.join("d.zip");
        fs::write(&z, stored_zip(&[("DUMP/BlackOps3.exe", b"e"), ("MicrosoftGame.config", b"c")])).unwrap();
        assert!(validate_dump_source(&sb.core, &z));
        fs::write(&z, stored_zip(&[("sub/BlackOps3.exe", b"e")])).unwrap();
        assert!(!validate_dump_source(&sb.core, &z));
        fs::write(&z, b"plain file").unwrap();
        assert!(!validate_dump_source(&sb.core, &z));
        assert!(sb.log().contains("Error: Dump source is not a valid zip file or directory"));
        assert!(!validate_dump_source(&sb.core, &sb.root.join("nope")));
        assert!(sb.log().contains("Dump source not found: "));
    }

    #[test]
    fn installs_from_directory_with_backups_then_uninstalls() {
        let sb = Sandbox::new();
        enhanced_zip(&sb);
        let dump = dump_dir(&sb, "DUMP");
        fs::write(sb.game.join("BlackOps3.exe"), b"steam-exe").unwrap();
        fs::write(sb.game.join("WindowsCodecs.dll"), b"old-codecs").unwrap();

        assert!(install_enhanced_files(&sb.core, &sb.game, &sb.core.paths.mod_files, &dump));
        assert_eq!(fs::read(sb.game.join("BlackOps3.exe")).unwrap(), b"gdk-exe");
        assert_eq!(fs::read(sb.game.join("BlackOps3.exe.patchops.bak")).unwrap(), b"steam-exe");
        assert_eq!(fs::read(sb.game.join("Party.dll")).unwrap(), b"party");
        assert!(!sb.game.join("random.dll").exists());
        assert_eq!(fs::read(sb.game.join("WindowsCodecs.dll")).unwrap(), b"new-WindowsCodecs.dll");
        assert_eq!(fs::read(sb.game.join("WindowsCodecs.dll.patchops.bak")).unwrap(), b"old-codecs");
        assert!(crate::ops::enhanced::detect_enhanced_install(&sb.game));
        let st = load_state(&sb.core);
        assert_eq!(st["installed"], Value::Bool(true));
        assert!(st["detected_at"].as_str().unwrap().ends_with('Z'));
        assert_eq!(installed_files(&st), ["BlackOps3.exe", "MicrosoftGame.config", "Party.dll", "T7InternalWS.dll", "T7WSBootstrapper.dll", "WindowsCodecs.dll", "steam_api65.dll"]);
        assert!(state_file::status_summary(&sb.core, &sb.game).installed);

        // Hash is recorded and the variant marker written.
        finish_install(&sb.core, &sb.game);
        assert_eq!(crate::game::read_exe_variant(&sb.game).as_deref(), Some("enhanced"));
        let known = crate::ops::enhanced::known_enhanced_hashes(&sb.core, Some(&sb.game.to_string_lossy()));
        assert_eq!(known, vec![crate::hash::sha256_hex(b"gdk-exe")]);

        // Uninstall (legacy behaviour): only the four Enhanced DLLs are restored/removed; dump
        // members are the business of `uninstall_dump_only` (nothing tracked there), so the GDK
        // exe stays and its `.patchops.bak` is left for the reset-to-stock exe restore.
        assert!(uninstall_enhanced_files(&sb.core, &sb.game, &sb.core.paths.mod_files));
        assert_eq!(fs::read(sb.game.join("WindowsCodecs.dll")).unwrap(), b"old-codecs");
        assert!(!sb.game.join("T7InternalWS.dll").exists());
        assert_eq!(fs::read(sb.game.join("BlackOps3.exe")).unwrap(), b"gdk-exe");
        assert_eq!(fs::read(sb.game.join("BlackOps3.exe.patchops.bak")).unwrap(), b"steam-exe");
        assert!(!crate::ops::enhanced::detect_enhanced_install(&sb.game));
        let st = load_state(&sb.core);
        assert_eq!((st["installed"].as_bool(), installed_files(&st).len()), (Some(false), 0));
        assert!(sb.log().contains("Success: Uninstall completed. Restored"));
    }

    #[test]
    fn installs_from_deflated_zip() {
        let sb = Sandbox::new();
        enhanced_zip(&sb);
        let zip = sb.root.join("dump.zip");
        fs::write(&zip, hex(DEFLATED_DUMP_HEX)).unwrap();
        assert!(validate_dump_source(&sb.core, &zip));
        assert!(install_enhanced_files(&sb.core, &sb.game, &sb.core.paths.mod_files, &zip));
        let exe = fs::read(sb.game.join("BlackOps3.exe")).unwrap();
        assert_eq!(exe.len(), 400);
        assert_eq!(fs::read(sb.game.join("MicrosoftGame.config")).unwrap(), "cfg".repeat(50).into_bytes());
    }

    #[test]
    fn failed_install_rolls_back() {
        let sb = Sandbox::new();
        enhanced_zip(&sb);
        // Corrupt one DLL's payload (CRC mismatch) so the DLL step fails after the dump was placed.
        let zp = sb.core.paths.mod_files.join(ENHANCED_ARCHIVE_NAME);
        let mut bytes = fs::read(&zp).unwrap();
        let at = bytes.windows(19).position(|w| w == b"new-T7InternalWS.dl").unwrap();
        bytes[at] = b'X';
        fs::write(&zp, bytes).unwrap();
        fs::write(sb.game.join("BlackOps3.exe"), b"steam-exe").unwrap();
        fs::write(sb.game.join("T7WSBootstrapper.dll"), b"mine").unwrap();
        let dump = dump_dir(&sb, "");
        assert!(!install_enhanced_files(&sb.core, &sb.game, &sb.core.paths.mod_files, &dump));
        assert_eq!(fs::read(sb.game.join("BlackOps3.exe")).unwrap(), b"steam-exe");
        assert!(!sb.game.join("BlackOps3.exe.patchops.bak").exists());
        assert!(!sb.game.join("Party.dll").exists());
        assert!(!sb.game.join("T7InternalWS.dll").exists(), "half-written file removed");
        assert_eq!(fs::read(sb.game.join("T7WSBootstrapper.dll")).unwrap(), b"mine");
        assert!(!sb.game.join("T7WSBootstrapper.dll.patchops.bak").exists());
        assert!(sb.log().contains("Error: Failed to install BO3 Enhanced: Bad CRC-32"));
    }

    #[test]
    fn prepare_install_checks_and_remembers_source() {
        let sb = Sandbox::new();
        assert_eq!(prepare_install(&sb.core, "/definitely/not/here").unwrap_err(), "The selected dump source does not exist.");
        let bad = sb.root.join("bad");
        fs::create_dir_all(&bad).unwrap();
        assert_eq!(prepare_install(&sb.core, &bad.to_string_lossy()).unwrap_err(), "The dump source is missing required files.");
        let ok = dump_dir(&sb, "");
        let norm = prepare_install(&sb.core, &ok.to_string_lossy()).unwrap();
        assert_eq!(sb.core.settings().enhanced_dump_source(), norm);
    }

    #[test]
    fn stale_installed_flag_is_cleared_by_status() {
        let sb = Sandbox::new();
        let mut st = state_file::StateMap::new();
        st.insert("installed".into(), Value::Bool(true));
        st.insert("detected_at".into(), Value::String("2026-01-01T00:00:00Z".into()));
        save_state(&sb.core, &st);
        let s = state_file::status_summary(&sb.core, &sb.game);
        assert!(!s.installed);
        assert_eq!(s.detected_at.as_deref(), Some("2026-01-01T00:00:00Z"));
        let after = load_state(&sb.core);
        assert_eq!(after["installed"], Value::Bool(false));
        assert_eq!(after["detected_at"], Value::String("2026-01-01T00:00:00Z".into()));
    }
}
