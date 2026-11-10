//! Pure application logic for PatchOpsIII. No GUI dependencies, blocking std I/O only.
//!
//! * [`Core`] is the context (paths, log, cached settings); wrap it in an `Arc` to share it.
//! * [`ops`] holds the op groups. Each op is a typed Rust function (called directly by the GUI)
//!   plus a thin JSON layer ([`ops::dispatch`], used by the `--rpc-stdio` / bench protocol).
//! * [`State`] is the application state object returned by `status` and most mutating ops.
//!
//! See `crates/patchops-core/README.md` for conventions and the parity-test workflow.

pub mod archive;
pub mod cli;
pub mod backup;
pub mod clock;
pub mod config_ini;
pub mod context;
pub mod elevate;
pub mod enhanced;
pub mod enhanced_net;
pub mod error;
pub mod fsutil;
pub mod game;
pub mod hash;
pub mod log;
pub mod net;
pub mod ops;
pub mod paths;
pub mod presets;
pub mod pyfmt;
pub mod pyjson;
pub mod rpc;
pub mod settings;
pub mod state;
pub mod steam;
pub mod t7_install;

pub use context::Core;
pub use error::{AppError, AppResult, OpError};
pub use state::State;

/// Crate version (fallback app version; the effective one is `Core::version`).
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
