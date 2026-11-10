//! Filesystem locations, resolved exactly like the legacy `utils.get_app_data_dir()` & co.
//! (`bench/OPS.md` section 2).

use std::path::PathBuf;

pub const APP_DIR_NAME: &str = "PatchOpsIII";
pub const SETTINGS_FILE: &str = "electron-settings.json";
pub const LOG_FILE: &str = "PatchOpsIII.log";
pub const MOD_FILES_DIR_NAME: &str = "BO3 Mod Files";
pub const BACKUPS_DIR_NAME: &str = "backups";
pub const PRESETS_FILE: &str = "presets.json";
/// Write-once "first seen" copies of files the app is about to rewrite (see
/// `steam::launch_options::preserve_first_seen`). Lives in the state dir, not the app data dir.
pub const ORIGINALS_DIR_NAME: &str = "originals";

/// All fixed locations of one app instance. Plain data; cheap to clone.
#[derive(Debug, Clone)]
pub struct Paths {
    /// User home (`HOME` / `USERPROFILE`).
    pub home: PathBuf,
    /// `$XDG_DATA_HOME/PatchOpsIII` (Linux), `%APPDATA%\PatchOpsIII`, `~/Library/Application Support/PatchOpsIII`.
    pub app_data: PathBuf,
    /// `<app_data>/electron-settings.json`
    pub settings: PathBuf,
    /// `<app_data>/PatchOpsIII.log`
    pub log: PathBuf,
    /// `<app_data>/BO3 Mod Files`
    pub mod_files: PathBuf,
    /// `<app_data>/backups`
    pub backups: PathBuf,
    /// `PATCHOPS_PRESETS`: optional on-disk presets file replacing the embedded `presets.json`.
    pub presets_override: Option<PathBuf>,
    /// Write-once first-seen copies of Steam files (`<prefix>_<8hex>_original.vdf`, the hex being a
    /// stable hash of the source path; see `steam::launch_options::first_seen_name`). Outside
    /// `app_data` on Linux/Windows so it is never part of the app data tree:
    /// - Linux/other unix: `$XDG_STATE_HOME/PatchOpsIII/originals` (else `~/.local/state/...`)
    /// - Windows: `%LOCALAPPDATA%\PatchOpsIII\originals` (else `<home>\AppData\Local\...`)
    /// - macOS: `<app_data>/originals`
    /// - [`Paths::with_app_data`] (tests, portable): `<app_data>/originals`, so every temp sandbox
    ///   stays isolated and never points into a real user directory.
    pub originals: PathBuf,
}

/// Python's `os.path.expanduser("~")`.
pub fn home_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(p) = std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()) {
            return PathBuf::from(p);
        }
    }
    if let Some(p) = std::env::var_os("HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(p);
    }
    #[cfg(unix)]
    {
        // Python falls back to the passwd entry.
        if let Some(h) = passwd_home() {
            return h;
        }
    }
    PathBuf::from("~")
}

#[cfg(unix)]
fn passwd_home() -> Option<PathBuf> {
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStringExt;
    // SAFETY: getpwuid returns a pointer to static storage or null; we copy immediately.
    unsafe {
        let pw = libc::getpwuid(libc::getuid());
        if pw.is_null() || (*pw).pw_dir.is_null() {
            return None;
        }
        let bytes = CStr::from_ptr((*pw).pw_dir).to_bytes().to_vec();
        Some(PathBuf::from(std::ffi::OsString::from_vec(bytes)))
    }
}

/// `platform.system()`.
pub fn system_name() -> &'static str {
    if cfg!(windows) {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "Darwin"
    } else {
        "Linux"
    }
}

impl Paths {
    /// Resolve from the process environment.
    pub fn from_env() -> Paths {
        let home = home_dir();
        let base = if cfg!(windows) {
            std::env::var_os("APPDATA")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join("AppData").join("Roaming"))
        } else if cfg!(target_os = "macos") {
            home.join("Library").join("Application Support")
        } else {
            std::env::var_os("XDG_DATA_HOME")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".local").join("share"))
        };
        let state_env = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
        let app_data = base.join(APP_DIR_NAME);
        let originals = originals_dir_for(
            cfg!(windows),
            cfg!(target_os = "macos"),
            state_env("XDG_STATE_HOME").filter(|p| p.is_absolute()),
            state_env("LOCALAPPDATA").filter(|p| p.is_absolute()),
            &home,
            &app_data,
        );
        let mut p = Paths::with_app_data(home, app_data);
        p.originals = originals;
        p.presets_override = std::env::var_os("PATCHOPS_PRESETS").filter(|v| !v.is_empty()).map(PathBuf::from);
        p
    }

    /// Build from explicit directories (tests, portable mode).
    pub fn with_app_data(home: PathBuf, app_data: PathBuf) -> Paths {
        Paths {
            settings: app_data.join(SETTINGS_FILE),
            log: app_data.join(LOG_FILE),
            mod_files: app_data.join(MOD_FILES_DIR_NAME),
            backups: app_data.join(BACKUPS_DIR_NAME),
            originals: app_data.join(ORIGINALS_DIR_NAME),
            app_data,
            home,
            presets_override: None,
        }
    }
}

/// Pure resolution of [`Paths::originals`] from the platform flags and the (already filtered)
/// environment values, so it can be tested without touching the process environment.
fn originals_dir_for(
    windows: bool,
    macos: bool,
    xdg_state: Option<PathBuf>,
    local_app_data: Option<PathBuf>,
    home: &std::path::Path,
    app_data: &std::path::Path,
) -> PathBuf {
    if macos {
        return app_data.join(ORIGINALS_DIR_NAME);
    }
    let state = if windows {
        local_app_data.unwrap_or_else(|| home.join("AppData").join("Local"))
    } else {
        xdg_state.unwrap_or_else(|| home.join(".local").join("state"))
    };
    state.join(APP_DIR_NAME).join(ORIGINALS_DIR_NAME)
}

/// App version: `PATCHOPSIII_VERSION_OVERRIDE`, `PATCHOPSIII_VERSION`, else the crate version.
pub fn app_version() -> String {
    for name in ["PATCHOPSIII_VERSION_OVERRIDE", "PATCHOPSIII_VERSION"] {
        if let Ok(v) = std::env::var(name) {
            let v = v.trim();
            if !v.is_empty() {
                return v.to_string();
            }
        }
    }
    env!("CARGO_PKG_VERSION").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn originals_follow_xdg_state_then_home_fallback() {
        let home = PathBuf::from("/h/user");
        let app = PathBuf::from("/h/user/.local/share/PatchOpsIII");
        assert_eq!(
            originals_dir_for(false, false, Some("/s".into()), None, &home, &app),
            PathBuf::from("/s/PatchOpsIII/originals")
        );
        assert_eq!(
            originals_dir_for(false, false, None, None, &home, &app),
            PathBuf::from("/h/user/.local/state/PatchOpsIII/originals")
        );
        // LOCALAPPDATA is ignored off Windows.
        assert_eq!(
            originals_dir_for(false, false, None, Some("/la".into()), &home, &app),
            PathBuf::from("/h/user/.local/state/PatchOpsIII/originals")
        );
    }

    #[test]
    fn originals_on_windows_and_macos() {
        let home = PathBuf::from("C:/Users/u");
        let app = PathBuf::from("C:/Users/u/AppData/Roaming/PatchOpsIII");
        assert_eq!(
            originals_dir_for(true, false, None, Some("D:/local".into()), &home, &app),
            PathBuf::from("D:/local/PatchOpsIII/originals")
        );
        assert_eq!(
            originals_dir_for(true, false, None, None, &home, &app),
            home.join("AppData").join("Local").join("PatchOpsIII").join("originals")
        );
        assert_eq!(
            originals_dir_for(false, true, Some("/ignored".into()), None, &home, &app),
            app.join("originals")
        );
    }

    #[test]
    fn with_app_data_keeps_originals_inside_the_given_tree() {
        let p = Paths::with_app_data(PathBuf::from("/tmp/x/home"), PathBuf::from("/tmp/x/appdata"));
        assert_eq!(p.originals, PathBuf::from("/tmp/x/appdata/originals"));
        assert!(p.originals.starts_with(&p.app_data));
    }
}
