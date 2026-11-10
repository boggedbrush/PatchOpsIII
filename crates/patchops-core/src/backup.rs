//! `.patchops.bak` / legacy `.bak` sibling-backup conventions (legacy `utils.py` helpers).
//!
//! New backups always use `.patchops.bak`; when restoring, `.patchops.bak` wins over `.bak`.

use std::path::{Path, PathBuf};

pub const PATCHOPS_SUFFIX: &str = ".patchops.bak";
pub const LEGACY_SUFFIX: &str = ".bak";

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// `<path>.patchops.bak`
pub fn patchops_path(path: &Path) -> PathBuf {
    with_suffix(path, PATCHOPS_SUFFIX)
}

/// `<path>.bak`
pub fn legacy_path(path: &Path) -> PathBuf {
    with_suffix(path, LEGACY_SUFFIX)
}

/// The existing backup of `path` (`.patchops.bak` first, then `.bak`).
pub fn existing(path: &Path) -> Option<PathBuf> {
    [patchops_path(path), legacy_path(path)].into_iter().find(|p| p.exists())
}

/// Legacy `_restore_backup`: replace `target` with its backup. `Ok(false)` when no backup exists.
pub fn restore(target: &Path) -> std::io::Result<bool> {
    let Some(backup) = existing(target) else { return Ok(false) };
    if target.exists() {
        std::fs::remove_file(target)?;
    }
    std::fs::rename(backup, target)?;
    Ok(true)
}

/// Strip a backup suffix (`.patchops.bak` or `.bak`) from a file name.
pub fn original_name(name: &str) -> Option<&str> {
    name.strip_suffix(PATCHOPS_SUFFIX).or_else(|| name.strip_suffix(LEGACY_SUFFIX))
}
