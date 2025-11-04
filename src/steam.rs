use std::path::{Path, PathBuf};

use anyhow::Result;
use once_cell::sync::Lazy;

#[derive(Debug, Clone)]
pub struct SteamPaths {
    pub userdata: PathBuf,
    pub steam_exe: PathBuf,
}

pub fn detect() -> Option<SteamPaths> {
    static PATHS: Lazy<Option<SteamPaths>> = Lazy::new(|| detect_inner().ok());
    PATHS.clone()
}

fn detect_inner() -> Result<SteamPaths> {
    let system = std::env::consts::OS;
    match system {
        "windows" => detect_windows(),
        "linux" => detect_linux(),
        "macos" => detect_macos(),
        _ => anyhow::bail!("Unsupported platform: {}", system),
    }
}

#[cfg(target_os = "windows")]
fn detect_windows() -> Result<SteamPaths> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_registry::RegKey;

    let mut candidates: Vec<PathBuf> = Vec::new();

    for (hive, key, value) in [
        (
            windows_registry::HKEY_CURRENT_USER,
            "Software\\Valve\\Steam",
            "SteamPath",
        ),
        (
            windows_registry::HKEY_LOCAL_MACHINE,
            "SOFTWARE\\WOW6432Node\\Valve\\Steam",
            "InstallPath",
        ),
        (
            windows_registry::HKEY_LOCAL_MACHINE,
            "SOFTWARE\\Valve\\Steam",
            "InstallPath",
        ),
    ] {
        if let Ok(reg_key) = RegKey::predef(hive).open(key) {
            if let Ok(value) = reg_key.get_string(value) {
                candidates.push(PathBuf::from(value));
            }
        }
    }

    if let Some(program_files) = std::env::var_os("PROGRAMFILES(X86)") {
        candidates.push(PathBuf::from(program_files).join("Steam"));
    }
    if let Some(program_files) = std::env::var_os("PROGRAMFILES") {
        candidates.push(PathBuf::from(program_files).join("Steam"));
    }
    candidates.push(PathBuf::from(r"C:\\Program Files (x86)\\Steam"));
    candidates.push(PathBuf::from(r"C:\\Program Files\\Steam"));

    for candidate in candidates {
        let exe = candidate.join("steam.exe");
        if exe.exists() {
            return Ok(SteamPaths {
                userdata: candidate.join("userdata"),
                steam_exe: exe,
            });
        }
    }
    anyhow::bail!("Unable to locate Steam installation")
}

#[cfg(not(target_os = "windows"))]
fn detect_windows() -> Result<SteamPaths> {
    anyhow::bail!("windows detection invoked on non-windows target")
}

#[cfg(target_os = "linux")]
fn detect_linux() -> Result<SteamPaths> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Missing home directory"))?;
    Ok(SteamPaths {
        userdata: home.join(".steam/steam/userdata"),
        steam_exe: PathBuf::from("steam"),
    })
}

#[cfg(not(target_os = "linux"))]
fn detect_linux() -> Result<SteamPaths> {
    anyhow::bail!("linux detection invoked on non-linux target")
}

#[cfg(target_os = "macos")]
fn detect_macos() -> Result<SteamPaths> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Missing home directory"))?;
    Ok(SteamPaths {
        userdata: home.join("Library/Application Support/Steam/userdata"),
        steam_exe: PathBuf::from("open"),
    })
}

#[cfg(not(target_os = "macos"))]
fn detect_macos() -> Result<SteamPaths> {
    anyhow::bail!("macos detection invoked on non-macos target")
}

pub fn find_user_id(paths: &SteamPaths) -> Option<String> {
    let entries = std::fs::read_dir(&paths.userdata).ok()?;
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        if let Some(name) = file_name.to_str() {
            if name.chars().all(|c| c.is_ascii_digit()) {
                return Some(name.to_owned());
            }
        }
    }
    None
}

pub fn steam_userdata_path(paths: Option<&SteamPaths>) -> Option<PathBuf> {
    paths.map(|p| p.userdata.clone())
}

pub fn steam_executable(paths: Option<&SteamPaths>) -> Option<PathBuf> {
    paths.map(|p| p.steam_exe.clone())
}
