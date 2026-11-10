#![allow(dead_code)] // foundation crate: some APIs are not used by the demo yet

mod app;
mod rpc;
mod state;
mod theme;
mod views;
mod widgets;

use std::sync::Arc;

use state::SharedState;

/// Process start, for `PATCHOPS_TRACE_STARTUP=1` (prints time-to-first-render to stderr).
pub static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

fn main() {
    START.get_or_init(std::time::Instant::now);
    #[cfg(feature = "debug-log")]
    env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version") {
        println!("patchops {}", patchops_core::version());
        return;
    }
    // Headless mode: must not touch GPUI (no display, no GPU initialisation).
    if args.iter().any(|a| a == "--rpc-stdio") {
        rpc::run_headless();
        return;
    }

    let shared = Arc::new(SharedState::new());
    let ui_rx = shared.attach_ui();
    if std::env::var_os("PATCHOPS_BENCH_RPC").is_some_and(|v| v == "1") {
        rpc::spawn_in_gui(shared.clone());
    }
    app::run(shared, ui_rx);
}
