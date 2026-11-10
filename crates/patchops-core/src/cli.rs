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
    match crate::t7_install::install_t7_patch(core, net, Path::new(game_dir), &mut no_progress) {
        Ok(()) => Some(0),
        Err(e) => {
            core.log.error(format!("T7 Patch install failed: {e}"));
            Some(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_hidden_flag_is_a_command() {
        assert_eq!(run_cli_command(&[]), None);
        assert_eq!(run_cli_command(&["--rpc-stdio".to_string()]), None);
        assert_eq!(run_cli_command(&["x".to_string(), INSTALL_T7_FLAG.to_string()]), None);
    }
}
