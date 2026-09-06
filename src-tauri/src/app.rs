use std::{
    collections::VecDeque,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::{AppHandle, Emitter, Manager};
use time::OffsetDateTime;

use crate::{
    fs_ops::{atomic_write, backup_path, existing_backup},
    models::{AdvancedState, GraphicsState, LogEntry, QolState},
};

pub const APP_VERSION: &str = env!("PATCHOPSIII_VERSION");

#[derive(Clone)]
pub struct AppState(Arc<AppStateInner>);

struct AppStateInner {
    app: AppHandle,
    data_dir: PathBuf,
    logs: Mutex<VecDeque<LogEntry>>,
    operation: Mutex<()>,
}

pub fn has_game_executable(directory: &Path) -> bool {
    directory.is_dir()
        && ["BlackOpsIII.exe", "BlackOps3.exe"]
            .iter()
            .any(|name| directory.join(name).is_file())
}

pub fn set_game_directory(state: &AppState, path: &str) -> Result<(), String> {
    let candidate = Path::new(path)
        .canonicalize()
        .map_err(|error| format!("Unable to open the selected directory: {error}"))?;
    if !has_game_executable(&candidate) {
        return Err("BlackOps3.exe or BlackOpsIII.exe was not found.".into());
    }
    let mut settings = state.load_settings();
    settings.game_dir = Some(candidate.to_string_lossy().into_owned());
    state.save_settings(&settings)?;
    state.log(
        "Success",
        format!("Game directory set to {}", candidate.display()),
    );
    Ok(())
}

pub fn release_channel(settings: &Settings) -> &str {
    match settings.release_channel.as_deref() {
        Some("beta") => "beta",
        _ => "stable",
    }
}

pub fn set_release_channel(state: &AppState, channel: &str) -> Result<(), String> {
    if !matches!(channel, "stable" | "beta") {
        return Err("Release channel must be beta or stable.".into());
    }
    let mut settings = state.load_settings();
    settings.release_channel = Some(channel.to_owned());
    state.save_settings(&settings)?;
    state.log(
        "Success",
        format!("Release channel set to {}.", title_case(channel)),
    );
    Ok(())
}

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

pub fn config_path(game_dir: &Path) -> PathBuf {
    game_dir.join("players").join("config.ini")
}

pub fn read_config(game_dir: &Path) -> String {
    fs::read(config_path(game_dir))
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

fn config_value<'a>(content: &'a str, key: &str) -> Option<&'a str> {
    content.lines().find_map(|line| {
        let line = line.trim_start();
        let rest = line.strip_prefix(key)?;
        if !rest.starts_with(char::is_whitespace) && !rest.starts_with('=') {
            return None;
        }
        let value = rest.trim_start().strip_prefix('=')?.trim_start();
        let value = value.strip_prefix('"')?;
        value.find('"').map(|end| &value[..end])
    })
}

fn config_i32(content: &str, key: &str, default: i32) -> i32 {
    config_value(content, key)
        .and_then(|value| value.parse::<f64>().ok())
        .map(|value| value as i32)
        .unwrap_or(default)
}

fn config_f64(content: &str, key: &str, default: f64) -> f64 {
    config_value(content, key)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn config_bool(content: &str, key: &str, enabled: &str, default: &str) -> bool {
    config_value(content, key).unwrap_or(default) == enabled
}

pub fn graphics_state(content: &str) -> GraphicsState {
    GraphicsState {
        max_fps: config_i32(content, "MaxFPS", 165),
        fov: config_i32(content, "FOV", 80),
        display_mode: config_i32(content, "FullScreenMode", 1),
        resolution: config_value(content, "WindowSize")
            .unwrap_or("1920x1080")
            .to_owned(),
        refresh_rate: config_f64(content, "RefreshRate", 60.0),
        render_resolution: config_i32(content, "ResolutionPercent", 100),
        vsync: config_bool(content, "Vsync", "1", "1"),
        draw_fps: config_bool(content, "DrawFPS", "1", "0"),
    }
}

pub fn advanced_state(game_dir: Option<&Path>, content: &str) -> AdvancedState {
    let video_memory = config_value(content, "VideoMemory").unwrap_or("1");
    let stream_min = config_value(content, "StreamMinResident").unwrap_or("0");
    AdvancedState {
        smooth_framerate: config_bool(content, "SmoothFramerate", "1", "0"),
        unlock_options: config_bool(content, "RestrictGraphicsOptions", "0", "1"),
        reduce_cpu: config_bool(content, "SerializeRender", "2", "0"),
        max_frame_latency: config_i32(content, "MaxFrameLatency", 1),
        vram_limited: video_memory != "1" || stream_min != "0",
        vram_target: (config_f64(content, "VideoMemory", 0.75) * 100.0) as i32,
        config_readonly: game_dir
            .map(config_path)
            .and_then(|path| path.metadata().ok())
            .is_some_and(|metadata| metadata.permissions().readonly()),
    }
}

fn valid_config_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

pub fn write_config_values(
    game_dir: &Path,
    updates: &[(String, String, String)],
) -> Result<(), String> {
    let path = config_path(game_dir);
    if !path.is_file() {
        return Err(format!("config.ini not found at {}", path.display()));
    }
    if path
        .metadata()
        .map_err(|error| error.to_string())?
        .permissions()
        .readonly()
    {
        return Err("config.ini is read-only. Unlock it before making changes.".into());
    }
    let mut content = read_config(game_dir);
    let line_ending = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut lines: Vec<String> = content.lines().map(str::to_owned).collect();

    for (key, value, comment) in updates {
        if !valid_config_key(key)
            || value.chars().any(char::is_control)
            || comment
                .chars()
                .any(|character| matches!(character, '\r' | '\n'))
        {
            return Err("Invalid configuration value.".into());
        }
        let replacement = format!("{key} = \"{value}\" // {comment}");
        let mut matched = false;
        for line in &mut lines {
            let trimmed = line.trim_start();
            let Some(rest) = trimmed.strip_prefix(key) else {
                continue;
            };
            if rest.starts_with(char::is_whitespace) || rest.starts_with('=') {
                *line = replacement.clone();
                matched = true;
            }
        }
        if !matched {
            lines.push(replacement);
        }
    }
    content = lines.join(line_ending);
    content.push_str(line_ending);
    atomic_write(&path, content.as_bytes())
}

fn config_input(key: &str, value: &Value) -> Result<(String, &'static str), String> {
    let raw = match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        _ => return Err("Invalid configuration value.".into()),
    };
    let number = || {
        raw.parse::<i32>()
            .map_err(|_| "Expected a whole number.".to_string())
    };
    let ranged = |minimum, maximum| {
        let value = number()?;
        if (minimum..=maximum).contains(&value) {
            Ok(value.to_string())
        } else {
            Err(format!("Value must be between {minimum} and {maximum}."))
        }
    };
    let result = match key {
        "MaxFPS" => (ranged(0, 1000)?, "Maximum FPS cap"),
        "FOV" => (ranged(65, 120)?, "Field of view"),
        "FullScreenMode" => (
            ranged(0, 2)?,
            "0=Windowed,1=Fullscreen,2=Fullscreen Windowed",
        ),
        "RefreshRate" => (ranged(1, 240)?, "1 to 240"),
        "ResolutionPercent" => (ranged(50, 200)?, "50 to 200"),
        "MaxFrameLatency" => (ranged(0, 4)?, "Maximum frame latency"),
        "Vsync" if matches!(raw.as_str(), "0" | "1") => (raw, "Vertical sync"),
        "DrawFPS" if matches!(raw.as_str(), "0" | "1") => (raw, "FPS counter"),
        "SmoothFramerate" if matches!(raw.as_str(), "0" | "1") => (raw, "Frame smoothing"),
        "RestrictGraphicsOptions" if matches!(raw.as_str(), "0" | "1") => {
            (raw, "Expose all graphics options")
        }
        "SerializeRender" if matches!(raw.as_str(), "0" | "2") => (raw, "Reduce CPU pressure"),
        "WindowSize" => {
            let mut parts = raw.split('x');
            let width = parts.next().and_then(|part| part.parse::<u32>().ok());
            let height = parts.next().and_then(|part| part.parse::<u32>().ok());
            if parts.next().is_some()
                || !width.is_some_and(|value| (320..=16384).contains(&value))
                || !height.is_some_and(|value| (200..=16384).contains(&value))
            {
                return Err("Resolution must look like 1920x1080.".into());
            }
            (raw, "Screen resolution")
        }
        _ => return Err("Unsupported configuration key or value.".into()),
    };
    Ok(result)
}

pub fn set_config_value(
    state: &AppState,
    game_dir: &Path,
    key: &str,
    value: Value,
) -> Result<(), String> {
    let (value, comment) = config_input(key, &value)?;
    write_config_values(
        game_dir,
        &[(key.to_owned(), value.clone(), comment.to_owned())],
    )?;
    state.log("Success", format!("Set {key} to {value}."));
    Ok(())
}

pub fn set_vram_target(
    state: &AppState,
    game_dir: &Path,
    limited: bool,
    target: i32,
) -> Result<(), String> {
    if limited && !(75..=100).contains(&target) {
        return Err("VRAM target must be between 75 and 100.".into());
    }
    let updates = if limited {
        vec![
            (
                "VideoMemory".into(),
                format!("{:.2}", target as f64 / 100.0)
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_owned(),
                "0.75 to 1".into(),
            ),
            ("StreamMinResident".into(), "1".into(), "0 or 1".into()),
        ]
    } else {
        vec![
            ("VideoMemory".into(), "1".into(), "0.75 to 1".into()),
            ("StreamMinResident".into(), "0".into(), "0 or 1".into()),
        ]
    };
    write_config_values(game_dir, &updates)?;
    state.log(
        "Success",
        if limited {
            format!("Limited VRAM usage set to {target}%.")
        } else {
            "Enabled full VRAM usage.".into()
        },
    );
    Ok(())
}

pub fn set_config_readonly(state: &AppState, game_dir: &Path, enabled: bool) -> Result<(), String> {
    let path = config_path(game_dir);
    let metadata = path
        .metadata()
        .map_err(|_| "config.ini was not found.".to_string())?;
    let mut permissions = metadata.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = permissions.mode();
        permissions.set_mode(if enabled { mode & !0o222 } else { mode | 0o200 });
    }
    #[cfg(not(unix))]
    permissions.set_readonly(enabled);
    fs::set_permissions(&path, permissions).map_err(|error| error.to_string())?;
    state.log(
        "Success",
        format!(
            "config.ini set to {}.",
            if enabled { "read-only" } else { "writable" }
        ),
    );
    Ok(())
}

pub fn qol_state(game_dir: Option<&Path>) -> QolState {
    let Some(game_dir) = game_dir else {
        return QolState::default();
    };
    let video_dir = game_dir.join("video");
    let intro = video_dir.join("BO3_Global_Logo_LogoSequence.mkv");
    let d3dcompiler = game_dir.join("d3dcompiler_46.dll");
    let mut mkv = 0;
    let mut backups = 0;
    if let Ok(entries) = fs::read_dir(video_dir) {
        for path in entries.flatten().map(|entry| entry.path()) {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if name.ends_with(".mkv") {
                mkv += 1;
            }
            if name.ends_with(".mkv.patchops.bak") || name.ends_with(".mkv.bak") {
                backups += 1;
            }
        }
    }
    QolState {
        d3dcompiler: existing_backup(&d3dcompiler).is_some(),
        intro: existing_backup(&intro).is_some(),
        all_intros: backups > 0 && mkv == 0,
    }
}

pub fn set_intro_skip(state: &AppState, game_dir: &Path, enabled: bool) -> Result<(), String> {
    let target = game_dir
        .join("video")
        .join("BO3_Global_Logo_LogoSequence.mkv");
    toggle_backup(&target, enabled)?;
    state.log(
        "Success",
        if enabled {
            "Intro video skipped."
        } else {
            "Intro video restored."
        },
    );
    Ok(())
}

pub fn set_d3dcompiler(state: &AppState, game_dir: &Path, enabled: bool) -> Result<(), String> {
    let target = game_dir.join("d3dcompiler_46.dll");
    toggle_backup(&target, enabled)?;
    state.log(
        "Success",
        if enabled {
            "Renamed d3dcompiler_46.dll to reduce stuttering."
        } else {
            "Restored d3dcompiler_46.dll."
        },
    );
    Ok(())
}

fn toggle_backup(target: &Path, enabled: bool) -> Result<(), String> {
    if enabled {
        if existing_backup(target).is_some() {
            return Ok(());
        }
        if !target.is_file() {
            return Err(format!("{} was not found.", target.display()));
        }
        fs::rename(target, backup_path(target)).map_err(|error| error.to_string())
    } else {
        let backup = existing_backup(target)
            .ok_or_else(|| format!("No backup was found for {}.", target.display()))?;
        if target.exists() {
            return Err(format!("Refusing to overwrite {}.", target.display()));
        }
        fs::rename(backup, target).map_err(|error| error.to_string())
    }
}

pub fn set_all_intro_skip(state: &AppState, game_dir: &Path, enabled: bool) -> Result<(), String> {
    let video_dir = game_dir.join("video");
    if !video_dir.is_dir() {
        return Err("Video directory was not found.".into());
    }
    let mut operations = Vec::new();
    for path in fs::read_dir(&video_dir)
        .map_err(|error| error.to_string())?
        .flatten()
        .map(|entry| entry.path())
    {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if enabled && name.ends_with(".mkv") && existing_backup(&path).is_none() {
            operations.push((path.clone(), backup_path(&path)));
        } else if !enabled && (name.ends_with(".mkv.patchops.bak") || name.ends_with(".mkv.bak")) {
            let suffix = if name.ends_with(".patchops.bak") {
                ".patchops.bak"
            } else {
                ".bak"
            };
            let target = path.with_file_name(&name[..name.len() - suffix.len()]);
            if !target.exists() {
                operations.push((path.clone(), target));
            }
        }
    }
    let mut completed = Vec::new();
    for (source, destination) in &operations {
        if let Err(error) = fs::rename(source, destination) {
            for (source, destination) in completed.into_iter().rev() {
                let _ = fs::rename(destination, source);
            }
            return Err(error.to_string());
        }
        completed.push((source.clone(), destination.clone()));
    }
    state.log(
        "Success",
        if enabled {
            "All intro videos skipped."
        } else {
            "Intro videos restored."
        },
    );
    Ok(())
}

static PRESETS: OnceLock<Map<String, Value>> = OnceLock::new();

fn presets() -> &'static Map<String, Value> {
    PRESETS.get_or_init(|| {
        serde_json::from_str(include_str!("../../presets.json"))
            .expect("bundled presets.json must be valid")
    })
}

pub fn preset_names() -> Vec<String> {
    presets().keys().cloned().collect()
}

pub fn apply_preset(state: &AppState, game_dir: &Path, name: &str) -> Result<(), String> {
    let preset = presets()
        .get(name)
        .and_then(Value::as_object)
        .ok_or_else(|| "Unknown preset.".to_string())?;
    let mut updates = Vec::with_capacity(preset.len());
    for (key, item) in preset {
        if key == "ReduceStutter" {
            continue;
        }
        let values = item
            .as_array()
            .filter(|values| values.len() == 2)
            .ok_or_else(|| format!("Preset value for {key} is invalid."))?;
        let value = values[0]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| values[0].to_string());
        let comment = values[1]
            .as_str()
            .ok_or_else(|| format!("Preset comment for {key} is invalid."))?;
        updates.push((key.clone(), value, comment.to_owned()));
    }
    write_config_values(game_dir, &updates)?;
    state.log("Success", format!("Applied preset '{name}'."));
    Ok(())
}

pub fn log_payload(state: &AppState) -> String {
    let body = fs::read_to_string(state.log_path()).unwrap_or_default();
    format!(
        "PatchOpsIII {APP_VERSION} - {} {} ({}) logs:\n```\n{}\n```",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::env::consts::FAMILY,
        if body.trim().is_empty() {
            "(no log entries found)"
        } else {
            body.trim()
        }
    )
}

pub fn clear_mod_files(state: &AppState) -> Result<(), String> {
    let directory = state.mod_files_dir();
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    for entry in fs::read_dir(&directory).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            fs::remove_dir_all(path).map_err(|error| error.to_string())?;
        } else {
            fs::remove_file(path).map_err(|error| error.to_string())?;
        }
    }
    state.log(
        "Success",
        format!("Cleared mod files in {}", directory.display()),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "patchops-app-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn config_reader_uses_first_quoted_value() {
        let content = "MaxFPS = \"120\" // first\nMaxFPS = \"240\" // second\nLoose=12\n";
        assert_eq!(config_value(content, "MaxFPS"), Some("120"));
        assert_eq!(config_value(content, "Loose"), None);
    }

    #[test]
    fn only_known_config_inputs_are_accepted() {
        assert!(config_input("MaxFPS", &Value::from(240)).is_ok());
        assert!(config_input("MaxFPS", &Value::from(1001)).is_err());
        assert!(config_input("WindowSize", &Value::from("1920x1080")).is_ok());
        assert!(config_input("WindowSize", &Value::from("1920x1080\nInjected=1")).is_err());
        assert!(config_input("Unknown", &Value::from(1)).is_err());
    }

    #[test]
    fn legacy_settings_round_trip_without_losing_unknown_fields() {
        let settings: Settings = serde_json::from_str(
            r#"{"game_dir":"C:\\Games\\BO3","release_channel":"beta","future":true}"#,
        )
        .unwrap();
        assert_eq!(settings.game_dir.as_deref(), Some(r"C:\Games\BO3"));
        assert_eq!(release_channel(&settings), "beta");

        let saved = serde_json::to_value(settings).unwrap();
        assert_eq!(saved["game_dir"], r"C:\Games\BO3");
        assert_eq!(saved["release_channel"], "beta");
        assert_eq!(saved["future"], true);
    }

    #[test]
    fn readonly_config_cannot_be_replaced() {
        let game = temp_dir("readonly-config");
        let path = config_path(&game);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = b"MaxFPS = \"120\" // Maximum FPS cap\n";
        fs::write(&path, original).unwrap();
        let mut permissions = path.metadata().unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions).unwrap();

        let updates = vec![("MaxFPS".into(), "240".into(), "Maximum FPS cap".into())];
        let error = write_config_values(&game, &updates).unwrap_err();
        assert!(error.contains("read-only"));
        assert_eq!(fs::read(&path).unwrap(), original);

        let mut permissions = path.metadata().unwrap().permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(0o644);
        }
        #[cfg(not(unix))]
        permissions.set_readonly(false);
        fs::set_permissions(&path, permissions).unwrap();
        fs::remove_dir_all(game).unwrap();
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub game_dir: Option<String>,
    #[serde(default)]
    pub release_channel: Option<String>,
    #[serde(default)]
    pub enhanced_dump_source: Option<String>,
    #[serde(default)]
    pub enhanced_exe_hash: Option<String>,
    #[serde(default)]
    pub enhanced_exe_hashes: Map<String, Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl AppState {
    pub fn new(app: AppHandle) -> Result<Self, String> {
        let data_dir = app
            .path()
            .data_dir()
            .map_err(|error| error.to_string())?
            .join("PatchOpsIII");
        fs::create_dir_all(&data_dir).map_err(|error| error.to_string())?;
        Ok(Self(Arc::new(AppStateInner {
            app,
            data_dir,
            logs: Mutex::new(VecDeque::with_capacity(300)),
            operation: Mutex::new(()),
        })))
    }

    pub fn app(&self) -> &AppHandle {
        &self.0.app
    }

    pub fn data_dir(&self) -> &Path {
        &self.0.data_dir
    }

    pub fn settings_path(&self) -> PathBuf {
        self.data_dir().join("electron-settings.json")
    }

    pub fn mod_files_dir(&self) -> PathBuf {
        self.data_dir().join("BO3 Mod Files")
    }

    pub fn log_path(&self) -> PathBuf {
        self.data_dir().join("PatchOpsIII.log")
    }

    pub fn lock_operation(&self) -> Result<std::sync::MutexGuard<'_, ()>, String> {
        self.0
            .operation
            .lock()
            .map_err(|_| "another operation failed unexpectedly".to_string())
    }

    pub fn load_settings(&self) -> Settings {
        fs::read_to_string(self.settings_path())
            .ok()
            .and_then(|body| serde_json::from_str(&body).ok())
            .unwrap_or_default()
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        let body = serde_json::to_vec_pretty(settings).map_err(|error| error.to_string())?;
        atomic_write(&self.settings_path(), &body)
    }

    pub fn log(&self, category: &str, message: impl Into<String>) {
        let message = message.into();
        let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
        let timestamp = format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            now.year(),
            now.month() as u8,
            now.day(),
            now.hour(),
            now.minute(),
            now.second()
        );
        let line = format!("{timestamp} - {category}: {message}");
        let entry = LogEntry {
            category: category.to_owned(),
            message,
            line: line.clone(),
        };

        if let Ok(mut logs) = self.0.logs.lock() {
            if logs.len() == 300 {
                logs.pop_front();
            }
            logs.push_back(entry.clone());
        }
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.log_path())
        {
            let _ = writeln!(file, "{line}");
        }
        let _ = self.app().emit("patchops-log", entry);
    }

    pub fn recent_logs(&self) -> Vec<LogEntry> {
        self.0
            .logs
            .lock()
            .map(|logs| logs.iter().rev().take(80).cloned().collect::<Vec<_>>())
            .map(|mut logs| {
                logs.reverse();
                logs
            })
            .unwrap_or_default()
    }

    pub fn clear_logs(&self) -> Result<(), String> {
        atomic_write(&self.log_path(), b"")?;
        self.0
            .logs
            .lock()
            .map_err(|_| "log buffer is unavailable".to_string())?
            .clear();
        Ok(())
    }
}
