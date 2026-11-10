#![allow(dead_code)] // some controller / widget APIs are only used by views that are still being filled in

mod actions;
mod app;
mod controller;
mod rpc;
mod state;
mod theme;
mod views;
mod widgets;

use std::sync::Arc;

use patchops_core::Core;

/// Process start, for `PATCHOPS_TRACE_STARTUP=1` (prints time-to-first-render to stderr).
pub static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

fn main() {
    START.get_or_init(std::time::Instant::now);
    #[cfg(feature = "debug-log")]
    env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Hidden elevated-helper commands (e.g. the Windows T7 Patch install child).
    if let Some(code) = patchops_core::cli::run_cli_command(&args) {
        std::process::exit(code);
    }
    if args.iter().any(|a| a == "--version") {
        println!("patchops {}", patchops_core::version());
        return;
    }
    // Headless mode: must not touch GPUI (no display, no GPU initialisation).
    if args.iter().any(|a| a == "--rpc-stdio") {
        rpc::run_headless();
        return;
    }

    let core = Arc::new(Core::new());
    let (ui_tx, ui_rx) = async_channel::unbounded();
    if std::env::var_os("PATCHOPS_BENCH_RPC").is_some_and(|v| v == "1") {
        rpc::spawn_in_gui(core.clone(), ui_tx);
    }
    app::run(core, ui_rx);
}
