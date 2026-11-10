//! Group `qol`: `intro-skip`, `all-intros-skip`, `d3dcompiler` plus the `qol` state builder.
//! Fully ported.
//!
//! Legacy reference: `backend/api.py` (`intro_skip`, `all_intros_skip`, `d3dcompiler`,
//! `_restore_backup`, `_restore_intro_videos`, `_qol_status`). Shared helpers for the
//! reset-to-stock port: [`restore_backup`] and [`restore_intro_videos`].

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{state_reply, Args, Group};
use crate::backup;
use crate::context::Core;
use crate::error::{AppError, AppResult, OpError};
use crate::pyfmt::{io_error_text, repr_str};
use crate::state::{State, StateCx};

pub const OPS: &[&str] = &["intro-skip", "all-intros-skip", "d3dcompiler"];

pub const GROUP: Group = Group { name: "qol", ops: OPS, call };

const VIDEO_DIR: &str = "video";
const INTRO_VIDEO: &str = "BO3_Global_Logo_LogoSequence.mkv";
const D3DCOMPILER_DLL: &str = "d3dcompiler_46.dll";

fn call(core: &Core, op: &str, a: &Args) -> Result<Value, OpError> {
    match op {
        "intro-skip" => {
            let enabled = a.bool_req("enabled")?;
            state_reply(set_intro_skip(core, enabled))
        }
        "all-intros-skip" => {
            let enabled = a.bool_req("enabled")?;
            state_reply(set_all_intros_skip(core, enabled))
        }
        "d3dcompiler" => {
            let enabled = a.bool_req("enabled")?;
            state_reply(set_d3dcompiler(core, enabled))
        }
        _ => Err(OpError::UnknownOp(op.to_string())),
    }
}

// ---- helpers --------------------------------------------------------------------------------

fn game_dir(core: &Core) -> AppResult<PathBuf> {
    core.game_dir().map(PathBuf::from).ok_or_else(|| AppError::new("Game directory is not set."))
}

/// Log an unexpected failure (legacy `write_log(f"...: {exc}", "Error")`) and return it as the app error.
fn fail(core: &Core, prefix: &str, msg: String) -> AppError {
    core.log.error(format!("{prefix}: {msg}"));
    AppError::new(msg)
}

/// Python's `OSError` text for `Path.rename`: `[Errno 13] Permission denied: 'a' -> 'b'`.
fn rename_text(e: &std::io::Error, from: &Path, to: &Path) -> String {
    format!("{}: {} -> {}", io_error_text(e, None), repr_str(&from.to_string_lossy()), repr_str(&to.to_string_lossy()))
}

fn rename(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::rename(from, to).map_err(|e| rename_text(&e, from, to))
}

fn remove(path: &Path) -> Result<(), String> {
    std::fs::remove_file(path).map_err(|e| io_error_text(&e, Some(path)))
}

/// Entries of `dir` whose name ends with `suffix`, skipping hidden names (Python `Path.glob("*<suffix>")`).
fn glob_suffix(dir: &Path, suffix: &str) -> Result<Vec<PathBuf>, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| io_error_text(&e, Some(dir)))?;
    let mut out = Vec::new();
    for entry in rd {
        let entry = entry.map_err(|e| io_error_text(&e, Some(dir)))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with('.') && name.ends_with(suffix) {
            out.push(entry.path());
        }
    }
    Ok(out)
}

// ---- shared restore helpers (reset-to-stock) ------------------------------------------------

/// Legacy `_restore_backup`: replace `target` with its backup (`.patchops.bak` wins over `.bak`),
/// logging `success_message` on success. `Ok(false)` when there is no backup. Errors are the
/// Python exception text.
pub fn restore_backup(core: &Core, target: &Path, success_message: &str) -> Result<bool, String> {
    let Some(existing) = backup::existing(target) else { return Ok(false) };
    if target.exists() {
        remove(target)?;
    }
    rename(&existing, target)?;
    core.log.success(success_message);
    Ok(true)
}

/// Legacy `_restore_intro_videos`: rename every `*.mkv.patchops.bak` / `*.mkv.bak` in `<game>/video`
/// back to its original name unless that name exists. Logs the count when anything was restored.
pub fn restore_intro_videos(core: &Core, game_dir: &Path) -> Result<(), String> {
    let video = game_dir.join(VIDEO_DIR);
    if !video.exists() {
        return Ok(());
    }
    let restored = unskip_intro_files(&video)?;
    if restored > 0 {
        core.log.success(format!("Restored {restored} intro video file(s)."));
    }
    Ok(())
}

// ---- intro-skip -----------------------------------------------------------------------------

/// Rename the logo sequence to its backup (`enabled`) or back (`!enabled`). Missing files are not an error.
fn intro_skip_files(core: &Core, video: &Path, enabled: bool) -> Result<(), String> {
    let intro = video.join(INTRO_VIDEO);
    if enabled {
        if intro.exists() {
            rename(&intro, &backup::patchops_path(&intro))?;
        }
        core.log.success("Intro video skipped.");
    } else {
        if let Some(existing) = backup::existing(&intro) {
            rename(&existing, &intro)?;
        }
        core.log.success("Intro video restored.");
    }
    Ok(())
}

/// Legacy `intro_skip`: the intro is skipped even when it is already missing (logs success either way).
pub fn set_intro_skip(core: &Core, enabled: bool) -> AppResult<State> {
    let game = game_dir(core)?;
    intro_skip_files(core, &game.join(VIDEO_DIR), enabled).map_err(|e| fail(core, "Failed to update intro video", e))?;
    Ok(State::build(core))
}

// ---- all-intros-skip ------------------------------------------------------------------------

/// Backup every `*.mkv` without an existing backup; returns how many were renamed.
fn skip_intro_files(video: &Path) -> Result<usize, String> {
    let mut changed = 0;
    for mkv in glob_suffix(video, ".mkv")? {
        let backup_path = backup::patchops_path(&mkv);
        if !backup_path.exists() {
            rename(&mkv, &backup_path)?;
            changed += 1;
        }
    }
    Ok(changed)
}

/// Restore every backup whose original name is free; returns how many were renamed.
fn unskip_intro_files(video: &Path) -> Result<usize, String> {
    let mut backups = glob_suffix(video, &format!(".mkv{}", backup::PATCHOPS_SUFFIX))?;
    backups.extend(glob_suffix(video, &format!(".mkv{}", backup::LEGACY_SUFFIX))?);
    let mut changed = 0;
    for backup_file in backups {
        let name = backup_file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let original = backup::original_name(&name).unwrap_or(name.as_str());
        let target = video.join(original);
        if !target.exists() {
            rename(&backup_file, &target)?;
            changed += 1;
        }
    }
    Ok(changed)
}

pub fn set_all_intros_skip(core: &Core, enabled: bool) -> AppResult<State> {
    let game = game_dir(core)?;
    let video = game.join(VIDEO_DIR);
    if !video.exists() {
        return Err(AppError::new("Video directory was not found."));
    }
    if enabled {
        let changed = skip_intro_files(&video).map_err(|e| fail(core, "Failed to update intro videos", e))?;
        core.log.success(if changed > 0 { "All intro videos skipped." } else { "Intro videos were already skipped." });
    } else {
        let changed = unskip_intro_files(&video).map_err(|e| fail(core, "Failed to update intro videos", e))?;
        core.log.success(if changed > 0 { "Intro videos restored." } else { "No intro video backups were found." });
    }
    Ok(State::build(core))
}

// ---- d3dcompiler ----------------------------------------------------------------------------

pub fn set_d3dcompiler(core: &Core, enabled: bool) -> AppResult<State> {
    let game = game_dir(core)?;
    let dll = game.join(D3DCOMPILER_DLL);
    if enabled {
        if dll.exists() {
            rename(&dll, &backup::patchops_path(&dll)).map_err(|e| fail(core, "Failed to update d3dcompiler_46.dll", e))?;
            core.log.success("Renamed d3dcompiler_46.dll to reduce stuttering.");
        } else if backup::existing(&dll).is_some() {
            core.log.success("Already using latest d3dcompiler.");
        } else {
            return Err(AppError::new("d3dcompiler_46.dll was not found."));
        }
    } else {
        let Some(existing) = backup::existing(&dll) else {
            return Err(AppError::new("No d3dcompiler backup was found."));
        };
        rename(&existing, &dll).map_err(|e| fail(core, "Failed to update d3dcompiler_46.dll", e))?;
        core.log.success("Restored d3dcompiler_46.dll.");
    }
    Ok(State::build(core))
}

// ---- state ----------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QolState {
    pub d3dcompiler: bool,
    pub intro: bool,
    pub all_intros: bool,
}

pub fn qol_state(cx: &StateCx) -> QolState {
    let Some(game) = cx.game_dir.as_deref() else {
        return QolState { d3dcompiler: false, intro: false, all_intros: false };
    };
    let game = std::path::Path::new(game);
    let video = game.join("video");
    let mut all_intros = false;
    if video.exists() {
        if let Ok(rd) = std::fs::read_dir(&video) {
            let (mut mkv, mut bak) = (0usize, 0usize);
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.ends_with(".mkv") {
                    mkv += 1;
                }
                if name.ends_with(&format!(".mkv{}", backup::PATCHOPS_SUFFIX)) || name.ends_with(&format!(".mkv{}", backup::LEGACY_SUFFIX)) {
                    bak += 1;
                }
            }
            all_intros = bak > 0 && mkv == 0;
        }
    }
    QolState {
        d3dcompiler: backup::existing(&game.join("d3dcompiler_46.dll")).is_some(),
        intro: backup::existing(&video.join("BO3_Global_Logo_LogoSequence.mkv")).is_some(),
        all_intros,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    /// Core over a fresh temp HOME with a game dir (`BlackOps3.exe` + `video/`) saved in settings.
    fn setup(name: &str) -> (Core, PathBuf) {
        let root = std::env::temp_dir().join(format!("patchops-qol-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        let game = root.join("game");
        std::fs::create_dir_all(game.join("video")).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(game.join("BlackOps3.exe"), b"exe").unwrap();
        let core = Core::with_paths(Paths::with_app_data(home, root.join("appdata")), "test".into());
        let game_s = game.to_string_lossy().into_owned();
        core.update_settings(|s| s.set_str("game_dir", game_s)).unwrap();
        (core, game)
    }

    fn touch(p: &Path) {
        std::fs::write(p, b"video").unwrap();
    }

    #[test]
    fn intro_skip_round_trip_logs_even_without_file() {
        let (core, game) = setup("intro");
        let video = game.join("video");
        touch(&video.join(INTRO_VIDEO));
        let st = set_intro_skip(&core, true).unwrap();
        assert!(st.qol.intro);
        assert!(!video.join(INTRO_VIDEO).exists());
        assert!(video.join("BO3_Global_Logo_LogoSequence.mkv.patchops.bak").exists());
        // Second call: nothing to rename, still logs success.
        set_intro_skip(&core, true).unwrap();
        let st = set_intro_skip(&core, false).unwrap();
        assert!(!st.qol.intro);
        assert!(video.join(INTRO_VIDEO).exists());
        let msgs: Vec<String> = core.log.recent(10).into_iter().map(|e| e.message).collect();
        assert_eq!(msgs, ["Intro video skipped.", "Intro video skipped.", "Intro video restored."]);
        let _ = std::fs::remove_dir_all(core.paths.home.parent().unwrap());
    }

    #[test]
    fn all_intros_skip_and_restore() {
        let (core, game) = setup("all");
        let video = game.join("video");
        touch(&video.join("a.mkv"));
        touch(&video.join("notes.txt"));
        touch(&video.join("b.mkv"));
        let st = set_all_intros_skip(&core, true).unwrap();
        assert!(st.qol.all_intros);
        set_all_intros_skip(&core, true).unwrap();
        let st = set_all_intros_skip(&core, false).unwrap();
        assert!(!st.qol.all_intros);
        assert!(video.join("a.mkv").exists() && video.join("b.mkv").exists());
        let msgs: Vec<String> = core.log.recent(10).into_iter().map(|e| e.message).collect();
        assert_eq!(msgs, ["All intro videos skipped.", "Intro videos were already skipped.", "Intro videos restored."]);
        let _ = std::fs::remove_dir_all(core.paths.home.parent().unwrap());
    }

    #[test]
    fn d3dcompiler_states_and_errors() {
        let (core, game) = setup("d3d");
        let dll = game.join(D3DCOMPILER_DLL);
        assert_eq!(set_d3dcompiler(&core, true).unwrap_err().message, "d3dcompiler_46.dll was not found.");
        assert_eq!(set_d3dcompiler(&core, false).unwrap_err().message, "No d3dcompiler backup was found.");
        touch(&dll);
        let st = set_d3dcompiler(&core, true).unwrap();
        assert!(st.qol.d3dcompiler && !dll.exists());
        // Already renamed: still ok, backup kept.
        assert!(set_d3dcompiler(&core, true).unwrap().qol.d3dcompiler);
        let st = set_d3dcompiler(&core, false).unwrap();
        assert!(!st.qol.d3dcompiler && dll.exists());
        // reset-to-stock helper: no backup -> Ok(false); with a backup -> restored and logged.
        assert!(!restore_backup(&core, &dll, "unused").unwrap());
        std::fs::rename(&dll, backup::patchops_path(&dll)).unwrap();
        assert!(!dll.exists());
        assert!(restore_backup(&core, &dll, "Restored d3dcompiler_46.dll.").unwrap());
        assert!(dll.exists());
        assert_eq!(core.log.recent(1)[0].message, "Restored d3dcompiler_46.dll.");
        let _ = std::fs::remove_dir_all(core.paths.home.parent().unwrap());
    }

    #[test]
    fn missing_game_dir_is_app_error() {
        let root = std::env::temp_dir().join(format!("patchops-qol-nogame-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("home")).unwrap();
        let core = Core::with_paths(Paths::with_app_data(root.join("home"), root.join("appdata")), "test".into());
        assert_eq!(set_intro_skip(&core, true).unwrap_err().message, "Game directory is not set.");
        let _ = std::fs::remove_dir_all(root);
    }
}
