//! Linux (Proton) side of BO3 Enhanced: compatibility-tool install/removal, `CompatToolMapping`
//! in Steam's `config/config.vdf`, and launch-option bookkeeping (legacy `utils.py`:
//! `configure_bo3_enhanced_linux`, `cleanup_bo3_enhanced_linux` and helpers).
//!
//! Steam process control, `config.vdf` backup/edit and `localconfig.vdf` writes come from
//! [`crate::steam`]. What is *not* here: resolving/downloading the GDK-Proton "BO3 Enhanced"
//! tool (network): pass the resolved directory to [`configure_bo3_enhanced_linux`].

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::context::Core;
use crate::pyfmt::io_error_text;
use crate::steam::launch_options::{
    backup_file, modify_vdf_file, modify_vdf_file_if, normalize, restore_file, set_launch_options_exact, steam_config_vdf_path, strip_token_ci,
};
use crate::steam::vdf::{Node, Obj};
use crate::steam::{self, BO3_APP_ID};

pub const ENHANCED_TOOL_NAME: &str = "BO3 Enhanced";
pub const ENHANCED_LAUNCH_OPTIONS: &str = "WINEDLLOVERRIDES=\"WindowsCodecs=n,b\" %command%";
const CONFIG_BACKUP: &str = "steam_config_backup.vdf";

fn is_linux() -> bool {
    crate::paths::system_name() == "Linux"
}

// ---- launch option text -----------------------------------------------------------------------

/// Legacy `_normalize_launch_options`: collapse whitespace runs, trim.
pub fn normalize_launch_options(s: &str) -> String {
    normalize(s)
}

/// Legacy `_strip_enhanced_launch_override`: drop the whitespace-delimited, case-insensitive
/// `WINEDLLOVERRIDES="WindowsCodecs=n,b"` token, then normalise whitespace.
pub fn strip_enhanced_launch_override(options: &str) -> String {
    const OVERRIDE: &str = "WINEDLLOVERRIDES=\"WindowsCodecs=n,b\"";
    normalize(&strip_token_ci(options, OVERRIDE, true))
}

// ---- previous-state snapshots (backups/*.json) -------------------------------------------------

fn state_file(core: &Core, kind: &str) -> PathBuf {
    core.paths.backups.join(format!("{kind}_{BO3_APP_ID}.json"))
}

fn save_json_atomic(core: &Core, path: &Path, payload: &Value, what: &str) -> bool {
    let r = std::fs::create_dir_all(&core.paths.backups)
        .and_then(|_| crate::fsutil::atomic_write(path, crate::pyjson::dumps_indent2(payload).as_bytes()));
    match r {
        Ok(()) => true,
        Err(e) => {
            core.log.error(format!("Failed to save {what}: {}", io_error_text(&e, Some(path))));
            false
        }
    }
}

fn load_json_object(path: &Path) -> Option<Map<String, Value>> {
    match serde_json::from_slice::<Value>(&fs::read(path).ok()?).ok()? {
        Value::Object(m) => Some(m),
        _ => None,
    }
}

fn save_previous_launch_options(core: &Core, options: &str) -> bool {
    save_json_atomic(core, &state_file(core, "launch_options"), &json!({ "launch_options": options }), "previous launch options state")
}

/// `None` when no snapshot exists (or it is unreadable).
fn load_previous_launch_options(core: &Core) -> Option<String> {
    let m = load_json_object(&state_file(core, "launch_options"))?;
    Some(m.get("launch_options").and_then(Value::as_str).unwrap_or("").to_string())
}

fn clear_previous(core: &Core, kind: &str) {
    let _ = std::fs::remove_file(state_file(core, kind));
}

fn node_to_json(n: &Node) -> Value {
    match n {
        Node::Str(s) => Value::String(s.clone()),
        Node::Obj(o) => Value::Object(o.0.iter().map(|(k, v)| (k.clone(), node_to_json(v))).collect()),
    }
}

fn json_to_node(v: &Value) -> Node {
    match v {
        Value::String(s) => Node::Str(s.clone()),
        Value::Object(m) => Node::Obj(Obj(m.iter().map(|(k, v)| (k.clone(), json_to_node(v))).collect())),
        Value::Null => Node::Str("None".into()),
        Value::Bool(b) => Node::Str(if *b { "True" } else { "False" }.into()),
        other => Node::Str(other.to_string()),
    }
}

struct PrevMapping {
    had_entry: bool,
    entry: Option<Value>,
}

fn load_previous_compat_mapping(core: &Core) -> Option<PrevMapping> {
    let m = load_json_object(&state_file(core, "compat_mapping"))?;
    let had = crate::enhanced::state_file::truthy(m.get("had_entry"));
    Some(PrevMapping { had_entry: had, entry: if had { m.get("entry").filter(|v| !v.is_null()).cloned() } else { None } })
}

// ---- config.vdf: CompatToolMapping ------------------------------------------------------------

fn compat_map(data: &mut Obj) -> &mut Obj {
    data.section_mut("InstallConfigStore").section_mut("Software").section_mut("Valve").section_mut("Steam").section_mut("CompatToolMapping")
}

fn remove_key(o: &mut Obj, key: &str) {
    o.0.retain(|(k, _)| k != key);
}

/// Resolve `config.vdf`, logging the legacy errors; `None` aborts the caller.
fn config_vdf(core: &Core) -> Option<PathBuf> {
    let Some(path) = steam_config_vdf_path(core) else {
        core.log.error("Steam root path could not be resolved.");
        return None;
    };
    if !path.exists() {
        core.log.error(format!("Steam config not found at {}", path.display()));
        return None;
    }
    if !backup_file(core, &path, CONFIG_BACKUP) {
        core.log.error("Aborting due to backup failure");
        return None;
    }
    Some(path)
}

/// Legacy `set_compatibility_tool_mapping`.
pub fn set_compatibility_tool_mapping(core: &Core, tool_name: &str) -> bool {
    if !is_linux() {
        return true;
    }
    let Some(path) = config_vdf(core) else { return false };
    let result = modify_vdf_file_if(&path, |data| {
        let map = compat_map(data);
        if load_previous_compat_mapping(core).is_none() {
            let existing = map.get(BO3_APP_ID).map(node_to_json);
            let payload = json!({ "had_entry": existing.is_some(), "entry": existing });
            if !save_json_atomic(core, &state_file(core, "compat_mapping"), &payload, "previous compatibility mapping state") {
                // Legacy aborts before writing: `None` leaves config.vdf untouched.
                return None;
            }
        }
        map.set(
            BO3_APP_ID,
            Node::Obj(Obj(vec![
                ("name".into(), Node::Str(tool_name.to_string())),
                ("config".into(), Node::Str(String::new())),
                ("priority".into(), Node::Str("250".into())),
            ])),
        );
        Some(())
    });
    match result {
        Ok(Some(())) => {
            core.log.success(format!("Mapped AppID {BO3_APP_ID} to compatibility tool '{tool_name}'."));
            true
        }
        Ok(None) => {
            core.log.error("Unable to persist prior compatibility mapping; aborting mapping update.");
            false
        }
        Err(e) => {
            core.log.error(format!("Failed to update compatibility mapping: {e}"));
            restore_file(core, &path, CONFIG_BACKUP);
            false
        }
    }
}

/// Legacy `clear_compatibility_tool_mapping`: restore the snapshot of the previous mapping, or
/// drop the entry.
pub fn clear_compatibility_tool_mapping(core: &Core) -> bool {
    if !is_linux() {
        return true;
    }
    let Some(path) = config_vdf(core) else { return false };
    let result = modify_vdf_file(&path, |data| {
        let map = compat_map(data);
        match load_previous_compat_mapping(core) {
            Some(prev) => {
                if prev.had_entry {
                    match prev.entry {
                        None => {
                            remove_key(map, BO3_APP_ID);
                            core.log.warning(format!("Previous mapping snapshot was empty for AppID {BO3_APP_ID}; cleared mapping instead."));
                        }
                        Some(entry) => {
                            map.set(BO3_APP_ID, json_to_node(&entry));
                            core.log.success(format!("Restored prior compatibility mapping for AppID {BO3_APP_ID}."));
                        }
                    }
                } else {
                    remove_key(map, BO3_APP_ID);
                    core.log.success(format!("Cleared compatibility mapping for AppID {BO3_APP_ID}."));
                }
                clear_previous(core, "compat_mapping");
            }
            None => {
                remove_key(map, BO3_APP_ID);
                core.log.success(format!("Cleared compatibility mapping for AppID {BO3_APP_ID}."));
            }
        }
    });
    match result {
        Ok(()) => true,
        Err(e) => {
            core.log.error(format!("Failed to clear compatibility mapping: {e}"));
            restore_file(core, &path, CONFIG_BACKUP);
            false
        }
    }
}

// ---- compatibilitytools.d ---------------------------------------------------------------------

fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir(dst)?;
    for e in fs::read_dir(src)? {
        let e = e?;
        let to = dst.join(e.file_name());
        let ft = e.file_type()?;
        if ft.is_dir() {
            copy_tree(&e.path(), &to)?;
        } else if ft.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(fs::read_link(e.path())?, &to)?;
            #[cfg(not(unix))]
            fs::copy(e.path(), &to)?;
        } else {
            crate::fsutil::copy2(&e.path(), &to)?;
        }
    }
    Ok(())
}

/// Legacy `_write_compatibilitytool_manifest`.
pub fn write_compat_manifest(path: &Path, tool_name: &str) -> Result<(), String> {
    let mut data = std::fs::read(path).ok().and_then(|b| String::from_utf8(b).ok()).and_then(|t| steam::vdf::parse(&t).ok()).unwrap_or_default();
    let root = data.section_mut("compatibilitytools");
    let existing: Option<Obj> = root.get_obj("compat_tools").and_then(|c| c.0.first()).and_then(|(_, v)| if let Node::Obj(o) = v { Some(o.clone()) } else { None });
    let pick = |k: &str, d: &str| existing.as_ref().and_then(|o| o.get_str(k)).unwrap_or(d).to_string();
    let entry = Obj(vec![
        ("install_path".into(), Node::Str(pick("install_path", "."))),
        ("display_name".into(), Node::Str(tool_name.to_string())),
        ("from_oslist".into(), Node::Str(pick("from_oslist", "windows"))),
        ("to_oslist".into(), Node::Str(pick("to_oslist", "linux"))),
    ]);
    root.set("compat_tools", Node::Obj(Obj(vec![(tool_name.to_string(), Node::Obj(entry))])));
    steam::vdf::write_file(path, &data).map_err(|e| io_error_text(&e, Some(path)))
}

fn stamp() -> String {
    let t = crate::clock::now();
    format!("{:04}{:02}{:02}-{:02}{:02}{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second)
}

/// Legacy `install_compatibility_tool`: copy `source` to `compatibilitytools.d/<name>` (staged,
/// previous install renamed to `<name>.patchops.bak-<ts>`).
pub fn install_compatibility_tool(core: &Core, source: &Path, tool_name: &str) -> bool {
    if !is_linux() {
        return true;
    }
    if !source.is_dir() {
        core.log.error(format!("Compatibility tool source not found: {}", source.display()));
        return false;
    }
    let Some(root) = steam::root_path(core) else {
        core.log.error("Steam root path could not be resolved.");
        return false;
    };
    let compat_dir = Path::new(&root).join("compatibilitytools.d");
    let destination = compat_dir.join(tool_name);
    let ts = stamp();
    let staging = compat_dir.join(format!("{tool_name}.tmp-{ts}"));
    let r = (|| -> Result<(), String> {
        fs::create_dir_all(&compat_dir).map_err(|e| io_error_text(&e, Some(&compat_dir)))?;
        if staging.exists() {
            let _ = fs::remove_dir_all(&staging);
        }
        copy_tree(source, &staging).map_err(|e| io_error_text(&e, Some(&staging)))?;
        write_compat_manifest(&staging.join("compatibilitytool.vdf"), tool_name)?;
        if destination.exists() {
            let backup = compat_dir.join(format!("{tool_name}.patchops.bak-{ts}"));
            fs::rename(&destination, &backup).map_err(|e| crate::pyfmt::rename_error_text(&e, &destination, &backup))?;
            core.log.info(format!("Existing compatibility tool backed up to {}", backup.display()));
        }
        fs::rename(&staging, &destination).map_err(|e| crate::pyfmt::rename_error_text(&e, &staging, &destination))?;
        core.log.success(format!("Installed compatibility tool '{tool_name}' to {}", destination.display()));
        Ok(())
    })();
    match r {
        Ok(()) => true,
        Err(e) => {
            core.log.error(format!("Failed to install compatibility tool '{tool_name}': {e}"));
            let _ = fs::remove_dir_all(&staging);
            false
        }
    }
}

/// Legacy `remove_compatibility_tool`: the tool and its `.patchops.bak-*` variants.
pub fn remove_compatibility_tool(core: &Core, tool_name: &str) -> bool {
    if !is_linux() {
        return true;
    }
    let Some(root) = steam::root_path(core) else {
        core.log.error("Steam root path could not be resolved.");
        return false;
    };
    let compat_dir = Path::new(&root).join("compatibilitytools.d");
    let target = compat_dir.join(tool_name);
    let prefix = format!("{tool_name}.patchops.bak-");
    let mut candidates: Vec<PathBuf> = Vec::new();
    if target.exists() {
        candidates.push(target);
    }
    if let Ok(rd) = fs::read_dir(&compat_dir) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().starts_with(&prefix) {
                candidates.push(e.path());
            }
        }
    }
    if candidates.is_empty() {
        core.log.info(format!("Compatibility tool '{tool_name}' was already absent."));
        return true;
    }
    let mut errors = Vec::new();
    let mut removed = 0usize;
    for p in &candidates {
        let r = if p.is_dir() { fs::remove_dir_all(p) } else { fs::remove_file(p) };
        match r {
            Ok(()) => removed += 1,
            Err(e) => errors.push(format!("{}: {}", p.display(), io_error_text(&e, Some(p)))),
        }
    }
    if !errors.is_empty() {
        core.log.error(format!("Removed {removed} compatibility tool path(s) but failed to remove: {}", errors.join("; ")));
        return false;
    }
    core.log.success(format!("Removed compatibility tool '{tool_name}' and {} backup path(s) from Steam.", removed.saturating_sub(1)));
    true
}

// ---- orchestration ------------------------------------------------------------------------------

/// Why the Linux Steam flow did not complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowError {
    /// Steam would not exit in time; no Steam file was touched ([`steam::STEAM_CLOSE_TIMEOUT_MSG`]).
    SteamNotClosed,
    /// Anything else (already logged).
    Failed,
}

/// Legacy `configure_bo3_enhanced_linux` with an already-resolved tool directory (the caller
/// resolves/downloads the GDK-Proton tool: a bundled `bo3-enhanced-proton/BO3 Enhanced` or the
/// cached download).
pub fn configure_bo3_enhanced_linux(core: &Core, resolved_tool_source: Option<&Path>) -> Result<(), FlowError> {
    if !is_linux() {
        return Ok(());
    }
    let Some(user_id) = steam::find_user_id(core) else {
        core.log.error("Steam user ID not found; cannot configure Linux BO3 Enhanced flow.");
        return Err(FlowError::Failed);
    };
    let Some(source) = resolved_tool_source else {
        core.log.error("Could not resolve a BO3 Enhanced Proton source (local bundle or cached download).");
        return Err(FlowError::Failed);
    };
    if steam::close_steam(core).is_err() {
        return Err(FlowError::SteamNotClosed);
    }
    let success = (|| {
        if !install_compatibility_tool(core, source, ENHANCED_TOOL_NAME) || !set_compatibility_tool_mapping(core, ENHANCED_TOOL_NAME) {
            return false;
        }
        if load_previous_launch_options(core).is_none() {
            let Some(current) = steam::read_launch_options(core, &user_id, BO3_APP_ID) else {
                core.log.error("Unable to read current launch options; aborting Linux BO3 Enhanced setup.");
                return false;
            };
            if !save_previous_launch_options(core, &current) {
                core.log.error("Unable to persist previous launch options; aborting Linux BO3 Enhanced setup.");
                return false;
            }
        }
        set_launch_options_exact(core, &user_id, BO3_APP_ID, ENHANCED_LAUNCH_OPTIONS)
    })();
    steam::open_steam(core);
    if success {
        core.log.success("Linux BO3 Enhanced flow configured: tool installed, AppID mapped, and launch options applied.");
    } else {
        core.log.warning("Linux BO3 Enhanced flow setup was only partially applied. Review previous log entries.");
    }
    if success { Ok(()) } else { Err(FlowError::Failed) }
}

/// Legacy `cleanup_bo3_enhanced_linux`: close Steam, restore/clear the compat mapping, restore
/// (or strip the Enhanced override from) the launch options, remove the tool, re-open Steam.
pub fn cleanup_bo3_enhanced_linux(core: &Core) -> Result<(), FlowError> {
    if !is_linux() {
        return Ok(());
    }
    let user_id = steam::find_user_id(core);
    if user_id.is_none() {
        core.log.error("Steam user ID not found; cannot clear BO3 launch options.");
    }
    if steam::close_steam(core).is_err() {
        return Err(FlowError::SteamNotClosed);
    }
    let mut success = true;
    success = clear_compatibility_tool_mapping(core) && success;
    if let Some(uid) = &user_id {
        if let Some(previous) = load_previous_launch_options(core) {
            success = set_launch_options_exact(core, uid, BO3_APP_ID, &previous) && success;
            if success {
                clear_previous(core, "launch_options");
            }
        } else {
            match steam::read_launch_options(core, uid, BO3_APP_ID) {
                None => {
                    core.log.error("Unable to read current launch options during cleanup.");
                    success = false;
                }
                Some(current) => {
                    let cleaned = strip_enhanced_launch_override(&current);
                    if cleaned != normalize_launch_options(&current) {
                        success = set_launch_options_exact(core, uid, BO3_APP_ID, &cleaned) && success;
                    } else {
                        core.log.info("No BO3 Enhanced launch override found; leaving launch options unchanged.");
                    }
                }
            }
        }
    } else {
        success = false;
    }
    success = remove_compatibility_tool(core, ENHANCED_TOOL_NAME) && success;
    steam::open_steam(core);
    if success {
        core.log.success("Linux BO3 Enhanced cleanup complete: mapping cleared, launch options reset, and tool removed.");
    } else {
        core.log.warning("Linux BO3 Enhanced cleanup was only partially applied. Review previous log entries.");
    }
    if success { Ok(()) } else { Err(FlowError::Failed) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_override_token() {
        assert_eq!(strip_enhanced_launch_override("WINEDLLOVERRIDES=\"WindowsCodecs=n,b\" %command%"), "%command%");
        assert_eq!(strip_enhanced_launch_override("PRE winedlloverrides=\"windowscodecs=n,b\" %command% +x"), "PRE %command% +x");
        assert_eq!(strip_enhanced_launch_override("X=WINEDLLOVERRIDES=\"WindowsCodecs=n,b\"y"), "X=WINEDLLOVERRIDES=\"WindowsCodecs=n,b\"y");
        assert_eq!(strip_enhanced_launch_override("  a   b "), "a b");
        assert_eq!(normalize_launch_options(""), "");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn compat_mapping_is_snapshotted_and_restored() {
        use crate::enhanced::testkit::Sandbox;
        let sb = Sandbox::new();
        let cfg = sb.steam.join("config/config.vdf");
        fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        let original = "\"InstallConfigStore\"\n{\n\t\"Software\"\n\t{\n\t\t\"Valve\"\n\t\t{\n\t\t\t\"Steam\"\n\t\t\t{\n\t\t\t\t\"CompatToolMapping\"\n\t\t\t\t{\n\t\t\t\t\t\"311210\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"name\"\t\t\"proton_9\"\n\t\t\t\t\t\t\"config\"\t\t\"\"\n\t\t\t\t\t\t\"priority\"\t\t\"75\"\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n}\n";
        fs::write(&cfg, original).unwrap();
        assert!(set_compatibility_tool_mapping(&sb.core, ENHANCED_TOOL_NAME));
        let t = fs::read_to_string(&cfg).unwrap();
        assert!(t.contains("\"name\" \"BO3 Enhanced\"") && t.contains("\"priority\" \"250\""), "{t}");
        assert!(state_file(&sb.core, "compat_mapping").exists());
        assert!(clear_compatibility_tool_mapping(&sb.core));
        let t = fs::read_to_string(&cfg).unwrap();
        assert!(t.contains("\"name\" \"proton_9\"") && t.contains("\"priority\" \"75\""), "{t}");
        assert!(!state_file(&sb.core, "compat_mapping").exists());
        assert!(sb.log().contains("Restored prior compatibility mapping for AppID 311210."));
        // Without a snapshot the entry is simply dropped.
        assert!(clear_compatibility_tool_mapping(&sb.core));
        assert!(!fs::read_to_string(&cfg).unwrap().contains("311210"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn tool_install_and_removal() {
        use crate::enhanced::testkit::Sandbox;
        let sb = Sandbox::new();
        let src = sb.root.join("tool");
        fs::create_dir_all(src.join("files")).unwrap();
        fs::write(src.join("files/proton"), b"p").unwrap();
        assert!(install_compatibility_tool(&sb.core, &src, ENHANCED_TOOL_NAME));
        let dest = sb.steam.join("compatibilitytools.d").join(ENHANCED_TOOL_NAME);
        assert!(dest.join("files/proton").exists());
        let manifest = fs::read_to_string(dest.join("compatibilitytool.vdf")).unwrap();
        assert!(manifest.contains("\"display_name\" \"BO3 Enhanced\"") && manifest.contains("\"to_oslist\" \"linux\""), "{manifest}");
        // Re-install keeps the previous copy as a .patchops.bak-<ts> that removal also sweeps.
        fs::rename(&dest, sb.steam.join("compatibilitytools.d").join("BO3 Enhanced.patchops.bak-20240101-000000")).unwrap();
        assert!(install_compatibility_tool(&sb.core, &src, ENHANCED_TOOL_NAME));
        assert!(remove_compatibility_tool(&sb.core, ENHANCED_TOOL_NAME));
        assert_eq!(fs::read_dir(sb.steam.join("compatibilitytools.d")).unwrap().count(), 0);
        assert!(sb.log().contains("Removed compatibility tool 'BO3 Enhanced' and 1 backup path(s) from Steam."));
        assert!(remove_compatibility_tool(&sb.core, ENHANCED_TOOL_NAME));
        assert!(sb.log().contains("Compatibility tool 'BO3 Enhanced' was already absent."));
    }
}
