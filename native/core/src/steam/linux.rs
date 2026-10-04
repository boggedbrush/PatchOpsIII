//! Linux Proton installation, compatibility mappings, and rollback transactions.

use super::{
    APP_ID, AppState, VdfEntry, VdfObject, VdfValue, close_steam, ensure_path, entry_index, fs_ops,
    local_config_path_in, open_steam, parse_vdf, read_launch_options_at, read_vdf, serialize_vdf,
    set_launch_options_at, set_string, steam_root, update_vdf_with_backup, user_id_in, value_at,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue, json};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const ENHANCED_TOOL_NAME: &str = "BO3 Enhanced";
const ENHANCED_LAUNCH_OPTIONS: &str = "WINEDLLOVERRIDES=\"WindowsCodecs=n,b\" %command%";

const ENHANCED_PROTON_TAG: &str = "release10-32";
const ENHANCED_PROTON_ASSET: &str = "GDK-Proton10-32.tar.gz";
const ENHANCED_PROTON_DOWNLOAD: &str = "https://github.com/Weather-OS/GDK-Proton/releases/download/release10-32/GDK-Proton10-32.tar.gz";
const ENHANCED_PROTON_SHA256: &str =
    "1e80f4e714f877f42101d5775bd38ca0a15a38d304e24af1f15c6deec4ebac2d";
const MAX_PROTON_ARCHIVE_BYTES: u64 = 4 * 1024 * 1024 * 1024;

const TOOL_OWNERSHIP_VERSION: u8 = 1;
const TOOL_MARKER_FILENAME: &str = ".patchops-owner.json";

fn set_value(object: &mut VdfObject, key: &str, value: VdfValue) {
    if let Some(index) = entry_index(object, key) {
        object[index].value = value;
    } else {
        object.push(VdfEntry {
            key: key.into(),
            value,
            condition: None,
        });
    }
}

fn update_vdf(
    path: &Path,
    legacy_backup: Option<&Path>,
    change: impl FnOnce(&mut VdfObject) -> Result<(), String>,
) -> Result<(), String> {
    update_vdf_with_backup(path, legacy_backup, false, change)
}

fn write_new_or_update_vdf(
    path: &Path,
    change: impl FnOnce(&mut VdfObject) -> Result<(), String>,
) -> Result<(), String> {
    if path.exists() {
        return update_vdf(path, None, change);
    }
    let mut document = Vec::new();
    change(&mut document)?;
    let updated = serialize_vdf(&document);
    parse_vdf(&updated).map_err(|error| format!("refusing to write invalid VDF: {error}"))?;
    fs_ops::atomic_write(path, updated.as_bytes())
}

pub(super) fn wait_for_steam_exit_with(
    checks: usize,
    mut is_running: impl FnMut() -> Result<bool, String>,
    mut pause: impl FnMut(),
) -> Result<(), String> {
    for check in 0..checks {
        if !is_running()? {
            return Ok(());
        }
        if check + 1 < checks {
            pause();
        }
    }
    Err("Steam did not exit before the configuration timeout.".into())
}

pub(super) fn linux_steam_running() -> Result<bool, String> {
    for entry in fs::read_dir("/proc").map_err(|error| format!("/proc: {error}"))? {
        let Ok(entry) = entry else {
            continue;
        };
        if !entry
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|byte| byte.is_ascii_digit())
        {
            continue;
        }
        if fs::read_to_string(entry.path().join("comm")).is_ok_and(|name| name.trim() == "steam") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn vdf_to_json(value: &VdfValue) -> JsonValue {
    match value {
        VdfValue::String(value) => JsonValue::String(value.clone()),
        VdfValue::Object(object) => {
            let mut values = JsonMap::new();
            for entry in object {
                values.insert(entry.key.clone(), vdf_to_json(&entry.value));
            }
            JsonValue::Object(values)
        }
    }
}

fn json_to_vdf(value: &JsonValue) -> Option<VdfValue> {
    match value {
        JsonValue::String(value) => Some(VdfValue::String(value.clone())),
        JsonValue::Object(object) => Some(VdfValue::Object(
            object
                .iter()
                .filter_map(|(key, value)| {
                    Some(VdfEntry {
                        key: key.clone(),
                        value: json_to_vdf(value)?,
                        condition: None,
                    })
                })
                .collect(),
        )),
        JsonValue::Bool(value) => Some(VdfValue::String(value.to_string())),
        JsonValue::Number(value) => Some(VdfValue::String(value.to_string())),
        _ => None,
    }
}

fn atomic_json(path: &Path, value: &JsonValue) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    fs_ops::atomic_write(path, &bytes)
}

fn mapping_snapshot_path(data_dir: &Path) -> PathBuf {
    data_dir.join(format!("backups/compat_mapping_{APP_ID}.json"))
}

fn launch_snapshot_path(data_dir: &Path) -> PathBuf {
    data_dir.join(format!("backups/launch_options_{APP_ID}.json"))
}

fn tool_ownership_path(data_dir: &Path) -> PathBuf {
    data_dir.join(format!("backups/compat_tool_{APP_ID}.json"))
}

fn required_json(path: &Path) -> Result<JsonValue, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    let body = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_slice(&body)
        .map_err(|error| format!("{} is not valid ownership data: {error}", path.display()))
}

fn enhanced_mapping_value() -> VdfValue {
    VdfValue::Object(vec![
        VdfEntry {
            key: "name".into(),
            value: VdfValue::String(ENHANCED_TOOL_NAME.into()),
            condition: None,
        },
        VdfEntry {
            key: "config".into(),
            value: VdfValue::String(String::new()),
            condition: None,
        },
        VdfEntry {
            key: "priority".into(),
            value: VdfValue::String("250".into()),
            condition: None,
        },
    ])
}

fn mapping_restore_value(snapshot: &Path) -> Result<(bool, Option<VdfValue>), String> {
    let saved = required_json(snapshot)?;
    let object = saved
        .as_object()
        .ok_or_else(|| format!("{} has invalid mapping ownership data", snapshot.display()))?;
    let had_entry = object
        .get("had_entry")
        .and_then(JsonValue::as_bool)
        .ok_or_else(|| format!("{} has invalid mapping ownership data", snapshot.display()))?;
    let previous = object.get("entry").and_then(json_to_vdf);
    if had_entry && previous.is_none() {
        return Err(format!(
            "{} is missing the original compatibility mapping",
            snapshot.display()
        ));
    }
    Ok((had_entry, previous))
}

fn set_compatibility_mapping_at(
    config: &Path,
    legacy_backup: &Path,
    snapshot: &Path,
) -> Result<(), String> {
    let map_path = [
        "InstallConfigStore",
        "Software",
        "Valve",
        "Steam",
        "CompatToolMapping",
    ];
    match fs::symlink_metadata(snapshot) {
        Ok(_) => {
            mapping_restore_value(snapshot)?;
            let document = read_vdf(config)?;
            let mut managed_path = map_path.to_vec();
            managed_path.push(APP_ID);
            if value_at(&document, &managed_path) != Some(&enhanced_mapping_value()) {
                return Err(
                    "Steam compatibility mapping changed after PatchOpsIII configured it; refusing to overwrite it."
                        .into(),
                );
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let document = read_vdf(config)?;
            let previous = match value_at(&document, &map_path) {
                Some(VdfValue::Object(map)) => {
                    entry_index(map, APP_ID).map(|index| map[index].value.clone())
                }
                Some(VdfValue::String(_)) => {
                    return Err("Steam CompatToolMapping is not an object".into());
                }
                None => None,
            };
            atomic_json(
                snapshot,
                &json!({
                    "had_entry": previous.is_some(),
                    "entry": previous.as_ref().map(vdf_to_json).unwrap_or(JsonValue::Null),
                }),
            )?;
        }
        Err(error) => return Err(format!("{}: {error}", snapshot.display())),
    }

    update_vdf(config, Some(legacy_backup), |document| {
        let map = ensure_path(
            document,
            &[
                "InstallConfigStore",
                "Software",
                "Valve",
                "Steam",
                "CompatToolMapping",
            ],
        )?;
        set_value(map, APP_ID, enhanced_mapping_value());
        Ok(())
    })
}

fn clear_compatibility_mapping_at(
    config: &Path,
    legacy_backup: &Path,
    snapshot: &Path,
) -> Result<(), String> {
    let (had_entry, previous) = mapping_restore_value(snapshot)?;
    let document = read_vdf(config)?;
    let map_path = [
        "InstallConfigStore",
        "Software",
        "Valve",
        "Steam",
        "CompatToolMapping",
    ];
    let mut managed_path = map_path.to_vec();
    managed_path.push(APP_ID);
    if value_at(&document, &managed_path) != Some(&enhanced_mapping_value()) {
        return Err(
            "Steam compatibility mapping changed after PatchOpsIII configured it; leaving it untouched."
                .into(),
        );
    }

    update_vdf(config, Some(legacy_backup), |document| {
        let map = ensure_path(
            document,
            &[
                "InstallConfigStore",
                "Software",
                "Valve",
                "Steam",
                "CompatToolMapping",
            ],
        )?;
        if let (true, Some(previous)) = (had_entry, previous) {
            set_value(map, APP_ID, previous);
        } else {
            map.retain(|entry| !entry.key.eq_ignore_ascii_case(APP_ID));
        }
        Ok(())
    })?;
    if snapshot.exists() {
        fs::remove_file(snapshot).map_err(|error| format!("{}: {error}", snapshot.display()))?;
    }
    Ok(())
}

fn update_compatibility_manifest(path: &Path) -> Result<(), String> {
    write_new_or_update_vdf(path, |document| {
        let tools = ensure_path(document, &["compatibilitytools", "compat_tools"])?;
        let index = entry_index(tools, ENHANCED_TOOL_NAME).or_else(|| {
            tools
                .iter()
                .position(|entry| matches!(entry.value, VdfValue::Object(_)))
        });
        let index = match index {
            Some(index) => {
                tools[index].key = ENHANCED_TOOL_NAME.into();
                index
            }
            None => {
                tools.push(VdfEntry {
                    key: ENHANCED_TOOL_NAME.into(),
                    value: VdfValue::Object(Vec::new()),
                    condition: None,
                });
                tools.len() - 1
            }
        };
        let VdfValue::Object(tool) = &mut tools[index].value else {
            return Err("compatibility tool manifest entry is not an object".into());
        };
        for (key, default) in [
            ("install_path", "."),
            ("display_name", ENHANCED_TOOL_NAME),
            ("from_oslist", "windows"),
            ("to_oslist", "linux"),
        ] {
            let value = if matches!(key, "install_path" | "from_oslist" | "to_oslist") {
                match value_at(tool, &[key]) {
                    Some(VdfValue::String(value)) => value.clone(),
                    _ => default.into(),
                }
            } else {
                default.into()
            };
            set_string(tool, key, value)?;
        }
        Ok(())
    })
}

fn copy_directory(source: &Path, destination: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(source).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "Compatibility tool source is not a safe directory: {}",
            source.display()
        ));
    }
    let root = source.canonicalize().map_err(|error| error.to_string())?;
    copy_directory_inside(&root, &root, destination, &mut HashSet::new())
}

fn copy_directory_inside(
    root: &Path,
    source: &Path,
    destination: &Path,
    active: &mut HashSet<PathBuf>,
) -> Result<(), String> {
    let resolved = source.canonicalize().map_err(|error| error.to_string())?;
    if !resolved.starts_with(root) {
        return Err(format!(
            "Compatibility tool link escapes its source: {}",
            source.display()
        ));
    }
    if !active.insert(resolved.clone()) {
        return Err(format!(
            "Compatibility tool contains a directory-link cycle: {}",
            source.display()
        ));
    }
    fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    let result = (|| {
        for entry in
            fs::read_dir(&resolved).map_err(|error| format!("{}: {error}", source.display()))?
        {
            let entry = entry.map_err(|error| error.to_string())?;
            let source_path = entry.path();
            let destination_path = destination.join(entry.file_name());
            let metadata = fs::symlink_metadata(&source_path).map_err(|error| error.to_string())?;
            let resolved_path = if metadata.file_type().is_symlink() {
                let target = fs::read_link(&source_path).map_err(|error| error.to_string())?;
                if target.is_absolute() {
                    return Err(format!(
                        "Compatibility tool contains an absolute link: {}",
                        source_path.display()
                    ));
                }
                source_path
                    .canonicalize()
                    .map_err(|error| format!("{}: {error}", source_path.display()))?
            } else {
                source_path.clone()
            };
            if !resolved_path.starts_with(root) {
                return Err(format!(
                    "Compatibility tool link escapes its source: {}",
                    source_path.display()
                ));
            }
            let resolved_metadata =
                fs::metadata(&resolved_path).map_err(|error| error.to_string())?;
            if resolved_metadata.is_dir() {
                copy_directory_inside(root, &resolved_path, &destination_path, active)?;
            } else if resolved_metadata.is_file() {
                fs::copy(&resolved_path, &destination_path).map_err(|error| error.to_string())?;
            } else {
                return Err(format!(
                    "unsupported compatibility tool file: {}",
                    source_path.display()
                ));
            }
        }
        Ok(())
    })();
    active.remove(&resolved);
    result
}

fn unique_suffix() -> String {
    format!(
        "{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        std::process::id()
    )
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ToolOwnership {
    version: u8,
    transaction_id: String,
    original_backup: Option<String>,
}

#[derive(Debug)]
struct ToolInstallTransaction {
    destination: PathBuf,
    displaced_managed: Option<PathBuf>,
    original_backup: Option<PathBuf>,
    ownership_path: PathBuf,
    transaction_id: String,
    new_ownership: bool,
}

#[derive(Debug)]
struct ToolRemovalTransaction {
    destination: PathBuf,
    displaced_managed: Option<PathBuf>,
    restored_backup: Option<PathBuf>,
    ownership_path: PathBuf,
    transaction_id: String,
}

#[derive(Debug)]
struct LegacyToolAdoption {
    destination: PathBuf,
    ownership_path: PathBuf,
    transaction_id: String,
}

fn tool_directory(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(true),
        Ok(_) => Err(format!("{} is not a safe tool directory", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

fn unique_tool_path(directory: &Path, prefix: &str) -> PathBuf {
    for index in 0..1000_u16 {
        let candidate = directory.join(format!("{prefix}-{}-{index}", unique_suffix()));
        if fs::symlink_metadata(&candidate).is_err() {
            return candidate;
        }
    }
    directory.join(format!("{prefix}-{}", unique_suffix()))
}

fn load_tool_ownership(path: &Path) -> Result<Option<ToolOwnership>, String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display())),
        Ok(_) => {}
    }
    let value = required_json(path)?;
    let ownership: ToolOwnership = serde_json::from_value(value).map_err(|error| {
        format!(
            "{} has invalid tool ownership data: {error}",
            path.display()
        )
    })?;
    if ownership.version != TOOL_OWNERSHIP_VERSION
        || ownership.transaction_id.is_empty()
        || !ownership
            .transaction_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(format!(
            "{} has invalid tool ownership data",
            path.display()
        ));
    }
    if let Some(name) = &ownership.original_backup {
        let path = Path::new(name);
        if path.parent() != Some(Path::new(""))
            || !name.starts_with(&format!("{ENHANCED_TOOL_NAME}.patchops.original-"))
        {
            return Err("Compatibility tool ownership contains an unsafe backup path.".into());
        }
    }
    Ok(Some(ownership))
}

fn compare_tool_trees(
    expected: &Path,
    actual: &Path,
    allow_actual_links: bool,
    ignore_source_marker: bool,
) -> Result<(), String> {
    let actual_metadata =
        fs::symlink_metadata(actual).map_err(|error| format!("{}: {error}", actual.display()))?;
    if actual_metadata.file_type().is_symlink() || !actual_metadata.is_dir() {
        return Err(format!("{} is not a safe tool directory", actual.display()));
    }
    let actual_root = actual.canonicalize().map_err(|error| error.to_string())?;
    compare_tool_trees_inside(
        expected,
        actual,
        Path::new(""),
        &actual_root,
        allow_actual_links,
        ignore_source_marker,
        &mut HashSet::new(),
    )
}

#[allow(clippy::too_many_arguments)]
fn compare_tool_trees_inside(
    expected: &Path,
    actual: &Path,
    relative: &Path,
    actual_root: &Path,
    allow_actual_links: bool,
    ignore_source_marker: bool,
    active: &mut HashSet<PathBuf>,
) -> Result<(), String> {
    let expected_metadata = fs::symlink_metadata(expected)
        .map_err(|error| format!("{}: {error}", expected.display()))?;
    if expected_metadata.file_type().is_symlink() {
        return Err(format!(
            "Trusted compatibility tool contains an unexpected link at {}.",
            relative.display()
        ));
    }
    let metadata =
        fs::symlink_metadata(actual).map_err(|error| format!("{}: {error}", actual.display()))?;
    let actual_resolved = if metadata.file_type().is_symlink() {
        if !allow_actual_links {
            return Err(format!(
                "Legacy compatibility tool contains an unsafe link at {}.",
                relative.display()
            ));
        }
        let target = fs::read_link(actual).map_err(|error| error.to_string())?;
        if target.is_absolute() {
            return Err(format!(
                "Compatibility tool contains an absolute link at {}.",
                relative.display()
            ));
        }
        actual
            .canonicalize()
            .map_err(|error| format!("{}: {error}", actual.display()))?
    } else {
        actual.to_path_buf()
    };
    if !actual_resolved
        .canonicalize()
        .is_ok_and(|path| path.starts_with(actual_root))
    {
        return Err(format!(
            "Compatibility tool link escapes its source at {}.",
            relative.display()
        ));
    }
    let actual_metadata = fs::metadata(&actual_resolved).map_err(|error| error.to_string())?;
    if expected_metadata.is_dir() && actual_metadata.is_dir() {
        let resolved_directory = actual_resolved
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if !active.insert(resolved_directory.clone()) {
            return Err(format!(
                "Compatibility tool contains a directory-link cycle at {}.",
                relative.display()
            ));
        }
        let names = |directory: &Path, ignore_marker: bool| -> Result<Vec<_>, String> {
            let mut names = fs::read_dir(directory)
                .map_err(|error| format!("{}: {error}", directory.display()))?
                .map(|entry| {
                    entry
                        .map(|entry| entry.file_name())
                        .map_err(|error| error.to_string())
                })
                .collect::<Result<Vec<_>, _>>()?;
            if ignore_marker {
                names.retain(|name| name != ".patchops-source.json");
            }
            names.sort();
            Ok(names)
        };
        let expected_names = names(expected, false)?;
        if expected_names
            != names(
                &actual_resolved,
                ignore_source_marker && relative.as_os_str().is_empty(),
            )?
        {
            active.remove(&resolved_directory);
            return Err(format!(
                "Legacy compatibility tool contents differ at {}.",
                relative.display()
            ));
        }
        for name in expected_names {
            compare_tool_trees_inside(
                &expected.join(&name),
                &actual.join(&name),
                &relative.join(name),
                actual_root,
                allow_actual_links,
                ignore_source_marker,
                active,
            )?;
        }
        active.remove(&resolved_directory);
        return Ok(());
    }
    if expected_metadata.is_file() && actual_metadata.is_file() {
        if relative == Path::new("compatibilitytool.vdf") {
            if read_vdf(expected)? != read_vdf(actual)? {
                return Err("Legacy compatibility-tool manifest differs from release10-32.".into());
            }
        } else if fs_ops::sha256_file(expected)? != fs_ops::sha256_file(actual)? {
            return Err(format!(
                "Legacy compatibility tool bytes differ at {}.",
                relative.display()
            ));
        }
        return Ok(());
    }
    Err(format!(
        "Legacy compatibility tool has an unexpected file type at {}.",
        relative.display()
    ))
}

fn require_legacy_compatibility_state(
    steam_config: &Path,
    mapping_snapshot: &Path,
    local_config: &Path,
    launch_snapshot: &Path,
) -> Result<(), String> {
    mapping_restore_value(mapping_snapshot)?;
    launch_restore_value(launch_snapshot)?;
    let document = read_vdf(steam_config)?;
    if value_at(
        &document,
        &[
            "InstallConfigStore",
            "Software",
            "Valve",
            "Steam",
            "CompatToolMapping",
            APP_ID,
        ],
    ) != Some(&enhanced_mapping_value())
    {
        return Err("Legacy Steam compatibility mapping is not the exact managed value.".into());
    }
    if read_launch_options_at(local_config, APP_ID)? != ENHANCED_LAUNCH_OPTIONS {
        return Err("Legacy Steam launch options are not the exact managed value.".into());
    }
    Ok(())
}

fn legacy_tool_backup(directory: &Path) -> Result<Option<PathBuf>, String> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", directory.display())),
    };
    let prefix = format!("{ENHANCED_TOOL_NAME}.patchops.bak-");
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(&prefix))
        {
            return Ok(Some(entry.path()));
        }
    }
    Ok(None)
}

fn compatibility_is_fully_clean(
    steam_root: &Path,
    ownership_path: &Path,
    steam_config: &Path,
    mapping_snapshot: &Path,
    local_config: &Path,
    launch_snapshot: &Path,
) -> Result<bool, String> {
    if load_tool_ownership(ownership_path)?.is_some() {
        return Ok(false);
    }
    let destination = steam_root
        .join("compatibilitytools.d")
        .join(ENHANCED_TOOL_NAME);
    if tool_directory(&destination)? {
        return Ok(false);
    }
    if legacy_tool_backup(&steam_root.join("compatibilitytools.d"))?.is_some() {
        return Ok(false);
    }
    for snapshot in [mapping_snapshot, launch_snapshot] {
        match fs::symlink_metadata(snapshot) {
            Ok(_) => return Ok(false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("{}: {error}", snapshot.display())),
        }
    }
    let document = read_vdf(steam_config)?;
    let managed_mapping = value_at(
        &document,
        &[
            "InstallConfigStore",
            "Software",
            "Valve",
            "Steam",
            "CompatToolMapping",
            APP_ID,
        ],
    ) == Some(&enhanced_mapping_value());
    let managed_launch = read_launch_options_at(local_config, APP_ID)? == ENHANCED_LAUNCH_OPTIONS;
    Ok(!managed_mapping && !managed_launch)
}

fn adopt_legacy_compatibility_tool_at(
    steam_root: &Path,
    trusted_source: &Path,
    ownership_path: &Path,
    steam_config: &Path,
    mapping_snapshot: &Path,
    local_config: &Path,
    launch_snapshot: &Path,
) -> Result<LegacyToolAdoption, String> {
    if load_tool_ownership(ownership_path)?.is_some() {
        return Err(
            "Compatibility-tool ownership already exists; legacy adoption is invalid.".into(),
        );
    }
    let directory = steam_root.join("compatibilitytools.d");
    let destination = directory.join(ENHANCED_TOOL_NAME);
    if !tool_directory(&destination)? {
        return Err("Legacy BO3 Enhanced compatibility tool is missing.".into());
    }
    match fs::symlink_metadata(destination.join(TOOL_MARKER_FILENAME)) {
        Ok(_) => {
            return Err(
                "Untracked compatibility tool already contains an ownership marker.".into(),
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    if let Some(backup) = legacy_tool_backup(&directory)? {
        return Err(format!(
            "Legacy compatibility-tool backup is ambiguous: {}.",
            backup.display()
        ));
    }
    require_legacy_compatibility_state(
        steam_config,
        mapping_snapshot,
        local_config,
        launch_snapshot,
    )?;
    compare_tool_trees(trusted_source, &destination, false, false)?;

    let transaction_id = unique_suffix();
    atomic_json(
        &destination.join(TOOL_MARKER_FILENAME),
        &json!({ "transaction_id": transaction_id }),
    )?;
    if let Err(error) = save_tool_ownership(
        ownership_path,
        &ToolOwnership {
            version: TOOL_OWNERSHIP_VERSION,
            transaction_id: transaction_id.clone(),
            original_backup: None,
        },
    ) {
        let rollback = fs::remove_file(destination.join(TOOL_MARKER_FILENAME));
        return Err(match rollback {
            Ok(()) => error,
            Err(rollback) => format!("{error}; removing the adoption marker failed: {rollback}"),
        });
    }
    Ok(LegacyToolAdoption {
        destination,
        ownership_path: ownership_path.to_path_buf(),
        transaction_id,
    })
}

impl LegacyToolAdoption {
    fn rollback(self) -> Result<(), String> {
        require_managed_tool(&self.destination, &self.transaction_id)?;
        fs::remove_file(self.destination.join(TOOL_MARKER_FILENAME))
            .map_err(|error| error.to_string())?;
        fs::remove_file(&self.ownership_path)
            .map_err(|error| format!("{}: {error}", self.ownership_path.display()))
    }
}

fn finish_legacy_adoption<T>(
    adoption: Option<LegacyToolAdoption>,
    result: Result<T, String>,
) -> Result<T, String> {
    match (adoption, result) {
        (_, Ok(value)) => Ok(value),
        (None, Err(error)) => Err(error),
        (Some(adoption), Err(error)) => match adoption.rollback() {
            Ok(()) => Err(error),
            Err(rollback) => Err(format!(
                "{error}; legacy adoption rollback failed: {rollback}"
            )),
        },
    }
}

fn save_tool_ownership(path: &Path, ownership: &ToolOwnership) -> Result<(), String> {
    let value = serde_json::to_value(ownership).map_err(|error| error.to_string())?;
    atomic_json(path, &value)
}

fn tool_marker_id(path: &Path) -> Result<String, String> {
    required_json(&path.join(TOOL_MARKER_FILENAME))?
        .get("transaction_id")
        .and_then(JsonValue::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("{} has invalid PatchOpsIII ownership", path.display()))
}

fn require_managed_tool(path: &Path, transaction_id: &str) -> Result<(), String> {
    if tool_marker_id(path)? != transaction_id {
        return Err(format!(
            "{} is no longer the compatibility tool installed by PatchOpsIII; leaving it untouched",
            path.display()
        ));
    }
    Ok(())
}

fn exact_tool_backup(
    directory: &Path,
    ownership: &ToolOwnership,
) -> Result<Option<PathBuf>, String> {
    let Some(name) = &ownership.original_backup else {
        return Ok(None);
    };
    let backup = directory.join(name);
    if !tool_directory(&backup)? {
        return Err(format!(
            "The exact PatchOpsIII-owned tool backup is missing: {}",
            backup.display()
        ));
    }
    Ok(Some(backup))
}

fn remove_managed_tool(path: &Path, transaction_id: &str) -> Result<(), String> {
    require_managed_tool(path, transaction_id)?;
    fs::remove_dir_all(path).map_err(|error| format!("{}: {error}", path.display()))
}

impl ToolInstallTransaction {
    fn rollback(self) -> Result<(), String> {
        remove_managed_tool(&self.destination, &self.transaction_id)?;
        let restore = if self.new_ownership {
            self.original_backup.as_ref()
        } else {
            self.displaced_managed.as_ref()
        };
        if let Some(restore) = restore {
            fs::rename(restore, &self.destination).map_err(|error| {
                format!(
                    "failed to restore compatibility tool {}: {error}",
                    self.destination.display()
                )
            })?;
        }
        if self.new_ownership {
            fs::remove_file(&self.ownership_path)
                .map_err(|error| format!("{}: {error}", self.ownership_path.display()))?;
        }
        Ok(())
    }

    fn commit(mut self) -> Result<(), Box<(String, Self)>> {
        if let Some(displaced) = &self.displaced_managed
            && let Err(error) = require_managed_tool(displaced, &self.transaction_id)
        {
            return Err(Box::new((error, self)));
        }
        if let Some(displaced) = self.displaced_managed.take() {
            // Best-effort reaping keeps an unlink failure from becoming an
            // unrollbackable partial commit. Add persisted cleanup if this ever leaks.
            let _ = fs::remove_dir_all(displaced);
        }
        Ok(())
    }
}

impl ToolRemovalTransaction {
    fn rollback(self) -> Result<(), String> {
        if let Some(backup) = self.restored_backup {
            if tool_directory(&backup)? {
                return Err(format!(
                    "Cannot roll back because {} already exists",
                    backup.display()
                ));
            }
            fs::rename(&self.destination, &backup).map_err(|error| {
                format!(
                    "failed to return the original compatibility tool to {}: {error}",
                    backup.display()
                )
            })?;
        }
        if let Some(displaced) = self.displaced_managed {
            fs::rename(&displaced, &self.destination).map_err(|error| {
                format!("failed to restore the managed compatibility tool: {error}")
            })?;
        }
        Ok(())
    }

    fn commit(mut self) -> Result<(), Box<(String, Self)>> {
        if let Some(displaced) = &self.displaced_managed
            && let Err(error) = require_managed_tool(displaced, &self.transaction_id)
        {
            return Err(Box::new((error, self)));
        }
        if let Err(error) = fs::remove_file(&self.ownership_path) {
            return Err(Box::new((
                format!("{}: {error}", self.ownership_path.display()),
                self,
            )));
        }
        if let Some(displaced) = self.displaced_managed.take() {
            // Best-effort reaping keeps the ownership commit rollback-safe.
            // Add persisted cleanup if managed leftovers become measurable.
            let _ = fs::remove_dir_all(displaced);
        }
        Ok(())
    }
}

fn install_compatibility_tool_at(
    steam_root: &Path,
    source: &Path,
    ownership_path: &Path,
) -> Result<ToolInstallTransaction, String> {
    if !source.is_dir() {
        return Err(format!(
            "Compatibility tool source not found: {}",
            source.display()
        ));
    }
    let directory = steam_root.join("compatibilitytools.d");
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let destination = directory.join(ENHANCED_TOOL_NAME);
    let existing_ownership = load_tool_ownership(ownership_path)?;
    let new_ownership = existing_ownership.is_none();
    let transaction_id = existing_ownership
        .as_ref()
        .map(|ownership| ownership.transaction_id.clone())
        .unwrap_or_else(unique_suffix);
    let original_backup = match &existing_ownership {
        Some(ownership) => exact_tool_backup(&directory, ownership)?,
        None if tool_directory(&destination)? => Some(unique_tool_path(
            &directory,
            &format!("{ENHANCED_TOOL_NAME}.patchops.original"),
        )),
        None => None,
    };
    if existing_ownership.is_some() && tool_directory(&destination)? {
        require_managed_tool(&destination, &transaction_id)?;
    }

    let staging = unique_tool_path(&directory, &format!("{ENHANCED_TOOL_NAME}.patchops.tmp"));
    if let Err(error) = copy_directory(source, &staging) {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    if let Err(error) = update_compatibility_manifest(&staging.join("compatibilitytool.vdf")) {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    if let Err(error) = atomic_json(
        &staging.join(TOOL_MARKER_FILENAME),
        &json!({ "transaction_id": transaction_id }),
    ) {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }

    if new_ownership {
        let original_backup_name = original_backup
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .map(str::to_owned);
        if let Err(error) = save_tool_ownership(
            ownership_path,
            &ToolOwnership {
                version: TOOL_OWNERSHIP_VERSION,
                transaction_id: transaction_id.clone(),
                original_backup: original_backup_name,
            },
        ) {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
    }

    let displaced_managed = if !new_ownership && tool_directory(&destination)? {
        let displaced = unique_tool_path(
            &directory,
            &format!("{ENHANCED_TOOL_NAME}.patchops.rollback"),
        );
        if let Err(error) = fs::rename(&destination, &displaced) {
            let _ = fs::remove_dir_all(&staging);
            return Err(error.to_string());
        }
        Some(displaced)
    } else {
        None
    };
    if new_ownership
        && let Some(backup) = &original_backup
        && let Err(error) = fs::rename(&destination, backup)
    {
        let _ = fs::remove_dir_all(&staging);
        let _ = fs::remove_file(ownership_path);
        return Err(error.to_string());
    }
    if let Err(error) = fs::rename(&staging, &destination) {
        let _ = fs::remove_dir_all(&staging);
        let restore = if new_ownership {
            original_backup.as_ref()
        } else {
            displaced_managed.as_ref()
        };
        if let Some(restore) = restore
            && let Err(rollback_error) = fs::rename(restore, &destination)
        {
            return Err(format!(
                "{error}; restoring the previous compatibility tool also failed: {rollback_error}"
            ));
        }
        if new_ownership {
            let _ = fs::remove_file(ownership_path);
        }
        return Err(error.to_string());
    }
    Ok(ToolInstallTransaction {
        destination,
        displaced_managed,
        original_backup,
        ownership_path: ownership_path.to_path_buf(),
        transaction_id,
        new_ownership,
    })
}

fn remove_compatibility_tool_at(
    steam_root: &Path,
    ownership_path: &Path,
) -> Result<ToolRemovalTransaction, String> {
    let directory = steam_root.join("compatibilitytools.d");
    let ownership = load_tool_ownership(ownership_path)?.ok_or_else(|| {
        "No PatchOpsIII compatibility-tool ownership record exists; refusing unsafe cleanup."
            .to_string()
    })?;
    let destination = directory.join(ENHANCED_TOOL_NAME);
    let original_backup = exact_tool_backup(&directory, &ownership)?;
    let displaced_managed = if tool_directory(&destination)? {
        require_managed_tool(&destination, &ownership.transaction_id)?;
        let displaced =
            unique_tool_path(&directory, &format!("{ENHANCED_TOOL_NAME}.patchops.remove"));
        fs::rename(&destination, &displaced).map_err(|error| error.to_string())?;
        Some(displaced)
    } else {
        None
    };
    if let Some(backup) = &original_backup
        && let Err(error) = fs::rename(backup, &destination)
    {
        if let Some(displaced) = &displaced_managed
            && let Err(rollback) = fs::rename(displaced, &destination)
        {
            return Err(format!(
                "{error}; restoring the managed compatibility tool also failed: {rollback}"
            ));
        }
        return Err(error.to_string());
    }
    Ok(ToolRemovalTransaction {
        destination,
        displaced_managed,
        restored_backup: original_backup,
        ownership_path: ownership_path.to_path_buf(),
        transaction_id: ownership.transaction_id,
    })
}

#[derive(Clone)]
struct FileSnapshot {
    path: PathBuf,
    contents: Option<Vec<u8>>,
}

fn capture_files(paths: &[PathBuf]) -> Result<Vec<FileSnapshot>, String> {
    paths
        .iter()
        .map(|path| {
            let contents = match fs::symlink_metadata(path) {
                Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                    Some(fs::read(path).map_err(|error| {
                        format!("failed to snapshot {}: {error}", path.display())
                    })?)
                }
                Ok(_) => {
                    return Err(format!(
                        "Refusing to snapshot unsafe transaction path: {}",
                        path.display()
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(format!("{}: {error}", path.display())),
            };
            Ok(FileSnapshot {
                path: path.clone(),
                contents,
            })
        })
        .collect()
}

fn restore_files(snapshots: &[FileSnapshot]) -> Result<(), String> {
    let mut errors = Vec::new();
    for snapshot in snapshots.iter().rev() {
        let result = match &snapshot.contents {
            Some(contents) => fs_ops::atomic_write(&snapshot.path, contents),
            None => match fs::symlink_metadata(&snapshot.path) {
                Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                    fs::remove_file(&snapshot.path).map_err(|error| error.to_string())
                }
                Ok(_) => Err("transaction created an unsafe path".into()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.to_string()),
            },
        };
        if let Err(error) = result {
            errors.push(format!("{}: {error}", snapshot.path.display()));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn launch_restore_value(snapshot: &Path) -> Result<String, String> {
    required_json(snapshot)?
        .get("launch_options")
        .and_then(JsonValue::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            format!(
                "{} has invalid launch-option ownership data",
                snapshot.display()
            )
        })
}

fn set_enhanced_launch_options_at(
    config: &Path,
    legacy_backup: &Path,
    snapshot: &Path,
) -> Result<(), String> {
    match fs::symlink_metadata(snapshot) {
        Ok(_) => {
            launch_restore_value(snapshot)?;
            if read_launch_options_at(config, APP_ID)? != ENHANCED_LAUNCH_OPTIONS {
                return Err(
                    "Steam launch options changed after PatchOpsIII configured them; refusing to overwrite them."
                        .into(),
                );
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let previous = read_launch_options_at(config, APP_ID)?;
            atomic_json(snapshot, &json!({ "launch_options": previous }))?;
        }
        Err(error) => return Err(format!("{}: {error}", snapshot.display())),
    }
    set_launch_options_at(
        config,
        legacy_backup,
        APP_ID,
        ENHANCED_LAUNCH_OPTIONS,
        false,
        true,
    )
    .map(|_| ())
}

fn restore_enhanced_launch_options_at(
    config: &Path,
    legacy_backup: &Path,
    snapshot: &Path,
) -> Result<(), String> {
    let previous = launch_restore_value(snapshot)?;
    if read_launch_options_at(config, APP_ID)? != ENHANCED_LAUNCH_OPTIONS {
        return Err(
            "Steam launch options changed after PatchOpsIII configured them; leaving them untouched."
                .into(),
        );
    }
    set_launch_options_at(config, legacy_backup, APP_ID, &previous, false, true)?;
    fs::remove_file(snapshot).map_err(|error| format!("{}: {error}", snapshot.display()))
}

fn steam_transaction_files(
    steam_config: &Path,
    steam_backup: &Path,
    mapping_snapshot: &Path,
    local_config: &Path,
    local_backup: &Path,
    launch_snapshot: &Path,
) -> Vec<PathBuf> {
    vec![
        steam_config.to_path_buf(),
        fs_ops::backup_path(steam_config),
        steam_backup.to_path_buf(),
        mapping_snapshot.to_path_buf(),
        local_config.to_path_buf(),
        fs_ops::backup_path(local_config),
        local_backup.to_path_buf(),
        launch_snapshot.to_path_buf(),
    ]
}

fn rollback_error(error: String, file_error: Option<String>, tool_error: Option<String>) -> String {
    let mut rollbacks = Vec::new();
    if let Some(error) = file_error {
        rollbacks.push(format!("Steam file rollback failed: {error}"));
    }
    if let Some(error) = tool_error {
        rollbacks.push(format!("compatibility tool rollback failed: {error}"));
    }
    if rollbacks.is_empty() {
        error
    } else {
        format!("{error}; {}", rollbacks.join("; "))
    }
}

fn finish_tool_install(
    tool: ToolInstallTransaction,
    snapshots: Vec<FileSnapshot>,
    destination: PathBuf,
    result: Result<(), String>,
) -> Result<PathBuf, String> {
    match result {
        Ok(()) => match tool.commit() {
            Ok(()) => Ok(destination),
            Err(failure) => {
                let (error, tool) = *failure;
                let file_error = restore_files(&snapshots).err();
                let tool_error = tool.rollback().err();
                Err(rollback_error(error, file_error, tool_error))
            }
        },
        Err(error) => {
            let file_error = restore_files(&snapshots).err();
            let tool_error = tool.rollback().err();
            Err(rollback_error(error, file_error, tool_error))
        }
    }
}

fn finish_tool_removal(
    tool: ToolRemovalTransaction,
    snapshots: Vec<FileSnapshot>,
    result: Result<(), String>,
) -> Result<(), String> {
    match result {
        Ok(()) => match tool.commit() {
            Ok(()) => Ok(()),
            Err(failure) => {
                let (error, tool) = *failure;
                let file_error = restore_files(&snapshots).err();
                let tool_error = tool.rollback().err();
                Err(rollback_error(error, file_error, tool_error))
            }
        },
        Err(error) => {
            let file_error = restore_files(&snapshots).err();
            let tool_error = tool.rollback().err();
            Err(rollback_error(error, file_error, tool_error))
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn configure_compatibility_at(
    steam_root: &Path,
    source: &Path,
    trusted_legacy_source: Option<&Path>,
    ownership_path: &Path,
    steam_config: &Path,
    steam_backup: &Path,
    mapping_snapshot: &Path,
    local_config: &Path,
    local_backup: &Path,
    launch_snapshot: &Path,
) -> Result<PathBuf, String> {
    let snapshots = capture_files(&steam_transaction_files(
        steam_config,
        steam_backup,
        mapping_snapshot,
        local_config,
        local_backup,
        launch_snapshot,
    ))?;
    let existing_ownership = load_tool_ownership(ownership_path)?;
    if existing_ownership.is_some() {
        mapping_restore_value(mapping_snapshot)?;
        launch_restore_value(launch_snapshot)?;
    }
    let destination = steam_root
        .join("compatibilitytools.d")
        .join(ENHANCED_TOOL_NAME);
    let destination_exists = match fs::symlink_metadata(&destination) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(format!("{}: {error}", destination.display())),
    };
    let adoption = if existing_ownership.is_none() && destination_exists {
        Some(adopt_legacy_compatibility_tool_at(
            steam_root,
            trusted_legacy_source.ok_or_else(|| {
                "The exact release10-32 source is required to adopt a legacy compatibility tool."
                    .to_string()
            })?,
            ownership_path,
            steam_config,
            mapping_snapshot,
            local_config,
            launch_snapshot,
        )?)
    } else {
        None
    };
    let result = (|| {
        let tool = install_compatibility_tool_at(steam_root, source, ownership_path)?;
        let destination = tool.destination.clone();
        let result = (|| {
            set_compatibility_mapping_at(steam_config, steam_backup, mapping_snapshot)?;
            set_enhanced_launch_options_at(local_config, local_backup, launch_snapshot)
        })();
        finish_tool_install(tool, snapshots, destination, result)
    })();
    finish_legacy_adoption(adoption, result)
}

#[allow(clippy::too_many_arguments)]
fn cleanup_compatibility_at(
    steam_root: &Path,
    trusted_legacy_source: Option<&Path>,
    ownership_path: &Path,
    steam_config: &Path,
    steam_backup: &Path,
    mapping_snapshot: &Path,
    local_config: &Path,
    local_backup: &Path,
    launch_snapshot: &Path,
) -> Result<(), String> {
    if compatibility_is_fully_clean(
        steam_root,
        ownership_path,
        steam_config,
        mapping_snapshot,
        local_config,
        launch_snapshot,
    )? {
        return Ok(());
    }
    let snapshots = capture_files(&steam_transaction_files(
        steam_config,
        steam_backup,
        mapping_snapshot,
        local_config,
        local_backup,
        launch_snapshot,
    ))?;
    let adoption = if load_tool_ownership(ownership_path)?.is_none() {
        Some(adopt_legacy_compatibility_tool_at(
            steam_root,
            trusted_legacy_source.ok_or_else(|| {
                "The exact release10-32 source is required to adopt a legacy compatibility tool."
                    .to_string()
            })?,
            ownership_path,
            steam_config,
            mapping_snapshot,
            local_config,
            launch_snapshot,
        )?)
    } else {
        None
    };
    let result = (|| {
        let tool = remove_compatibility_tool_at(steam_root, ownership_path)?;
        let result = (|| {
            clear_compatibility_mapping_at(steam_config, steam_backup, mapping_snapshot)?;
            restore_enhanced_launch_options_at(local_config, local_backup, launch_snapshot)
        })();
        finish_tool_removal(tool, snapshots, result)
    })();
    finish_legacy_adoption(adoption, result)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_proton_layout(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return false;
    }
    let Ok(root) = path.canonicalize() else {
        return false;
    };
    [("proton", false), ("files", true)]
        .into_iter()
        .all(|(name, directory)| {
            let entry = path.join(name);
            let Ok(resolved) = entry.canonicalize() else {
                return false;
            };
            resolved.starts_with(&root)
                && fs::metadata(resolved).is_ok_and(|metadata| {
                    if directory {
                        metadata.is_dir()
                    } else {
                        metadata.is_file()
                    }
                })
        })
}

fn valid_cached_proton(path: &Path) -> bool {
    if !valid_proton_layout(path) {
        return false;
    }
    let Ok(manifest) = read_vdf(&path.join("compatibilitytool.vdf")) else {
        return false;
    };
    if !matches!(
        value_at(
            &manifest,
            &["compatibilitytools", "compat_tools", ENHANCED_TOOL_NAME]
        ),
        Some(VdfValue::Object(_))
    ) {
        return false;
    }
    let Ok(marker) = required_json(&path.join(".patchops-source.json")) else {
        return false;
    };
    marker.get("tag").and_then(JsonValue::as_str) == Some(ENHANCED_PROTON_TAG)
        && marker.get("asset").and_then(JsonValue::as_str) == Some(ENHANCED_PROTON_ASSET)
        && marker
            .get("sha256")
            .and_then(JsonValue::as_str)
            .is_some_and(|hash| hash.eq_ignore_ascii_case(ENHANCED_PROTON_SHA256))
}

#[derive(Debug)]
struct TrustedProtonSource {
    temporary_root: PathBuf,
    source: PathBuf,
}

impl Drop for TrustedProtonSource {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.temporary_root);
    }
}

fn trusted_proton_source_at(
    cache_root: &Path,
    expected_digest: &str,
) -> Result<TrustedProtonSource, String> {
    if !valid_sha256(expected_digest) {
        return Err("Pinned GDK-Proton archive digest is invalid.".into());
    }
    let archive = cache_root.join(format!("GDK-Proton-{ENHANCED_PROTON_TAG}.tar.gz"));
    let metadata = fs::symlink_metadata(&archive)
        .map_err(|error| format!("Verified GDK-Proton archive is unavailable: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("Verified GDK-Proton archive is not a regular file.".into());
    }
    if !fs_ops::sha256_file(&archive)?.eq_ignore_ascii_case(expected_digest) {
        return Err("Cached GDK-Proton archive failed release10-32 verification.".into());
    }

    let temporary_root = cache_root
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!(".patchops-trusted-proton-{}", unique_suffix()));
    fs::create_dir(&temporary_root).map_err(|error| error.to_string())?;
    let result = (|| {
        let extraction = temporary_root.join("extracted");
        fs_ops::extract_tar_gz(&archive, &extraction)?;
        if !fs_ops::sha256_file(&archive)?.eq_ignore_ascii_case(expected_digest) {
            return Err("Cached GDK-Proton archive changed during verification.".into());
        }
        let mut candidates = Vec::new();
        if valid_proton_layout(&extraction) {
            candidates.push(extraction.clone());
        }
        for entry in fs::read_dir(&extraction)
            .map_err(|error| format!("{}: {error}", extraction.display()))?
        {
            let path = entry.map_err(|error| error.to_string())?.path();
            if valid_proton_layout(&path) {
                candidates.push(path);
            }
        }
        if candidates.len() != 1 {
            return Err(
                "Verified GDK-Proton archive did not contain one unambiguous tool directory."
                    .into(),
            );
        }
        let source = temporary_root.join("normalized");
        copy_directory(&candidates[0], &source)?;
        update_compatibility_manifest(&source.join("compatibilitytool.vdf"))?;
        fs::remove_dir_all(&extraction).map_err(|error| error.to_string())?;
        Ok(source)
    })();
    match result {
        Ok(source) => Ok(TrustedProtonSource {
            temporary_root,
            source,
        }),
        Err(error) => {
            let _ = fs::remove_dir_all(&temporary_root);
            Err(error)
        }
    }
}

fn prepare_enhanced_proton_cache(app: &AppState) -> Result<PathBuf, String> {
    let cache_root = app.data_dir().join("bo3-enhanced-proton-cache");
    let target = cache_root.join(ENHANCED_TOOL_NAME);
    if valid_cached_proton(&target)
        && let Ok(trusted) = trusted_proton_source_at(&cache_root, ENHANCED_PROTON_SHA256)
        && compare_tool_trees(&trusted.source, &target, true, true).is_ok()
    {
        return Ok(target);
    }
    fs::create_dir_all(&cache_root).map_err(|error| error.to_string())?;
    let digest = ENHANCED_PROTON_SHA256.to_owned();
    let archive = cache_root.join(format!("GDK-Proton-{ENHANCED_PROTON_TAG}.tar.gz"));
    let archive_valid = fs::symlink_metadata(&archive).is_ok_and(|metadata| {
        !metadata.file_type().is_symlink()
            && metadata.is_file()
            && fs_ops::sha256_file(&archive)
                .is_ok_and(|actual| actual.eq_ignore_ascii_case(&digest))
    });
    if !archive_valid {
        match fs::symlink_metadata(&archive) {
            Ok(_) => fs::remove_file(&archive).map_err(|error| error.to_string())?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        app.log(
            "Info",
            format!("Downloading BO3 Enhanced Proton ({ENHANCED_PROTON_TAG})..."),
        );
        fs_ops::download(
            app,
            ENHANCED_PROTON_DOWNLOAD,
            &archive,
            Some(&digest),
            MAX_PROTON_ARCHIVE_BYTES,
        )?;
    }

    let suffix = unique_suffix();
    let extraction = cache_root.join(format!("extract-{ENHANCED_PROTON_TAG}-{suffix}"));
    let staging = cache_root.join(format!("{ENHANCED_TOOL_NAME}.tmp-{suffix}"));
    let backup = cache_root.join(format!("{ENHANCED_TOOL_NAME}.patchops.bak-{suffix}"));
    let result = (|| {
        fs_ops::extract_tar_gz(&archive, &extraction)?;
        let source = if valid_proton_layout(&extraction) {
            extraction.clone()
        } else {
            fs::read_dir(&extraction)
                .map_err(|error| error.to_string())?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .find(|path| valid_proton_layout(path))
                .ok_or_else(|| {
                    "Verified GDK-Proton archive did not contain a valid tool directory."
                        .to_string()
                })?
        };
        fs::rename(&source, &staging).map_err(|error| error.to_string())?;
        update_compatibility_manifest(&staging.join("compatibilitytool.vdf"))?;
        atomic_json(
            &staging.join(".patchops-source.json"),
            &json!({
                "tag": ENHANCED_PROTON_TAG,
                "asset": ENHANCED_PROTON_ASSET,
                "sha256": digest,
            }),
        )?;
        if !valid_cached_proton(&staging) {
            return Err("Prepared GDK-Proton cache failed validation.".into());
        }

        let moved_existing = if tool_directory(&target)? {
            fs::rename(&target, &backup).map_err(|error| error.to_string())?;
            true
        } else {
            false
        };
        if let Err(error) = fs::rename(&staging, &target) {
            if moved_existing && let Err(rollback_error) = fs::rename(&backup, &target) {
                return Err(format!(
                    "{error}; restoring the previous Proton cache also failed: {rollback_error}"
                ));
            }
            return Err(error.to_string());
        }
        if moved_existing {
            let _ = fs::remove_dir_all(&backup);
        }
        Ok(target.clone())
    })();
    let _ = fs::remove_dir_all(&extraction);
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    if result.is_ok() {
        app.log(
            "Success",
            format!(
                "Prepared verified BO3 Enhanced Proton cache at {}.",
                target.display()
            ),
        );
    }
    result
}

fn resolve_enhanced_tool_source(
    app: &AppState,
    preferred: Option<&Path>,
) -> Result<PathBuf, String> {
    if let Some(preferred) = preferred {
        if valid_proton_layout(preferred) {
            return Ok(preferred.to_path_buf());
        }
        return Err(format!(
            "Compatibility tool source not found: {}",
            preferred.display()
        ));
    }
    if let Some(resources) = app.resource_dir() {
        let bundled = resources.join("bo3-enhanced-proton/BO3 Enhanced");
        if valid_proton_layout(&bundled) {
            return Ok(bundled);
        }
    }
    prepare_enhanced_proton_cache(app)
}

pub fn configure_bo3_enhanced_linux(
    app: &AppState,
    tool_source: Option<&Path>,
) -> Result<(), String> {
    let root = steam_root().ok_or_else(|| "Steam root path could not be resolved.".to_string())?;
    let user = user_id_in(&root).ok_or_else(|| "Steam user ID not found.".to_string())?;
    let local_config = local_config_path_in(&root, &user)
        .ok_or_else(|| "Steam userdata path not found.".to_string())?;
    let steam_config = root.join("config/config.vdf");
    let steam_backup = app.data_dir().join("backups/steam_config_backup.vdf");
    let local_backup = app.data_dir().join("backups/localconfig_backup.vdf");
    let mapping_snapshot = mapping_snapshot_path(app.data_dir());
    let launch_snapshot = launch_snapshot_path(app.data_dir());
    let ownership = tool_ownership_path(app.data_dir());
    let destination = root.join("compatibilitytools.d").join(ENHANCED_TOOL_NAME);
    let destination_exists = match fs::symlink_metadata(&destination) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(format!("{}: {error}", destination.display())),
    };
    let trusted = if load_tool_ownership(&ownership)?.is_none() && destination_exists {
        Some(trusted_proton_source_at(
            &app.data_dir().join("bo3-enhanced-proton-cache"),
            ENHANCED_PROTON_SHA256,
        )?)
    } else {
        None
    };
    let source = match &trusted {
        Some(trusted) => trusted.source.clone(),
        None => resolve_enhanced_tool_source(app, tool_source)?,
    };

    close_steam(app)?;
    let result = configure_compatibility_at(
        &root,
        &source,
        trusted.as_ref().map(|trusted| trusted.source.as_path()),
        &ownership,
        &steam_config,
        &steam_backup,
        &mapping_snapshot,
        &local_config,
        &local_backup,
        &launch_snapshot,
    )
    .map(|destination| {
        app.log(
            "Success",
            format!(
                "Installed '{ENHANCED_TOOL_NAME}' to {}.",
                destination.display()
            ),
        );
    });
    if let Err(error) = open_steam(app) {
        app.log("Warning", format!("Steam could not be reopened: {error}"));
    }
    if result.is_ok() {
        app.log(
            "Success",
            "Linux BO3 Enhanced compatibility tool, mapping, and launch options configured.",
        );
    }
    result
}

pub fn cleanup_bo3_enhanced_linux(app: &AppState) -> Result<(), String> {
    let root = steam_root().ok_or_else(|| "Steam root path could not be resolved.".to_string())?;
    let user = user_id_in(&root).ok_or_else(|| "Steam user ID not found.".to_string())?;
    let local_config = local_config_path_in(&root, &user)
        .ok_or_else(|| "Steam userdata path not found.".to_string())?;
    let steam_config = root.join("config/config.vdf");
    let steam_backup = app.data_dir().join("backups/steam_config_backup.vdf");
    let local_backup = app.data_dir().join("backups/localconfig_backup.vdf");
    let mapping_snapshot = mapping_snapshot_path(app.data_dir());
    let launch_snapshot = launch_snapshot_path(app.data_dir());
    let ownership = tool_ownership_path(app.data_dir());
    if compatibility_is_fully_clean(
        &root,
        &ownership,
        &steam_config,
        &mapping_snapshot,
        &local_config,
        &launch_snapshot,
    )? {
        app.log(
            "Info",
            "Linux BO3 Enhanced compatibility state was already clean.",
        );
        return Ok(());
    }
    let trusted = if load_tool_ownership(&ownership)?.is_none() {
        Some(trusted_proton_source_at(
            &app.data_dir().join("bo3-enhanced-proton-cache"),
            ENHANCED_PROTON_SHA256,
        )?)
    } else {
        None
    };
    close_steam(app)?;

    let result = cleanup_compatibility_at(
        &root,
        trusted.as_ref().map(|trusted| trusted.source.as_path()),
        &ownership,
        &steam_config,
        &steam_backup,
        &mapping_snapshot,
        &local_config,
        &local_backup,
        &launch_snapshot,
    );
    if let Err(error) = open_steam(app) {
        app.log("Warning", format!("Steam could not be reopened: {error}"));
    }
    if result.is_ok() {
        app.log(
            "Success",
            "Linux BO3 Enhanced compatibility mapping, launch options, and tool removed.",
        );
    }
    result
}

#[cfg(test)]
mod tests {
    use super::super::tests::{fixture_dir, steam_config, string_value};
    use super::*;

    fn tool_source(root: &Path) -> PathBuf {
        let source = root.join("source");
        fs::create_dir_all(source.join("files")).unwrap();
        fs::write(source.join("proton"), b"managed proton").unwrap();
        source
    }

    fn normalized_tool_source(root: &Path, name: &str, proton: &[u8]) -> PathBuf {
        let source = root.join(name);
        fs::create_dir_all(source.join("files")).unwrap();
        fs::write(source.join("proton"), proton).unwrap();
        fs::write(source.join("files/payload"), b"release payload").unwrap();
        update_compatibility_manifest(&source.join("compatibilitytool.vdf")).unwrap();
        source
    }

    fn compatibility_config() -> &'static str {
        r#""InstallConfigStore" {
  "Unrelated" "preserve"
  "Software" { "Valve" { "Steam" { "CompatToolMapping" {
    "311210" { "name" "GE-Proton" "priority" "75" "custom" "yes" }
  } } } }
}"#
    }

    fn enhanced_compatibility_config() -> &'static str {
        r#""InstallConfigStore"
{
  "Software"
  {
    "Valve"
    {
      "Steam"
      {
        "CompatToolMapping"
        {
          "311210" { "name" "BO3 Enhanced" "config" "" "priority" "250" }
        }
      }
    }
  }
}"#
    }

    struct LegacyCompatibilityFixture {
        root: PathBuf,
        steam: PathBuf,
        trusted: PathBuf,
        destination: PathBuf,
        steam_vdf: PathBuf,
        local_vdf: PathBuf,
        steam_backup: PathBuf,
        local_backup: PathBuf,
        mapping_snapshot: PathBuf,
        launch_snapshot: PathBuf,
        ownership: PathBuf,
    }

    fn legacy_compatibility_fixture(label: &str) -> LegacyCompatibilityFixture {
        let root = fixture_dir(label);
        let steam = root.join("steam");
        let trusted = normalized_tool_source(&root, "trusted", b"legacy proton");
        let destination = steam.join("compatibilitytools.d").join(ENHANCED_TOOL_NAME);
        copy_directory(&trusted, &destination).unwrap();
        let steam_vdf = steam.join("config/config.vdf");
        let local_vdf = steam.join("userdata/1/config/localconfig.vdf");
        fs::create_dir_all(steam_vdf.parent().unwrap()).unwrap();
        fs::create_dir_all(local_vdf.parent().unwrap()).unwrap();
        fs::write(&steam_vdf, enhanced_compatibility_config()).unwrap();
        fs::write(&local_vdf, steam_config(ENHANCED_LAUNCH_OPTIONS)).unwrap();
        let backups = root.join("data/backups");
        let mapping_snapshot = backups.join("compat_mapping_311210.json");
        let launch_snapshot = backups.join("launch_options_311210.json");
        let ownership = backups.join("compat_tool_311210.json");
        atomic_json(
            &mapping_snapshot,
            &json!({
                "had_entry": true,
                "entry": { "name": "GE-Proton", "priority": "75", "custom": "yes" }
            }),
        )
        .unwrap();
        atomic_json(&launch_snapshot, &json!({ "launch_options": "-novid" })).unwrap();
        LegacyCompatibilityFixture {
            root,
            steam,
            trusted,
            destination,
            steam_vdf,
            local_vdf,
            steam_backup: backups.join("steam_config_backup.vdf"),
            local_backup: backups.join("localconfig_backup.vdf"),
            mapping_snapshot,
            launch_snapshot,
            ownership,
        }
    }

    fn assert_legacy_unowned(fixture: &LegacyCompatibilityFixture) {
        assert!(!fixture.ownership.exists());
        assert!(!fixture.destination.join(TOOL_MARKER_FILENAME).exists());
        assert_eq!(
            fs::read(fixture.destination.join("proton")).unwrap(),
            b"legacy proton"
        );
        assert_eq!(
            read_launch_options_at(&fixture.local_vdf, APP_ID).unwrap(),
            ENHANCED_LAUNCH_OPTIONS
        );
    }

    fn adopt_fixture(fixture: &LegacyCompatibilityFixture) -> Result<LegacyToolAdoption, String> {
        adopt_legacy_compatibility_tool_at(
            &fixture.steam,
            &fixture.trusted,
            &fixture.ownership,
            &fixture.steam_vdf,
            &fixture.mapping_snapshot,
            &fixture.local_vdf,
            &fixture.launch_snapshot,
        )
    }

    fn proton_archive_fixture(cache_root: &Path) -> (PathBuf, String) {
        use flate2::{Compression, write::GzEncoder};
        use tar::{Builder, EntryType, Header};

        fs::create_dir_all(cache_root).unwrap();
        let archive = cache_root.join(format!("GDK-Proton-{ENHANCED_PROTON_TAG}.tar.gz"));
        let encoder = GzEncoder::new(fs::File::create(&archive).unwrap(), Compression::default());
        let mut builder = Builder::new(encoder);
        for path in [
            "GDK-Proton10-32/",
            "GDK-Proton10-32/files/",
            "GDK-Proton10-32/protonfixes/",
            "GDK-Proton10-32/protonfixes/gamefixes-steam/",
            "GDK-Proton10-32/protonfixes/gamefixes-umu/",
        ] {
            let mut header = Header::new_gnu();
            header.set_entry_type(EntryType::Directory);
            header.set_size(0);
            header.set_mode(0o755);
            header.set_path(path).unwrap();
            header.set_cksum();
            builder.append(&header, std::io::empty()).unwrap();
        }
        {
            let mut append = |path: &str, contents: &[u8]| {
                let mut header = Header::new_gnu();
                header.set_entry_type(EntryType::Regular);
                header.set_size(contents.len() as u64);
                header.set_mode(0o755);
                header.set_path(path).unwrap();
                header.set_cksum();
                builder.append(&header, contents).unwrap();
            };
            append("GDK-Proton10-32/proton", b"proton");
            append("GDK-Proton10-32/files/payload", b"payload");
            append(
                "GDK-Proton10-32/compatibilitytool.vdf",
                br#""compatibilitytools" { "compat_tools" { "GDK-Proton10-32" { "install_path" "." "display_name" "GDK-Proton10-32" "from_oslist" "windows" "to_oslist" "linux" } } }"#,
            );
            append(
                "GDK-Proton10-32/protonfixes/gamefixes-steam/271590.py",
                b"fix",
            );
        }
        let mut link = Header::new_gnu();
        link.set_entry_type(EntryType::Symlink);
        link.set_size(0);
        link.set_mode(0o777);
        link.set_path("GDK-Proton10-32/protonfixes/gamefixes-umu/umu-271590.py")
            .unwrap();
        link.set_link_name("../gamefixes-steam/271590.py").unwrap();
        link.set_cksum();
        builder.append(&link, std::io::empty()).unwrap();
        builder.into_inner().unwrap().finish().unwrap();
        let digest = fs_ops::sha256_file(&archive).unwrap();
        (archive, digest)
    }

    #[test]
    fn steam_exit_wait_is_bounded_and_needs_no_test_sleep() {
        let mut checks = 0;
        let mut pauses = 0;
        wait_for_steam_exit_with(
            5,
            || {
                checks += 1;
                Ok(checks < 3)
            },
            || pauses += 1,
        )
        .unwrap();
        assert_eq!(checks, 3);
        assert_eq!(pauses, 2);

        let mut timeout_checks = 0;
        let error = wait_for_steam_exit_with(
            3,
            || {
                timeout_checks += 1;
                Ok(true)
            },
            || {},
        )
        .unwrap_err();
        assert_eq!(timeout_checks, 3);
        assert!(error.contains("did not exit"));
    }

    #[test]
    fn mapping_snapshot_restores_prior_entry_and_unknown_fields() {
        let root = fixture_dir("mapping");
        let config = root.join("config.vdf");
        let legacy = root.join("backups/steam_config_backup.vdf");
        let snapshot = root.join("backups/compat_mapping_311210.json");
        fs::write(
            &config,
            r#"
"InstallConfigStore" {
  "Unrelated" "preserve"
  "Software" { "Valve" { "Steam" { "CompatToolMapping" {
    "311210" { "name" "GE-Proton" "priority" "75" "custom" "yes" }
    "other" { "name" "Other Tool" }
  } } } }
}
"#,
        )
        .unwrap();

        set_compatibility_mapping_at(&config, &legacy, &snapshot).unwrap();
        let mapped = read_vdf(&config).unwrap();
        assert_eq!(
            string_value(
                &mapped,
                &[
                    "InstallConfigStore",
                    "Software",
                    "Valve",
                    "Steam",
                    "CompatToolMapping",
                    APP_ID,
                    "name",
                ],
            ),
            ENHANCED_TOOL_NAME
        );
        assert_eq!(
            string_value(&mapped, &["InstallConfigStore", "Unrelated"]),
            "preserve"
        );

        clear_compatibility_mapping_at(&config, &legacy, &snapshot).unwrap();
        let restored = read_vdf(&config).unwrap();
        assert_eq!(
            string_value(
                &restored,
                &[
                    "InstallConfigStore",
                    "Software",
                    "Valve",
                    "Steam",
                    "CompatToolMapping",
                    APP_ID,
                    "name",
                ],
            ),
            "GE-Proton"
        );
        assert_eq!(
            string_value(
                &restored,
                &[
                    "InstallConfigStore",
                    "Software",
                    "Valve",
                    "Steam",
                    "CompatToolMapping",
                    APP_ID,
                    "custom",
                ],
            ),
            "yes"
        );
        assert!(!snapshot.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exact_legacy_tool_is_adopted_and_cleaned_up() {
        let fixture = legacy_compatibility_fixture("legacy-cleanup");

        cleanup_compatibility_at(
            &fixture.steam,
            Some(&fixture.trusted),
            &fixture.ownership,
            &fixture.steam_vdf,
            &fixture.steam_backup,
            &fixture.mapping_snapshot,
            &fixture.local_vdf,
            &fixture.local_backup,
            &fixture.launch_snapshot,
        )
        .unwrap();

        assert!(!fixture.destination.exists());
        assert!(!fixture.ownership.exists());
        assert!(!fixture.mapping_snapshot.exists());
        assert!(!fixture.launch_snapshot.exists());
        assert_eq!(
            string_value(
                &read_vdf(&fixture.steam_vdf).unwrap(),
                &[
                    "InstallConfigStore",
                    "Software",
                    "Valve",
                    "Steam",
                    "CompatToolMapping",
                    APP_ID,
                    "name",
                ],
            ),
            "GE-Proton"
        );
        assert_eq!(
            read_launch_options_at(&fixture.local_vdf, APP_ID).unwrap(),
            "-novid"
        );
        fs::remove_dir_all(fixture.root).unwrap();
    }

    #[test]
    fn exact_legacy_tool_can_be_upgraded_then_cleaned_up() {
        let fixture = legacy_compatibility_fixture("legacy-upgrade");
        let upgrade = normalized_tool_source(&fixture.root, "upgrade", b"upgraded proton");

        configure_compatibility_at(
            &fixture.steam,
            &upgrade,
            Some(&fixture.trusted),
            &fixture.ownership,
            &fixture.steam_vdf,
            &fixture.steam_backup,
            &fixture.mapping_snapshot,
            &fixture.local_vdf,
            &fixture.local_backup,
            &fixture.launch_snapshot,
        )
        .unwrap();
        assert_eq!(
            fs::read(fixture.destination.join("proton")).unwrap(),
            b"upgraded proton"
        );
        assert_eq!(
            load_tool_ownership(&fixture.ownership)
                .unwrap()
                .unwrap()
                .original_backup,
            None
        );

        cleanup_compatibility_at(
            &fixture.steam,
            None,
            &fixture.ownership,
            &fixture.steam_vdf,
            &fixture.steam_backup,
            &fixture.mapping_snapshot,
            &fixture.local_vdf,
            &fixture.local_backup,
            &fixture.launch_snapshot,
        )
        .unwrap();
        assert!(!fixture.destination.exists());
        assert_eq!(
            read_launch_options_at(&fixture.local_vdf, APP_ID).unwrap(),
            "-novid"
        );
        fs::remove_dir_all(fixture.root).unwrap();
    }

    #[test]
    fn compatibility_cleanup_is_idempotent_only_for_the_fully_clean_tuple() {
        let clean = legacy_compatibility_fixture("compat-clean-retry");
        fs::remove_dir_all(&clean.destination).unwrap();
        fs::remove_file(&clean.mapping_snapshot).unwrap();
        fs::remove_file(&clean.launch_snapshot).unwrap();
        fs::write(&clean.steam_vdf, compatibility_config()).unwrap();
        fs::write(&clean.local_vdf, steam_config("-novid")).unwrap();
        let steam_before = fs::read(&clean.steam_vdf).unwrap();
        let local_before = fs::read(&clean.local_vdf).unwrap();

        cleanup_compatibility_at(
            &clean.steam,
            None,
            &clean.ownership,
            &clean.steam_vdf,
            &clean.steam_backup,
            &clean.mapping_snapshot,
            &clean.local_vdf,
            &clean.local_backup,
            &clean.launch_snapshot,
        )
        .unwrap();

        assert_eq!(fs::read(&clean.steam_vdf).unwrap(), steam_before);
        assert_eq!(fs::read(&clean.local_vdf).unwrap(), local_before);
        assert!(!clean.ownership.exists());
        assert!(!clean.destination.exists());
        fs::remove_dir_all(clean.root).unwrap();
    }

    #[test]
    fn compatibility_cleanup_rejects_partial_unowned_state_without_mutation() {
        let stale_snapshot = legacy_compatibility_fixture("compat-partial-snapshot");
        fs::remove_dir_all(&stale_snapshot.destination).unwrap();
        fs::remove_file(&stale_snapshot.launch_snapshot).unwrap();
        fs::write(&stale_snapshot.steam_vdf, compatibility_config()).unwrap();
        fs::write(&stale_snapshot.local_vdf, steam_config("-novid")).unwrap();
        let mapping_before = fs::read(&stale_snapshot.mapping_snapshot).unwrap();
        let error = cleanup_compatibility_at(
            &stale_snapshot.steam,
            Some(&stale_snapshot.trusted),
            &stale_snapshot.ownership,
            &stale_snapshot.steam_vdf,
            &stale_snapshot.steam_backup,
            &stale_snapshot.mapping_snapshot,
            &stale_snapshot.local_vdf,
            &stale_snapshot.local_backup,
            &stale_snapshot.launch_snapshot,
        )
        .unwrap_err();
        assert!(error.contains("missing"), "{error}");
        assert_eq!(
            fs::read(&stale_snapshot.mapping_snapshot).unwrap(),
            mapping_before
        );
        assert!(!stale_snapshot.ownership.exists());
        assert!(!stale_snapshot.destination.exists());
        fs::remove_dir_all(stale_snapshot.root).unwrap();

        let managed_values = legacy_compatibility_fixture("compat-partial-values");
        fs::remove_dir_all(&managed_values.destination).unwrap();
        fs::remove_file(&managed_values.mapping_snapshot).unwrap();
        fs::remove_file(&managed_values.launch_snapshot).unwrap();
        let steam_before = fs::read(&managed_values.steam_vdf).unwrap();
        let local_before = fs::read(&managed_values.local_vdf).unwrap();
        let error = cleanup_compatibility_at(
            &managed_values.steam,
            Some(&managed_values.trusted),
            &managed_values.ownership,
            &managed_values.steam_vdf,
            &managed_values.steam_backup,
            &managed_values.mapping_snapshot,
            &managed_values.local_vdf,
            &managed_values.local_backup,
            &managed_values.launch_snapshot,
        )
        .unwrap_err();
        assert!(error.contains("missing"), "{error}");
        assert_eq!(fs::read(&managed_values.steam_vdf).unwrap(), steam_before);
        assert_eq!(fs::read(&managed_values.local_vdf).unwrap(), local_before);
        assert!(!managed_values.ownership.exists());
        fs::remove_dir_all(managed_values.root).unwrap();
    }

    #[test]
    fn owned_reconfigure_requires_both_restore_snapshots_before_mutation() {
        for (label, remove_mapping) in [
            ("owned-missing-mapping", true),
            ("owned-missing-launch", false),
        ] {
            let fixture = legacy_compatibility_fixture(label);
            let installed = normalized_tool_source(&fixture.root, "installed", b"installed");
            configure_compatibility_at(
                &fixture.steam,
                &installed,
                Some(&fixture.trusted),
                &fixture.ownership,
                &fixture.steam_vdf,
                &fixture.steam_backup,
                &fixture.mapping_snapshot,
                &fixture.local_vdf,
                &fixture.local_backup,
                &fixture.launch_snapshot,
            )
            .unwrap();
            let missing = if remove_mapping {
                &fixture.mapping_snapshot
            } else {
                &fixture.launch_snapshot
            };
            fs::remove_file(missing).unwrap();
            let ownership_before = fs::read(&fixture.ownership).unwrap();
            let marker_before = fs::read(fixture.destination.join(TOOL_MARKER_FILENAME)).unwrap();
            let steam_before = fs::read(&fixture.steam_vdf).unwrap();
            let local_before = fs::read(&fixture.local_vdf).unwrap();
            let replacement = normalized_tool_source(&fixture.root, "replacement", b"replacement");

            let error = configure_compatibility_at(
                &fixture.steam,
                &replacement,
                None,
                &fixture.ownership,
                &fixture.steam_vdf,
                &fixture.steam_backup,
                &fixture.mapping_snapshot,
                &fixture.local_vdf,
                &fixture.local_backup,
                &fixture.launch_snapshot,
            )
            .unwrap_err();
            assert!(error.contains(&missing.display().to_string()), "{error}");
            assert!(!missing.exists());
            assert_eq!(fs::read(&fixture.ownership).unwrap(), ownership_before);
            assert_eq!(
                fs::read(fixture.destination.join(TOOL_MARKER_FILENAME)).unwrap(),
                marker_before
            );
            assert_eq!(
                fs::read(fixture.destination.join("proton")).unwrap(),
                b"installed"
            );
            assert_eq!(fs::read(&fixture.steam_vdf).unwrap(), steam_before);
            assert_eq!(fs::read(&fixture.local_vdf).unwrap(), local_before);
            fs::remove_dir_all(fixture.root).unwrap();
        }
    }

    #[test]
    fn legacy_adoption_rejects_missing_or_mismatched_state_without_mutation() {
        let missing = legacy_compatibility_fixture("legacy-missing-snapshot");
        fs::remove_file(&missing.launch_snapshot).unwrap();
        assert!(
            adopt_fixture(&missing)
                .unwrap_err()
                .contains("launch_options")
        );
        assert_legacy_unowned(&missing);
        fs::remove_dir_all(missing.root).unwrap();

        let mapping = legacy_compatibility_fixture("legacy-mapping-mismatch");
        fs::write(&mapping.steam_vdf, compatibility_config()).unwrap();
        assert!(adopt_fixture(&mapping).unwrap_err().contains("mapping"));
        assert_legacy_unowned(&mapping);
        fs::remove_dir_all(mapping.root).unwrap();

        let launch = legacy_compatibility_fixture("legacy-launch-mismatch");
        fs::write(&launch.local_vdf, steam_config("-novid")).unwrap();
        assert!(
            adopt_fixture(&launch)
                .unwrap_err()
                .contains("launch options")
        );
        assert!(!launch.ownership.exists());
        assert!(!launch.destination.join(TOOL_MARKER_FILENAME).exists());
        fs::remove_dir_all(launch.root).unwrap();
    }

    #[test]
    fn legacy_adoption_rejects_payload_mismatch_and_backup_sibling_without_mutation() {
        let mismatch = legacy_compatibility_fixture("legacy-payload-mismatch");
        fs::write(mismatch.destination.join("proton"), b"modified").unwrap();
        assert!(
            adopt_fixture(&mismatch)
                .unwrap_err()
                .contains("bytes differ")
        );
        assert!(!mismatch.ownership.exists());
        assert!(!mismatch.destination.join(TOOL_MARKER_FILENAME).exists());
        assert_eq!(
            fs::read(mismatch.destination.join("proton")).unwrap(),
            b"modified"
        );
        fs::remove_dir_all(mismatch.root).unwrap();

        let backup = legacy_compatibility_fixture("legacy-backup");
        let ambiguous = backup
            .steam
            .join("compatibilitytools.d/BO3 Enhanced.patchops.bak-20240101-000000");
        fs::create_dir_all(&ambiguous).unwrap();
        assert!(adopt_fixture(&backup).unwrap_err().contains("ambiguous"));
        assert_legacy_unowned(&backup);
        assert!(ambiguous.exists());
        fs::remove_dir_all(backup.root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn legacy_adoption_rejects_a_symlinked_payload_without_mutation() {
        use std::os::unix::fs::symlink;

        let fixture = legacy_compatibility_fixture("legacy-symlink");
        fs::remove_file(fixture.destination.join("files/payload")).unwrap();
        symlink(
            fixture.trusted.join("files/payload"),
            fixture.destination.join("files/payload"),
        )
        .unwrap();

        assert!(adopt_fixture(&fixture).unwrap_err().contains("unsafe link"));
        assert!(!fixture.ownership.exists());
        assert!(!fixture.destination.join(TOOL_MARKER_FILENAME).exists());
        assert!(
            fs::symlink_metadata(fixture.destination.join("files/payload"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        fs::remove_dir_all(fixture.root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn safe_relative_source_links_are_materialized_and_escaping_links_are_rejected() {
        use std::os::unix::fs::symlink;

        let root = fixture_dir("source-links");
        let source = normalized_tool_source(&root, "linked-source", b"proton");
        fs::create_dir_all(source.join("protonfixes/gamefixes-steam")).unwrap();
        fs::create_dir_all(source.join("protonfixes/gamefixes-umu")).unwrap();
        fs::write(
            source.join("protonfixes/gamefixes-steam/271590.py"),
            b"trusted fix",
        )
        .unwrap();
        symlink(
            "../gamefixes-steam/271590.py",
            source.join("protonfixes/gamefixes-umu/umu-271590.py"),
        )
        .unwrap();
        let copied = root.join("copied");
        copy_directory(&source, &copied).unwrap();
        assert_eq!(
            fs::read(copied.join("protonfixes/gamefixes-umu/umu-271590.py")).unwrap(),
            b"trusted fix"
        );
        assert!(
            fs::symlink_metadata(copied.join("protonfixes/gamefixes-umu/umu-271590.py"))
                .unwrap()
                .is_file()
        );

        let outside = root.join("outside");
        fs::write(&outside, b"outside").unwrap();
        symlink(
            "../../../outside",
            source.join("protonfixes/gamefixes-umu/escape"),
        )
        .unwrap();
        let error = copy_directory(&source, &root.join("rejected")).unwrap_err();
        assert!(error.contains("escapes its source"), "{error}");
        assert_eq!(fs::read(outside).unwrap(), b"outside");
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(all(target_os = "linux", unix))]
    #[test]
    fn trusted_archive_requires_exact_regular_bytes_and_materializes_safe_links() {
        use std::os::unix::fs::symlink;

        let root = fixture_dir("trusted-archive");
        let cache = root.join("cache");
        let (archive, digest) = proton_archive_fixture(&cache);
        {
            let trusted = trusted_proton_source_at(&cache, &digest).unwrap();
            assert_eq!(fs::read(trusted.source.join("proton")).unwrap(), b"proton");
            let link = trusted
                .source
                .join("protonfixes/gamefixes-umu/umu-271590.py");
            assert_eq!(fs::read(&link).unwrap(), b"fix");
            assert!(!fs::symlink_metadata(link).unwrap().file_type().is_symlink());
        }
        assert!(archive.is_file());

        fs::remove_file(&archive).unwrap();
        assert!(
            trusted_proton_source_at(&cache, &digest)
                .unwrap_err()
                .contains("unavailable")
        );
        let outside = root.join("outside.tar.gz");
        fs::write(&outside, b"not trusted").unwrap();
        symlink(&outside, &archive).unwrap();
        assert!(
            trusted_proton_source_at(&cache, &digest)
                .unwrap_err()
                .contains("regular file")
        );
        assert!(
            fs::symlink_metadata(&archive)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(outside).unwrap(), b"not trusted");
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn failed_configure_rolls_legacy_adoption_back_to_unowned() {
        use std::os::unix::fs::symlink;

        let fixture = legacy_compatibility_fixture("legacy-rollback");
        let invalid_source = normalized_tool_source(&fixture.root, "invalid-upgrade", b"new");
        fs::write(fixture.root.join("outside"), b"outside").unwrap();
        symlink("../../outside", invalid_source.join("files/escape")).unwrap();

        let error = configure_compatibility_at(
            &fixture.steam,
            &invalid_source,
            Some(&fixture.trusted),
            &fixture.ownership,
            &fixture.steam_vdf,
            &fixture.steam_backup,
            &fixture.mapping_snapshot,
            &fixture.local_vdf,
            &fixture.local_backup,
            &fixture.launch_snapshot,
        )
        .unwrap_err();
        assert!(error.contains("escapes its source"), "{error}");
        assert_legacy_unowned(&fixture);
        assert!(fixture.mapping_snapshot.exists());
        assert!(fixture.launch_snapshot.exists());
        fs::remove_dir_all(fixture.root).unwrap();
    }

    #[test]
    fn tool_cleanup_restores_only_its_exact_backup() {
        let root = fixture_dir("tool-ownership");
        let steam = root.join("steam");
        let directory = steam.join("compatibilitytools.d");
        let destination = directory.join(ENHANCED_TOOL_NAME);
        let stale = directory.join(format!("{ENHANCED_TOOL_NAME}.patchops.bak-stale"));
        let ownership = root.join("data/backups/compat_tool_311210.json");
        let source = tool_source(&root);
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("original"), b"original tool").unwrap();
        fs::create_dir_all(&stale).unwrap();
        fs::write(stale.join("keep"), b"unowned").unwrap();

        install_compatibility_tool_at(&steam, &source, &ownership)
            .unwrap()
            .commit()
            .unwrap();
        let state = load_tool_ownership(&ownership).unwrap().unwrap();
        let exact_backup = directory.join(state.original_backup.as_ref().unwrap());
        assert_eq!(
            fs::read(exact_backup.join("original")).unwrap(),
            b"original tool"
        );

        remove_compatibility_tool_at(&steam, &ownership)
            .unwrap()
            .commit()
            .unwrap();
        assert_eq!(
            fs::read(destination.join("original")).unwrap(),
            b"original tool"
        );
        assert_eq!(fs::read(stale.join("keep")).unwrap(), b"unowned");
        assert!(!ownership.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn tool_cleanup_refuses_a_replaced_destination() {
        let root = fixture_dir("tool-conflict");
        let steam = root.join("steam");
        let directory = steam.join("compatibilitytools.d");
        let destination = directory.join(ENHANCED_TOOL_NAME);
        let ownership = root.join("data/backups/compat_tool_311210.json");
        let source = tool_source(&root);
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("original"), b"original tool").unwrap();

        install_compatibility_tool_at(&steam, &source, &ownership)
            .unwrap()
            .commit()
            .unwrap();
        let state = load_tool_ownership(&ownership).unwrap().unwrap();
        let exact_backup = directory.join(state.original_backup.as_ref().unwrap());
        atomic_json(
            &destination.join(TOOL_MARKER_FILENAME),
            &json!({ "transaction_id": "someone-else" }),
        )
        .unwrap();

        let error = match remove_compatibility_tool_at(&steam, &ownership) {
            Ok(_) => panic!("replaced tool must not be removed"),
            Err(error) => error,
        };
        assert!(error.contains("leaving it untouched"));
        assert!(destination.join("proton").is_file());
        assert_eq!(
            fs::read(exact_backup.join("original")).unwrap(),
            b"original tool"
        );
        assert!(ownership.is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn configure_failure_restores_tool_mapping_launch_and_backups() {
        let root = fixture_dir("configure-rollback");
        let steam = root.join("steam");
        let destination = steam.join("compatibilitytools.d").join(ENHANCED_TOOL_NAME);
        let steam_vdf = steam.join("config/config.vdf");
        let local_vdf = steam.join("userdata/1/config/localconfig.vdf");
        let data = root.join("data/backups");
        let steam_backup = data.join("steam_config_backup.vdf");
        let local_backup = data.join("localconfig_backup.vdf");
        let mapping_snapshot = data.join("compat_mapping_311210.json");
        let launch_snapshot = data.join("launch_options_311210.json");
        let ownership = data.join("compat_tool_311210.json");
        let source = tool_source(&root);
        fs::create_dir_all(steam_vdf.parent().unwrap()).unwrap();
        fs::create_dir_all(local_vdf.parent().unwrap()).unwrap();
        fs::write(&steam_vdf, enhanced_compatibility_config()).unwrap();
        fs::write(&local_vdf, b"invalid-vdf").unwrap();
        install_compatibility_tool_at(&steam, &source, &ownership)
            .unwrap()
            .commit()
            .unwrap();
        atomic_json(
            &mapping_snapshot,
            &json!({ "had_entry": false, "entry": null }),
        )
        .unwrap();
        atomic_json(&launch_snapshot, &json!({ "launch_options": "-novid" })).unwrap();
        let original_steam = fs::read(&steam_vdf).unwrap();

        let error = configure_compatibility_at(
            &steam,
            &source,
            None,
            &ownership,
            &steam_vdf,
            &steam_backup,
            &mapping_snapshot,
            &local_vdf,
            &local_backup,
            &launch_snapshot,
        )
        .unwrap_err();
        assert!(error.contains("localconfig.vdf"));
        assert_eq!(fs::read(&steam_vdf).unwrap(), original_steam);
        assert_eq!(fs::read(&local_vdf).unwrap(), b"invalid-vdf");
        assert_eq!(
            fs::read(destination.join("proton")).unwrap(),
            b"managed proton"
        );
        assert!(mapping_snapshot.is_file());
        assert!(launch_snapshot.is_file());
        for path in [
            steam_backup,
            local_backup,
            fs_ops::backup_path(&steam_vdf),
            fs_ops::backup_path(&local_vdf),
        ] {
            assert!(!path.exists(), "rollback left {}", path.display());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cleanup_conflict_rolls_back_tool_and_mapping() {
        let root = fixture_dir("cleanup-rollback");
        let steam = root.join("steam");
        let destination = steam.join("compatibilitytools.d").join(ENHANCED_TOOL_NAME);
        let steam_vdf = steam.join("config/config.vdf");
        let local_vdf = steam.join("userdata/1/config/localconfig.vdf");
        let data = root.join("data/backups");
        let steam_backup = data.join("steam_config_backup.vdf");
        let local_backup = data.join("localconfig_backup.vdf");
        let mapping_snapshot = data.join("compat_mapping_311210.json");
        let launch_snapshot = data.join("launch_options_311210.json");
        let ownership = data.join("compat_tool_311210.json");
        let source = tool_source(&root);
        fs::create_dir_all(&destination).unwrap();
        fs::create_dir_all(steam_vdf.parent().unwrap()).unwrap();
        fs::create_dir_all(local_vdf.parent().unwrap()).unwrap();
        fs::write(destination.join("original"), b"original tool").unwrap();
        fs::write(&steam_vdf, compatibility_config()).unwrap();
        fs::write(&local_vdf, steam_config("-novid")).unwrap();
        install_compatibility_tool_at(&steam, &source, &ownership)
            .unwrap()
            .commit()
            .unwrap();
        atomic_json(
            &mapping_snapshot,
            &json!({
                "had_entry": true,
                "entry": { "name": "GE-Proton", "priority": "75", "custom": "yes" }
            }),
        )
        .unwrap();
        atomic_json(&launch_snapshot, &json!({ "launch_options": "-novid" })).unwrap();
        fs::write(&steam_vdf, enhanced_compatibility_config()).unwrap();
        fs::write(&local_vdf, steam_config(ENHANCED_LAUNCH_OPTIONS)).unwrap();
        fs::write(&local_vdf, steam_config("user-modified")).unwrap();
        let mapping_before = fs::read(&steam_vdf).unwrap();

        let error = cleanup_compatibility_at(
            &steam,
            None,
            &ownership,
            &steam_vdf,
            &steam_backup,
            &mapping_snapshot,
            &local_vdf,
            &local_backup,
            &launch_snapshot,
        )
        .unwrap_err();
        assert!(error.contains("launch options changed"));
        assert_eq!(fs::read(&steam_vdf).unwrap(), mapping_before);
        assert_eq!(
            read_launch_options_at(&local_vdf, APP_ID).unwrap(),
            "user-modified"
        );
        assert_eq!(
            fs::read(destination.join("proton")).unwrap(),
            b"managed proton"
        );
        let state = load_tool_ownership(&ownership).unwrap().unwrap();
        assert!(
            steam
                .join("compatibilitytools.d")
                .join(state.original_backup.unwrap())
                .join("original")
                .is_file()
        );
        assert!(mapping_snapshot.is_file());
        assert!(launch_snapshot.is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn configure_commit_failure_restores_tool_and_file_snapshots() {
        let root = fixture_dir("configure-commit-rollback");
        let steam = root.join("steam");
        let directory = steam.join("compatibilitytools.d");
        let destination = directory.join(ENHANCED_TOOL_NAME);
        let ownership = root.join("data/backups/compat_tool_311210.json");
        let config = root.join("config.vdf");
        let source = tool_source(&root);
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("original"), b"original tool").unwrap();
        fs::write(&config, b"before").unwrap();
        install_compatibility_tool_at(&steam, &source, &ownership)
            .unwrap()
            .commit()
            .unwrap();

        fs::write(source.join("proton"), b"replacement proton").unwrap();
        let snapshots = capture_files(std::slice::from_ref(&config)).unwrap();
        let tool = install_compatibility_tool_at(&steam, &source, &ownership).unwrap();
        let displaced = tool.displaced_managed.as_ref().unwrap();
        atomic_json(
            &displaced.join(TOOL_MARKER_FILENAME),
            &json!({ "transaction_id": "commit-conflict" }),
        )
        .unwrap();
        fs::write(&config, b"after").unwrap();

        let error = finish_tool_install(tool, snapshots, destination.clone(), Ok(())).unwrap_err();
        assert!(error.contains("leaving it untouched"));
        assert_eq!(fs::read(&config).unwrap(), b"before");
        assert_eq!(
            fs::read(destination.join("proton")).unwrap(),
            b"managed proton"
        );
        assert!(ownership.is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cleanup_commit_failure_restores_tool_and_file_snapshots() {
        let root = fixture_dir("cleanup-commit-rollback");
        let steam = root.join("steam");
        let directory = steam.join("compatibilitytools.d");
        let destination = directory.join(ENHANCED_TOOL_NAME);
        let ownership = root.join("data/backups/compat_tool_311210.json");
        let config = root.join("config.vdf");
        let source = tool_source(&root);
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("original"), b"original tool").unwrap();
        fs::write(&config, b"before").unwrap();
        install_compatibility_tool_at(&steam, &source, &ownership)
            .unwrap()
            .commit()
            .unwrap();
        let state = load_tool_ownership(&ownership).unwrap().unwrap();
        let exact_backup = directory.join(state.original_backup.unwrap());

        let snapshots = capture_files(std::slice::from_ref(&config)).unwrap();
        let tool = remove_compatibility_tool_at(&steam, &ownership).unwrap();
        let displaced = tool.displaced_managed.as_ref().unwrap();
        atomic_json(
            &displaced.join(TOOL_MARKER_FILENAME),
            &json!({ "transaction_id": "commit-conflict" }),
        )
        .unwrap();
        fs::write(&config, b"after").unwrap();

        let error = finish_tool_removal(tool, snapshots, Ok(())).unwrap_err();
        assert!(error.contains("leaving it untouched"));
        assert_eq!(fs::read(&config).unwrap(), b"before");
        assert_eq!(
            fs::read(destination.join("proton")).unwrap(),
            b"managed proton"
        );
        assert_eq!(
            fs::read(exact_backup.join("original")).unwrap(),
            b"original tool"
        );
        assert!(ownership.is_file());
        fs::remove_dir_all(root).unwrap();
    }
}
