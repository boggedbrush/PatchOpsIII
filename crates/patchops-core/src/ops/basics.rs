//! Group `basics`: `health`, `status`, `browse`, `game-directory`, `release-channel`,
//! `logs/payload`, `logs/clear`, `mod-files/clear`. Fully ported.
//!
//! Legacy reference: `backend/api.py` (`health`, `status`, `_browse_payload`,
//! `set_game_directory`, `release_channel`, `_log_payload`, `logs_clear`, `_clear_mod_files`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{state_reply, to_json, Args, Group};
use crate::context::Core;
use crate::error::{AppError, AppResult, OpError};
use crate::game::has_game_executable;
use crate::pyfmt::{expanduser, io_error_text, normpath, path_str, resolve};
use crate::state::{State, StateCx};

pub const OPS: &[&str] = &[
    "health",
    "status",
    "browse",
    "game-directory",
    "release-channel",
    "logs/payload",
    "logs/clear",
    "mod-files/clear",
];

pub const GROUP: Group = Group { name: "basics", ops: OPS, call };

fn call(core: &Core, op: &str, a: &Args) -> Result<Value, OpError> {
    match op {
        "health" => to_json(&health(core)),
        "status" => to_json(&status(core)),
        "browse" => {
            let path = a.str_opt("path")?;
            to_json(&browse(core, path.as_deref()))
        }
        "game-directory" => {
            let path = a.str_req("path", 1)?;
            state_reply(set_game_directory(core, &path))
        }
        "release-channel" => {
            let channel = a.str_req("channel", 0)?;
            state_reply(set_release_channel(core, &channel))
        }
        "logs/payload" => to_json(&LogsPayload { ok: true, payload: log_payload(core) }),
        "logs/clear" => state_reply(clear_logs(core)),
        "mod-files/clear" => state_reply(clear_mod_files(core)),
        _ => Err(OpError::UnknownOp(op.to_string())),
    }
}

// ---- health / status ------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Health {
    pub ok: bool,
    pub version: String,
}

pub fn health(core: &Core) -> Health {
    Health { ok: true, version: core.version.clone() }
}

/// The full application state (`status`).
pub fn status(core: &Core) -> State {
    State::build(core)
}

// ---- maintenance state ----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaintenanceState {
    pub mod_files_dir: String,
    pub log_payload: LogPayload,
}

pub fn maintenance_state(cx: &StateCx) -> MaintenanceState {
    MaintenanceState { mod_files_dir: cx.core.paths.mod_files.to_string_lossy().into_owned(), log_payload: LogPayload::lazy(cx.core) }
}

/// `maintenance.logPayload`: computed on demand. Building a `State` only captures a handle; the
/// text is produced (from the logger's validated cache) when something serialises it or calls
/// [`LogPayload::get`]. The GUI never does, so its refreshes and action replies never touch the
/// log; the JSON/RPC path serialises it and gets the same string as before.
#[derive(Debug, Clone)]
pub struct LogPayload(Source);

#[derive(Clone)]
enum Source {
    Ready(Arc<str>),
    Lazy { file: Arc<crate::log::FileLog>, version: String },
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::Ready(s) => write!(f, "Ready({} bytes)", s.len()),
            Source::Lazy { .. } => f.write_str("Lazy"),
        }
    }
}

impl LogPayload {
    pub fn lazy(core: &Core) -> LogPayload {
        LogPayload(Source::Lazy { file: core.log.file(), version: core.version.clone() })
    }

    pub fn ready(text: impl Into<Arc<str>>) -> LogPayload {
        LogPayload(Source::Ready(text.into()))
    }

    /// The payload text (forces a lazy value).
    pub fn get(&self) -> Arc<str> {
        match &self.0 {
            Source::Ready(s) => s.clone(),
            Source::Lazy { file, version } => file.payload(|body| format_payload(version, body)),
        }
    }
}

impl Serialize for LogPayload {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&self.get())
    }
}

impl<'de> Deserialize<'de> for LogPayload {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        String::deserialize(de).map(LogPayload::ready)
    }
}

// ---- browse ---------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Root {
    pub label: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub has_game_executable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowsePayload {
    pub path: String,
    pub parent: Option<String>,
    pub has_game_executable: bool,
    pub roots: Vec<Root>,
    pub shortcuts: Vec<Root>,
    pub entries: Vec<DirEntry>,
}

const MAX_BROWSE_ENTRIES: usize = 300;

fn browse_roots() -> Vec<Root> {
    #[cfg(windows)]
    {
        return ('A'..='Z')
            .map(|l| format!("{l}:\\"))
            .filter(|d| Path::new(d).is_dir())
            .map(|d| Root { label: d.clone(), path: d })
            .collect();
    }
    #[cfg(not(windows))]
    vec![Root { label: "/".into(), path: "/".into() }]
}

fn browse_shortcuts(core: &Core) -> Vec<Root> {
    let mut out: Vec<Root> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let home = &core.paths.home;
    let mut add = |label: &str, path: Option<&str>| {
        let Some(p) = path.filter(|p| !p.is_empty()) else { return };
        let resolved = resolve(&expanduser(p, home));
        let resolved_s = resolved.to_string_lossy().into_owned();
        let mut key = normpath(&resolved_s);
        if cfg!(windows) {
            key = key.to_lowercase();
        }
        if seen.contains(&key) || !resolved.is_dir() {
            return;
        }
        seen.push(key);
        out.push(Root { label: label.to_string(), path: resolved_s });
    };
    let home_s = home.to_string_lossy().into_owned();
    add("Home", Some(&home_s));
    add("Desktop", Some(&home.join("Desktop").to_string_lossy()));
    add("Downloads", Some(&home.join("Downloads").to_string_lossy()));
    let settings = core.settings();
    add("Saved game folder", settings.get_str("game_dir"));
    let detected = core.game_dir();
    add("Detected BO3 folder", detected.as_deref());
    for (i, lib) in crate::steam::library_paths(core).iter().enumerate() {
        add(&format!("Steam Library {}", i + 1), Some(lib));
    }
    out
}

fn directory_entries(dir: &Path) -> Vec<DirEntry> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut entries: Vec<DirEntry> = rd
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if !p.is_dir() {
                return None;
            }
            Some(DirEntry {
                name: e.file_name().to_string_lossy().into_owned(),
                has_game_executable: has_game_executable(&p),
                path: p.to_string_lossy().into_owned(),
            })
        })
        .collect();
    entries.sort_by_cached_key(|e| (!e.has_game_executable, e.name.to_lowercase()));
    entries.truncate(MAX_BROWSE_ENTRIES);
    entries
}

/// Directory listing for the folder picker. `path` defaults to the saved game dir, then `$HOME`;
/// missing / non-directory paths fall back to `$HOME`.
pub fn browse(core: &Core, path: Option<&str>) -> BrowsePayload {
    let home = &core.paths.home;
    let settings = core.settings();
    let selected: String = path
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .or_else(|| settings.game_dir().map(str::to_string))
        .unwrap_or_else(|| home.to_string_lossy().into_owned());
    let mut current = expanduser(&selected, home);
    if !current.is_dir() {
        current = home.clone();
    }
    let current = resolve(&current);
    let parent = current.parent().filter(|p| !p.as_os_str().is_empty() && *p != current).map(|p| p.to_string_lossy().into_owned());
    BrowsePayload {
        path: current.to_string_lossy().into_owned(),
        parent,
        has_game_executable: has_game_executable(&current),
        roots: browse_roots(),
        shortcuts: browse_shortcuts(core),
        entries: directory_entries(&current),
    }
}

// ---- game directory / release channel -------------------------------------------------------

/// Save `path` as the game directory if it contains a game executable.
pub fn set_game_directory(core: &Core, path: &str) -> AppResult<State> {
    let _op = core.lock_ops();
    if !has_game_executable(Path::new(path)) {
        core.log.error(format!("Selected directory is not a Black Ops III install: {path}"));
        return Err(AppError::new("BlackOps3.exe or BlackOpsIII.exe was not found."));
    }
    core.update_settings(|s| s.set_str("game_dir", path_str(path)))
        .map_err(|e| AppError::new(io_error_text(&e, Some(&core.paths.settings))))?;
    core.log.success(format!("Game directory set to {path}"));
    Ok(State::build(core))
}

/// `channel` is trimmed + lower-cased; must be `stable` or `beta`.
pub fn set_release_channel(core: &Core, channel: &str) -> AppResult<State> {
    let _op = core.lock_ops();
    let channel = channel.trim().to_lowercase();
    if channel != "stable" && channel != "beta" {
        return Err(AppError::new("Release channel must be beta or stable."));
    }
    core.update_settings(|s| s.set_str("release_channel", channel.clone()))
        .map_err(|e| AppError::new(io_error_text(&e, Some(&core.paths.settings))))?;
    let title = if channel == "beta" { "Beta" } else { "Stable" };
    core.log.success(format!("Release channel set to {title}."));
    Ok(State::build(core))
}

// ---- logs / mod files -----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogsPayload {
    pub ok: bool,
    pub payload: String,
}

/// Markdown block users paste into bug reports (cached; see [`crate::log::FileLog::payload`]).
pub fn log_payload_arc(core: &Core) -> Arc<str> {
    let version = core.version.clone();
    core.log.file().payload(|body| format_payload(&version, body))
}

pub fn log_payload(core: &Core) -> String {
    log_payload_arc(core).to_string()
}

fn format_payload(version: &str, body: &str) -> String {
    static LABEL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let label = LABEL.get_or_init(|| {
        let (release, machine) = crate::clock::uname();
        format!("{} {} ({})", crate::paths::system_name(), release, machine).trim().to_string()
    });
    let body = body.trim();
    format!(
        "PatchOpsIII {} - {} logs:\n```\n{}\n```",
        version,
        label,
        if body.is_empty() { "(no log entries found)" } else { body }
    )
}

/// Truncate the log file and the in-memory ring, then log `Logs cleared.`.
pub fn clear_logs(core: &Core) -> AppResult<State> {
    let _op = core.lock_ops();
    if !core.log.clear_file() {
        return Err(AppError::new("Failed to clear log file."));
    }
    core.log.clear_ring();
    core.log.success("Logs cleared.");
    Ok(State::build(core))
}

fn wipe_dir(dir: &Path) -> Result<(), (std::io::Error, PathBuf)> {
    std::fs::create_dir_all(dir).map_err(|e| (e, dir.to_path_buf()))?;
    for entry in std::fs::read_dir(dir).map_err(|e| (e, dir.to_path_buf()))? {
        let entry = entry.map_err(|e| (e, dir.to_path_buf()))?;
        let p = entry.path();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let r = if is_dir { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
        r.map_err(|e| (e, p))?;
    }
    Ok(())
}

/// Create `BO3 Mod Files` if needed and delete everything inside it.
pub fn clear_mod_files(core: &Core) -> AppResult<State> {
    let _op = core.lock_ops();
    let dir = &core.paths.mod_files;
    if let Err((e, p)) = wipe_dir(dir) {
        let msg = io_error_text(&e, Some(&p));
        core.log.error(format!("Failed to clear mod files: {msg}"));
        return Err(AppError::new(msg));
    }
    core.log.success(format!("Cleared mod files in {}", dir.display()));
    Ok(State::build(core))
}


#[cfg(test)]
mod log_payload_tests {
    use super::*;
    use crate::paths::Paths;
    use serde_json::json;

    fn core(tag: &str) -> (Core, PathBuf) {
        let root = std::env::temp_dir().join(format!("patchops-basics-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        (Core::with_paths(Paths::with_app_data(home, root.join("appdata")), "test".into()), root)
    }

    #[test]
    fn maintenance_json_is_unchanged_for_small_logs() {
        let (c, root) = core("json");
        c.log.info("hello");
        c.log.warning("wörld");
        let (release, machine) = crate::clock::uname();
        let label = format!("{} {} ({})", crate::paths::system_name(), release, machine);
        let m = State::build(&c).maintenance; // building the state itself may log
        let body = std::fs::read_to_string(&c.paths.log).unwrap();
        // The pre-change formatting, spelled out.
        let expected = format!("PatchOpsIII test - {} logs:\n```\n{}\n```", label.trim(), body.trim());
        let got = serde_json::to_string(&m).unwrap();
        let want = serde_json::to_string(&json!({"modFilesDir": c.paths.mod_files.to_string_lossy(), "logPayload": expected})).unwrap();
        assert_eq!(got, want);
        assert_eq!(&*m.log_payload.get(), expected.as_str());
        assert_eq!(log_payload(&c), expected);
        // empty log
        assert!(c.log.clear_file());
        assert!(log_payload(&c).contains("```\n(no log entries found)\n```"));
        let back: MaintenanceState = serde_json::from_str(&got).unwrap();
        assert_eq!(&*back.log_payload.get(), expected.as_str());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn lazy_payload_is_not_forced_by_building_state() {
        let (c, root) = core("lazy");
        let m = maintenance_state(&StateCx { core: &c, game_dir: None, config: String::new(), launch_options: None });
        c.log.info("logged after the state was built");
        assert!(m.log_payload.get().contains("logged after the state was built"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn payload_op_on_over_cap_file_is_bounded() {
        let (c, root) = core("cap");
        let line = format!("2024-01-01 00:00:00 - Info: {}\n", "y".repeat(100));
        std::fs::create_dir_all(c.paths.log.parent().unwrap()).unwrap();
        std::fs::write(&c.paths.log, line.repeat(40_000)).unwrap(); // ~4.8 MB, written behind the logger's back
        let p = super::super::dispatch(&c, "logs/payload", &Value::Null).unwrap();
        let text = p["payload"].as_str().unwrap();
        assert!(text.len() < 600 * 1024, "{}", text.len());
        assert!(text.ends_with("yyyy\n```"));
        assert!(c.paths.log.with_extension("log.1").exists());
        let _ = std::fs::remove_dir_all(root);
    }
}
