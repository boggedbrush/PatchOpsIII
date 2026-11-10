//! Hidden command-line commands of the application binary (legacy `backend/api.py::_run_cli_command`).
//!
//! Only one exists: `--install-t7-patch <game_dir>`, the elevated child that the Windows T7 install
//! spawns with a UAC prompt (see [`crate::elevate`]). The shell calls [`run_cli_command`] first
//! thing in `main`:
//!
//! ```ignore
//! let args: Vec<String> = std::env::args().skip(1).collect();
//! if let Some(code) = patchops_core::cli::run_cli_command(&args) {
//!     std::process::exit(code);
//! }
//! ```

use std::path::Path;

use crate::context::Core;
use crate::net::{no_progress, Net};

/// Argument that selects the elevated T7 Patch install.
pub const INSTALL_T7_FLAG: &str = "--install-t7-patch";

/// `Some(exit code)` when `args` (without argv[0]) is a hidden command that was executed,
/// `None` for a normal start. Exit codes as in the legacy code: 0 ok, 1 failed, 2 missing argument.
pub fn run_cli_command(args: &[String]) -> Option<i32> {
    run_cli_command_with(&Core::new(), &Net::production(), args)
}

pub fn run_cli_command_with(core: &Core, net: &Net, args: &[String]) -> Option<i32> {
    if args.first().map(String::as_str) != Some(INSTALL_T7_FLAG) {
        return None;
    }
    let Some(game_dir) = args.get(1) else {
        core.log.error("T7 Patch install failed: game directory was not provided.");
        return Some(2);
    };
    // This process is elevated and `game_dir` comes from the (unelevated) command line of whoever
    // started it: only act on an absolute path that really is a Black Ops III install, so the hidden
    // command cannot be used to write files / add Defender exclusions anywhere else.
    if let Err(msg) = check_elevated_target(Path::new(game_dir)) {
        core.log.error(format!("T7 Patch install failed: {msg}"));
        return Some(1);
    }
    match crate::t7_install::install_t7_patch(core, net, Path::new(game_dir), &mut no_progress) {
        Ok(()) => Some(0),
        Err(e) => {
            core.log.error(format!("T7 Patch install failed: {e}"));
            Some(1)
        }
    }
}

/// What the elevated child accepts as its game directory.
pub fn check_elevated_target(game_dir: &Path) -> Result<(), String> {
    if !game_dir.is_absolute() {
        return Err("game directory must be an absolute path.".into());
    }
    if !crate::game::has_game_executable(game_dir) {
        return Err("the given directory is not a Black Ops III install.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elevated_child_refuses_non_game_directories() {
        assert!(check_elevated_target(Path::new("relative/dir")).is_err());
        let d = std::env::temp_dir().join(format!("patchops-cli-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        assert!(check_elevated_target(&d).unwrap_err().contains("not a Black Ops III install"));
        std::fs::write(d.join("BlackOps3.exe"), b"x").unwrap();
        assert!(check_elevated_target(&d).is_ok());
        let core = Core::with_paths(crate::paths::Paths::with_app_data(d.join("h"), d.join("a")), "t".into());
        // A refused target exits 1 without touching the network (dead endpoint would otherwise log a download error).
        let other = d.join("empty");
        std::fs::create_dir_all(&other).unwrap();
        assert_eq!(run_cli_command_with(&core, &Net::local("http://127.0.0.1:9"), &[INSTALL_T7_FLAG.into(), other.display().to_string()]), Some(1));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn only_the_hidden_flag_is_a_command() {
        assert_eq!(run_cli_command(&[]), None);
        assert_eq!(run_cli_command(&["--rpc-stdio".to_string()]), None);
        assert_eq!(run_cli_command(&["x".to_string(), INSTALL_T7_FLAG.to_string()]), None);
    }
}
