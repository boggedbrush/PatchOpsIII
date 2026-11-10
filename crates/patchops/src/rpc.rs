//! RPC entry points of the GUI binary; the protocol itself lives in `patchops_core::rpc`
//! (contract: `bench/OPS.md` section 1).
//!
//! * `run_headless` (`patchops --rpc-stdio`): no GPUI, no display, no GPU; exits on stdin EOF.
//! * `spawn_in_gui` (env `PATCHOPS_BENCH_RPC=1`): same loop on a background thread inside the
//!   GUI process, sharing the GUI's `Core`.

use std::sync::Arc;

use patchops_core::Core;

/// `patchops --rpc-stdio`: never touches GPUI.
pub fn run_headless() {
    patchops_core::rpc::serve_stdio(&Core::new());
}

/// Run the same loop on a background thread inside the GUI process.
pub fn spawn_in_gui(core: Arc<Core>) {
    std::thread::Builder::new()
        .name("patchops-rpc".into())
        .spawn(move || patchops_core::rpc::serve_stdio(&core))
        .expect("spawn rpc thread");
}
