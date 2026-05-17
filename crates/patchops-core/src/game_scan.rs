use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::{error::Result, file_integrity::sha256_file};

pub const GAME_EXECUTABLE_NAMES: [&str; 2] = ["BlackOpsIII.exe", "BlackOps3.exe"];
const T7_INSTALL_MARKERS: [&str; 2] = ["t7patch.dll", "t7patchloader.dll"];
const T7_CONFIG: &str = "t7patch.conf";
const DXVK_MARKERS: [&str; 2] = ["dxgi.dll", "d3d11.dll"];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusOutput {
    pub ok: bool,
    pub game_detected: bool,
    pub config_exists: bool,
    pub config_readonly: bool,
    pub executable: String,
    pub executable_name: String,
    pub executable_hash: String,
    pub t7_installed: bool,
    pub t7_config_exists: bool,
    pub dxvk_installed: bool,
}

pub fn find_executable(game_dir: &Path) -> Option<PathBuf> {
    GAME_EXECUTABLE_NAMES
        .iter()
        .map(|name| game_dir.join(name))
        .find(|candidate| candidate.is_file())
}

fn config_readonly(path: &Path) -> bool {
    path.metadata()
        .map(|metadata| metadata.permissions().readonly())
        .unwrap_or(false)
}

pub fn status(game_dir: &Path) -> Result<StatusOutput> {
    let executable = find_executable(game_dir);
    let config = game_dir.join("players").join("config.ini");
    let executable_hash = match executable.as_deref() {
        Some(path) => sha256_file(path)?,
        None => String::new(),
    };

    Ok(StatusOutput {
        ok: true,
        game_detected: executable.is_some(),
        config_exists: config.is_file(),
        config_readonly: config.is_file() && config_readonly(&config),
        executable: executable
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
        executable_name: executable
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        executable_hash,
        t7_installed: T7_INSTALL_MARKERS
            .iter()
            .any(|marker| game_dir.join(marker).is_file()),
        t7_config_exists: game_dir.join(T7_CONFIG).is_file(),
        dxvk_installed: DXVK_MARKERS
            .iter()
            .all(|marker| game_dir.join(marker).is_file()),
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{find_executable, status};

    #[test]
    fn detects_supported_executable_names_in_order() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("BlackOps3.exe"), b"old").unwrap();
        fs::write(dir.path().join("BlackOpsIII.exe"), b"new").unwrap();

        let executable = find_executable(dir.path()).unwrap();
        assert_eq!(executable.file_name().unwrap(), "BlackOpsIII.exe");
    }

    #[test]
    fn missing_config_is_structured_status() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("BlackOps3.exe"), b"exe").unwrap();

        let output = status(dir.path()).unwrap();
        assert!(output.game_detected);
        assert!(!output.config_exists);
        assert!(!output.config_readonly);
    }

    #[test]
    fn missing_game_dir_is_structured_status() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("missing-game");

        let output = status(&missing).unwrap();
        assert!(output.ok);
        assert!(!output.game_detected);
        assert!(!output.config_exists);
        assert!(!output.config_readonly);
        assert!(output.executable.is_empty());
        assert!(output.executable_name.is_empty());
        assert!(output.executable_hash.is_empty());
        assert!(!output.t7_installed);
        assert!(!output.t7_config_exists);
        assert!(!output.dxvk_installed);
    }

    #[test]
    fn detects_t7_install_markers_without_treating_config_as_installed() {
        let dir = tempdir().unwrap();

        fs::write(dir.path().join("t7patch.conf"), b"playername=PatchOps").unwrap();
        let config_only = status(dir.path()).unwrap();
        assert!(!config_only.t7_installed);
        assert!(config_only.t7_config_exists);

        fs::write(dir.path().join("t7patch.dll"), b"dll").unwrap();
        let with_dll = status(dir.path()).unwrap();
        assert!(with_dll.t7_installed);
        assert!(with_dll.t7_config_exists);
    }

    #[test]
    fn dxvk_requires_all_python_recognized_files() {
        let dir = tempdir().unwrap();

        fs::write(dir.path().join("dxgi.dll"), b"dxgi").unwrap();
        let partial = status(dir.path()).unwrap();
        assert!(!partial.dxvk_installed);

        fs::write(dir.path().join("d3d11.dll"), b"d3d11").unwrap();
        let complete = status(dir.path()).unwrap();
        assert!(complete.dxvk_installed);
    }
}
