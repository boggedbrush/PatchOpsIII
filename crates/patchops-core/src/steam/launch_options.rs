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
fn strip_token(s: &str, token: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while let Some(off) = s[i..].find(token) {
        let a = i + off;
        let b = a + token.len();
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
fn normalize(s: &str) -> String {
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

/// `shutil.copy2`: contents, permissions and modification time.
fn copy2(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::copy(from, to)?;
    if let Ok(m) = std::fs::metadata(from).and_then(|m| m.modified()) {
        if let Ok(f) = std::fs::OpenOptions::new().write(true).open(to) {
            let _ = f.set_modified(m);
        }
    }
    Ok(())
}

/// Legacy `_backup_file`: copy `config_path` to `<appdata>/backups/<backup_name>`.
pub fn backup_file(core: &Core, config_path: &Path, backup_name: &str) -> bool {
    let dir = &core.paths.backups;
    if let Err(e) = std::fs::create_dir_all(dir) {
        core.log.error(format!("Failed to prepare backup directory '{}': {}", dir.display(), io_error_text(&e, Some(dir))));
        return false;
    }
    let dest = dir.join(backup_name);
    match copy2(config_path, &dest) {
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
    match copy2(&src, config_path) {
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
            core.log.error(format!("Error updating launch options: {e}"));
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
/// `Err("Steam user ID not found!")` before Steam is touched.
pub fn apply_launch_options(core: &Core, options: &str, preserve_fs_game: bool) -> AppResult<bool> {
    let user_id = super::find_user_id(core).ok_or_else(|| AppError::new("Steam user ID not found!"))?;
    client::close_steam(core);
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
    client::close_steam(core);
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
    let mut data = vdf::load_file(path)?;
    let out = edit(&mut data);
    vdf::write_file(path, &data).map_err(|e| io_error_text(&e, Some(path)))?;
    Ok(out)
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
