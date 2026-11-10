//! Steam client discovery shared by several op groups: install roots, library folders,
//! the active user id, workshop item state and launch-option *reads*.
//!
//! Writing (`localconfig.vdf` merges, closing/starting the client) lives in `launch_options` and `client`.
//! Legacy reference: `utils.py` (`get_steam_paths`, `_candidate_steam_roots`,
//! `get_steam_library_paths`, `find_steam_user_id`, `get_workshop_item_state`, `_read_launch_options`).

pub mod client;
pub mod launch_options;
pub mod vdf;

// Shared Steam control helpers (used by ops::steam_ops, enhanced, maintenance).
pub use client::{close_steam, launch_game, open_external_url, open_steam};
pub use launch_options::{
    apply_launch_options, apply_launch_options_exact, clear_launch_options, merge_launch_options, modify_vdf_file,
    set_launch_options, set_launch_options_exact, steam_config_vdf_path,
};

use std::path::{Path, PathBuf};

use crate::context::Core;
use crate::pyfmt::{abspath, normpath};

pub const BO3_APP_ID: &str = "311210";

/// `<steam root>/userdata` as the legacy code derives it (does not need to exist).
pub fn userdata_dir(core: &Core) -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(root) = windows_steam_root() {
            return root.join("userdata");
        }
        let base = std::env::var_os("PROGRAMFILES(X86)")
            .or_else(|| std::env::var_os("PROGRAMFILES"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"));
        base.join("Steam").join("userdata")
    }
    #[cfg(target_os = "macos")]
    {
        core.paths.home.join("Library/Application Support/Steam/userdata")
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        core.paths.home.join(".steam/steam/userdata")
    }
}

#[cfg(windows)]
fn windows_steam_root() -> Option<PathBuf> {
    // TODO(windows): the legacy code first consults the registry (HKCU\Software\Valve\Steam
    // SteamPath, HKLM\...\InstallPath). That needs winreg access; fall back to Program Files.
    let mut candidates: Vec<PathBuf> = Vec::new();
    for var in ["PROGRAMFILES(X86)", "PROGRAMFILES"] {
        if let Some(v) = std::env::var_os(var) {
            candidates.push(PathBuf::from(v).join("Steam"));
        }
    }
    candidates.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
    candidates.push(PathBuf::from(r"C:\Program Files\Steam"));
    candidates.into_iter().find(|c| c.join("steam.exe").exists())
}

/// Keep existing directories, de-duplicated by normalised absolute path (legacy `_dedupe_existing_dirs`).
fn dedupe_existing_dirs<I: IntoIterator<Item = String>>(paths: I) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for p in paths {
        if p.is_empty() {
            continue;
        }
        let mut key = abspath(&p);
        if cfg!(windows) {
            key = key.to_lowercase();
        }
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        if Path::new(&p).is_dir() {
            out.push(normpath(&p));
        }
    }
    out
}

/// Steam installation roots that exist (legacy `_candidate_steam_roots`).
pub fn candidate_roots(core: &Core) -> Vec<String> {
    let mut roots: Vec<String> = Vec::new();
    if let Some(parent) = userdata_dir(core).parent() {
        roots.push(parent.to_string_lossy().into_owned());
    }
    let home = &core.paths.home;
    if cfg!(target_os = "macos") {
        roots.push(home.join("Library/Application Support/Steam").to_string_lossy().into_owned());
    } else if !cfg!(windows) {
        roots.push(home.join(".steam").join("steam").to_string_lossy().into_owned());
        roots.push(home.join(".local").join("share").join("Steam").to_string_lossy().into_owned());
    }
    #[cfg(windows)]
    if let Some(r) = windows_steam_root() {
        roots.push(r.to_string_lossy().into_owned());
    }
    dedupe_existing_dirs(roots)
}

/// Legacy `get_steam_root_path`: dirname of userdata if it is a directory, else the first candidate.
pub fn root_path(core: &Core) -> Option<String> {
    if let Some(parent) = userdata_dir(core).parent() {
        if parent.is_dir() {
            return Some(normpath(&parent.to_string_lossy()));
        }
    }
    candidate_roots(core).into_iter().next()
}

fn library_paths_from_vdf(file: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read(file) else { return Vec::new() };
    let Ok(doc) = vdf::parse(&String::from_utf8_lossy(&text)) else { return Vec::new() };
    let Some(folders) = doc.get_obj("libraryfolders") else { return Vec::new() };
    let mut libs = Vec::new();
    for (k, v) in &folders.0 {
        if k == "contentstatsid" {
            continue;
        }
        match v {
            vdf::Node::Str(s) => libs.push(s.clone()),
            vdf::Node::Obj(o) => {
                if let Some(p) = o.get_str("path") {
                    libs.push(p.to_string());
                }
            }
        }
    }
    dedupe_existing_dirs(libs)
}

/// Steam library folders (roots first, then every `path` in `libraryfolders.vdf`), existing and
/// de-duplicated. Cached until a `libraryfolders.vdf` mtime changes.
pub fn library_paths(core: &Core) -> Vec<String> {
    let roots = candidate_roots(core);
    let key: Vec<(String, Option<std::time::SystemTime>)> = roots
        .iter()
        .map(|r| {
            let m = std::fs::metadata(Path::new(r).join("steamapps").join("libraryfolders.vdf")).and_then(|m| m.modified()).ok();
            (r.clone(), m)
        })
        .collect();
    if let Some((k, v)) = core.steam_cache.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        if *k == key {
            return v.clone();
        }
    }
    let mut libs: Vec<String> = Vec::new();
    for root in &roots {
        libs.push(root.clone());
        libs.extend(library_paths_from_vdf(&Path::new(root).join("steamapps").join("libraryfolders.vdf")));
    }
    let resolved = dedupe_existing_dirs(libs);
    *core.steam_cache.lock().unwrap_or_else(|e| e.into_inner()) = Some((key, resolved.clone()));
    resolved
}

/// Legacy `find_steam_user_id`: first all-digit directory in `userdata` (directory order).
/// Logs file-only warnings like the legacy code when nothing is found.
pub fn find_user_id(core: &Core) -> Option<String> {
    let dir = userdata_dir(core);
    if !dir.exists() {
        core.log.log_file_only(crate::log::Category::Warning, "Steam userdata path not found!");
        return None;
    }
    let found = std::fs::read_dir(&dir).ok().and_then(|rd| {
        rd.filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .find(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
    });
    if found.is_none() {
        core.log.log_file_only(crate::log::Category::Warning, "No Steam user ID found!");
    }
    found
}

/// `<userdata>/<user>/config/localconfig.vdf`
pub fn localconfig_path(core: &Core, user_id: &str) -> PathBuf {
    userdata_dir(core).join(user_id).join("config").join("localconfig.vdf")
}

/// Legacy `_read_launch_options`: `None` when the file is missing/unparsable, `Some("")` when
/// the app has no `LaunchOptions` entry.
pub fn read_launch_options(core: &Core, user_id: &str, app_id: &str) -> Option<String> {
    let bytes = std::fs::read(localconfig_path(core, user_id)).ok()?;
    let doc = vdf::parse(&String::from_utf8(bytes).ok()?).ok()?;
    let v = doc
        .get_path(&["UserLocalConfigStore", "Software", "Valve", "Steam", "apps", app_id, "LaunchOptions"])
        .and_then(|n| match n {
            vdf::Node::Str(s) => Some(s.clone()),
            _ => None,
        });
    Some(v.unwrap_or_default())
}

/// Workshop item state in the Steam libraries (legacy `get_workshop_item_state`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkshopState {
    pub state: &'static str,
    pub installed: bool,
    pub subscribed: bool,
    pub path: Option<String>,
}

fn contains_workshop_id(o: &vdf::Obj, id: &str) -> bool {
    o.0.iter().any(|(k, v)| {
        k == id
            || match v {
                vdf::Node::Str(s) => s == id,
                vdf::Node::Obj(inner) => contains_workshop_id(inner, id),
            }
    })
}

pub fn workshop_item_state(core: &Core, app_id: &str, workshop_id: &str) -> WorkshopState {
    let mut subscribed = false;
    for lib in library_paths(core) {
        let install = Path::new(&lib).join("steamapps").join("workshop").join("content").join(app_id).join(workshop_id);
        if install.is_dir() && std::fs::read_dir(&install).map(|mut d| d.next().is_some()).unwrap_or(false) {
            return WorkshopState {
                state: "Installed",
                installed: true,
                subscribed: true,
                path: Some(install.to_string_lossy().into_owned()),
            };
        }
        let acf = Path::new(&lib).join("steamapps").join("workshop").join(format!("appworkshop_{app_id}.acf"));
        if let Ok(bytes) = std::fs::read(&acf) {
            if let Ok(doc) = vdf::parse(&String::from_utf8_lossy(&bytes)) {
                if let Some(vdf::Node::Obj(w)) = doc.get("AppWorkshop") {
                    if contains_workshop_id(w, workshop_id) {
                        subscribed = true;
                    }
                }
            }
        }
    }
    if subscribed {
        WorkshopState { state: "Subscribed (not installed yet)", installed: false, subscribed: true, path: None }
    } else {
        WorkshopState { state: "Not Subscribed", installed: false, subscribed: false, path: None }
    }
}
