//! Group `t7`: `t7-config`, `t7-install`, `t7-uninstall` plus the `t7` state builder.
//!
//! Legacy reference: `backend/api.py` (`set_t7_config`, `install_t7_patch`, `_uninstall_t7_patch`,
//! `uninstall_t7_patch`) and `t7_patch.py` (`update_t7patch_conf`, `restore_lpc_backups`). The
//! download / extract / LPC machinery is in [`crate::t7_install`]; Windows elevation in
//! [`crate::elevate`] + [`crate::cli`].

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{state_reply, Args, Group};
use crate::context::Core;
use crate::error::{AppError, AppResult, OpError};
use crate::net::{no_progress, Net, ProgressFn};
use crate::pyfmt::{io_error_text, repr_str};
use crate::state::{State, StateCx};

pub const OPS: &[&str] = &["t7-config", "t7-install", "t7-uninstall"];

pub const GROUP: Group = Group { name: "t7", ops: OPS, call };

/// Files removed by `t7-uninstall` (legacy `T7_GAME_FILES`).
pub const T7_GAME_FILES: [&str; 6] =
    ["t7patch.dll", "t7patch.conf", "discord_game_sdk.dll", "dsound.dll", "t7patchloader.dll", "zbr2.dll"];

fn call(core: &Core, op: &str, args: &Args) -> Result<Value, OpError> {
    match op {
        "t7-config" => {
            // Validation of every field happens before the handler runs (pydantic order).
            let gamertag = args.str_opt("gamertag")?;
            let color_code = args.str_or("colorCode", "")?;
            let network_password = args.str_opt("networkPassword")?;
            let friends_only = args.bool_opt("friendsOnly")?;
            state_reply(set_t7_config(core, gamertag.as_deref(), &color_code, network_password.as_deref(), friends_only))
        }
        "t7-install" => state_reply(t7_install(core)),
        "t7-uninstall" => state_reply(t7_uninstall(core)),
        _ => Err(OpError::UnknownOp(op.to_string())),
    }
}

/// Files that only the compatible (2.04) patch ships (legacy `T7PATCH_COMPATIBLE_ONLY_FILES`).
pub const T7_PATCH_COMPATIBLE_ONLY_FILES: [&str; 2] = ["discord_game_sdk.dll", "zbr2.dll"];

// ---- t7-install -----------------------------------------------------------------------------

/// Legacy `install_t7_patch` route against the real endpoints, without progress reporting.
pub fn t7_install(core: &Core) -> AppResult<State> {
    t7_install_with(core, &Net::production(), &mut no_progress)
}

/// Legacy `install_t7_patch`. On Windows without administrator rights the application re-launches
/// itself elevated (hidden CLI command, see [`crate::cli`]) and waits for it; elsewhere the install
/// runs in-process. `progress` receives download progress (not available for the elevated child).
pub fn t7_install_with(core: &Core, net: &Net, progress: ProgressFn<'_>) -> AppResult<State> {
    let _op = core.lock_ops();
    let game_dir = core.game_dir().ok_or_else(|| AppError::new("Game directory is not set."))?;
    let result = if crate::paths::system_name() == "Windows" && !crate::elevate::is_admin() {
        install_elevated(core, &game_dir)
    } else {
        crate::t7_install::install_t7_patch(core, net, Path::new(&game_dir), progress)
    };
    match result {
        Ok(()) => Ok(State::build(core)),
        Err(e) => {
            core.log.error(format!("T7 Patch install failed: {e}"));
            Err(AppError::new(e))
        }
    }
}

/// Legacy `_install_t7_patch_elevated`.
fn install_elevated(core: &Core, game_dir: &str) -> Result<(), String> {
    core.log.info("Requesting Windows administrator approval...");
    let code = crate::elevate::run_elevated_and_wait(&[crate::cli::INSTALL_T7_FLAG.to_string(), game_dir.to_string()]).map_err(|e| e.to_string())?;
    if code != 0 {
        return Err(format!("Elevated T7 Patch install failed with code {code}."));
    }
    core.log.success("T7 Patch install completed with administrator approval.");
    Ok(())
}

// ---- t7-config ------------------------------------------------------------------------------

/// Edits applied by one `update_t7patch_conf` call (legacy keyword arguments; `None` = leave alone).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfEdit {
    /// Full `playername` value, colour code included.
    pub new_name: Option<String>,
    /// Empty string clears the password.
    pub new_password: Option<String>,
    pub friends_only: Option<bool>,
}

/// Legacy `set_t7_config`: fields are applied in the order gamertag, network password, friends-only;
/// each needs `t7patch.conf` to exist; an earlier success is kept when a later field fails.
pub fn set_t7_config(
    core: &Core,
    gamertag: Option<&str>,
    color_code: &str,
    network_password: Option<&str>,
    friends_only: Option<bool>,
) -> AppResult<State> {
    let _op = core.lock_ops();
    let game_dir = core.game_dir().ok_or_else(|| AppError::new("Game directory is not set."))?;
    let game_dir = Path::new(&game_dir);
    let conf = game_dir.join("t7patch.conf");

    if let Some(name) = gamertag {
        let plain = name.trim();
        if !conf.exists() {
            return Err(AppError::new("t7patch.conf was not found. Install T7 Patch before updating the gamertag."));
        }
        if plain.is_empty() {
            return Err(AppError::new("Gamertag cannot be empty."));
        }
        if plain.chars().count() > 20 {
            return Err(AppError::new("Gamertag cannot exceed 20 characters."));
        }
        let edit = ConfEdit { new_name: Some(format!("{color_code}{plain}")), ..ConfEdit::default() };
        update_t7patch_conf(core, game_dir, &edit);
    }

    if let Some(password) = network_password {
        let password = password.trim();
        if !conf.exists() {
            return Err(AppError::new("t7patch.conf was not found. Install T7 Patch before changing the network password."));
        }
        let edit = ConfEdit { new_password: Some(password.to_string()), ..ConfEdit::default() };
        update_t7patch_conf(core, game_dir, &edit);
    }

    if let Some(on) = friends_only {
        if !conf.exists() {
            return Err(AppError::new("t7patch.conf was not found. Install T7 Patch before changing Friends Only mode."));
        }
        let edit = ConfEdit { friends_only: Some(on), ..ConfEdit::default() };
        update_t7patch_conf(core, game_dir, &edit);
    }

    Ok(State::build(core))
}

/// Legacy `update_t7patch_conf`. Never fails: problems are logged (as the legacy code did) and
/// the caller carries on.
pub fn update_t7patch_conf(core: &Core, game_dir: &Path, edit: &ConfEdit) {
    let _op = core.lock_ops();
    let conf = game_dir.join("t7patch.conf");
    if !conf.exists() {
        core.log.warning(format!("t7patch.conf not found in {}.", game_dir.display()));
        return;
    }
    let text = match read_conf_text(&conf) {
        Ok(t) => t,
        Err(msg) => {
            core.log.error(msg);
            return;
        }
    };
    if let Err(e) = std::fs::write(&conf, edit_conf(&text, edit)) {
        core.log.error(conf_error_text(&e, &conf));
        return;
    }
    if let Some(name) = &edit.new_name {
        core.log.success(format!("Updated 'playername' in t7patch.conf to '{name}'."));
    }
    if let Some(password) = &edit.new_password {
        if password.is_empty() {
            core.log.success("Cleared network password in t7patch.conf.");
        } else {
            core.log.success("Updated network password in t7patch.conf.");
        }
    }
    if let Some(on) = edit.friends_only {
        core.log.success(format!("Updated 'isfriendsonly' in t7patch.conf to {}.", if on { "On" } else { "Off" }));
    }
}

/// Legacy `open()` failure text: `PermissionError` gets its own message, everything else `Error updating config`.
fn conf_error_text(e: &std::io::Error, conf: &Path) -> String {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        format!("Cannot modify {}. Run as administrator.", conf.display())
    } else {
        format!("Error updating config: {}", io_error_text(e, Some(conf)))
    }
}

/// Read `t7patch.conf` the way Python's text mode does: UTF-8, universal newlines (`\r\n` and lone
/// `\r` become `\n`).
fn read_conf_text(conf: &Path) -> Result<String, String> {
    let bytes = std::fs::read(conf).map_err(|e| conf_error_text(&e, conf))?;
    let text = String::from_utf8(bytes).map_err(|e| format!("Error updating config: {}", utf8_error_text(e.as_bytes(), e.utf8_error())))?;
    Ok(text.replace("\r\n", "\n").replace('\r', "\n"))
}

/// Python's `UnicodeDecodeError` text for the common cases.
fn utf8_error_text(bytes: &[u8], err: std::str::Utf8Error) -> String {
    let start = err.valid_up_to();
    let lead = bytes[start];
    if err.error_len().is_none() {
        let end = bytes.len() - 1;
        if end == start {
            return format!("'utf-8' codec can't decode byte 0x{lead:02x} in position {start}: unexpected end of data");
        }
        return format!("'utf-8' codec can't decode bytes in position {start}-{end}: unexpected end of data");
    }
    let what = if (0xC2..=0xF4).contains(&lead) { "invalid continuation byte" } else { "invalid start byte" };
    format!("'utf-8' codec can't decode byte 0x{lead:02x} in position {start}: {what}")
}

/// The line-level edit of legacy `update_t7patch_conf`. `text` must already use `\n` line ends.
/// Matching is on the raw line start (`playername=` etc.); missing keys are inserted at the top in
/// the order name, password, friends-only (each `insert(0, ...)`, so the friends line ends up first).
pub fn edit_conf(text: &str, edit: &ConfEdit) -> String {
    let mut name_found = false;
    let mut password_found = false;
    let mut friends_found = false;
    let mut lines: Vec<String> = Vec::new();
    for line in text.split_inclusive('\n') {
        if let Some(name) = edit.new_name.as_deref().filter(|_| line.starts_with("playername=")) {
            lines.push(format!("playername={name}\n"));
            name_found = true;
        } else if let Some(pw) = edit.new_password.as_deref().filter(|_| line.starts_with("networkpassword=")) {
            lines.push(format!("networkpassword={pw}\n"));
            password_found = true;
        } else if let Some(on) = edit.friends_only.filter(|_| line.starts_with("isfriendsonly=")) {
            lines.push(format!("isfriendsonly={}\n", u8::from(on)));
            friends_found = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if let Some(name) = &edit.new_name {
        if !name_found {
            lines.insert(0, format!("playername={name}\n"));
        }
    }
    if let Some(pw) = &edit.new_password {
        if !password_found {
            lines.insert(0, format!("networkpassword={pw}\n"));
        }
    }
    if let Some(on) = edit.friends_only {
        if !friends_found {
            lines.insert(0, format!("isfriendsonly={}\n", u8::from(on)));
        }
    }
    lines.concat()
}

// ---- t7-uninstall ---------------------------------------------------------------------------

/// Legacy `uninstall_t7_patch`: the op wrapper (`t7-uninstall`) logs failures; the error text is
/// the legacy `str(exc)`.
fn t7_uninstall(core: &Core) -> AppResult<State> {
    let game_dir = core.game_dir().ok_or_else(|| AppError::new("Game directory is not set."))?;
    if let Err(e) = uninstall(core, Path::new(&game_dir)) {
        core.log.error(format!("T7 Patch uninstall failed: {}", e.message));
        return Err(e);
    }
    Ok(State::build(core))
}

/// Legacy `_uninstall_t7_patch`: remove the T7 files from `game_dir`, the cached mod files, and
/// restore the LPC backups. Returns the first I/O failure as an [`AppError`] (already in the legacy
/// text); success messages are logged as they happen. Shared with the reset-to-stock flow, which
/// decides itself how to log a failure.
pub fn uninstall(core: &Core, game_dir: &Path) -> AppResult<()> {
    let _op = core.lock_ops();
    let io_fail = |e: std::io::Error, p: &Path| AppError::new(io_error_text(&e, Some(p)));

    let mut removed_game = false;
    for name in T7_GAME_FILES {
        let target = game_dir.join(name);
        if target.exists() {
            std::fs::remove_file(&target).map_err(|e| io_fail(e, &target))?;
            removed_game = true;
        }
    }
    if removed_game {
        core.log.success("Uninstalled T7 Patch files from the game directory.");
    }

    let mod_files = &core.paths.mod_files;
    let mut removed_mod = false;
    for cache in [mod_files.join("linux"), mod_files.join("Linux.Steamdeck.and.Manual.Windows.Install"), mod_files.join("T7Patch_extracted")] {
        if cache.exists() {
            std::fs::remove_dir_all(&cache).map_err(|e| io_fail(e, &cache))?;
            removed_mod = true;
        }
    }
    let zip = mod_files.join("T7Patch.zip");
    if zip.exists() {
        std::fs::remove_file(&zip).map_err(|e| io_fail(e, &zip))?;
        removed_mod = true;
    }
    if removed_mod {
        core.log.success("Removed cached T7 Patch files.");
    }

    restore_lpc_backups(core, game_dir);
    core.log.success("T7 Patch has been completely uninstalled.");
    Ok(())
}

/// Legacy `restore_lpc_backups`: `<name>.patchops.bak` wins over `<name>.bak`; each backup replaces
/// its `<name>` in `LPC/`. Errors are logged, never returned (as in the legacy code).
pub fn restore_lpc_backups(core: &Core, game_dir: &Path) {
    let _op = core.lock_ops();
    let lpc = game_dir.join("LPC");
    if !lpc.exists() {
        return;
    }
    match restore_lpc_inner(&lpc) {
        Ok(0) => {}
        Ok(n) => core.log.success(format!("Restored {n} LPC backup files")),
        Err(msg) => core.log.error(format!("Error restoring LPC backups: {msg}")),
    }
}

const PATCHOPS_BAK: &str = ".patchops.bak";
const LEGACY_BAK: &str = ".bak";

fn restore_lpc_inner(lpc: &Path) -> Result<usize, String> {
    let entries = std::fs::read_dir(lpc).map_err(|e| io_error_text(&e, Some(lpc)))?;
    let mut selected: std::collections::BTreeMap<String, PathBuf> = std::collections::BTreeMap::new();
    let mut legacy: Vec<(String, PathBuf)> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| io_error_text(&e, Some(lpc)))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if let Some(base) = name.strip_suffix(PATCHOPS_BAK) {
            selected.insert(base.to_string(), path);
        } else if let Some(base) = name.strip_suffix(LEGACY_BAK) {
            legacy.push((base.to_string(), path));
        }
    }
    for (base, path) in legacy {
        selected.entry(base).or_insert(path);
    }

    let mut restored = 0;
    for (base, src) in selected {
        let dst = lpc.join(&base);
        if dst.exists() {
            std::fs::remove_file(&dst).map_err(|e| io_error_text(&e, Some(&dst)))?;
        }
        std::fs::rename(&src, &dst).map_err(|e| {
            format!("{}: {} -> {}", io_error_text(&e, None), repr_str(&src.to_string_lossy()), repr_str(&dst.to_string_lossy()))
        })?;
        restored += 1;
    }
    Ok(restored)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct T7State {
    pub installed: bool,
    pub conf_exists: bool,
    pub gamertag: String,
    pub plain_name: String,
    pub color_code: String,
    pub network_password: String,
    pub friends_only: bool,
    /// `Unknown|Default|Custom|Enhanced`
    pub mode: String,
}

/// Markers whose presence means T7 Patch is installed (legacy `T7_INSTALL_MARKERS`).
pub const T7_INSTALL_MARKERS: [&str; 2] = ["t7patchloader.dll", "t7patch.dll"];

/// Legacy `check_t7_patch_status` + `_t7_status` + `_t7_mode` (state side; complete).
/// `mode` relies on `enhanced::detect_enhanced_install` (complete) and the exe hash.
pub fn t7_state(cx: &StateCx) -> T7State {
    let mut st = T7State {
        installed: false,
        conf_exists: false,
        gamertag: String::new(),
        plain_name: String::new(),
        color_code: String::new(),
        network_password: String::new(),
        friends_only: false,
        mode: "Unknown".into(),
    };
    let Some(game) = cx.game_dir.as_deref().map(std::path::Path::new) else { return st };
    st.installed = game.is_dir() && T7_INSTALL_MARKERS.iter().any(|m| game.join(m).exists());
    let conf = game.join("t7patch.conf");
    st.conf_exists = conf.exists();
    if let Ok(bytes) = std::fs::read(&conf) {
        for line in String::from_utf8_lossy(&bytes).split_inclusive('\n') {
            let value = |l: &str| l.trim().split_once('=').map(|(_, v)| v.to_string()).unwrap_or_default();
            if line.starts_with("playername=") {
                st.gamertag = value(line);
                if st.gamertag.starts_with('^') && st.gamertag.chars().count() > 2 {
                    st.color_code = st.gamertag.chars().take(2).collect();
                    st.plain_name = st.gamertag.chars().skip(2).collect();
                } else {
                    st.plain_name = st.gamertag.clone();
                }
            } else if line.starts_with("networkpassword=") {
                st.network_password = value(line);
            } else if line.starts_with("isfriendsonly=") {
                st.friends_only = value(line) == "1";
            }
        }
    }
    st.mode = t7_mode(game);
    st
}

fn t7_mode(game: &std::path::Path) -> String {
    if super::enhanced::detect_enhanced_install(game) {
        return "Enhanced".into();
    }
    let exe = crate::game::find_executable(game);
    // Same signature-validated cache the `exeSwap` state builder uses, so one `status` hashes the
    // executable at most once (and an unchanged exe costs one `stat`).
    let hash = exe.as_deref().and_then(crate::enhanced::hashcache::file_sha256_cached);
    if hash.as_deref() == Some(super::enhanced::DEFAULT_STEAM_EXE_SHA256) || crate::game::read_exe_variant(game).as_deref() == Some("default") {
        "Default".into()
    } else if exe.is_some() {
        "Custom".into()
    } else {
        "Unknown".into()
    }
}

/// `state.mods.t7Patch`: `t7patch.exe` or `t7patch.dll` exists.
pub fn installed_marker(cx: &StateCx) -> bool {
    cx.game_dir.as_deref().is_some_and(|g| {
        let g = std::path::Path::new(g);
        g.join("t7patch.exe").exists() || g.join("t7patch.dll").exists()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(s: &str) -> ConfEdit {
        ConfEdit { new_name: Some(s.into()), ..ConfEdit::default() }
    }

    #[test]
    fn replaces_matching_lines_and_keeps_others() {
        let text = "playername=old\nnetworkpassword=\nisfriendsonly=0\nunrelated=keep\n";
        assert_eq!(edit_conf(text, &name("^5Bench")), "playername=^5Bench\nnetworkpassword=\nisfriendsonly=0\nunrelated=keep\n");
        let pw = ConfEdit { new_password: Some("".into()), ..ConfEdit::default() };
        assert_eq!(edit_conf(text, &pw), "playername=old\nnetworkpassword=\nisfriendsonly=0\nunrelated=keep\n");
        let fr = ConfEdit { friends_only: Some(true), ..ConfEdit::default() };
        assert_eq!(edit_conf(text, &fr), "playername=old\nnetworkpassword=\nisfriendsonly=1\nunrelated=keep\n");
    }

    #[test]
    fn missing_keys_are_inserted_at_top_in_legacy_order() {
        let all = ConfEdit { new_name: Some("N".into()), new_password: Some("pw".into()), friends_only: Some(false) };
        assert_eq!(edit_conf("x=1\n", &all), "isfriendsonly=0\nnetworkpassword=pw\nplayername=N\nx=1\n");
        assert_eq!(edit_conf("x=1\n", &name("N")), "playername=N\nx=1\n");
    }

    #[test]
    fn matching_is_on_raw_line_start_and_last_line_may_lack_newline() {
        assert_eq!(edit_conf(" playername=a\n", &name("N")), "playername=N\n playername=a\n");
        assert_eq!(edit_conf("playername=a", &name("N")), "playername=N\n");
    }

    #[test]
    fn utf8_error_texts_match_python() {
        let bad = vec![b'a', 0xFF];
        let e = std::str::from_utf8(&bad).unwrap_err();
        assert_eq!(utf8_error_text(&bad, e), "'utf-8' codec can't decode byte 0xff in position 1: invalid start byte");
        let cont = vec![0xE2, b'A'];
        let e = std::str::from_utf8(&cont).unwrap_err();
        assert_eq!(utf8_error_text(&cont, e), "'utf-8' codec can't decode byte 0xe2 in position 0: invalid continuation byte");
    }
}
