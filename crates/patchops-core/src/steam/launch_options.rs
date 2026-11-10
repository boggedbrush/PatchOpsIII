//! `localconfig.vdf` launch-option writes (legacy `utils.py`: `set_launch_options`,
//! `set_launch_options_exact`, `apply_launch_options`, `_backup_file`/`_restore_file`).
//!
//! Safety: [`apply_launch_options`] closes Steam first and waits until the process is gone
//! ([`super::client::close_steam`]) *before* the file is read, so a client shutting down cannot
//! overwrite the edit; the client is re-opened afterwards.

use std::path::{Path, PathBuf};

use super::vdf::{self, Node, Obj};
use super::{client, BO3_APP_ID};
use crate::context::Core;
use crate::error::{AppError, AppResult};
use crate::pyfmt::io_error_text;

pub const WINE_OVERRIDE: &str = "WINEDLLOVERRIDES=\"dsound=n,b\"";
pub const COMMAND_MARKER: &str = "%command%";
pub const LOCALCONFIG_BACKUP: &str = "localconfig_backup.vdf";

// ---- merge algorithm ------------------------------------------------------------------------

fn is_ws(c: char) -> bool {
    c.is_whitespace()
}

/// Byte ranges of every `\+set\s+fs_game\s+\S+` match (leftmost, non-overlapping).
fn fs_game_matches(s: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(off) = s[i..].find("+set") {
        let start = i + off;
        let mut p = start + 4;
        let ws1 = skip_while(s, p, is_ws);
        let ok = ws1 > p && s[ws1..].starts_with("fs_game");
        if ok {
            p = ws1 + "fs_game".len();
            let ws2 = skip_while(s, p, is_ws);
            if ws2 > p {
                let end = skip_while(s, ws2, |c| !is_ws(c));
                if end > ws2 {
                    out.push((start, end));
                    i = end;
                    continue;
                }
            }
        }
        i = start + 1;
    }
    out
}

fn skip_while(s: &str, from: usize, f: impl Fn(char) -> bool) -> usize {
    let mut p = from;
    for c in s[from..].chars() {
        if !f(c) {
            break;
        }
        p += c.len_utf8();
    }
    p
}

fn remove_ranges(s: &str, ranges: &[(usize, usize)]) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for &(a, b) in ranges {
        out.push_str(&s[last..a]);
        last = b;
    }
    out.push_str(&s[last..]);
    out
}

/// `re.sub(rf'(?<!\S){re.escape(token)}(?!\S)', '', s)`
pub(crate) fn strip_token(s: &str, token: &str) -> String {
    strip_token_ci(s, token, false)
}

/// [`strip_token`], optionally matching the (ASCII) token case-insensitively (`re.IGNORECASE`).
/// ASCII lowercasing keeps byte offsets, so positions found in the lowered copy index `s`.
pub(crate) fn strip_token_ci(s: &str, token: &str, ignore_ascii_case: bool) -> String {
    let lowered;
    let (hay, needle): (&str, std::borrow::Cow<str>) = if ignore_ascii_case {
        lowered = s.to_ascii_lowercase();
        (lowered.as_str(), token.to_ascii_lowercase().into())
    } else {
        (s, token.into())
    };
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while let Some(off) = hay[i..].find(needle.as_ref()) {
        let a = i + off;
        let b = a + needle.len();
        let before_ok = s[..a].chars().next_back().is_none_or(is_ws);
        let after_ok = s[b..].chars().next().is_none_or(is_ws);
        if before_ok && after_ok {
            out.push_str(&s[i..a]);
            i = b;
        } else {
            // Keep one char of the false match and rescan after it.
            let step = s[a..].chars().next().map_or(1, char::len_utf8);
            out.push_str(&s[i..a + step]);
            i = a + step;
        }
    }
    out.push_str(&s[i..]);
    out
}

/// `re.sub(r'\s+', ' ', s).strip()`
pub(crate) fn normalize(s: &str) -> String {
    s.split(is_ws).filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ")
}

/// The legacy merge: `[WINEDLLOVERRIDES=..] [%command%] <rest of existing> <rest of requested> <fs_game>`.
/// Requested options are appended to the existing non-`fs_game` content, never replacing it.
pub fn merge_launch_options(current: &str, requested: &str, preserve_fs_game: bool) -> String {
    let existing_fs: Vec<&str> = fs_game_matches(current).iter().map(|&(a, b)| &current[a..b]).collect();
    let new_fs: Vec<&str> = fs_game_matches(requested).iter().map(|&(a, b)| &requested[a..b]).collect();
    let fs_entries: Vec<&str> = if !new_fs.is_empty() {
        new_fs
    } else if preserve_fs_game {
        existing_fs
    } else {
        Vec::new()
    };

    let cleaned_current = remove_ranges(current, &fs_game_matches(current)).trim().to_string();
    let cleaned_launch = remove_ranges(requested, &fs_game_matches(requested)).trim().to_string();

    let has_current_wine = cleaned_current.contains(WINE_OVERRIDE);
    let has_current_command = cleaned_current.contains(COMMAND_MARKER);
    let has_new_wine = requested.contains(WINE_OVERRIDE);
    let has_new_command = requested.contains(COMMAND_MARKER);

    let cleaned_current = strip_token(&strip_token(&cleaned_current, WINE_OVERRIDE), COMMAND_MARKER);
    let cleaned_launch = strip_token(&strip_token(&cleaned_launch, WINE_OVERRIDE), COMMAND_MARKER);

    let include_wine = has_new_wine || has_current_wine;
    let include_command = has_new_command || has_current_command || include_wine;

    let mut segments: Vec<String> = Vec::new();
    if include_wine {
        segments.push(WINE_OVERRIDE.to_string());
    }
    if include_command {
        segments.push(COMMAND_MARKER.to_string());
    }
    if !cleaned_current.is_empty() {
        segments.push(normalize(&cleaned_current));
    }
    if !cleaned_launch.is_empty() {
        segments.push(normalize(&cleaned_launch));
    }
    if !fs_entries.is_empty() {
        let mut unique: Vec<&str> = Vec::new();
        for e in fs_entries {
            if !unique.contains(&e) {
                unique.push(e);
            }
        }
        segments.push(normalize(&unique.join(" ")));
    }
    normalize(&segments.join(" "))
}

// ---- backup / restore -----------------------------------------------------------------------

/// Legacy `_backup_file`: copy `config_path` to `<appdata>/backups/<backup_name>`.
pub fn backup_file(core: &Core, config_path: &Path, backup_name: &str) -> bool {
    let dir = &core.paths.backups;
    if let Err(e) = std::fs::create_dir_all(dir) {
        core.log.error(format!("Failed to prepare backup directory '{}': {}", dir.display(), io_error_text(&e, Some(dir))));
        return false;
    }
    let dest = dir.join(backup_name);
    match crate::fsutil::copy2(config_path, &dest) {
        Ok(()) => {
            core.log.success(format!("Config backup created at {}", dest.display()));
            true
        }
        Err(e) => {
            core.log.error(format!("Failed to create backup: {}", io_error_text(&e, Some(config_path))));
            false
        }
    }
}

/// Legacy `_restore_file` (only the user backup directory is consulted).
pub fn restore_file(core: &Core, config_path: &Path, backup_name: &str) -> bool {
    let src = core.paths.backups.join(backup_name);
    if !src.exists() {
        core.log.warning("No backup file found");
        return false;
    }
    match crate::fsutil::copy2(&src, config_path) {
        Ok(()) => {
            core.log.success("Config restored from backup");
            true
        }
        Err(e) => {
            core.log.error(format!("Failed to restore backup from {}: {}", src.display(), io_error_text(&e, Some(&src))));
            false
        }
    }
}

// ---- first-seen copies ----------------------------------------------------------------------

/// Temp files of an interrupted copy older than this are removed on the next call.
const STALE_TMP: std::time::Duration = std::time::Duration::from_secs(60);

/// File name of the first-seen copy of `source`: `<prefix>_<8 hex of a stable hash of the canonical
/// source path>_original.vdf`, so the same Steam user / file under two Steam roots (native and
/// Flatpak, or two library roots) never share a copy.
pub fn first_seen_name(prefix: &str, source: &Path) -> String {
    let canon = std::fs::canonicalize(source).unwrap_or_else(|_| source.to_path_buf());
    // FNV-1a 64, folded to 32 bits: stable across runs and platforms (unlike `DefaultHasher`).
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in canon.to_string_lossy().as_bytes() {
        h = (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{prefix}_{:08x}_original.vdf", (h ^ (h >> 32)) as u32)
}

/// Write-once "first seen" copy of `source` at `<paths.originals>/<name>`.
///
/// Unlike the rolling `backups/` slot this is never overwritten: if the destination already
/// holds a copy nothing happens, so it keeps the oldest content the app ever saw. The copy is made
/// to a private temp file in the same directory and then placed with a link/rename that fails
/// instead of clobbering ([`place_no_clobber`]); the final file is made read-only last (0o400 on unix) and
/// the directory is created 0o700.
///
/// Silent by design: it never logs and never fails the caller. Returns `Some(path)` only when a
/// regular file exists at the destination afterwards (created now or from before); anything else
/// there (a directory, a dangling symlink) is left alone and yields `None`.
pub fn preserve_first_seen(core: &Core, source: &Path, name: &str) -> Option<PathBuf> {
    let dir = &core.paths.originals;
    sweep_stale_temps(dir);
    let dest = dir.join(name);
    match std::fs::symlink_metadata(&dest) {
        Ok(m) if m.is_file() => return Some(dest),
        Ok(_) => return None,
        Err(_) => {}
    }
    if std::fs::symlink_metadata(source).is_err() || create_private_dir(dir).is_err() {
        return None;
    }
    let tmp = dir.join(format!(".{name}.tmp-{}-{}", std::process::id(), tmp_nonce()));
    // Order matters on Windows: after a successful hard link the temp and the destination are one
    // NTFS file sharing its read-only attribute, so the temp is written writable, placed, removed
    // (plain delete, no attribute games), and only then is the destination made read-only.
    let made = write_new_copy(source, &tmp).and_then(|()| place_no_clobber(&tmp, &dest));
    let _ = std::fs::remove_file(&tmp);
    let exists_now = || std::fs::symlink_metadata(&dest).is_ok_and(|m| m.is_file());
    match made {
        Ok(()) => {
            // Hardening only: a copy that stays writable is still the first-seen content.
            let _ = set_read_only(&dest);
            Some(dest)
        }
        // Another writer won the race: their copy is the first-seen one.
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && exists_now() => Some(dest),
        Err(_) => None,
    }
}

/// Human-readable suffix for an error message: names the first-seen copy when there is one.
pub fn first_seen_note(copy: Option<&Path>) -> String {
    // The path depends on the environment (XDG_STATE_HOME / LOCALAPPDATA / the sandbox); never rely
    // on this suffix in recorded goldens.
    match copy {
        Some(p) => format!(" The earliest copy PatchOpsIII saw is kept at {}.", p.display()),
        None => String::new(),
    }
}

fn tmp_nonce() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())
}

/// `create_dir_all`, with mode 0o700 for the directories it creates on unix (the copies are the
/// user's Steam config, which holds account names).
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    let mut b = std::fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    b.create(dir)
}

/// Remove `.<name>_original.vdf.tmp-*` leftovers (a crash between copy and link) older than a
/// minute; younger ones may belong to a copy in flight in another process.
fn sweep_stale_temps(dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !(name.starts_with('.') && name.contains("_original.vdf.tmp-")) {
            continue;
        }
        let old = e.metadata().ok().filter(|m| m.is_file()).and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).is_some_and(|age| age > STALE_TMP);
        if old {
            remove_stale_temp(dir, &e.path());
        }
    }
}

/// Plain `remove_file`. Temps made by this version are always writable, so that is all there is to
/// do. Only a leftover of an older build (its temp was made read-only before it was linked) can
/// refuse: on Windows that can be the very file a kept copy is hard-linked to, so clearing the
/// read-only attribute on it would make the kept copy writable. The simplest correct handling (std
/// cannot report a link count): clear it, delete, then put the read-only attribute back on every
/// `*_original.vdf` in the directory (the kept copies are all meant to be read-only).
fn remove_stale_temp(dir: &Path, tmp: &Path) {
    if std::fs::remove_file(tmp).is_ok() {
        return;
    }
    #[cfg(windows)]
    if let Ok(m) = std::fs::metadata(tmp) {
        let mut p = m.permissions();
        if p.readonly() {
            #[allow(clippy::permissions_set_readonly_false)]
            p.set_readonly(false);
            if std::fs::set_permissions(tmp, p).is_ok() && std::fs::remove_file(tmp).is_ok() {
                if let Ok(rd) = std::fs::read_dir(dir) {
                    for e in rd.flatten() {
                        let n = e.file_name().to_string_lossy().into_owned();
                        if !n.starts_with('.') && n.ends_with("_original.vdf") && e.metadata().is_ok_and(|m| m.is_file()) {
                            let _ = set_read_only(&e.path());
                        }
                    }
                }
            }
        }
    }
    #[cfg(not(windows))]
    let _ = dir;
}

/// Make `tmp` appear at `dest` without ever replacing something already there: a hard link first
/// (atomic, fails with `AlreadyExists`); where links are unsupported (FAT/exFAT, some SMB/ReFS
/// shares) a no-clobber rename of the complete temp file. The final name never holds partial
/// content. The caller removes `tmp` afterwards (a no-op after a rename).
fn place_no_clobber(tmp: &Path, dest: &Path) -> std::io::Result<()> {
    match std::fs::hard_link(tmp, dest) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(e),
        // Links unsupported (or refused): only fall back while the destination is still free.
        Err(e) => match std::fs::symlink_metadata(dest) {
            Err(m) if m.kind() == std::io::ErrorKind::NotFound => rename_no_replace(tmp, dest),
            _ => Err(e),
        },
    }
}

#[cfg(target_os = "linux")]
fn rename_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let c = |p: &Path| std::ffi::CString::new(p.as_os_str().as_bytes()).map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput));
    let (f, t) = (c(from)?, c(to)?);
    // SAFETY: both pointers are valid NUL-terminated strings for the duration of the call.
    let rc = unsafe { libc::renameat2(libc::AT_FDCWD, f.as_ptr(), libc::AT_FDCWD, t.as_ptr(), libc::RENAME_NOREPLACE) };
    if rc == 0 {
        return Ok(());
    }
    let err = std::io::Error::last_os_error();
    match err.raw_os_error() {
        // The file system (or kernel) has no RENAME_NOREPLACE: check-then-rename, accepting the tiny
        // window in which another writer could create `to` (it would then be replaced by an equally
        // valid first-seen copy of the same source).
        Some(libc::EINVAL) | Some(libc::ENOSYS) | Some(libc::ENOTSUP) => rename_if_absent(from, to),
        _ => Err(err),
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
fn rename_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    rename_if_absent(from, to)
}

/// `MoveFileExW` without `MOVEFILE_REPLACE_EXISTING`: fails with `ERROR_ALREADY_EXISTS` instead of
/// replacing (`std::fs::rename` replaces on Windows).
#[cfg(windows)]
fn rename_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let w = |p: &Path| p.as_os_str().encode_wide().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (f, t) = (w(from), w(to));
    // SAFETY: both buffers are NUL-terminated wide strings that outlive the call.
    if unsafe { MoveFileExW(f.as_ptr(), t.as_ptr(), 0) } != 0 {
        return Ok(());
    }
    let e = std::io::Error::last_os_error();
    // ERROR_ALREADY_EXISTS (183) / ERROR_FILE_EXISTS (80)
    Err(match e.raw_os_error() {
        Some(183) | Some(80) => std::io::Error::from(std::io::ErrorKind::AlreadyExists),
        _ => e,
    })
}

/// Existence check followed by a plain rename; only used where no atomic no-clobber rename exists.
#[allow(dead_code)]
fn rename_if_absent(from: &Path, to: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(to).is_ok() {
        return Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists));
    }
    std::fs::rename(from, to)
}

/// Byte copy into a file that must not exist yet (`create_new`), so a stray temp is never reused.
fn write_new_copy(src: &Path, dst: &Path) -> std::io::Result<()> {
    let mut input = std::fs::File::open(src)?;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    let mut out = o.open(dst)?;
    std::io::copy(&mut input, &mut out)?;
    Ok(())
}

fn set_read_only(path: &Path) -> std::io::Result<()> {
    let mut perms = std::fs::metadata(path)?.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o400);
    }
    #[cfg(not(unix))]
    perms.set_readonly(true);
    std::fs::set_permissions(path, perms)
}

// ---- localconfig.vdf ------------------------------------------------------------------------

/// `UserLocalConfigStore/Software/Valve/Steam/apps/<app_id>` (created on demand).
pub fn app_entry_mut<'a>(data: &'a mut Obj, app_id: &str) -> &'a mut Obj {
    data.section_mut("UserLocalConfigStore")
        .section_mut("Software")
        .section_mut("Valve")
        .section_mut("Steam")
        .section_mut("apps")
        .section_mut(app_id)
}

/// Shared skeleton of `set_launch_options[_exact]`: path/backup checks, load, `edit` (returns
/// the options string to log), write, restore-on-error.
fn update_localconfig(core: &Core, user_id: &str, edit: impl FnOnce(&mut Obj) -> String) -> bool {
    let path = super::localconfig_path(core, user_id);
    if !path.exists() {
        core.log.error("localconfig.vdf not found!");
        return false;
    }
    let first_seen = preserve_first_seen(core, &path, &first_seen_name(&format!("localconfig_{user_id}"), &path));
    if !backup_file(core, &path, LOCALCONFIG_BACKUP) {
        core.log.error("Aborting due to backup failure");
        return false;
    }
    let result = (|| -> Result<(), String> {
        let mut data = vdf::load_file(&path)?;
        let final_options = edit(&mut data);
        core.log.info(format!("Setting launch options to: {final_options}"));
        vdf::write_file(&path, &data).map_err(|e| io_error_text(&e, Some(&path)))
    })();
    match result {
        Ok(()) => true,
        Err(e) => {
            let note = first_seen_note(first_seen.as_deref());
            core.log.error(format!("Error updating launch options: {e}{note}"));
            restore_file(core, &path, LOCALCONFIG_BACKUP);
            false
        }
    }
}

/// Legacy `set_launch_options`: back up and merge `options` into the app's `LaunchOptions`.
/// Does *not* touch the Steam process; use [`apply_launch_options`] for the full cycle.
pub fn set_launch_options(core: &Core, user_id: &str, app_id: &str, options: &str, preserve_fs_game: bool) -> bool {
    update_localconfig(core, user_id, |data| {
        let entry = app_entry_mut(data, app_id);
        let current = entry.get_str("LaunchOptions").unwrap_or("").to_string();
        let merged = merge_launch_options(&current, options, preserve_fs_game);
        entry.set("LaunchOptions", Node::Str(merged.clone()));
        merged
    })
}

/// Legacy `set_launch_options_exact`: replace `LaunchOptions` verbatim.
pub fn set_launch_options_exact(core: &Core, user_id: &str, app_id: &str, options: &str) -> bool {
    update_localconfig(core, user_id, |data| {
        app_entry_mut(data, app_id).set("LaunchOptions", Node::Str(options.to_string()));
        options.to_string()
    })
}

/// Legacy `apply_launch_options(options, preserve_fs_game)` for app 311210: close Steam (wait
/// until gone), merge into `localconfig.vdf`, re-open Steam. `Ok(wrote_file)`;
/// `Err("Steam user ID not found!")` before Steam is touched; `Err(STEAM_CLOSE_TIMEOUT_MSG)` when
/// Steam would not exit in time (no file was read or written, Steam is left running).
pub fn apply_launch_options(core: &Core, options: &str, preserve_fs_game: bool) -> AppResult<bool> {
    let user_id = super::find_user_id(core).ok_or_else(|| AppError::new("Steam user ID not found!"))?;
    client::close_steam(core).map_err(AppError::new)?;
    let ok = set_launch_options(core, &user_id, BO3_APP_ID, options, preserve_fs_game);
    client::open_steam(core);
    Ok(ok)
}

/// `apply_launch_options("")` as reset-to-stock / Enhanced cleanup call it: with an empty
/// request the existing non-`fs_game` options (and wine override / `%command%`) are kept, the
/// `fs_game` entry is dropped. Full Steam close/open cycle.
pub fn clear_launch_options(core: &Core) -> AppResult<bool> {
    apply_launch_options(core, "", false)
}

/// Same cycle as [`apply_launch_options`] but writing `options` verbatim
/// (`set_launch_options_exact`), as the Enhanced install/uninstall code does.
pub fn apply_launch_options_exact(core: &Core, options: &str) -> AppResult<bool> {
    let user_id = super::find_user_id(core).ok_or_else(|| AppError::new("Steam user ID not found!"))?;
    client::close_steam(core).map_err(AppError::new)?;
    let ok = set_launch_options_exact(core, &user_id, BO3_APP_ID, options);
    client::open_steam(core);
    Ok(ok)
}

// ---- generic VDF edit helper (config.vdf / CompatToolMapping) --------------------------------

/// `<steam root>/config/config.vdf` (root as [`super::root_path`]).
pub fn steam_config_vdf_path(core: &Core) -> Option<PathBuf> {
    super::root_path(core).map(|r| Path::new(&r).join("config").join("config.vdf"))
}

/// Load `path`, run `edit` on the document, write it back with the legacy writer. `Err` carries
/// the text to log. Callers are responsible for closing Steam first and for backups
/// ([`backup_file`]).
pub fn modify_vdf_file<T>(path: &Path, edit: impl FnOnce(&mut Obj) -> T) -> Result<T, String> {
    modify_vdf_file_if(path, |d| Some(edit(d))).map(|o| o.expect("edit always returns Some"))
}

/// [`modify_vdf_file`] where `edit` may return `None` to abort: the file is then left untouched
/// (nothing is written) and `Ok(None)` is returned.
pub fn modify_vdf_file_if<T>(path: &Path, edit: impl FnOnce(&mut Obj) -> Option<T>) -> Result<Option<T>, String> {
    let mut data = vdf::load_file(path)?;
    let Some(out) = edit(&mut data) else { return Ok(None) };
    vdf::write_file(path, &data).map_err(|e| io_error_text(&e, Some(path)))?;
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: &str = WINE_OVERRIDE;

    #[test]
    fn offline_over_wine_existing() {
        let cur = format!("{W} %command%");
        assert_eq!(merge_launch_options(&cur, "+set fs_game offlinemp", false), format!("{W} %command% +set fs_game offlinemp"));
    }

    #[test]
    fn default_drops_fs_game_but_keeps_rest() {
        let cur = format!("{W} %command% +set fs_game 2994481309");
        assert_eq!(merge_launch_options(&cur, "", false), format!("{W} %command%"));
        assert_eq!(merge_launch_options(&cur, "", true), format!("{W} %command% +set fs_game 2994481309"));
    }

    #[test]
    fn requested_is_appended_and_fs_game_replaced() {
        let cur = "-novid +set fs_game 1";
        assert_eq!(merge_launch_options(cur, "+set fs_game 2", true), "-novid +set fs_game 2");
        assert_eq!(merge_launch_options("-novid", "-windowed", false), "-novid -windowed");
    }

    #[test]
    fn wine_without_command_forces_command() {
        assert_eq!(merge_launch_options("", W, false), format!("{W} %command%"));
        assert_eq!(merge_launch_options("", "%command%", false), "%command%");
    }

    #[test]
    fn tokens_only_stripped_on_boundaries() {
        // "x%command%" is not the token: kept as-is, and has_command is still true (substring).
        assert_eq!(merge_launch_options("x%command%", "", false), "%command% x%command%");
        assert_eq!(strip_token("a %command% b", "%command%"), "a  b");
    }

    #[test]
    fn whitespace_variants_and_duplicates() {
        assert_eq!(merge_launch_options("", "+set  fs_game   7 +set fs_game 7", false), "+set fs_game 7 +set fs_game 7");
        assert_eq!(merge_launch_options("  a \t b  ", "", false), "a b");
        assert_eq!(fs_game_matches("+set fs_game").len(), 0);
        assert_eq!(fs_game_matches("x +set fs_game y").len(), 1);
    }
}

#[cfg(test)]
mod first_seen_tests {
    use super::*;
    use crate::enhanced::testkit::Sandbox;
    use crate::steam::{localconfig_path, read_launch_options};

    const USER: &str = "12345";
    const APP: &str = "311210";

    fn vdf(options: &str) -> String {
        format!(
            "\"UserLocalConfigStore\"\n{{\n\t\"Software\"\n\t{{\n\t\t\"Valve\"\n\t\t{{\n\t\t\t\"Steam\"\n\t\t\t{{\n\t\t\t\t\"apps\"\n\t\t\t\t{{\n\t\t\t\t\t\"{APP}\"\n\t\t\t\t\t{{\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"{options}\"\n\t\t\t\t\t}}\n\t\t\t\t}}\n\t\t\t}}\n\t\t}}\n\t}}\n}}\n"
        )
    }

    fn write_localconfig(core: &Core, text: &str) -> PathBuf {
        let path = localconfig_path(core, USER);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    }

    fn first_copy(core: &Core) -> PathBuf {
        core.paths.originals.join(first_seen_name(&format!("localconfig_{USER}"), &localconfig_path(core, USER)))
    }

    #[test]
    fn first_update_keeps_byte_identical_first_copy() {
        let sb = Sandbox::new();
        let pre = vdf("-novid");
        write_localconfig(&sb.core, &pre);
        assert!(set_launch_options_exact(&sb.core, USER, APP, "-windowed"));
        assert_eq!(std::fs::read(first_copy(&sb.core)).unwrap(), pre.as_bytes());
        let mode = std::fs::metadata(first_copy(&sb.core)).unwrap().permissions();
        #[cfg(unix)]
        assert_eq!(std::os::unix::fs::PermissionsExt::mode(&mode) & 0o777, 0o400);
        #[cfg(not(unix))]
        assert!(mode.readonly());
    }

    #[test]
    fn second_update_keeps_first_copy_and_rolls_backup() {
        let sb = Sandbox::new();
        let first = vdf("-novid");
        write_localconfig(&sb.core, &first);
        assert!(set_launch_options_exact(&sb.core, USER, APP, "-windowed"));
        let second = vdf("-windowed -console");
        write_localconfig(&sb.core, &second);
        assert!(set_launch_options_exact(&sb.core, USER, APP, "-console"));
        assert_eq!(std::fs::read(first_copy(&sb.core)).unwrap(), first.as_bytes());
        let rolling = sb.core.paths.backups.join(LOCALCONFIG_BACKUP);
        assert_eq!(std::fs::read(rolling).unwrap(), second.as_bytes());
    }

    #[test]
    fn nothing_created_when_localconfig_missing() {
        let sb = Sandbox::new();
        assert!(!set_launch_options_exact(&sb.core, USER, APP, "-x"));
        assert!(!sb.core.paths.originals.exists());
        assert!(preserve_first_seen(&sb.core, &localconfig_path(&sb.core, USER), "x.vdf").is_none());
        assert!(!sb.core.paths.originals.exists());
    }

    #[test]
    fn blocked_originals_dir_does_not_fail_the_operation() {
        let sb = Sandbox::new();
        std::fs::create_dir_all(&sb.core.paths.app_data).unwrap();
        std::fs::write(&sb.core.paths.originals, b"in the way").unwrap();
        write_localconfig(&sb.core, &vdf("-novid"));
        assert!(set_launch_options_exact(&sb.core, USER, APP, "-windowed"));
        assert_eq!(read_launch_options(&sb.core, USER, APP).as_deref(), Some("-windowed"));
        assert!(preserve_first_seen(&sb.core, &localconfig_path(&sb.core, USER), "x.vdf").is_none());
    }

    #[test]
    fn preserve_never_logs() {
        let sb = Sandbox::new();
        let src = sb.root.join("src.vdf");
        std::fs::write(&src, "data").unwrap();
        let before = sb.log();
        assert!(preserve_first_seen(&sb.core, &src, "a.vdf").is_some());
        assert!(preserve_first_seen(&sb.core, &src, "a.vdf").is_some());
        assert!(preserve_first_seen(&sb.core, &sb.root.join("missing"), "b.vdf").is_none());
        assert_eq!(sb.log(), before);
    }

    #[test]
    fn first_seen_note_names_the_copy() {
        assert_eq!(first_seen_note(None), "");
        assert_eq!(
            first_seen_note(Some(Path::new("/x/y.vdf"))),
            " The earliest copy PatchOpsIII saw is kept at /x/y.vdf."
        );
    }

    #[test]
    fn existing_non_file_at_the_destination_is_not_a_copy_and_is_left_alone() {
        let sb = Sandbox::new();
        let src = sb.root.join("src.vdf");
        std::fs::write(&src, "data").unwrap();
        std::fs::create_dir_all(&sb.core.paths.originals).unwrap();
        let dir_dest = sb.core.paths.originals.join("d.vdf");
        std::fs::create_dir(&dir_dest).unwrap();
        assert!(preserve_first_seen(&sb.core, &src, "d.vdf").is_none());
        assert!(dir_dest.is_dir());
        #[cfg(unix)]
        {
            let link = sb.core.paths.originals.join("l.vdf");
            std::os::unix::fs::symlink("/nonexistent/x", &link).unwrap();
            assert!(preserve_first_seen(&sb.core, &src, "l.vdf").is_none());
            assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
            assert!(!Path::new("/nonexistent/x").exists());
        }
        // A regular file is a copy, whatever its content.
        std::fs::write(sb.core.paths.originals.join("f.vdf"), "older").unwrap();
        assert_eq!(preserve_first_seen(&sb.core, &src, "f.vdf"), Some(sb.core.paths.originals.join("f.vdf")));
        assert_eq!(std::fs::read(sb.core.paths.originals.join("f.vdf")).unwrap(), b"older");
    }

    #[test]
    fn no_clobber_placement_and_rename_fallback() {
        let sb = Sandbox::new();
        let d = &sb.root;
        let (tmp, dest) = (d.join("t1"), d.join("dest"));
        std::fs::write(&tmp, "new").unwrap();
        // The rename fallback (what runs where hard links are unsupported) moves into a free name...
        rename_no_replace(&tmp, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"new");
        assert!(!tmp.exists());
        // ...and refuses to replace an existing one, leaving both untouched.
        std::fs::write(&tmp, "other").unwrap();
        let e = rename_no_replace(&tmp, &dest).unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&dest).unwrap(), b"new");
        assert_eq!(std::fs::read(&tmp).unwrap(), b"other");
        // The check-then-rename variant (no atomic primitive available) behaves the same.
        assert_eq!(rename_if_absent(&tmp, &dest).unwrap_err().kind(), std::io::ErrorKind::AlreadyExists);
        std::fs::remove_file(&dest).unwrap();
        rename_if_absent(&tmp, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"other");
        // The normal path links (both names then hold the data until the caller removes the temp).
        let (tmp, dest) = (d.join("t2"), d.join("dest2"));
        std::fs::write(&tmp, "x").unwrap();
        place_no_clobber(&tmp, &dest).unwrap();
        assert_eq!(place_no_clobber(&tmp, &dest).unwrap_err().kind(), std::io::ErrorKind::AlreadyExists);
    }

    #[cfg(unix)]
    #[test]
    fn directory_is_private_copy_is_0400_and_stale_temps_are_swept() {
        use std::os::unix::fs::PermissionsExt;
        let sb = Sandbox::new();
        let src = sb.root.join("src.vdf");
        std::fs::write(&src, "data").unwrap();
        let dir = &sb.core.paths.originals;
        let stale = dir.join(".a_original.vdf.tmp-1-2");
        let fresh = dir.join(".b_original.vdf.tmp-1-3");
        let other = dir.join(".unrelated.tmp-1-4");
        std::fs::create_dir_all(dir).unwrap();
        for f in [&stale, &fresh, &other] {
            std::fs::write(f, "x").unwrap();
        }
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
        for f in [&stale, &other] {
            std::fs::OpenOptions::new().write(true).open(f).unwrap().set_modified(old).unwrap();
        }
        std::fs::set_permissions(&stale, std::fs::Permissions::from_mode(0o400)).unwrap();
        let dest = preserve_first_seen(&sb.core, &src, "c_original.vdf").unwrap();
        assert!(!stale.exists(), "old temp swept");
        assert!(fresh.exists(), "fresh temp may be in flight elsewhere");
        assert!(other.exists(), "only our temp pattern is swept");
        assert_eq!(std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777, 0o400);
        assert_eq!(std::os::unix::fs::MetadataExt::nlink(&std::fs::metadata(&dest).unwrap()), 1, "temp link removed before the copy is sealed");
        // Created from scratch: private directory.
        let sb2 = Sandbox::new();
        let src2 = sb2.root.join("s.vdf");
        std::fs::write(&src2, "d").unwrap();
        preserve_first_seen(&sb2.core, &src2, "c_original.vdf").unwrap();
        assert_eq!(std::fs::metadata(&sb2.core.paths.originals).unwrap().permissions().mode() & 0o777, 0o700);
        // No temp left behind by a normal run.
        assert!(std::fs::read_dir(&sb2.core.paths.originals).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().contains(".tmp-")));
    }

    #[test]
    fn copy_names_are_keyed_by_the_source_path() {
        let a = first_seen_name("steam_config", Path::new("/home/u/.steam/steam/config/config.vdf"));
        let b = first_seen_name("steam_config", Path::new("/home/u/.var/app/com.valvesoftware.Steam/.steam/steam/config/config.vdf"));
        assert_ne!(a, b);
        assert_eq!(a, first_seen_name("steam_config", Path::new("/home/u/.steam/steam/config/config.vdf")));
        assert!(a.starts_with("steam_config_") && a.ends_with("_original.vdf"), "{a}");
        assert_eq!(a.len(), "steam_config_".len() + 8 + "_original.vdf".len());
    }
}
