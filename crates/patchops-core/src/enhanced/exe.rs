//! Executable integrity, preservation and swapping (legacy `backend/api.py`:
//! `_executable_integrity_status`, `_validated_*_backup_path`, `_preserve_active_exe`,
//! `_install_compatible_exe`, `_restore_current_exe`, `_restore_enhanced_exe`, depot helpers).
//!
//! # Test seam
//! The known-hash table is data: [`KnownBuilds`]. Production code uses [`KnownBuilds::production`];
//! unit tests pass their own table (hashes of fake executables) to `*_with` entry points, so the
//! success paths - which the benchmark fixture cannot reach - run against a temp directory.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

use super::files::rename_error_text;
use crate::fsutil::copy2;
use super::hashcache;
use crate::backup;
use crate::context::Core;
use crate::game;
use crate::ops::enhanced::{
    detect_enhanced_install, Integrity, COMPATIBLE_BUILD_SHA256, COMPATIBLE_DEPOT_APP_ID, COMPATIBLE_DEPOT_ID, COMPATIBLE_DEPOT_MANIFEST_ID,
    COMPATIBLE_STEAM_BUILD_ID, CURRENT_STEAM_BUILD_ID, DEFAULT_STEAM_EXE_SHA256,
};
use crate::pyfmt::{abspath, io_error_text, normpath, path_str, resolve};

pub const CURRENT_EXE_ID: &str = "current";
pub const COMPATIBLE_EXE_ID: &str = "compatible";
pub const ENHANCED_EXE_ID: &str = "enhanced";

pub const DEPOT_REQUIRED_MESSAGE: &str =
    "Compatible Steam depot was not found. Copy the depot command, open Steam Console, run it, then continue once the download finishes.";

/// SHA-256 table of the Steam builds PatchOpsIII recognises.
#[derive(Debug, Clone)]
pub struct KnownBuilds {
    /// Latest ("current") Steam build.
    pub current_sha256: String,
    /// Compatible (older) builds.
    pub compatible_sha256: Vec<String>,
}

impl KnownBuilds {
    pub fn production() -> KnownBuilds {
        KnownBuilds {
            current_sha256: DEFAULT_STEAM_EXE_SHA256.to_lowercase(),
            compatible_sha256: COMPATIBLE_BUILD_SHA256.iter().map(|s| s.to_lowercase()).collect(),
        }
    }
}

/// Failure of a swap operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwapError {
    /// `CompatibleDepotRequired`: the user must download the depot first.
    DepotRequired,
    /// Any other exception; the text is Python's `str(exc)`.
    Msg(String),
}

impl SwapError {
    pub fn message(&self) -> &str {
        match self {
            SwapError::DepotRequired => DEPOT_REQUIRED_MESSAGE,
            SwapError::Msg(m) => m,
        }
    }
}

impl From<String> for SwapError {
    fn from(m: String) -> Self {
        SwapError::Msg(m)
    }
}

impl std::fmt::Display for SwapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

/// Legacy `_executable_integrity_status`.
pub fn integrity(b: &KnownBuilds, exe_hash: &str, exists: bool, enhanced_active: bool, enhanced_hash_known: bool) -> Integrity {
    let h = exe_hash.to_lowercase();
    if !exists {
        return Integrity { trusted: false, status: "missing", profile: "", message: "BlackOps3.exe or BlackOpsIII.exe was not found.".into() };
    }
    if h == b.current_sha256 {
        return Integrity { trusted: true, status: "trusted", profile: CURRENT_EXE_ID, message: format!("Executable matches latest Steam BuildID {CURRENT_STEAM_BUILD_ID}.") };
    }
    if b.compatible_sha256.contains(&h) {
        return Integrity { trusted: true, status: "trusted", profile: COMPATIBLE_EXE_ID, message: format!("Executable matches compatible BuildID {COMPATIBLE_STEAM_BUILD_ID}.") };
    }
    if enhanced_active || enhanced_hash_known {
        return Integrity { trusted: true, status: "enhanced", profile: ENHANCED_EXE_ID, message: "Executable is managed by BO3 Enhanced.".into() };
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

/// Legacy `_record_enhanced_hash`: remember (at most 5 per game dir) hashes of Enhanced executables.
pub fn record_enhanced_hash(core: &Core, game_dir: &Path, exe_hash: &str) {
    let normalized = exe_hash.to_lowercase();
    if normalized.is_empty() {
        return;
    }
    let key = path_str(game_dir);
    let _ = core.update_settings(|s| {
        let mut hashes = match s.0.get("enhanced_exe_hashes") {
            Some(Value::Object(m)) => m.clone(),
            _ => serde_json::Map::new(),
        };
        let mut values: Vec<Value> = match hashes.get(&key) {
            Some(Value::String(v)) => vec![Value::String(v.clone())],
            Some(Value::Array(a)) => a.clone(),
            _ => Vec::new(),
        };
        let existing: Vec<String> = values.iter().map(|v| v.as_str().map(str::to_lowercase).unwrap_or_else(|| v.to_string().to_lowercase())).collect();
        if !existing.contains(&normalized) {
            values.push(Value::String(normalized.clone()));
        }
        let start = values.len().saturating_sub(5);
        hashes.insert(key.clone(), Value::Array(values.split_off(start)));
        s.0.insert("enhanced_exe_hashes".into(), Value::Object(hashes));
    });
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn stem_of(p: &Path) -> String {
    p.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// `<stem>.<build_id>.bak` next to the exe.
pub fn inactive_build_path(target_exe: &Path, build_id: &str) -> PathBuf {
    target_exe.with_file_name(format!("{}.{build_id}.bak", stem_of(target_exe)))
}

/// `<stem>.enhanced.bak` next to the exe.
pub fn enhanced_backup_path(target_exe: &Path) -> PathBuf {
    target_exe.with_file_name(format!("{}.{ENHANCED_EXE_ID}.bak", stem_of(target_exe)))
}

fn mtime(p: &Path) -> SystemTime {
    fs::metadata(p).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH)
}

/// Files in the exe's directory named `<prefix>*<suffix>`, newest first (`glob` + mtime sort).
fn glob_newest(dir: &Path, prefix: &str, suffix: &str) -> Vec<PathBuf> {
    let mut v: Vec<(PathBuf, SystemTime)> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.len() >= prefix.len() + suffix.len() && n.starts_with(prefix) && n.ends_with(suffix)
        })
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .map(|p| {
            let t = mtime(&p);
            (p, t)
        })
        .collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.1));
    v.into_iter().map(|x| x.0).collect()
}

/// Legacy `_build_backup_candidates`.
pub fn build_backup_candidates(target_exe: &Path, build_id: &str, backup_role: &str) -> Vec<PathBuf> {
    let stem = stem_of(target_exe);
    let suffix = target_exe.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let mut out = Vec::new();
    let exact = inactive_build_path(target_exe, build_id);
    if exact.exists() {
        out.push(exact);
    }
    let old_exact = target_exe.with_file_name(format!("{stem}.{backup_role}-{build_id}{suffix}"));
    if old_exact.exists() {
        out.push(old_exact);
    }
    let dir = target_exe.parent().unwrap_or(Path::new("."));
    out.extend(glob_newest(dir, &format!("{stem}.{backup_role}-{build_id}-"), &suffix));
    out
}

fn candidate_dirs(core: &Core) -> Vec<PathBuf> {
    // Legacy: library paths, then the Steam root (+ Program Files locations on Windows).
    let mut roots: Vec<PathBuf> = crate::steam::library_paths(core).into_iter().map(PathBuf::from).collect();
    if let Some(root) = crate::steam::root_path(core) {
        roots.push(PathBuf::from(root));
    }
    if cfg!(windows) {
        let pf86 = std::env::var_os("PROGRAMFILES(X86)").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"));
        let pf = std::env::var_os("PROGRAMFILES").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
        roots.push(pf86.join("Steam"));
        roots.push(pf.join("Steam"));
    }
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for root in roots {
        let depot = root.join("steamapps").join("content").join(format!("app_{COMPATIBLE_DEPOT_APP_ID}")).join(format!("depot_{COMPATIBLE_DEPOT_ID}"));
        let key = if depot.exists() { resolve(&depot).to_string_lossy().into_owned() } else { abspath(&depot.to_string_lossy()) };
        let mut norm = normpath(&key);
        if cfg!(windows) {
            norm = norm.to_lowercase();
        }
        if seen.contains(&norm) {
            continue;
        }
        seen.push(norm);
        out.push(depot);
    }
    out
}

/// Legacy `_compatible_depot_download_exists` (no hashing: exe present and non-empty + install script).
pub fn compatible_depot_download_exists(core: &Core) -> bool {
    candidate_dirs(core).iter().any(|d| {
        let exe = d.join("BlackOps3.exe");
        exe.exists() && d.join("installscript_311210.vdf").exists() && fs::metadata(&exe).map(|m| m.len() > 0).unwrap_or(false)
    })
}

/// Executable swapper bound to a [`Core`] and a [`KnownBuilds`] table.
pub struct Swap<'a> {
    pub core: &'a Core,
    pub builds: &'a KnownBuilds,
    /// Use the signature-validated hash cache (state building) or always re-hash (operations).
    cached: bool,
    /// Per-op memo ([`hashcache::HashScope`]) of digests: an op asks for the hash of the same unchanged file up to
    /// three times (integrity check, `preserve_active_exe`, validation); the file is read once.
    /// Cleared at the start of every op and whenever the op itself renames/copies a file.
    memo: RefCell<hashcache::HashScope>,
    /// How a replacement exe is put in place (test seam; production: [`crate::fsutil::atomic_copy`]).
    copy_exe: fn(&Path, &Path) -> std::io::Result<()>,
}

impl<'a> Swap<'a> {
    /// For read-only state building: hashes go through the cache.
    pub fn for_state(core: &'a Core, builds: &'a KnownBuilds) -> Self {
        Swap { core, builds, cached: true, memo: RefCell::default(), copy_exe: crate::fsutil::atomic_copy }
    }

    /// For operations that rename/replace files: always re-hash.
    pub fn for_op(core: &'a Core, builds: &'a KnownBuilds) -> Self {
        Swap { core, builds, cached: false, memo: RefCell::default(), copy_exe: crate::fsutil::atomic_copy }
    }

    fn sha(&self, p: &Path) -> Option<String> {
        if self.cached {
            return hashcache::file_sha256_cached(p);
        }
        self.memo.borrow().sha256(p)
    }

    /// Start a new memo scope (the op changed files, or a new op begins).
    fn forget_hashes(&self) {
        *self.memo.borrow_mut() = hashcache::HashScope::new();
    }

    /// Rename that also drops every remembered digest.
    fn mv(&self, src: &Path, dst: &Path) -> Result<(), String> {
        self.forget_hashes();
        move_file(src, dst)
    }

    /// Run `install` (which puts a new exe at `target`) after moving the active exe aside to its
    /// per-build backup name. If `install` fails, the preserved exe is moved back, so a failed
    /// swap never leaves the game without its executable (and never with a truncated one: the
    /// copy-based installs go through [`crate::fsutil::atomic_copy`]).
    fn replace_active(&self, game_dir: &Path, target: &Path, install: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        let preserved = if target.exists() { self.preserve_active_exe(game_dir, target, None)? } else { None };
        let res = install();
        self.forget_hashes();
        let Err(e) = res else { return Ok(()) };
        if let Some(p) = preserved {
            // Whatever is at `target` now was put there by the failed install (the original is at `p`).
            let _ = fs::remove_file(target);
            match move_file(&p, target) {
                Ok(()) => self.warn(format!("Executable swap failed; restored the previous executable from {}.", name_of(&p))),
                Err(e2) => return Err(format!("{e} (the previous executable is preserved as {}, could not move it back: {e2})", name_of(&p))),
            }
        }
        Err(e)
    }

    fn known(&self, game_dir: &Path) -> Vec<String> {
        crate::ops::enhanced::known_enhanced_hashes(self.core, Some(&game_dir.to_string_lossy()))
    }

    fn warn(&self, m: String) {
        self.core.log.warning(m);
    }

    /// Legacy `_validated_preserved_compatible_exe`.
    pub fn validated_preserved_compatible_exe(&self, target_exe: &Path, log_untrusted: bool) -> Option<PathBuf> {
        for c in build_backup_candidates(target_exe, COMPATIBLE_STEAM_BUILD_ID, "compatible") {
            if let Some(h) = self.sha(&c).filter(|h| !h.is_empty()) {
                if self.builds.compatible_sha256.contains(&h.to_lowercase()) {
                    return Some(c);
                }
            }
            if log_untrusted {
                self.warn(format!("Ignoring preserved compatible executable with untrusted SHA-256: {}", name_of(&c)));
            }
        }
        None
    }

    /// Legacy `_validated_enhanced_backup_path`.
    pub fn validated_enhanced_backup_path(&self, game_dir: Option<&Path>, target_exe: &Path, log_untrusted: bool) -> Option<PathBuf> {
        let known = match game_dir {
            Some(g) => self.known(g),
            None => crate::ops::enhanced::known_enhanced_hashes(self.core, None),
        };
        let mut candidates = vec![enhanced_backup_path(target_exe)];
        let dir = target_exe.parent().unwrap_or(Path::new("."));
        candidates.extend(glob_newest(dir, &format!("{}.{ENHANCED_EXE_ID}-", stem_of(target_exe)), ".bak"));
        for c in candidates {
            if !c.exists() {
                continue;
            }
            if let Some(h) = self.sha(&c).filter(|h| !h.is_empty()) {
                if known.contains(&h.to_lowercase()) {
                    return Some(c);
                }
            }
            if log_untrusted {
                self.warn(format!("Ignoring preserved Enhanced executable with untrusted SHA-256: {}", name_of(&c)));
            }
        }
        None
    }

    /// Legacy `_validated_latest_build_backup_path`.
    pub fn validated_latest_build_backup_path(&self, target_exe: &Path, log_untrusted: bool) -> Option<PathBuf> {
        for c in build_backup_candidates(target_exe, CURRENT_STEAM_BUILD_ID, "current") {
            if self.sha(&c).is_some_and(|h| h.to_lowercase() == self.builds.current_sha256) {
                return Some(c);
            }
            if log_untrusted {
                self.warn(format!("Ignoring preserved latest executable with untrusted SHA-256: {}", name_of(&c)));
            }
        }
        if let Some(bak) = backup::existing(target_exe) {
            if self.sha(&bak).is_some_and(|h| h.to_lowercase() == self.builds.current_sha256) {
                return Some(bak);
            }
            if log_untrusted {
                self.warn(format!("Ignoring executable backup with untrusted SHA-256: {}", name_of(&bak)));
            }
        }
        None
    }

    /// Legacy `_active_exe_integrity`.
    pub fn active_exe_integrity(&self, game_dir: &Path, target_exe: &Path) -> Integrity {
        let exists = target_exe.exists();
        let hash = if exists { self.sha(target_exe).unwrap_or_default().to_lowercase() } else { String::new() };
        let known = !hash.is_empty() && self.known(game_dir).contains(&hash);
        integrity(self.builds, &hash, exists, detect_enhanced_install(game_dir), known)
    }

    fn preserved_path_for_profile(target_exe: &Path, profile: &str) -> Result<PathBuf, String> {
        match profile {
            CURRENT_EXE_ID => Ok(inactive_build_path(target_exe, CURRENT_STEAM_BUILD_ID)),
            COMPATIBLE_EXE_ID => Ok(inactive_build_path(target_exe, COMPATIBLE_STEAM_BUILD_ID)),
            ENHANCED_EXE_ID => Ok(enhanced_backup_path(target_exe)),
            _ => Err("Active executable is not a recognized PatchOpsIII build.".into()),
        }
    }

    /// Legacy `_preserve_active_exe`: move the active exe to its per-build backup name.
    /// Returns the path it was moved to (`None`: there was nothing to preserve).
    pub fn preserve_active_exe(&self, game_dir: &Path, target_exe: &Path, expected_profile: Option<&str>) -> Result<Option<PathBuf>, String> {
        if !target_exe.exists() {
            return Ok(None);
        }
        let integ = self.active_exe_integrity(game_dir, target_exe);
        if !integ.trusted || integ.profile.is_empty() {
            return Err(integ.message);
        }
        if expected_profile.is_some_and(|e| e != integ.profile) {
            return Err("Active executable does not match the expected build.".into());
        }
        let target_hash = self.sha(target_exe).unwrap_or_default().to_lowercase();
        let preserved = available_sibling_path(&Self::preserved_path_for_profile(target_exe, integ.profile)?)?;
        if integ.profile == ENHANCED_EXE_ID {
            record_enhanced_hash(self.core, game_dir, &target_hash);
        }
        self.mv(target_exe, &preserved)?;
        self.core.log.success(format!("Preserved active executable as {}.", name_of(&preserved)));
        Ok(Some(preserved))
    }

    /// Legacy `_find_valid_compatible_depot`.
    pub fn find_valid_compatible_depot(&self) -> Option<PathBuf> {
        let mut best: Option<(PathBuf, SystemTime)> = None;
        for d in candidate_dirs(self.core) {
            let exe = d.join("BlackOps3.exe");
            if !exe.exists() {
                continue;
            }
            let h = self.sha(&exe).unwrap_or_default().to_lowercase();
            if self.builds.compatible_sha256.contains(&h) {
                let t = mtime(&exe);
                if best.as_ref().is_none_or(|(_, bt)| t > *bt) {
                    best = Some((d, t));
                }
            }
        }
        best.map(|b| b.0)
    }

    fn copy_depot_installscript(&self, depot_dir: &Path, game_dir: &Path) -> Result<(), String> {
        let source = depot_dir.join("installscript_311210.vdf");
        if !source.exists() {
            return Ok(());
        }
        let target = game_dir.join("installscript_311210.vdf");
        let bak = backup::patchops_path(&target);
        if target.exists() && !bak.exists() {
            copy2(&target, &bak).map_err(|e| io_error_text(&e, Some(&bak)))?;
        }
        copy2(&source, &target).map_err(|e| io_error_text(&e, Some(&target)))
    }

    fn restore_installscript_backup(&self, game_dir: &Path) -> Result<(), String> {
        let target = game_dir.join("installscript_311210.vdf");
        let bak = backup::patchops_path(&target);
        if bak.exists() {
            copy2(&bak, &target).map_err(|e| io_error_text(&e, Some(&target)))?;
        }
        Ok(())
    }

    fn target_exe(game_dir: &Path) -> PathBuf {
        game::find_executable(game_dir).unwrap_or_else(|| game_dir.join("BlackOps3.exe"))
    }

    /// Legacy `_install_compatible_exe`.
    pub fn install_compatible_exe(&self, game_dir: &Path) -> Result<(), SwapError> {
        self.forget_hashes();
        let log = &self.core.log;
        if !game::has_game_executable(game_dir) {
            return Err(SwapError::Msg("BlackOps3.exe or BlackOpsIII.exe was not found.".into()));
        }
        let target = Self::target_exe(game_dir);
        let active = self.active_exe_integrity(game_dir, &target);
        if active.profile == COMPATIBLE_EXE_ID {
            log.info("Compatible executable is already active.");
            return Ok(());
        }
        if target.exists() && !active.trusted {
            return Err(SwapError::Msg(active.message));
        }
        if let Some(preserved) = self.validated_preserved_compatible_exe(&target, true) {
            self.replace_active(game_dir, &target, || self.mv(&preserved, &target))?;
            game::write_exe_variant(game_dir, COMPATIBLE_EXE_ID);
            log.success(format!("Reused preserved compatible executable from {}.", name_of(&preserved)));
            return Ok(());
        }
        let Some(depot) = self.find_valid_compatible_depot() else { return Err(SwapError::DepotRequired) };
        let src = depot.join("BlackOps3.exe");
        self.replace_active(game_dir, &target, || (self.copy_exe)(&src, &target).map_err(|e| io_error_text(&e, Some(&target))))?;
        hashcache::invalidate(&target);
        self.copy_depot_installscript(&depot, game_dir)?;
        game::write_exe_variant(game_dir, COMPATIBLE_EXE_ID);
        log.success(format!("Compatible executable installed from Steam depot {COMPATIBLE_DEPOT_MANIFEST_ID}."));
        Ok(())
    }

    /// Legacy `_restore_current_exe`.
    pub fn restore_current_exe(&self, game_dir: &Path) -> Result<(), SwapError> {
        self.forget_hashes();
        let log = &self.core.log;
        if game_dir.as_os_str().is_empty() || !game_dir.is_dir() {
            return Err(SwapError::Msg("Game directory is not set.".into()));
        }
        let target = Self::target_exe(game_dir);
        let active = self.active_exe_integrity(game_dir, &target);
        if active.profile == CURRENT_EXE_ID {
            log.info("Latest executable is already active.");
            return Ok(());
        }
        if target.exists() && !active.trusted {
            return Err(SwapError::Msg(active.message));
        }
        let Some(backup_path) = self.validated_latest_build_backup_path(&target, true) else {
            return Err(SwapError::Msg("No backup executable found. Cannot restore the current EXE.".into()));
        };
        self.replace_active(game_dir, &target, || self.mv(&backup_path, &target))?;
        self.restore_installscript_backup(game_dir)?;
        game::write_exe_variant(game_dir, "default");
        log.success("Current executable restored from backup.");
        Ok(())
    }

    /// Legacy `_restore_enhanced_exe`.
    pub fn restore_enhanced_exe(&self, game_dir: &Path) -> Result<(), SwapError> {
        self.forget_hashes();
        let log = &self.core.log;
        if game_dir.as_os_str().is_empty() || !game_dir.is_dir() {
            return Err(SwapError::Msg("Game directory is not set.".into()));
        }
        let target = Self::target_exe(game_dir);
        let active = self.active_exe_integrity(game_dir, &target);
        if active.profile == ENHANCED_EXE_ID {
            log.info("Enhanced executable is already active.");
            return Ok(());
        }
        if target.exists() && !active.trusted {
            return Err(SwapError::Msg(active.message));
        }
        let Some(backup_path) = self.validated_enhanced_backup_path(Some(game_dir), &target, true) else {
            return Err(SwapError::Msg("No PatchOpsIII-preserved Enhanced executable was found.".into()));
        };
        self.replace_active(game_dir, &target, || self.mv(&backup_path, &target))?;
        let restored = hashcache::file_sha256_fresh(&target).unwrap_or_default().to_lowercase();
        record_enhanced_hash(self.core, game_dir, &restored);
        game::write_exe_variant(game_dir, ENHANCED_EXE_ID);
        log.success("Enhanced executable restored from PatchOpsIII backup.");
        Ok(())
    }

    /// The active profile (`current|compatible|enhanced|""`) of the exe in `game_dir`
    /// (reset-to-stock asks `!= current` before restoring).
    pub fn active_profile(&self, game_dir: &Path) -> &'static str {
        self.forget_hashes();
        let target = Self::target_exe(game_dir);
        self.active_exe_integrity(game_dir, &target).profile
    }
}

/// Legacy `_available_sibling_path`.
fn available_sibling_path(path: &Path) -> Result<PathBuf, String> {
    if !path.exists() {
        return Ok(path.to_path_buf());
    }
    let stem = stem_of(path);
    let suffix = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    for i in 2..1000 {
        let c = path.with_file_name(format!("{stem}-{i}{suffix}"));
        if !c.exists() {
            return Ok(c);
        }
    }
    Err(format!("No available filename found near {}", path.display()))
}

/// `Path.rename` with Python's error text; also drops stale cached digests.
fn move_file(src: &Path, dst: &Path) -> Result<(), String> {
    fs::rename(src, dst).map_err(|e| rename_error_text(&e, src, dst))?;
    hashcache::invalidate(src);
    hashcache::invalidate(dst);
    Ok(())
}

// ---- public convenience wrappers (production table) ------------------------------------------

/// `Active exe profile` with the production hash table (see [`Swap::active_profile`]).
pub fn active_exe_profile(core: &Core, game_dir: &Path) -> &'static str {
    let b = KnownBuilds::production();
    Swap::for_op(core, &b).active_profile(game_dir)
}

/// Reset-to-stock helper: restore the current-build exe from its validated backup.
pub fn restore_current_exe(core: &Core, game_dir: &Path) -> Result<(), String> {
    let b = KnownBuilds::production();
    Swap::for_op(core, &b).restore_current_exe(game_dir).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enhanced::testkit::Sandbox;
    use crate::hash::sha256_hex;
    use crate::ops::enhanced::exe_swap_state_with;
    use crate::state::StateCx;

    const CUR: &[u8] = b"current-exe-bytes";
    const COMPAT: &[u8] = b"compatible-exe-bytes!";
    const ENH: &[u8] = b"enhanced-exe-bytes-longer";

    fn builds() -> KnownBuilds {
        KnownBuilds { current_sha256: sha256_hex(CUR), compatible_sha256: vec![sha256_hex(COMPAT)] }
    }

    fn read(p: &Path) -> Vec<u8> {
        fs::read(p).unwrap()
    }

    fn state(sb: &Sandbox, b: &KnownBuilds) -> crate::ops::enhanced::ExeSwapState {
        let cx = StateCx { core: &sb.core, game_dir: Some(sb.game.to_string_lossy().into_owned()), config: String::new(), launch_options: None };
        exe_swap_state_with(&cx, b)
    }

    #[test]
    fn production_table_matches_legacy_constants() {
        let b = KnownBuilds::production();
        assert_eq!(b.current_sha256, DEFAULT_STEAM_EXE_SHA256);
        assert_eq!(b.compatible_sha256, COMPATIBLE_BUILD_SHA256);
    }

    #[test]
    fn compatible_from_depot_then_back_to_current_then_reuse() {
        let sb = Sandbox::new();
        let b = builds();
        let exe = sb.game.join("BlackOps3.exe");
        fs::write(&exe, CUR).unwrap();
        fs::write(sb.game.join("installscript_311210.vdf"), b"orig-script").unwrap();
        let depot = sb.depot();
        fs::write(depot.join("BlackOps3.exe"), COMPAT).unwrap();
        fs::write(depot.join("installscript_311210.vdf"), b"depot-script").unwrap();

        assert!(compatible_depot_download_exists(&sb.core));
        let swap = Swap::for_op(&sb.core, &b);
        swap.install_compatible_exe(&sb.game).unwrap();
        assert_eq!(read(&exe), COMPAT);
        assert_eq!(read(&sb.game.join("BlackOps3.24784313.bak")), CUR);
        assert_eq!(read(&sb.game.join("installscript_311210.vdf")), b"depot-script");
        assert_eq!(read(&sb.game.join("installscript_311210.vdf.patchops.bak")), b"orig-script");
        assert_eq!(game::read_exe_variant(&sb.game).as_deref(), Some("compatible"));
        let st = state(&sb, &b);
        assert_eq!(st.profile, "compatible");
        assert!(st.compatible_active && st.latest_available && st.backup_available && !st.compatible_available);
        assert_eq!(st.patch_label, "T7 Patch 2.04");
        assert_eq!(st.active_build_id, COMPATIBLE_STEAM_BUILD_ID);

        // Already active: no-op.
        swap.install_compatible_exe(&sb.game).unwrap();
        assert!(sb.log().contains("Compatible executable is already active."));

        // Back to current from the preserved build; compatible exe gets preserved in turn.
        swap.restore_current_exe(&sb.game).unwrap();
        assert_eq!(read(&exe), CUR);
        assert_eq!(read(&sb.game.join("BlackOps3.10650222.bak")), COMPAT);
        assert!(!sb.game.join("BlackOps3.24784313.bak").exists());
        assert_eq!(read(&sb.game.join("installscript_311210.vdf")), b"orig-script");
        assert_eq!(game::read_exe_variant(&sb.game).as_deref(), Some("default"));
        assert!(state(&sb, &b).compatible_available);

        // And forth again without a depot: reuses the preserved compatible exe.
        fs::remove_dir_all(&depot).unwrap();
        swap.install_compatible_exe(&sb.game).unwrap();
        assert_eq!(read(&exe), COMPAT);
        assert!(sb.log().contains("Reused preserved compatible executable from BlackOps3.10650222.bak."));
        assert!(sb.log().contains("Preserved active executable as BlackOps3.24784313.bak."));
        assert!(sb.log().contains("Compatible executable installed from Steam depot 9084453472036406216."));
    }

    fn failing_copy(_src: &Path, dst: &Path) -> std::io::Result<()> {
        // A non-atomic copier dying half way (disk full / EIO): leaves a truncated file behind.
        fs::write(dst, b"trunc")?;
        Err(std::io::Error::other("No space left on device"))
    }

    #[test]
    fn failed_exe_copy_restores_the_previous_executable() {
        let sb = Sandbox::new();
        let b = builds();
        let exe = sb.game.join("BlackOps3.exe");
        fs::write(&exe, CUR).unwrap();
        fs::write(sb.depot().join("BlackOps3.exe"), COMPAT).unwrap();
        let mut swap = Swap::for_op(&sb.core, &b);
        swap.copy_exe = failing_copy;
        let err = swap.install_compatible_exe(&sb.game).unwrap_err();
        assert!(matches!(&err, SwapError::Msg(m) if m.contains("No space left on device")), "{err:?}");
        // The original exe is back, byte for byte, no truncated file, no stray backup.
        assert_eq!(read(&exe), CUR);
        assert!(!sb.game.join("BlackOps3.24784313.bak").exists());
        assert!(sb.log().contains("Executable swap failed; restored the previous executable from BlackOps3.24784313.bak."));
        // Not stuck: it is still trusted, and the next swap (with a working copy) succeeds.
        assert_eq!(state(&sb, &b).profile, "current");
        let ok = Swap::for_op(&sb.core, &b);
        ok.install_compatible_exe(&sb.game).unwrap();
        assert_eq!(read(&exe), COMPAT);
    }

    #[test]
    fn real_atomic_copy_never_leaves_a_truncated_exe_and_failed_rename_rolls_back() {
        let sb = Sandbox::new();
        let b = builds();
        let exe = sb.game.join("BlackOps3.exe");
        fs::write(&exe, CUR).unwrap();
        // Unreadable source is not reproducible as root, so use a source that vanishes before the copy.
        fs::write(sb.depot().join("BlackOps3.exe"), COMPAT).unwrap();
        let mut swap = Swap::for_op(&sb.core, &b);
        fn vanished(src: &Path, dst: &Path) -> std::io::Result<()> {
            fs::remove_file(src)?;
            crate::fsutil::atomic_copy(src, dst)
        }
        swap.copy_exe = vanished;
        assert!(swap.install_compatible_exe(&sb.game).is_err());
        assert_eq!(read(&exe), CUR);
        let leftovers: Vec<_> = fs::read_dir(&sb.game).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.contains(".tmp-")).collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[test]
    fn op_hashes_each_unchanged_file_once() {
        let sb = Sandbox::new();
        let b = builds();
        let f = sb.game.join("x.bin");
        fs::write(&f, CUR).unwrap();
        let swap = Swap::for_op(&sb.core, &b);
        let h = swap.sha(&f).unwrap();
        // Same-size rewrite with a different content: the memo (one op) still answers, a new op does not.
        fs::write(&f, b"current-exe-bytez").unwrap();
        assert_eq!(swap.sha(&f).unwrap(), h);
        swap.forget_hashes();
        assert_ne!(swap.sha(&f).unwrap(), h);
    }

    #[test]
    fn depot_missing_or_bad_hash_requires_depot() {
        let sb = Sandbox::new();
        let b = builds();
        fs::write(sb.game.join("BlackOps3.exe"), CUR).unwrap();
        let swap = Swap::for_op(&sb.core, &b);
        assert_eq!(swap.install_compatible_exe(&sb.game), Err(SwapError::DepotRequired));
        fs::write(sb.depot().join("BlackOps3.exe"), b"not the compatible build").unwrap();
        fs::write(sb.depot().join("installscript_311210.vdf"), b"x").unwrap();
        assert!(compatible_depot_download_exists(&sb.core));
        assert_eq!(swap.install_compatible_exe(&sb.game), Err(SwapError::DepotRequired));
        assert_eq!(read(&sb.game.join("BlackOps3.exe")), CUR, "nothing is moved when the depot is unusable");
    }

    #[test]
    fn untrusted_active_exe_is_refused() {
        let sb = Sandbox::new();
        let b = builds();
        fs::write(sb.game.join("BlackOps3.exe"), b"mystery").unwrap();
        let swap = Swap::for_op(&sb.core, &b);
        for r in [swap.install_compatible_exe(&sb.game), swap.restore_current_exe(&sb.game), swap.restore_enhanced_exe(&sb.game)] {
            let SwapError::Msg(m) = r.unwrap_err() else { panic!("expected message") };
            assert!(m.starts_with("Active executable hash "), "{m}");
        }
    }

    #[test]
    fn untrusted_backups_are_ignored_with_warnings_and_not_listed() {
        let sb = Sandbox::new();
        let b = builds();
        fs::write(sb.game.join("BlackOps3.exe"), COMPAT).unwrap();
        fs::write(sb.game.join("BlackOps3.24784313.bak"), b"junk").unwrap();
        fs::write(sb.game.join("BlackOps3.exe.patchops.bak"), b"junk2").unwrap();
        let swap = Swap::for_op(&sb.core, &b);
        let err = swap.restore_current_exe(&sb.game).unwrap_err();
        assert_eq!(err.to_string(), "No backup executable found. Cannot restore the current EXE.");
        let log = sb.log();
        assert!(log.contains("Warning: Ignoring preserved latest executable with untrusted SHA-256: BlackOps3.24784313.bak"));
        assert!(log.contains("Warning: Ignoring executable backup with untrusted SHA-256: BlackOps3.exe.patchops.bak"));
        assert!(!state(&sb, &b).latest_available);
        // A genuine plain backup (legacy single-exe backup) is accepted.
        fs::write(sb.game.join("BlackOps3.exe.patchops.bak"), CUR).unwrap();
        assert!(state(&sb, &b).latest_available);
        swap.restore_current_exe(&sb.game).unwrap();
        assert_eq!(read(&sb.game.join("BlackOps3.exe")), CUR);
        assert_eq!(read(&sb.game.join("BlackOps3.10650222.bak")), COMPAT);
    }

    #[test]
    fn enhanced_exe_is_preserved_recorded_and_restorable() {
        let sb = Sandbox::new();
        let b = builds();
        let exe = sb.game.join("BlackOps3.exe");
        fs::write(&exe, ENH).unwrap();
        for f in crate::ops::enhanced::EXPECTED_ENHANCED_FILES {
            fs::write(sb.game.join(f), b"dll").unwrap();
        }
        let st = state(&sb, &b);
        assert_eq!((st.profile.as_str(), st.enhanced_active, st.trusted_executable), ("enhanced", true, true));

        // A current-build backup exists; swapping to current preserves the Enhanced exe + records its hash.
        fs::write(sb.game.join("BlackOps3.24784313.bak"), CUR).unwrap();
        let swap = Swap::for_op(&sb.core, &b);
        swap.restore_current_exe(&sb.game).unwrap();
        assert_eq!(read(&exe), CUR);
        assert_eq!(read(&sb.game.join("BlackOps3.enhanced.bak")), ENH);
        let known = crate::ops::enhanced::known_enhanced_hashes(&sb.core, Some(&sb.game.to_string_lossy()));
        assert_eq!(known, vec![sha256_hex(ENH)]);

        // Remove the DLLs so only the recorded hash identifies the preserved exe.
        for f in crate::ops::enhanced::EXPECTED_ENHANCED_FILES {
            fs::remove_file(sb.game.join(f)).unwrap();
        }
        let st = state(&sb, &b);
        assert!(st.enhanced_available && !st.enhanced_exe_active && st.profile == "current");
        swap.restore_enhanced_exe(&sb.game).unwrap();
        assert_eq!(read(&exe), ENH);
        assert_eq!(read(&sb.game.join("BlackOps3.24784313.bak")), CUR);
        assert_eq!(game::read_exe_variant(&sb.game).as_deref(), Some("enhanced"));
        assert_eq!(state(&sb, &b).profile, "enhanced");
        swap.restore_enhanced_exe(&sb.game).unwrap();
        assert!(sb.log().contains("Enhanced executable is already active."));
    }

    #[test]
    fn enhanced_hash_list_keeps_five_and_dedupes() {
        let sb = Sandbox::new();
        for i in 0..7 {
            record_enhanced_hash(&sb.core, &sb.game, &format!("{i:064x}"));
        }
        record_enhanced_hash(&sb.core, &sb.game, &format!("{:064X}", 6));
        let known = crate::ops::enhanced::known_enhanced_hashes(&sb.core, Some(&sb.game.to_string_lossy()));
        assert_eq!(known.len(), 5);
        assert_eq!(known[0], format!("{:064x}", 2));
        assert_eq!(known[4], format!("{:064x}", 6));
    }

    #[test]
    fn sibling_names_when_backup_slot_is_taken() {
        let sb = Sandbox::new();
        let b = builds();
        let exe = sb.game.join("BlackOps3.exe");
        fs::write(&exe, CUR).unwrap();
        // A stale (untrusted) file occupies the canonical slot.
        fs::write(sb.game.join("BlackOps3.24784313.bak"), b"stale").unwrap();
        let depot = sb.depot();
        fs::write(depot.join("BlackOps3.exe"), COMPAT).unwrap();
        fs::write(depot.join("installscript_311210.vdf"), b"d").unwrap();
        Swap::for_op(&sb.core, &b).install_compatible_exe(&sb.game).unwrap();
        assert_eq!(read(&sb.game.join("BlackOps3.24784313-2.bak")), CUR);
        assert_eq!(read(&sb.game.join("BlackOps3.24784313.bak")), b"stale");
    }

    #[test]
    fn legacy_named_backups_are_found_newest_first() {
        let dir = std::env::temp_dir().join(format!("patchops-glob-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("BlackOps3.exe");
        fs::write(dir.join("BlackOps3.current-24784313.exe"), b"a").unwrap();
        fs::write(dir.join("BlackOps3.current-24784313-1.exe"), b"b").unwrap();
        fs::write(dir.join("BlackOps3.current-24784313-2.exe"), b"c").unwrap();
        let f = fs::File::options().write(true).open(dir.join("BlackOps3.current-24784313-2.exe")).unwrap();
        f.set_modified(SystemTime::now() + std::time::Duration::from_secs(60)).unwrap();
        let c = build_backup_candidates(&exe, "24784313", "current");
        let names: Vec<_> = c.iter().map(|p| name_of(p)).collect();
        assert_eq!(names[0], "BlackOps3.current-24784313.exe");
        assert_eq!(names[1], "BlackOps3.current-24784313-2.exe");
        assert_eq!(names[2], "BlackOps3.current-24784313-1.exe");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_exe_reports_missing() {
        let sb = Sandbox::new();
        let b = builds();
        let swap = Swap::for_op(&sb.core, &b);
        let SwapError::Msg(m) = swap.install_compatible_exe(&sb.game).unwrap_err() else { panic!() };
        assert_eq!(m, "BlackOps3.exe or BlackOpsIII.exe was not found.");
        let st = state(&sb, &b);
        assert_eq!((st.integrity_status.as_str(), st.active_build_id.as_str(), st.executable.as_str()), ("missing", "Unknown", ""));
    }
}
