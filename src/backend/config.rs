use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use regex::Regex;
use serde::Deserialize;

use crate::logging::{LogCategory, log};

#[derive(Debug, Clone, Deserialize)]
pub struct PresetEntry(pub String, pub String);

#[derive(Debug, Clone, Deserialize)]
pub struct Presets(pub HashMap<String, HashMap<String, (String, String)>>);

pub fn update_config_values(
    game_dir: &Path,
    changes: &[(Regex, String)],
    success_message: &str,
) -> Result<()> {
    let config_path = config_ini(game_dir);
    if !config_path.exists() {
        anyhow::bail!("config.ini not found at {}", config_path.display());
    }

    let mut contents = String::new();
    fs::File::open(&config_path)?.read_to_string(&mut contents)?;
    let mut lines: Vec<String> = contents.lines().map(|s| s.to_string()).collect();

    for line in &mut lines {
        for (pattern, replacement) in changes {
            if pattern.is_match(line) {
                *line = replacement.clone();
                break;
            }
        }
    }

    let mut file = fs::File::create(&config_path)?;
    for line in lines {
        writeln!(file, "{}", line)?;
    }
    log(LogCategory::Success, success_message);
    Ok(())
}

pub fn set_config_value(game_dir: &Path, key: &str, value: &str, comment: &str) -> Result<()> {
    let pattern = Regex::new(&format!(r"^\s*{}\s*=", regex::escape(key)))?;
    let replacement = format!("{} = \"{}\" // {}", key, value, comment);
    update_config_values(
        game_dir,
        &[(pattern, replacement)],
        &format!("Set {} to {}", key, value),
    )
}

pub fn toggle_stutter_reduction(game_dir: &Path, enable: bool) -> Result<()> {
    let dll_file = game_dir.join("d3dcompiler_46.dll");
    let dll_bak = dll_file.with_extension("dll.bak");
    if enable {
        if dll_file.exists() {
            fs::rename(&dll_file, &dll_bak)?;
            log(
                LogCategory::Success,
                "Renamed d3dcompiler_46.dll to reduce stuttering",
            );
        } else if dll_bak.exists() {
            log(LogCategory::Info, "Stutter reduction already enabled");
        } else {
            log(LogCategory::Warning, "d3dcompiler_46.dll not found");
        }
    } else if dll_bak.exists() {
        fs::rename(&dll_bak, &dll_file)?;
        log(LogCategory::Success, "Restored d3dcompiler_46.dll");
    } else {
        log(LogCategory::Warning, "Backup not found to restore");
    }
    Ok(())
}

pub fn set_config_readonly(game_dir: &Path, read_only: bool) -> Result<()> {
    let config_path = config_ini(game_dir);
    if !config_path.exists() {
        anyhow::bail!("config.ini not found at {}", config_path.display());
    }
    let metadata = fs::metadata(&config_path)?;
    let mut permissions = metadata.permissions();
    permissions.set_readonly(read_only);
    fs::set_permissions(&config_path, permissions)?;
    log(
        LogCategory::Success,
        if read_only {
            "config.ini set to read-only"
        } else {
            "config.ini set to writable"
        },
    );
    Ok(())
}

pub fn load_presets(path: &Path) -> Result<HashMap<String, HashMap<String, (String, String)>>> {
    let data = fs::read_to_string(path)
        .with_context(|| format!("Failed to read presets from {}", path.display()))?;
    let map = serde_json::from_str(&data)?;
    Ok(map)
}

pub fn apply_preset(
    game_dir: &Path,
    preset_name: &str,
    presets: &HashMap<String, HashMap<String, (String, String)>>,
) -> Result<()> {
    let preset = presets
        .get(preset_name)
        .ok_or_else(|| anyhow::anyhow!("Preset {} not found", preset_name))?;
    let config_path = config_ini(game_dir);
    if !config_path.exists() {
        anyhow::bail!("config.ini not found at {}", config_path.display());
    }

    let mut changes: Vec<(Regex, String)> = Vec::new();
    for (key, (value, comment)) in preset {
        if key == "ReduceStutter" {
            toggle_stutter_reduction(game_dir, value == "1")?;
            continue;
        }
        let pattern = Regex::new(&format!(r"^\s*{}\s*=", regex::escape(key)))?;
        let replacement = format!("{} = \"{}\" // {}", key, value, comment);
        changes.push((pattern, replacement));
        if key == "BackbufferCount" && value == "3" {
            let vsync_pattern = Regex::new(r"^\s*Vsync\s*=")?;
            changes.push((
                vsync_pattern,
                "Vsync = \"1\" // Enabled with triple-buffered V-sync".to_string(),
            ));
        }
    }

    update_config_values(
        game_dir,
        &changes,
        &format!("Applied preset '{}'", preset_name),
    )
}

#[derive(Debug, Default, Clone)]
pub struct EssentialStatus {
    pub max_fps: i32,
    pub fov: i32,
    pub display_mode: i32,
    pub resolution: String,
    pub refresh_rate: f32,
    pub vsync: bool,
    pub draw_fps: bool,
    pub all_settings: bool,
    pub smooth: bool,
    pub vram: bool,
    pub vram_value: f32,
    pub latency: i32,
    pub reduce_cpu: bool,
    pub skip_intro: bool,
}

pub fn check_essential_status(game_dir: &Path) -> Result<EssentialStatus> {
    let config_path = config_ini(game_dir);
    if !config_path.exists() {
        return Ok(EssentialStatus::default());
    }

    let content = fs::read_to_string(&config_path)?;
    let mut status = EssentialStatus::default();

    status.max_fps = capture_int(&content, r#"MaxFPS\s*=\s*"([^"]+)""#, 165);
    status.fov = capture_int(&content, r#"FOV\s*=\s*"([^"]+)""#, 80);
    status.display_mode = capture_int(&content, r#"FullScreenMode\s*=\s*"([^"]+)""#, 1);
    status.resolution = capture_string(
        &content,
        r#"WindowSize\s*=\s*"([^"]+)""#,
        "2560x1440".into(),
    );
    status.refresh_rate = capture_float(&content, r#"RefreshRate\s*=\s*"([^"]+)""#, 165.0);
    status.vsync = capture_bool(&content, r#"Vsync\s*=\s*"([^"]+)""#, true);
    status.draw_fps = capture_bool(&content, r#"DrawFPS\s*=\s*"([^"]+)""#, false);
    status.all_settings = capture_bool(
        &content,
        r#"RestrictGraphicsOptions\s*=\s*"([^"]+)""#,
        false,
    );
    status.smooth = capture_bool(&content, r#"SmoothFramerate\s*=\s*"([^"]+)""#, false);
    let vram_enabled = capture_string(&content, r#"VideoMemory\s*=\s*"([^"]+)""#, "1".into());
    let stream_min = capture_string(&content, r#"StreamMinResident\s*=\s*"([^"]+)""#, "0".into());
    status.vram = !(vram_enabled == "1" && stream_min == "0");
    status.vram_value = vram_enabled.parse().unwrap_or(0.75);
    status.latency = capture_int(&content, r#"MaxFrameLatency\s*=\s*"([^"]+)""#, 1);
    status.reduce_cpu = capture_bool(&content, r#"SerializeRender\s*=\s*"([^"]+)""#, false);
    let intro_bak = game_dir
        .join("video")
        .join("BO3_Global_Logo_LogoSequence.mkv.bak");
    status.skip_intro = intro_bak.exists();

    Ok(status)
}

fn config_ini(game_dir: &Path) -> PathBuf {
    game_dir.join("players").join("config.ini")
}

fn capture_int(content: &str, pattern: &str, default: i32) -> i32 {
    Regex::new(pattern)
        .ok()
        .and_then(|re| re.captures(content))
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(default)
}

fn capture_float(content: &str, pattern: &str, default: f32) -> f32 {
    Regex::new(pattern)
        .ok()
        .and_then(|re| re.captures(content))
        .and_then(|caps| caps.get(1))
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(default)
}

fn capture_string(content: &str, pattern: &str, default: String) -> String {
    Regex::new(pattern)
        .ok()
        .and_then(|re| re.captures(content))
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or(default)
}

fn capture_bool(content: &str, pattern: &str, default: bool) -> bool {
    Regex::new(pattern)
        .ok()
        .and_then(|re| re.captures(content))
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str() == "1")
        .unwrap_or(default)
}
