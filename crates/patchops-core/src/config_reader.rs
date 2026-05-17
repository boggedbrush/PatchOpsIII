use std::{collections::BTreeMap, fs, path::Path};

use serde::Serialize;

use crate::error::Result;

pub const CONFIG_KEYS: [&str; 14] = [
    "MaxFPS",
    "FOV",
    "FullScreenMode",
    "WindowSize",
    "RefreshRate",
    "ResolutionPercent",
    "Vsync",
    "DrawFPS",
    "SmoothFramerate",
    "RestrictGraphicsOptions",
    "SerializeRender",
    "MaxFrameLatency",
    "VideoMemory",
    "StreamMinResident",
];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigOutput {
    pub ok: bool,
    pub config_exists: bool,
    pub path: String,
    pub values: BTreeMap<String, String>,
}

pub fn parse_config(content: &str) -> BTreeMap<String, String> {
    let wanted: std::collections::BTreeSet<&str> = CONFIG_KEYS.into_iter().collect();
    let mut values = BTreeMap::new();

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with("//") || !line.contains('=') {
            continue;
        }
        let (key, raw_value) = match line.split_once('=') {
            Some(parts) => (parts.0.trim(), parts.1.trim()),
            None => continue,
        };
        if !wanted.contains(key) {
            continue;
        }

        let without_comment = raw_value.split("//").next().unwrap_or(raw_value).trim();
        let value = without_comment
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .unwrap_or(without_comment)
            .trim()
            .to_string();
        values.insert(key.to_string(), value);
    }

    values
}

pub fn read_config(game_dir: &Path) -> Result<ConfigOutput> {
    let path = game_dir.join("players").join("config.ini");
    if !path.is_file() {
        return Ok(ConfigOutput {
            ok: true,
            config_exists: false,
            path: path.to_string_lossy().into_owned(),
            values: BTreeMap::new(),
        });
    }

    let content = fs::read_to_string(&path)?;
    Ok(ConfigOutput {
        ok: true,
        config_exists: true,
        path: path.to_string_lossy().into_owned(),
        values: parse_config(&content),
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{parse_config, read_config};

    #[test]
    fn parses_config_values_and_ignores_comments() {
        let values = parse_config(
            r#"
            // ignored
            MaxFPS = "165" // Maximum FPS cap
            WindowSize = "2560x1440"
            VideoMemory = "0.85"
            Unknown = "ignored"
            "#,
        );

        assert_eq!(values.get("MaxFPS").unwrap(), "165");
        assert_eq!(values.get("WindowSize").unwrap(), "2560x1440");
        assert_eq!(values.get("VideoMemory").unwrap(), "0.85");
        assert!(!values.contains_key("Unknown"));
    }

    #[test]
    fn parses_config_edge_cases() {
        let values = parse_config(
            r#"
            MaxFPS=240
            FOV = "95"
            FullScreenMode = "0" // windowed
            DrawFPS = "0" // comment with = sign
            UnknownKey = "ignored"
            SmoothFramerate = "1"
            SmoothFramerate = "0"
            malformed line
            "#,
        );

        assert_eq!(values.get("MaxFPS").unwrap(), "240");
        assert_eq!(values.get("FOV").unwrap(), "95");
        assert_eq!(values.get("FullScreenMode").unwrap(), "0");
        assert_eq!(values.get("DrawFPS").unwrap(), "0");
        assert_eq!(values.get("SmoothFramerate").unwrap(), "0");
        assert!(!values.contains_key("UnknownKey"));
    }

    #[test]
    fn parses_all_requested_config_keys() {
        let config = super::CONFIG_KEYS
            .iter()
            .enumerate()
            .map(|(index, key)| format!(r#"{key} = "{index}""#))
            .collect::<Vec<_>>()
            .join("\n");
        let values = parse_config(&config);

        for (index, key) in super::CONFIG_KEYS.iter().enumerate() {
            assert_eq!(values.get(*key).unwrap(), &index.to_string());
        }
    }

    #[test]
    fn missing_config_file_returns_empty_values() {
        let dir = tempdir().unwrap();
        let output = read_config(dir.path()).unwrap();

        assert!(output.ok);
        assert!(!output.config_exists);
        assert!(output.values.is_empty());
    }

    #[test]
    fn reads_existing_config_file() {
        let dir = tempdir().unwrap();
        let players = dir.path().join("players");
        fs::create_dir_all(&players).unwrap();
        fs::write(players.join("config.ini"), "FOV = \"90\"\nDrawFPS = \"1\"").unwrap();

        let output = read_config(dir.path()).unwrap();
        assert!(output.config_exists);
        assert_eq!(output.values.get("FOV").unwrap(), "90");
        assert_eq!(output.values.get("DrawFPS").unwrap(), "1");
    }
}
