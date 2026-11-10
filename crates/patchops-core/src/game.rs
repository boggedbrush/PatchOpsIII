//! Game executable names and install-directory detection (legacy `api._find_game_directory`).

use std::path::{Path, PathBuf};

use crate::context::Core;
use crate::pyfmt::path_str;

/// Same order as the legacy `GAME_EXECUTABLE_NAMES`.
pub const GAME_EXECUTABLE_NAMES: [&str; 2] = ["BlackOpsIII.exe", "BlackOps3.exe"];
pub const GAME_DIR_NAME: &str = "Call of Duty Black Ops III";

pub fn has_game_executable(dir: &Path) -> bool {
    !dir.as_os_str().is_empty() && dir.is_dir() && GAME_EXECUTABLE_NAMES.iter().any(|n| dir.join(n).exists())
}

/// First existing executable in `game_dir` (`BlackOpsIII.exe` wins over `BlackOps3.exe`).
pub fn find_executable(game_dir: &Path) -> Option<PathBuf> {
    GAME_EXECUTABLE_NAMES.iter().map(|n| game_dir.join(n)).find(|p| p.exists())
}

/// `<game>/players/config.ini`
pub fn config_path(game_dir: &Path) -> PathBuf {
    game_dir.join("players").join("config.ini")
}

/// Detection order: settings `game_dir` (if it still has an exe), `<library>/steamapps/common/<game>` for each
/// Steam library, then platform default install locations.
pub fn find_game_dir(core: &Core) -> Option<String> {
    let settings = core.settings();
    if let Some(saved) = settings.game_dir() {
        if has_game_executable(Path::new(saved)) {
            return Some(path_str(saved));
        }
    }
    for lib in crate::steam::library_paths(core) {
        let cand = Path::new(&lib).join("steamapps").join("common").join(GAME_DIR_NAME);
        if has_game_executable(&cand) {
            return Some(cand.to_string_lossy().into_owned());
        }
    }
    let home = &core.paths.home;
    let mut cands: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        let pf86 = std::env::var_os("PROGRAMFILES(X86)").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"));
        let pf = std::env::var_os("PROGRAMFILES").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
        cands.push(pf86.join("Steam/steamapps/common").join(GAME_DIR_NAME));
        cands.push(pf.join("Steam/steamapps/common").join(GAME_DIR_NAME));
    } else if cfg!(target_os = "macos") {
        cands.push(home.join("Library/Application Support/Steam/steamapps/common").join(GAME_DIR_NAME));
    } else {
        cands.push(home.join(".steam/steam/steamapps/common").join(GAME_DIR_NAME));
        cands.push(home.join(".local/share/Steam/steamapps/common").join(GAME_DIR_NAME));
    }
    cands.into_iter().find(|c| has_game_executable(c)).map(|c| c.to_string_lossy().into_owned())
}

pub const EXE_VARIANT_FILE: &str = ".patchops_exe_variant.json";

/// Legacy `read_exe_variant`: `{"variant": "<str>"}` marker written next to the executable.
pub fn read_exe_variant(game_dir: &Path) -> Option<String> {
    let bytes = std::fs::read(game_dir.join(EXE_VARIANT_FILE)).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    v.get("variant")?.as_str().map(str::to_string)
}

/// Legacy `write_exe_variant` (errors are swallowed like the original).
pub fn write_exe_variant(game_dir: &Path, variant: &str) {
    let v = serde_json::json!({ "variant": variant });
    let _ = std::fs::create_dir_all(game_dir);
    let _ = std::fs::write(game_dir.join(EXE_VARIANT_FILE), crate::pyjson::dumps_indent2(&v));
}
