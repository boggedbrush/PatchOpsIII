use std::{
    cmp::Ordering,
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map as JsonMap, Value as JsonValue};
use tauri::Manager;

use crate::{app::AppState, fs_ops, models::LaunchProfile};

pub const APP_ID: &str = "311210";
pub const COMPATIBLE_DEPOT_APP_ID: &str = APP_ID;
pub const COMPATIBLE_DEPOT_ID: &str = "311211";
pub const ENHANCED_TOOL_NAME: &str = "BO3 Enhanced";
pub const ENHANCED_LAUNCH_OPTIONS: &str = "WINEDLLOVERRIDES=\"WindowsCodecs=n,b\" %command%";

const GAME_DIRECTORY: &str = "Call of Duty Black Ops III";
const GAME_EXECUTABLES: [&str; 2] = ["BlackOpsIII.exe", "BlackOps3.exe"];
const ENHANCED_PROTON_TAG: &str = "release10-32";
const ENHANCED_PROTON_ASSET: &str = "GDK-Proton10-32.tar.gz";
const ENHANCED_PROTON_DOWNLOAD: &str = "https://github.com/Weather-OS/GDK-Proton/releases/download/release10-32/GDK-Proton10-32.tar.gz";
const ENHANCED_PROTON_SHA256: &str =
    "1e80f4e714f877f42101d5775bd38ca0a15a38d304e24af1f15c6deec4ebac2d";
const MAX_PROTON_ARCHIVE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const COMPATIBLE_BUILD_SHA256: &str =
    "66b95eb4667bd5b3b3d230e7bed1d29ccd261d48ca2699f01216c863be24ff44";
const STEAM_ID64_OFFSET: u64 = 76_561_197_960_265_728;
const TOOL_OWNERSHIP_VERSION: u8 = 1;
const TOOL_MARKER_FILENAME: &str = ".patchops-owner.json";

#[derive(Clone, Copy)]
struct WorkshopProfile {
    id: &'static str,
    label: &'static str,
    workshop_id: &'static str,
    option: &'static str,
}

const WORKSHOP_PROFILES: [WorkshopProfile; 3] = [
    WorkshopProfile {
        id: "all_around",
        label: "All-around Enhancement Lite",
        workshop_id: "2994481309",
        option: "+set fs_game 2994481309",
    },
    WorkshopProfile {
        id: "ultimate",
        label: "Ultimate Experience Mod",
        workshop_id: "2942053577",
        option: "+set fs_game 2942053577",
    },
    WorkshopProfile {
        id: "reforged",
        label: "BO3 Reforged",
        workshop_id: "3667377161",
        option: "+set fs_game 3667377161",
    },
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkshopState {
    pub state: String,
    pub installed: bool,
    pub subscribed: bool,
    pub path: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum VdfValue {
    String(String),
    Object(VdfObject),
}

type VdfObject = Vec<VdfEntry>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct VdfEntry {
    key: String,
    value: VdfValue,
    condition: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Text(String),
    Open,
    Close,
    Condition(String),
}

fn lex_vdf(input: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = input.trim_start_matches('\u{feff}').chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index].is_whitespace() {
            index += 1;
            continue;
        }
        if chars[index] == '/' && chars.get(index + 1) == Some(&'/') {
            index += 2;
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            continue;
        }
        match chars[index] {
            '{' => {
                tokens.push(Token::Open);
                index += 1;
            }
            '}' => {
                tokens.push(Token::Close);
                index += 1;
            }
            '[' => {
                index += 1;
                let start = index;
                while index < chars.len() && chars[index] != ']' {
                    index += 1;
                }
                if index == chars.len() {
                    return Err("unterminated VDF condition".into());
                }
                tokens.push(Token::Condition(chars[start..index].iter().collect()));
                index += 1;
            }
            '"' => {
                index += 1;
                let mut value = String::new();
                let mut closed = false;
                while index < chars.len() {
                    match chars[index] {
                        '"' => {
                            index += 1;
                            closed = true;
                            break;
                        }
                        '\\' if index + 1 < chars.len() => {
                            let escaped = chars[index + 1];
                            match escaped {
                                '"' => value.push('"'),
                                '\\' => value.push('\\'),
                                'n' => value.push('\n'),
                                'r' => value.push('\r'),
                                't' => value.push('\t'),
                                other => {
                                    value.push('\\');
                                    value.push(other);
                                }
                            }
                            index += 2;
                        }
                        character => {
                            value.push(character);
                            index += 1;
                        }
                    }
                }
                if !closed {
                    return Err("unterminated quoted VDF value".into());
                }
                tokens.push(Token::Text(value));
            }
            _ => {
                let start = index;
                while index < chars.len()
                    && !chars[index].is_whitespace()
                    && !matches!(chars[index], '{' | '}' | '[' | '"')
                    && !(chars[index] == '/' && chars.get(index + 1) == Some(&'/'))
                {
                    index += 1;
                }
                if start == index {
                    return Err("invalid VDF token".into());
                }
                tokens.push(Token::Text(chars[start..index].iter().collect()));
            }
        }
    }
    Ok(tokens)
}

fn parse_vdf(input: &str) -> Result<VdfObject, String> {
    fn entries(tokens: &[Token], index: &mut usize, nested: bool) -> Result<VdfObject, String> {
        let mut result = Vec::new();
        while *index < tokens.len() {
            if tokens[*index] == Token::Close {
                if !nested {
                    return Err("unexpected closing VDF brace".into());
                }
                *index += 1;
                return Ok(result);
            }
            let key = match tokens.get(*index) {
                Some(Token::Text(value)) => value.clone(),
                Some(_) => return Err("expected a VDF key".into()),
                None => break,
            };
            *index += 1;
            let value = match tokens.get(*index) {
                Some(Token::Text(value)) => {
                    *index += 1;
                    VdfValue::String(value.clone())
                }
                Some(Token::Open) => {
                    *index += 1;
                    VdfValue::Object(entries(tokens, index, true)?)
                }
                _ => return Err(format!("missing VDF value for '{key}'")),
            };
            let condition = match tokens.get(*index) {
                Some(Token::Condition(value)) => {
                    *index += 1;
                    Some(value.clone())
                }
                _ => None,
            };
            result.push(VdfEntry {
                key,
                value,
                condition,
            });
        }
        if nested {
            Err("unterminated VDF object".into())
        } else {
            Ok(result)
        }
    }

    let tokens = lex_vdf(input)?;
    let mut index = 0;
    let object = entries(&tokens, &mut index, false)?;
    if index == tokens.len() {
        Ok(object)
    } else {
        Err("trailing VDF tokens".into())
    }
}

fn quote_vdf(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for character in value.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

fn serialize_vdf(object: &VdfObject) -> String {
    fn write_entries(object: &VdfObject, depth: usize, output: &mut String) {
        let indent = "\t".repeat(depth);
        for entry in object {
            output.push_str(&indent);
            output.push_str(&quote_vdf(&entry.key));
            match &entry.value {
                VdfValue::String(value) => {
                    output.push('\t');
                    output.push_str(&quote_vdf(value));
                    if let Some(condition) = &entry.condition {
                        output.push_str("\t[");
                        output.push_str(condition);
                        output.push(']');
                    }
                    output.push('\n');
                }
                VdfValue::Object(children) => {
                    output.push('\n');
                    output.push_str(&indent);
                    output.push_str("{\n");
                    write_entries(children, depth + 1, output);
                    output.push_str(&indent);
                    output.push('}');
                    if let Some(condition) = &entry.condition {
                        output.push_str("\t[");
                        output.push_str(condition);
                        output.push(']');
                    }
                    output.push('\n');
                }
            }
        }
    }

    let mut output = String::new();
    write_entries(object, 0, &mut output);
    output
}

fn entry_index(object: &VdfObject, key: &str) -> Option<usize> {
    object
        .iter()
        .rposition(|entry| entry.key.eq_ignore_ascii_case(key))
}

fn value_at<'a>(object: &'a VdfObject, path: &[&str]) -> Option<&'a VdfValue> {
    let (key, remaining) = path.split_first()?;
    let value = &object.get(entry_index(object, key)?)?.value;
    if remaining.is_empty() {
        return Some(value);
    }
    match value {
        VdfValue::Object(children) => value_at(children, remaining),
        VdfValue::String(_) => None,
    }
}

fn ensure_object<'a>(object: &'a mut VdfObject, key: &str) -> Result<&'a mut VdfObject, String> {
    let index = match entry_index(object, key) {
        Some(index) => index,
        None => {
            object.push(VdfEntry {
                key: key.into(),
                value: VdfValue::Object(Vec::new()),
                condition: None,
            });
            object.len() - 1
        }
    };
    match &mut object[index].value {
        VdfValue::Object(children) => Ok(children),
        VdfValue::String(_) => Err(format!("VDF key '{key}' is not an object")),
    }
}

fn ensure_path<'a>(object: &'a mut VdfObject, path: &[&str]) -> Result<&'a mut VdfObject, String> {
    match path.split_first() {
        Some((key, remaining)) => ensure_path(ensure_object(object, key)?, remaining),
        None => Ok(object),
    }
}

fn set_string(object: &mut VdfObject, key: &str, value: String) -> Result<(), String> {
    if let Some(index) = entry_index(object, key) {
        match &mut object[index].value {
            VdfValue::String(existing) => *existing = value,
            VdfValue::Object(_) => return Err(format!("VDF key '{key}' is not a string")),
        }
    } else {
        object.push(VdfEntry {
            key: key.into(),
            value: VdfValue::String(value),
            condition: None,
        });
    }
    Ok(())
}

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

fn read_vdf(path: &Path) -> Result<VdfObject, String> {
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let text = String::from_utf8(bytes)
        .map_err(|error| format!("{} is not valid UTF-8: {error}", path.display()))?;
    parse_vdf(&text).map_err(|error| format!("{}: {error}", path.display()))
}

fn atomic_temp_path(path: &Path) -> Option<PathBuf> {
    Some(
        path.parent()?
            .join(format!(".{}.patchops.tmp", path.file_name()?.to_str()?)),
    )
}

fn preserve_original(
    path: &Path,
    original: &[u8],
    legacy_backup: Option<&Path>,
) -> Result<(), String> {
    let backup = fs_ops::backup_path(path);
    match fs::metadata(&backup) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Err(format!("{} is not a file", backup.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs_ops::atomic_write(&backup, original)?;
        }
        Err(error) => return Err(format!("{}: {error}", backup.display())),
    }
    if let Some(legacy_backup) = legacy_backup {
        fs_ops::atomic_write(legacy_backup, original)?;
    }
    Ok(())
}

fn update_vdf(
    path: &Path,
    legacy_backup: Option<&Path>,
    change: impl FnOnce(&mut VdfObject) -> Result<(), String>,
) -> Result<(), String> {
    let original = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let text = String::from_utf8(original.clone())
        .map_err(|error| format!("{} is not valid UTF-8: {error}", path.display()))?;
    let mut document = parse_vdf(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    change(&mut document)?;
    let updated = serialize_vdf(&document);
    parse_vdf(&updated).map_err(|error| format!("refusing to write invalid VDF: {error}"))?;
    if updated.as_bytes() == original {
        return Ok(());
    }

    preserve_original(path, &original, legacy_backup)?;
    if let Err(write_error) = fs_ops::atomic_write(path, updated.as_bytes()) {
        if let Some(temporary) = atomic_temp_path(path) {
            let _ = fs::remove_file(temporary);
        }
        return match fs_ops::atomic_write(path, &original) {
            Ok(()) => Err(format!(
                "failed to update {}: {write_error}; original restored",
                path.display()
            )),
            Err(rollback_error) => Err(format!(
                "failed to update {}: {write_error}; rollback also failed: {rollback_error}",
                path.display()
            )),
        };
    }
    Ok(())
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

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    let variable = "USERPROFILE";
    #[cfg(not(windows))]
    let variable = "HOME";
    env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(windows)]
fn windows_steam_roots() -> Vec<PathBuf> {
    use winreg::{enums::*, RegKey};

    let mut candidates = Vec::new();
    for (hive, subkey, value_name) in [
        (HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath"),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Valve\Steam",
            "InstallPath",
        ),
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\Valve\Steam", "InstallPath"),
    ] {
        if let Ok(key) = RegKey::predef(hive).open_subkey(subkey) {
            if let Ok(path) = key.get_value::<String, _>(value_name) {
                candidates.push(PathBuf::from(path));
            }
        }
    }
    for variable in ["PROGRAMFILES(X86)", "PROGRAMFILES"] {
        if let Some(path) = env::var_os(variable) {
            candidates.push(PathBuf::from(path).join("Steam"));
        }
    }
    candidates.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
    candidates.push(PathBuf::from(r"C:\Program Files\Steam"));
    candidates
}

fn candidate_steam_roots() -> Vec<PathBuf> {
    #[cfg(windows)]
    let candidates = windows_steam_roots();

    #[cfg(target_os = "macos")]
    let candidates = home_dir()
        .map(|home| vec![home.join("Library/Application Support/Steam")])
        .unwrap_or_default();

    #[cfg(all(unix, not(target_os = "macos")))]
    let candidates = home_dir()
        .map(|home| vec![home.join(".steam/steam"), home.join(".local/share/Steam")])
        .unwrap_or_default();

    dedupe_existing_dirs(candidates)
}

fn normalized_path_key(path: &Path) -> String {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    #[cfg(windows)]
    return resolved.to_string_lossy().to_ascii_lowercase();
    #[cfg(not(windows))]
    resolved.to_string_lossy().into_owned()
}

fn dedupe_existing_dirs(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|path| path.is_dir())
        .filter(|path| seen.insert(normalized_path_key(path)))
        .collect()
}

pub(crate) fn library_paths_from_roots(roots: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut paths = roots.clone();
    for root in roots {
        let library_file = root.join("steamapps/libraryfolders.vdf");
        let Ok(document) = read_vdf(&library_file) else {
            continue;
        };
        let Some(VdfValue::Object(folders)) = value_at(&document, &["libraryfolders"]) else {
            continue;
        };
        for entry in folders {
            if entry.key.eq_ignore_ascii_case("contentstatsid") {
                continue;
            }
            match &entry.value {
                VdfValue::String(path) => paths.push(PathBuf::from(path)),
                VdfValue::Object(folder) => {
                    if let Some(VdfValue::String(path)) = value_at(folder, &["path"]) {
                        paths.push(PathBuf::from(path));
                    }
                }
            }
        }
    }
    dedupe_existing_dirs(paths)
}

pub fn steam_root() -> Option<PathBuf> {
    candidate_steam_roots().into_iter().next()
}

pub fn library_paths() -> Vec<PathBuf> {
    library_paths_from_roots(candidate_steam_roots())
}

fn has_game_executable(directory: &Path) -> bool {
    directory.is_dir()
        && GAME_EXECUTABLES
            .iter()
            .any(|executable| directory.join(executable).is_file())
}

pub fn find_game_directory(saved: Option<&str>) -> Option<PathBuf> {
    if let Some(saved) = saved
        .map(PathBuf::from)
        .filter(|path| has_game_executable(path))
    {
        return Some(saved);
    }
    for library in library_paths() {
        let candidate = library.join("steamapps/common").join(GAME_DIRECTORY);
        if has_game_executable(&candidate) {
            return Some(candidate);
        }
    }

    #[cfg(windows)]
    let candidates = windows_steam_roots()
        .into_iter()
        .map(|root| root.join("steamapps/common").join(GAME_DIRECTORY))
        .collect::<Vec<_>>();
    #[cfg(target_os = "macos")]
    let candidates = vec![home_dir()?
        .join("Library/Application Support/Steam/steamapps/common")
        .join(GAME_DIRECTORY)];
    #[cfg(all(unix, not(target_os = "macos")))]
    let candidates = {
        let home = home_dir()?;
        vec![
            home.join(".steam/steam/steamapps/common")
                .join(GAME_DIRECTORY),
            home.join(".local/share/Steam/steamapps/common")
                .join(GAME_DIRECTORY),
        ]
    };
    candidates
        .into_iter()
        .find(|path| has_game_executable(path))
}

fn numeric_id_cmp(left: &str, right: &str) -> Ordering {
    let left = left.trim_start_matches('0');
    let right = right.trim_start_matches('0');
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

fn numeric_user_ids(userdata: &Path) -> Vec<String> {
    let mut ids = fs::read_dir(userdata)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_digit()))
        .collect::<Vec<_>>();
    ids.sort_by(|left, right| numeric_id_cmp(left, right).then_with(|| left.cmp(right)));
    ids
}

fn user_id_in(steam_root: &Path) -> Option<String> {
    let ids = numeric_user_ids(&steam_root.join("userdata"));
    let login_users = read_vdf(&steam_root.join("config/loginusers.vdf")).ok();
    let recent = login_users
        .as_ref()
        .and_then(|document| value_at(document, &["users"]))
        .and_then(|value| match value {
            VdfValue::Object(users) => Some(users),
            VdfValue::String(_) => None,
        })
        .into_iter()
        .flatten()
        .filter(|entry| {
            matches!(
                &entry.value,
                VdfValue::Object(user)
                    if matches!(value_at(user, &["MostRecent"]), Some(VdfValue::String(value)) if value == "1")
            )
        })
        .filter_map(|entry| {
            if ids.iter().any(|id| id == &entry.key) {
                return Some(entry.key.clone());
            }
            entry
                .key
                .parse::<u64>()
                .ok()?
                .checked_sub(STEAM_ID64_OFFSET)
                .map(|id| id.to_string())
                .filter(|id| ids.iter().any(|candidate| candidate == id))
        })
        .min_by(|left, right| numeric_id_cmp(left, right).then_with(|| left.cmp(right)));
    recent.or_else(|| ids.into_iter().next())
}

pub fn user_id() -> Option<String> {
    user_id_in(&steam_root()?)
}

fn local_config_path_in(steam_root: &Path, user: &str) -> Option<PathBuf> {
    if user.is_empty() || !user.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(
        steam_root
            .join("userdata")
            .join(user)
            .join("config/localconfig.vdf"),
    )
}

fn local_config_path(user: &str) -> Option<PathBuf> {
    local_config_path_in(&steam_root()?, user)
}

fn read_launch_options_at(path: &Path, game_app_id: &str) -> Result<String, String> {
    let document = read_vdf(path)?;
    Ok(
        match value_at(
            &document,
            &[
                "UserLocalConfigStore",
                "Software",
                "Valve",
                "Steam",
                "apps",
                game_app_id,
                "LaunchOptions",
            ],
        ) {
            Some(VdfValue::String(options)) => options.clone(),
            _ => String::new(),
        },
    )
}

pub fn current_launch_options() -> Option<String> {
    let user = user_id()?;
    read_launch_options_at(&local_config_path(&user)?, APP_ID).ok()
}

fn normalized_options(options: &str) -> String {
    options.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn without_token(options: &str, token: &str) -> String {
    options
        .split_whitespace()
        .filter(|part| *part != token)
        .collect::<Vec<_>>()
        .join(" ")
}

fn merged_launch_options(current: &str, requested: &str, preserve_fs_game: bool) -> String {
    let fs_game = Regex::new(r"\+set\s+fs_game\s+\S+").expect("fixed regular expression");
    let existing_fs = fs_game
        .find_iter(current)
        .map(|matched| matched.as_str().to_owned())
        .collect::<Vec<_>>();
    let requested_fs = fs_game
        .find_iter(requested)
        .map(|matched| matched.as_str().to_owned())
        .collect::<Vec<_>>();
    let selected_fs = if requested_fs.is_empty() && preserve_fs_game {
        existing_fs
    } else {
        requested_fs
    };

    let wine_override = "WINEDLLOVERRIDES=\"dsound=n,b\"";
    let command_marker = "%command%";
    let current_without_fs = fs_game.replace_all(current, "");
    let requested_without_fs = fs_game.replace_all(requested, "");
    let has_wine = current_without_fs
        .split_whitespace()
        .any(|part| part == wine_override)
        || requested_without_fs
            .split_whitespace()
            .any(|part| part == wine_override);
    let mut has_command = current_without_fs
        .split_whitespace()
        .any(|part| part == command_marker)
        || requested_without_fs
            .split_whitespace()
            .any(|part| part == command_marker);
    if has_wine {
        has_command = true;
    }

    let cleaned_current = without_token(
        &without_token(&current_without_fs, wine_override),
        command_marker,
    );
    let cleaned_requested = without_token(
        &without_token(&requested_without_fs, wine_override),
        command_marker,
    );
    let mut segments = Vec::new();
    if has_wine {
        segments.push(wine_override.to_owned());
    }
    if has_command {
        segments.push(command_marker.to_owned());
    }
    if !cleaned_current.is_empty() {
        segments.push(normalized_options(&cleaned_current));
    }
    if !cleaned_requested.is_empty() {
        segments.push(normalized_options(&cleaned_requested));
    }
    let mut unique_fs = Vec::new();
    for entry in selected_fs {
        if !unique_fs.contains(&entry) {
            unique_fs.push(entry);
        }
    }
    if !unique_fs.is_empty() {
        segments.push(normalized_options(&unique_fs.join(" ")));
    }
    normalized_options(&segments.join(" "))
}

fn set_launch_options_at(
    config: &Path,
    legacy_backup: &Path,
    game_app_id: &str,
    requested: &str,
    preserve_fs_game: bool,
    exact: bool,
) -> Result<String, String> {
    let current = read_launch_options_at(config, game_app_id)?;
    let final_options = if exact {
        requested.to_owned()
    } else {
        merged_launch_options(&current, requested, preserve_fs_game)
    };
    update_vdf(config, Some(legacy_backup), |document| {
        let app = ensure_path(
            document,
            &[
                "UserLocalConfigStore",
                "Software",
                "Valve",
                "Steam",
                "apps",
                game_app_id,
            ],
        )?;
        set_string(app, "LaunchOptions", final_options.clone())
    })?;
    Ok(final_options)
}

fn workshop_profile(profile_id: &str) -> Option<WorkshopProfile> {
    WORKSHOP_PROFILES
        .iter()
        .copied()
        .find(|profile| profile.id == profile_id)
}

fn profile_option(profile_id: &str) -> Option<&'static str> {
    match profile_id {
        "default" => Some(""),
        "offline" => Some("+set fs_game offlinemp"),
        _ => workshop_profile(profile_id).map(|profile| profile.option),
    }
}

fn supported_option(option: &str) -> bool {
    option.is_empty()
        || option == "+set fs_game offlinemp"
        || WORKSHOP_PROFILES
            .iter()
            .any(|profile| profile.option == option)
}

fn vdf_contains(value: &VdfValue, needle: &str) -> bool {
    match value {
        VdfValue::String(value) => value == needle,
        VdfValue::Object(object) => object
            .iter()
            .any(|entry| entry.key == needle || vdf_contains(&entry.value, needle)),
    }
}

fn workshop_item_state_in(
    libraries: &[PathBuf],
    game_app_id: &str,
    workshop_id: &str,
) -> WorkshopState {
    let mut subscribed = false;
    for library in libraries {
        let install_dir = library
            .join("steamapps/workshop/content")
            .join(game_app_id)
            .join(workshop_id);
        if install_dir.is_dir()
            && fs::read_dir(&install_dir)
                .ok()
                .and_then(|mut entries| entries.next())
                .is_some()
        {
            return WorkshopState {
                state: "Installed".into(),
                installed: true,
                subscribed: true,
                path: Some(install_dir),
            };
        }

        let manifest = library
            .join("steamapps/workshop")
            .join(format!("appworkshop_{game_app_id}.acf"));
        if let Ok(document) = read_vdf(&manifest) {
            if let Some(value) = value_at(&document, &["AppWorkshop"]) {
                subscribed |= vdf_contains(value, workshop_id);
            }
        }
    }
    if subscribed {
        WorkshopState {
            state: "Subscribed (not installed yet)".into(),
            installed: false,
            subscribed: true,
            path: None,
        }
    } else {
        WorkshopState {
            state: "Not Subscribed".into(),
            installed: false,
            subscribed: false,
            path: None,
        }
    }
}

pub fn workshop_item_state(game_app_id: &str, workshop_id: &str) -> WorkshopState {
    workshop_item_state_in(&library_paths(), game_app_id, workshop_id)
}

pub fn launch_profiles(current: Option<&str>) -> Vec<LaunchProfile> {
    let current = current.unwrap_or_default();
    let mut profiles = vec![
        LaunchProfile {
            id: "default".into(),
            label: "Default (None)".into(),
            option: String::new(),
            active: current.trim().is_empty(),
            installed: true,
            subscribed: true,
            state: "Ready".into(),
            path: None,
        },
        LaunchProfile {
            id: "offline".into(),
            label: "Play Offline".into(),
            option: "+set fs_game offlinemp".into(),
            active: current.contains("+set fs_game offlinemp"),
            installed: true,
            subscribed: true,
            state: "Ready".into(),
            path: None,
        },
    ];
    for profile in WORKSHOP_PROFILES {
        let workshop = workshop_item_state(APP_ID, profile.workshop_id);
        profiles.push(LaunchProfile {
            id: profile.id.into(),
            label: profile.label.into(),
            option: profile.option.into(),
            active: current.contains(profile.option),
            installed: workshop.installed,
            subscribed: workshop.subscribed,
            state: workshop.state,
            path: workshop
                .path
                .map(|path| path.to_string_lossy().into_owned()),
        });
    }
    profiles
}

fn find_on_path(command: &str) -> Option<PathBuf> {
    env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| env::split_paths(&paths).collect::<Vec<_>>())
        .map(|directory| directory.join(command))
        .find(|path| path.is_file())
}

#[cfg(windows)]
fn steam_executable() -> Option<PathBuf> {
    candidate_steam_roots()
        .into_iter()
        .map(|root| root.join("steam.exe"))
        .find(|path| path.is_file())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn steam_executable() -> Option<PathBuf> {
    find_on_path("steam").or_else(|| {
        steam_root()
            .map(|root| root.join("steam.sh"))
            .filter(|path| path.is_file())
    })
}

fn wait_for_steam_exit_with(
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

#[cfg(target_os = "linux")]
fn linux_steam_running() -> Result<bool, String> {
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

fn close_steam(app: &AppState) -> Result<(), String> {
    #[cfg(windows)]
    let result = Command::new("taskkill")
        .args(["/F", "/IM", "steam.exe"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    #[cfg(target_os = "linux")]
    let result = Command::new("pkill")
        .args(["-x", "steam"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    #[cfg(target_os = "macos")]
    let result = Command::new("pkill")
        .args(["-x", "steam"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    #[cfg(windows)]
    match result {
        Ok(status) if status.success() => {
            app.log("Info", "Closed Steam before updating its configuration.")
        }
        Ok(_) => app.log("Info", "Steam was not running."),
        Err(error) => return Err(format!("Could not close Steam: {error}")),
    }

    #[cfg(unix)]
    match result {
        Ok(status) if status.success() => {
            app.log("Info", "Closed Steam before updating its configuration.")
        }
        Ok(status) if status.code() == Some(1) => app.log("Info", "Steam was not running."),
        Ok(status) => {
            return Err(format!(
                "Could not close Steam (exit code {}).",
                status
                    .code()
                    .map_or_else(|| "unknown".into(), |code| code.to_string())
            ));
        }
        Err(error) => return Err(format!("Could not close Steam: {error}")),
    }

    #[cfg(target_os = "linux")]
    wait_for_steam_exit_with(50, linux_steam_running, || {
        std::thread::sleep(Duration::from_millis(100))
    })?;

    Ok(())
}

fn open_steam(app: &AppState) -> Result<(), String> {
    #[cfg(windows)]
    let child = steam_executable()
        .ok_or_else(|| "Steam executable was not found.".to_string())
        .and_then(|executable| {
            Command::new(executable)
                .arg("-silent")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|error| error.to_string())
        });

    #[cfg(all(unix, not(target_os = "macos")))]
    let child = steam_executable()
        .ok_or_else(|| "Steam executable was not found.".to_string())
        .and_then(|executable| {
            Command::new(executable)
                .arg("-silent")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|error| error.to_string())
        });

    #[cfg(target_os = "macos")]
    let child = Command::new("open")
        .args(["-a", "Steam"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string());

    child.map(|_| app.log("Success", "Opened Steam."))
}

pub fn apply_launch_options(
    app: &AppState,
    options: &str,
    preserve_fs_game: bool,
) -> Result<(), String> {
    if !supported_option(options) {
        return Err("Unsupported launch option.".into());
    }
    let user = user_id().ok_or_else(|| "Steam user ID not found.".to_string())?;
    let config =
        local_config_path(&user).ok_or_else(|| "Steam userdata path not found.".to_string())?;
    let legacy_backup = app.data_dir().join("backups/localconfig_backup.vdf");
    close_steam(app)?;
    let result = set_launch_options_at(
        &config,
        &legacy_backup,
        APP_ID,
        options,
        preserve_fs_game,
        false,
    );
    if let Err(error) = open_steam(app) {
        app.log(
            "Warning",
            format!("Launch options were processed, but Steam could not be reopened: {error}"),
        );
    }
    result.map(|final_options| {
        app.log(
            "Success",
            format!("Set Steam launch options to: {final_options}"),
        );
    })
}

pub fn apply_launch_profile(app: &AppState, profile_id: &str) -> Result<(), String> {
    let option =
        profile_option(profile_id).ok_or_else(|| "Unsupported launch profile.".to_string())?;
    apply_launch_options(app, option, false)
}

fn open_workshop_page(workshop_id: &str) -> Result<(), String> {
    let uri = format!(
        "steam://openurl/https://steamcommunity.com/sharedfiles/filedetails/?id={workshop_id}"
    );
    #[cfg(windows)]
    let result = steam_executable()
        .ok_or_else(|| "Steam executable was not found.".to_string())
        .and_then(|executable| {
            Command::new(executable)
                .arg(&uri)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|error| error.to_string())
        });
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = if let Some(executable) = steam_executable() {
        Command::new(executable)
            .arg(&uri)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())
    } else if let Some(xdg_open) = find_on_path("xdg-open") {
        Command::new(xdg_open)
            .arg(&uri)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())
    } else {
        Err("No Steam URI launcher was found.".into())
    };
    #[cfg(target_os = "macos")]
    let result = Command::new("open")
        .arg(&uri)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string());
    result.map(|_| ())
}

pub fn install_workshop_profile(app: &AppState, profile_id: &str) -> Result<(), String> {
    let profile = workshop_profile(profile_id)
        .ok_or_else(|| "Select an installable Workshop mod.".to_string())?;
    apply_launch_profile(app, profile_id)?;
    open_workshop_page(profile.workshop_id)?;
    app.log(
        "Info",
        format!("Opened {} Workshop page in Steam.", profile.label),
    );
    Ok(())
}

pub fn launch_game(app: &AppState) -> Result<(), String> {
    #[cfg(windows)]
    let result = steam_executable()
        .ok_or_else(|| "Steam executable was not found.".to_string())
        .and_then(|executable| {
            Command::new(executable)
                .args(["-applaunch", APP_ID])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|error| error.to_string())
        });
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = if let Some(executable) = steam_executable() {
        Command::new(executable)
            .args(["-applaunch", APP_ID])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())
    } else if let Some(xdg_open) = find_on_path("xdg-open") {
        Command::new(xdg_open)
            .arg(format!("steam://rungameid/{APP_ID}"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())
    } else {
        Err("No Steam launcher was found.".into())
    };
    #[cfg(target_os = "macos")]
    let result = Command::new("open")
        .arg(format!("steam://rungameid/{APP_ID}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string());

    result.map(|_| {
        app.log(
            "Success",
            format!("Launched Black Ops III via Steam (AppID: {APP_ID})."),
        );
    })
}

pub fn compatible_depot_candidates() -> Vec<PathBuf> {
    let mut roots = library_paths();
    if let Some(root) = steam_root() {
        roots.push(root);
    }
    #[cfg(windows)]
    roots.extend(windows_steam_roots());
    let mut seen = HashSet::new();
    roots
        .into_iter()
        .map(|root| {
            root.join("steamapps/content")
                .join(format!("app_{COMPATIBLE_DEPOT_APP_ID}"))
                .join(format!("depot_{COMPATIBLE_DEPOT_ID}"))
        })
        .filter(|path| seen.insert(normalized_path_key(path)))
        .collect()
}

pub fn compatible_depot_download_exists() -> bool {
    compatible_depot_candidates().into_iter().any(|directory| {
        let executable = directory.join("BlackOps3.exe");
        executable
            .metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
            && directory.join("installscript_311210.vdf").is_file()
    })
}

pub fn find_valid_compatible_depot() -> Option<PathBuf> {
    compatible_depot_candidates()
        .into_iter()
        .filter(|directory| {
            fs_ops::sha256_file(&directory.join("BlackOps3.exe"))
                .is_ok_and(|hash| hash.eq_ignore_ascii_case(COMPATIBLE_BUILD_SHA256))
        })
        .max_by_key(|directory| {
            directory
                .join("BlackOps3.exe")
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(UNIX_EPOCH)
        })
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
                    return Err("Steam CompatToolMapping is not an object".into())
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
            return Err("Untracked compatibility tool already contains an ownership marker.".into())
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
        if let Some(displaced) = &self.displaced_managed {
            if let Err(error) = require_managed_tool(displaced, &self.transaction_id) {
                return Err(Box::new((error, self)));
            }
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
        if let Some(displaced) = &self.displaced_managed {
            if let Err(error) = require_managed_tool(displaced, &self.transaction_id) {
                return Err(Box::new((error, self)));
            }
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
    if new_ownership {
        if let Some(backup) = &original_backup {
            if let Err(error) = fs::rename(&destination, backup) {
                let _ = fs::remove_dir_all(&staging);
                let _ = fs::remove_file(ownership_path);
                return Err(error.to_string());
            }
        }
    }
    if let Err(error) = fs::rename(&staging, &destination) {
        let _ = fs::remove_dir_all(&staging);
        let restore = if new_ownership {
            original_backup.as_ref()
        } else {
            displaced_managed.as_ref()
        };
        if let Some(restore) = restore {
            if let Err(rollback_error) = fs::rename(restore, &destination) {
                return Err(format!(
                    "{error}; restoring the previous compatibility tool also failed: {rollback_error}"
                ));
            }
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
    if let Some(backup) = &original_backup {
        if let Err(error) = fs::rename(backup, &destination) {
            if let Some(displaced) = &displaced_managed {
                if let Err(rollback) = fs::rename(displaced, &destination) {
                    return Err(format!(
                        "{error}; restoring the managed compatibility tool also failed: {rollback}"
                    ));
                }
            }
            return Err(error.to_string());
        }
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

#[cfg(test)]
pub(crate) fn benchmark_configure_compatibility_transaction(
    steam_root: &Path,
    source: &Path,
    data_dir: &Path,
    local_config: &Path,
) -> Result<PathBuf, String> {
    configure_compatibility_at(
        steam_root,
        source,
        None,
        &tool_ownership_path(data_dir),
        &steam_root.join("config/config.vdf"),
        &data_dir.join("backups/steam_config_backup.vdf"),
        &mapping_snapshot_path(data_dir),
        local_config,
        &data_dir.join("backups/localconfig_backup.vdf"),
        &launch_snapshot_path(data_dir),
    )
}

#[cfg(test)]
pub(crate) fn benchmark_cleanup_compatibility_transaction(
    steam_root: &Path,
    data_dir: &Path,
    local_config: &Path,
) -> Result<(), String> {
    cleanup_compatibility_at(
        steam_root,
        None,
        &tool_ownership_path(data_dir),
        &steam_root.join("config/config.vdf"),
        &data_dir.join("backups/steam_config_backup.vdf"),
        &mapping_snapshot_path(data_dir),
        local_config,
        &data_dir.join("backups/localconfig_backup.vdf"),
        &launch_snapshot_path(data_dir),
    )
}

#[cfg(test)]
pub(crate) fn benchmark_compatibility_is_clean(
    steam_root: &Path,
    data_dir: &Path,
    local_config: &Path,
) -> Result<bool, String> {
    compatibility_is_fully_clean(
        steam_root,
        &tool_ownership_path(data_dir),
        &steam_root.join("config/config.vdf"),
        &mapping_snapshot_path(data_dir),
        local_config,
        &launch_snapshot_path(data_dir),
    )
}

#[cfg(target_os = "linux")]
fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct TrustedProtonSource {
    temporary_root: PathBuf,
    source: PathBuf,
}

#[cfg(target_os = "linux")]
impl Drop for TrustedProtonSource {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.temporary_root);
    }
}

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
fn prepare_enhanced_proton_cache(app: &AppState) -> Result<PathBuf, String> {
    let cache_root = app.data_dir().join("bo3-enhanced-proton-cache");
    let target = cache_root.join(ENHANCED_TOOL_NAME);
    if valid_cached_proton(&target) {
        if let Ok(trusted) = trusted_proton_source_at(&cache_root, ENHANCED_PROTON_SHA256) {
            if compare_tool_trees(&trusted.source, &target, true, true).is_ok() {
                return Ok(target);
            }
        }
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
            if moved_existing {
                if let Err(rollback_error) = fs::rename(&backup, &target) {
                    return Err(format!(
                        "{error}; restoring the previous Proton cache also failed: {rollback_error}"
                    ));
                }
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

#[cfg(target_os = "linux")]
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
    if let Ok(resources) = app.app().path().resource_dir() {
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
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (app, tool_source);
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        let root =
            steam_root().ok_or_else(|| "Steam root path could not be resolved.".to_string())?;
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
}

pub fn cleanup_bo3_enhanced_linux(app: &AppState) -> Result<(), String> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = app;
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        let root =
            steam_root().ok_or_else(|| "Steam root path could not be resolved.".to_string())?;
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_dir(name: &str) -> PathBuf {
        let path = env::temp_dir().join(format!(
            "patchops-steam-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn string_value<'a>(document: &'a VdfObject, path: &[&str]) -> &'a str {
        match value_at(document, path).unwrap() {
            VdfValue::String(value) => value,
            VdfValue::Object(_) => panic!("expected string"),
        }
    }

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

    fn steam_config(options: &str) -> String {
        format!(
            r#""UserLocalConfigStore" {{
  "Software" {{ "Valve" {{ "Steam" {{ "apps" {{
    "{APP_ID}" {{ "LaunchOptions" {} }}
  }} }} }} }}
}}"#,
            quote_vdf(options)
        )
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

    #[cfg(target_os = "linux")]
    fn proton_archive_fixture(cache_root: &Path) -> (PathBuf, String) {
        use flate2::{write::GzEncoder, Compression};
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
    fn vdf_launch_update_preserves_unknown_fields_and_original_backup() {
        let root = fixture_dir("vdf");
        let config = root.join("localconfig.vdf");
        let legacy = root.join("backups/localconfig_backup.vdf");
        let fixture = r#"
"UserLocalConfigStore"
{
    "UnknownTop" "keep me"
    "Software"
    {
        "Valve"
        {
            "Steam"
            {
                "apps"
                {
                    "311210"
                    {
                        "LaunchOptions" "-novid +set fs_game old"
                        "Cloud" { "enabled" "1" }
                    }
                }
            }
        }
    }
}
"OtherRoot" "still here" [$LINUX]
"#;
        fs::write(&config, fixture).unwrap();

        let options = set_launch_options_at(
            &config,
            &legacy,
            APP_ID,
            "+set fs_game 2994481309",
            false,
            false,
        )
        .unwrap();
        assert_eq!(options, "-novid +set fs_game 2994481309");
        let updated = read_vdf(&config).unwrap();
        assert_eq!(
            string_value(&updated, &["UserLocalConfigStore", "UnknownTop"]),
            "keep me"
        );
        assert_eq!(
            string_value(
                &updated,
                &[
                    "UserLocalConfigStore",
                    "Software",
                    "Valve",
                    "Steam",
                    "apps",
                    APP_ID,
                    "Cloud",
                    "enabled",
                ],
            ),
            "1"
        );
        assert_eq!(string_value(&updated, &["OtherRoot"]), "still here");
        assert_eq!(fs::read(&legacy).unwrap(), fixture.as_bytes());
        assert_eq!(
            fs::read(fs_ops::backup_path(&config)).unwrap(),
            fixture.as_bytes()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn discovers_modern_and_legacy_library_folders_and_game() {
        let root = fixture_dir("libraries");
        let primary = root.join("primary");
        let legacy = root.join("legacy");
        let modern = root.join("modern");
        for directory in [&primary, &legacy, &modern] {
            fs::create_dir_all(directory.join("steamapps")).unwrap();
        }
        fs::write(
            primary.join("steamapps/libraryfolders.vdf"),
            format!(
                "\"libraryfolders\"\n{{\n\"0\" \"{}\"\n\"1\" {{ \"path\" \"{}\" \"apps\" {{ \"311210\" \"1\" }} }}\n}}",
                legacy.display(),
                modern.display()
            ),
        )
        .unwrap();
        let game = modern.join("steamapps/common").join(GAME_DIRECTORY);
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("BlackOps3.exe"), b"exe").unwrap();

        let libraries = library_paths_from_roots(vec![primary.clone()]);
        assert_eq!(libraries, vec![primary, legacy, modern.clone()]);
        assert!(has_game_executable(
            &modern.join("steamapps/common").join(GAME_DIRECTORY)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn selects_only_numeric_steam_users_deterministically() {
        let root = fixture_dir("users");
        for name in ["config", "200", "00010", "9x", "9"] {
            fs::create_dir_all(root.join("userdata").join(name)).unwrap();
        }
        assert_eq!(user_id_in(&root).as_deref(), Some("9"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn selects_most_recent_login_user_when_present() {
        let root = fixture_dir("recent-user");
        fs::create_dir_all(root.join("userdata/10")).unwrap();
        fs::create_dir_all(root.join("userdata/20")).unwrap();
        fs::create_dir_all(root.join("config")).unwrap();
        fs::write(
            root.join("config/loginusers.vdf"),
            format!(
                r#""users" {{
  "{}" {{ "MostRecent" "0" }}
  "{}" {{ "MostRecent" "1" }}
}}"#,
                STEAM_ID64_OFFSET + 10,
                STEAM_ID64_OFFSET + 20,
            ),
        )
        .unwrap();

        assert_eq!(user_id_in(&root).as_deref(), Some("20"));
        fs::remove_dir_all(root).unwrap();
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
    fn reports_subscribed_then_installed_workshop_state() {
        let root = fixture_dir("workshop");
        let workshop = root.join("steamapps/workshop");
        fs::create_dir_all(&workshop).unwrap();
        fs::write(
            workshop.join("appworkshop_311210.acf"),
            r#""AppWorkshop" { "WorkshopItemsInstalled" { "2994481309" { "size" "4" } } }"#,
        )
        .unwrap();
        let libraries = vec![root.clone()];
        let state = workshop_item_state_in(&libraries, APP_ID, "2994481309");
        assert_eq!(state.state, "Subscribed (not installed yet)");
        assert!(state.subscribed);
        assert!(!state.installed);

        let content = workshop.join("content/311210/2994481309");
        fs::create_dir_all(&content).unwrap();
        fs::write(content.join("mod.ff"), b"data").unwrap();
        let state = workshop_item_state_in(&libraries, APP_ID, "2994481309");
        assert_eq!(state.state, "Installed");
        assert!(state.installed);
        assert_eq!(state.path.as_deref(), Some(content.as_path()));
        fs::remove_dir_all(root).unwrap();
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
            assert!(
                error.contains("No such file") || error.contains("not found"),
                "{error}"
            );
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
        assert!(adopt_fixture(&missing)
            .unwrap_err()
            .contains("launch_options"));
        assert_legacy_unowned(&missing);
        fs::remove_dir_all(missing.root).unwrap();

        let mapping = legacy_compatibility_fixture("legacy-mapping-mismatch");
        fs::write(&mapping.steam_vdf, compatibility_config()).unwrap();
        assert!(adopt_fixture(&mapping).unwrap_err().contains("mapping"));
        assert_legacy_unowned(&mapping);
        fs::remove_dir_all(mapping.root).unwrap();

        let launch = legacy_compatibility_fixture("legacy-launch-mismatch");
        fs::write(&launch.local_vdf, steam_config("-novid")).unwrap();
        assert!(adopt_fixture(&launch)
            .unwrap_err()
            .contains("launch options"));
        assert!(!launch.ownership.exists());
        assert!(!launch.destination.join(TOOL_MARKER_FILENAME).exists());
        fs::remove_dir_all(launch.root).unwrap();
    }

    #[test]
    fn legacy_adoption_rejects_payload_mismatch_and_backup_sibling_without_mutation() {
        let mismatch = legacy_compatibility_fixture("legacy-payload-mismatch");
        fs::write(mismatch.destination.join("proton"), b"modified").unwrap();
        assert!(adopt_fixture(&mismatch)
            .unwrap_err()
            .contains("bytes differ"));
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
        assert!(trusted_proton_source_at(&cache, &digest)
            .unwrap_err()
            .contains("unavailable"));
        let outside = root.join("outside.tar.gz");
        fs::write(&outside, b"not trusted").unwrap();
        symlink(&outside, &archive).unwrap();
        assert!(trusted_proton_source_at(&cache, &digest)
            .unwrap_err()
            .contains("regular file"));
        assert!(fs::symlink_metadata(&archive)
            .unwrap()
            .file_type()
            .is_symlink());
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
        assert!(steam
            .join("compatibilitytools.d")
            .join(state.original_backup.unwrap())
            .join("original")
            .is_file());
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

    #[test]
    fn merge_matches_legacy_launch_option_rules() {
        assert_eq!(
            merged_launch_options(
                "WINEDLLOVERRIDES=\"dsound=n,b\" %command% -novid +set fs_game old",
                "+set fs_game new",
                false,
            ),
            "WINEDLLOVERRIDES=\"dsound=n,b\" %command% -novid +set fs_game new"
        );
        assert_eq!(
            merged_launch_options("-novid +set fs_game old", "", true),
            "-novid +set fs_game old"
        );
    }
}
