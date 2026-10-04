use std::{
    cmp::Ordering,
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::UNIX_EPOCH,
};

use crate::{app::AppState, fs_ops, models::LaunchProfile};
use regex::Regex;

// Proton and compatibility-tool ownership are used only by Linux Enhanced installs.
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub(super) use linux::{cleanup_bo3_enhanced_linux, configure_bo3_enhanced_linux};

pub const APP_ID: &str = "311210";
pub const COMPATIBLE_DEPOT_APP_ID: &str = APP_ID;
pub const COMPATIBLE_DEPOT_ID: &str = "311211";

const GAME_DIRECTORY: &str = "Call of Duty Black Ops III";
const GAME_EXECUTABLES: [&str; 2] = ["BlackOpsIII.exe", "BlackOps3.exe"];
const COMPATIBLE_BUILD_SHA256: &str =
    "66b95eb4667bd5b3b3d230e7bed1d29ccd261d48ca2699f01216c863be24ff44";
const STEAM_ID64_OFFSET: u64 = 76_561_197_960_265_728;

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
                    output.push(' ');
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

fn update_vdf_with_backup(
    path: &Path,
    legacy_backup: Option<&Path>,
    rolling: bool,
    change: impl FnOnce(&mut VdfObject) -> Result<(), String>,
) -> Result<(), String> {
    let original = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let text = String::from_utf8(original.clone())
        .map_err(|error| format!("{} is not valid UTF-8: {error}", path.display()))?;
    let mut document = parse_vdf(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    change(&mut document)?;
    let updated = serialize_vdf(&document);
    parse_vdf(&updated).map_err(|error| format!("refusing to write invalid VDF: {error}"))?;
    if rolling {
        let backup =
            legacy_backup.ok_or_else(|| "Rolling VDF backup path is required.".to_owned())?;
        // Match Python copy2: refresh bytes and permissions on every apply.
        fs_ops::atomic_write(backup, &original)?;
        fs::set_permissions(
            backup,
            fs::metadata(path).map_err(|e| e.to_string())?.permissions(),
        )
        .map_err(|e| e.to_string())?;
    }
    if updated.as_bytes() == original {
        return Ok(());
    }
    if !rolling {
        preserve_original(path, &original, legacy_backup)?;
    }
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

#[cfg(not(windows))]
fn home_dir() -> Option<PathBuf> {
    let variable = "HOME";
    env::var_os(variable)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(windows)]
fn windows_steam_roots() -> Vec<PathBuf> {
    use winreg::{RegKey, enums::*};

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
        if let Ok(key) = RegKey::predef(hive).open_subkey(subkey)
            && let Ok(path) = key.get_value::<String, _>(value_name)
        {
            candidates.push(PathBuf::from(path));
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
    let candidates = vec![
        home_dir()?
            .join("Library/Application Support/Steam/steamapps/common")
            .join(GAME_DIRECTORY),
    ];
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
    update_vdf_with_backup(config, Some(legacy_backup), true, |document| {
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
        if let Ok(document) = read_vdf(&manifest)
            && let Some(value) = value_at(&document, &["AppWorkshop"])
        {
            subscribed |= vdf_contains(value, workshop_id);
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

#[cfg(all(unix, not(target_os = "macos")))]
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

fn close_steam(app: &AppState) -> Result<(), String> {
    if let Some(result) = app.steam_lifecycle_override(false) {
        return result;
    }
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
    linux::wait_for_steam_exit_with(50, linux::linux_steam_running, || {
        std::thread::sleep(std::time::Duration::from_millis(100))
    })?;

    Ok(())
}

fn open_steam(app: &AppState) -> Result<(), String> {
    if let Some(result) = app.steam_lifecycle_override(true) {
        return result;
    }
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
    let result = result.map(|final_options| {
        app.log(
            "Success",
            format!("Config backup created at {}", legacy_backup.display()),
        );
        app.log(
            "Info",
            format!("Setting launch options to: {final_options}"),
        );
    });
    if let Err(error) = open_steam(app) {
        app.log(
            "Warning",
            format!("Launch options were processed, but Steam could not be reopened: {error}"),
        );
    }
    result
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    pub(super) fn fixture_dir(name: &str) -> PathBuf {
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

    pub(super) fn string_value<'a>(document: &'a VdfObject, path: &[&str]) -> &'a str {
        match value_at(document, path).unwrap() {
            VdfValue::String(value) => value,
            VdfValue::Object(_) => panic!("expected string"),
        }
    }

    #[cfg(unix)]
    pub(super) fn steam_config(options: &str) -> String {
        format!(
            r#""UserLocalConfigStore" {{
  "Software" {{ "Valve" {{ "Steam" {{ "apps" {{
    "{APP_ID}" {{ "LaunchOptions" {} }}
  }} }} }} }}
}}"#,
            quote_vdf(options)
        )
    }

    #[test]
    fn vdf_launch_update_preserves_unknown_fields_and_rolling_backup() {
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
        assert!(!fs_ops::backup_path(&config).exists());
        let first_update = fs::read(&config).unwrap();
        set_launch_options_at(&config, &legacy, APP_ID, "", true, false).unwrap();
        assert_eq!(fs::read(&legacy).unwrap(), first_update);
        assert_eq!(fs::read(&config).unwrap(), first_update);
        set_launch_options_at(&config, &legacy, APP_ID, "+set fs_game next", false, false).unwrap();
        assert_eq!(fs::read(&legacy).unwrap(), first_update);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn launch_backup_failure_leaves_steam_config_unchanged() {
        let root = fixture_dir("launch-backup-failure");
        let config = root.join("localconfig.vdf");
        let original = b"\"UserLocalConfigStore\" { \"unknown\" \"keep\" }\n";
        fs::write(&config, original).unwrap();
        let blocked = root.join("blocked");
        fs::write(&blocked, b"not a directory").unwrap();
        assert!(
            set_launch_options_at(
                &config,
                &blocked.join("backup.vdf"),
                APP_ID,
                "-novid",
                false,
                false
            )
            .is_err()
        );
        assert_eq!(fs::read(&config).unwrap(), original);
        assert!(!fs_ops::backup_path(&config).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn rolling_launch_backup_preserves_source_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let root = fixture_dir("launch-backup-mode");
        let config = root.join("localconfig.vdf");
        let backup = root.join("backups/backup.vdf");
        fs::write(&config, b"\"UserLocalConfigStore\" {}\n").unwrap();
        fs::set_permissions(&config, fs::Permissions::from_mode(0o600)).unwrap();
        set_launch_options_at(&config, &backup, APP_ID, "-novid", false, false).unwrap();
        assert_eq!(
            config.metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            backup.metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::set_permissions(&config, fs::Permissions::from_mode(0o640)).unwrap();
        let first = fs::read(&config).unwrap();
        set_launch_options_at(&config, &backup, APP_ID, "", true, false).unwrap();
        assert_eq!(fs::read(&backup).unwrap(), first);
        assert_eq!(
            backup.metadata().unwrap().permissions().mode() & 0o777,
            0o640
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
