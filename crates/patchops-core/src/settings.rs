//! `electron-settings.json`: a flat JSON object shared with the legacy Electron app.
//!
//! Keys in use: `game_dir`, `release_channel`, `enhanced_dump_source`, `enhanced_exe_hashes`
//! (`{game_dir: [sha256, ...]}`; legacy single-value `enhanced_exe_hash` is still read).
//! Unknown keys are preserved, key order is preserved, and the file is written exactly as
//! Python's `json.dumps(settings, indent=2)` (see [`crate::pyjson`]).

use std::path::Path;

use serde_json::{Map, Value};

#[derive(Debug, Clone, Default)]
pub struct Settings(pub Map<String, Value>);

impl Settings {
    /// Read the file; any error or non-object content yields empty settings (legacy behaviour).
    pub fn load(path: &Path) -> Settings {
        let Ok(bytes) = std::fs::read(path) else { return Settings::default() };
        match serde_json::from_slice::<Value>(&bytes) {
            Ok(Value::Object(m)) => Settings(m),
            _ => Settings::default(),
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, crate::pyjson::dumps_indent2(&Value::Object(self.0.clone())))
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(Value::as_str)
    }

    pub fn set_str(&mut self, key: &str, value: impl Into<String>) {
        self.0.insert(key.to_string(), Value::String(value.into()));
    }

    pub fn game_dir(&self) -> Option<&str> {
        self.get_str("game_dir").filter(|s| !s.is_empty())
    }

    /// `stable` (default) or `beta`.
    pub fn release_channel(&self) -> &'static str {
        match self.get_str("release_channel").map(|s| s.trim().to_lowercase()).as_deref() {
            Some("beta") => "beta",
            _ => "stable",
        }
    }

    pub fn enhanced_dump_source(&self) -> &str {
        self.get_str("enhanced_dump_source").unwrap_or("")
    }
}
