//! Filesystem locations, resolved exactly like the legacy `utils.get_app_data_dir()` & co.
//! (`bench/OPS.md` section 2).

use std::path::PathBuf;

pub const APP_DIR_NAME: &str = "PatchOpsIII";
pub const SETTINGS_FILE: &str = "electron-settings.json";
pub const LOG_FILE: &str = "PatchOpsIII.log";
pub const MOD_FILES_DIR_NAME: &str = "BO3 Mod Files";
pub const BACKUPS_DIR_NAME: &str = "backups";
pub const PRESETS_FILE: &str = "presets.json";

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
        let mut p = Paths::with_app_data(home, base.join(APP_DIR_NAME));
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
            app_data,
            home,
            presets_override: None,
        }
    }
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
