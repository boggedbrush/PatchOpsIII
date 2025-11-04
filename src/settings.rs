use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};

const SETTINGS_FILE: &str = "PatchOpsIII_settings.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppSettings {
    pub game_directory: Option<PathBuf>,
}

impl AppSettings {
    pub fn load(app_dir: &Path) -> Result<Self> {
        let path = app_dir.join(SETTINGS_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = fs::read(&path)
            .with_context(|| format!("Failed to read settings from {}", path.display()))?;
        let mut settings: Self =
            serde_json::from_slice(&data).with_context(|| "Failed to parse settings JSON")?;
        if let Some(dir) = settings.game_directory.clone() {
            if !dir.exists() {
                settings.game_directory = None;
            }
        }
        Ok(settings)
    }

    pub fn save(&self, app_dir: &Path) -> Result<()> {
        fs::create_dir_all(app_dir)
            .with_context(|| format!("Failed to create app directory {}", app_dir.display()))?;
        let path = app_dir.join(SETTINGS_FILE);
        let data =
            serde_json::to_vec_pretty(self).with_context(|| "Failed to serialize settings")?;
        fs::write(&path, data)
            .with_context(|| format!("Failed to write settings to {}", path.display()))
    }
}

pub fn default_application_dir() -> Result<PathBuf> {
    if let Some(base) = BaseDirs::new() {
        let dir = base.data_dir().join("PatchOpsIII");
        fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
        Ok(dir)
    } else {
        anyhow::bail!("Unable to determine application data directory");
    }
}
