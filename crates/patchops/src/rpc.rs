//! RPC entry points of the GUI binary; the protocol itself lives in `patchops_core::rpc`
//! (contract: `bench/OPS.md` section 1).
//!
//! * `run_headless` (`patchops --rpc-stdio`): no GPUI, no display, no GPU; exits on stdin EOF.
//! * `spawn_in_gui` (env `PATCHOPS_BENCH_RPC=1`): same protocol on a background thread inside the
//!   GUI process, sharing the GUI's `Core`. After every op that can change state it sends
//!   `UiEvent::Refresh` so the visible UI re-reads the state (the core loop has no hook for
//!   that, hence the local copy of the read/answer loop).

use std::io::{BufRead, Write};
use std::sync::Arc;

use patchops_core::Core;

use crate::state::UiEvent;

/// `patchops --rpc-stdio`: never touches GPUI.
pub fn run_headless() {
    patchops_core::rpc::serve_stdio(&Core::new());
}

/// Ops that never mutate anything (no UI refresh needed after them).
const READ_ONLY: &[&str] = &["health", "status", "browse", "logs/payload", "exe-swap/compatible-depot"];

fn mutates(line: &str) -> bool {
    match serde_json::from_str::<serde_json::Value>(line) {
        Ok(v) => v.get("op").and_then(|o| o.as_str()).is_some_and(|op| !READ_ONLY.contains(&op)),
        Err(_) => false,
    }
}

/// Run the same loop on a background thread inside the GUI process.
pub fn spawn_in_gui(core: Arc<Core>, ui: async_channel::Sender<UiEvent>) {
    std::thread::Builder::new()
        .name("patchops-rpc".into())
        .spawn(move || {
            let stdin = std::io::stdin();
            let mut out = std::io::stdout().lock();
            let mut input = stdin.lock();
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match input.read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let line = String::from_utf8_lossy(&buf);
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let resp = patchops_core::rpc::handle_line(&core, line);
                if writeln!(out, "{resp}").and_then(|_| out.flush()).is_err() {
                    break;
                }
                if mutates(line) {
                    let _ = ui.try_send(UiEvent::Refresh);
                }
            }
        })
        .expect("spawn rpc thread");
}
