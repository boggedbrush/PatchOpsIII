use std::{collections::BTreeSet, env, fs, path::PathBuf};

use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamScanOutput {
    pub ok: bool,
    pub steam_roots: Vec<String>,
    pub library_paths: Vec<String>,
    pub game_dirs: Vec<String>,
}

pub fn scan_steam() -> SteamScanOutput {
    let roots = steam_roots();
    let mut libraries = BTreeSet::new();
    let mut game_dirs = BTreeSet::new();

    for root in &roots {
        if root.is_dir() {
            libraries.insert(root.to_path_buf());
        }
        let vdf = root.join("steamapps").join("libraryfolders.vdf");
        for library in parse_libraryfolders(&fs::read_to_string(vdf).unwrap_or_default()) {
            libraries.insert(library);
        }
    }

    for library in &libraries {
        let game_dir = library
            .join("steamapps")
            .join("common")
            .join("Call of Duty Black Ops III");
        if game_dir.is_dir() {
            game_dirs.insert(game_dir);
        }
    }

    SteamScanOutput {
        ok: true,
        steam_roots: roots.into_iter().map(display_path).collect(),
        library_paths: libraries.into_iter().map(display_path).collect(),
        game_dirs: game_dirs.into_iter().map(display_path).collect(),
    }
}

fn steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if cfg!(target_os = "windows") {
        if let Ok(program_files_x86) = env::var("PROGRAMFILES(X86)") {
            roots.push(PathBuf::from(program_files_x86).join("Steam"));
        }
        if let Ok(program_files) = env::var("PROGRAMFILES") {
            roots.push(PathBuf::from(program_files).join("Steam"));
        }
    } else if cfg!(target_os = "macos") {
        if let Some(home) = home_dir() {
            roots.push(
                home.join("Library")
                    .join("Application Support")
                    .join("Steam"),
            );
        }
    } else if let Some(home) = home_dir() {
        roots.push(home.join(".steam").join("steam"));
        roots.push(home.join(".local").join("share").join("Steam"));
    }

    dedupe_paths(roots)
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(PathBuf::from))
}

pub fn parse_libraryfolders(content: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('"') {
            continue;
        }
        let parts: Vec<&str> = trimmed.split('"').collect();
        if parts.len() < 4 || parts[1] != "path" {
            continue;
        }
        let path = parts[3].replace("\\\\", "\\");
        if !path.trim().is_empty() {
            paths.push(PathBuf::from(path));
        }
    }
    dedupe_paths(paths)
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    let mut output = Vec::new();
    for path in paths {
        let key = path.to_string_lossy().to_ascii_lowercase();
        if seen.insert(key) {
            output.push(path);
        }
    }
    output
}

fn display_path(path: PathBuf) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::parse_libraryfolders;

    #[test]
    fn parses_steam_libraryfolders_paths() {
        let paths = parse_libraryfolders(
            r#"
            "libraryfolders"
            {
              "0"
              {
                "path" "C:\\Program Files (x86)\\Steam"
              }
              "1"
              {
                "path" "D:\\SteamLibrary"
              }
            }
            "#,
        );

        assert_eq!(paths.len(), 2);
        assert!(paths[0].to_string_lossy().contains("Steam"));
        assert!(paths[1].to_string_lossy().contains("SteamLibrary"));
    }
}
