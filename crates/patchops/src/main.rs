// Release builds on Windows are GUI-subsystem (no console window behind the app, and none flashing
// for the hidden elevated `--install-t7-patch` child). The command-line modes (`--version`,
// `--rpc-stdio`, hidden CLI commands) still need stdio, so `console::attach_parent` re-attaches
// to the launching terminal when the process is started with arguments. Debug builds keep the
// console subsystem so `cargo run` output works as usual.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
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

/// Windows GUI-subsystem processes start without a console. When launched from a terminal with
/// command-line arguments, attach to that terminal and point any unset std handle at it. Handles
/// that were inherited (pipes, files: the `--rpc-stdio` host, the bench harness) are left alone.
#[cfg(windows)]
mod console {
    use std::fs::OpenOptions;
    use std::os::windows::io::IntoRawHandle;

    type Handle = *mut core::ffi::c_void;
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    const STD_INPUT_HANDLE: u32 = -10i32 as u32;
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;

    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
        fn GetStdHandle(std_handle: u32) -> Handle;
        fn SetStdHandle(std_handle: u32, handle: Handle) -> i32;
    }

    pub fn attach_parent() {
        // SAFETY: plain Win32 calls; the handles passed to SetStdHandle are leaked on purpose
        // (they must outlive every later stdio use).
        unsafe {
            if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
                return; // no parent console (or we already own one, as in debug builds)
            }
            for (std_handle, device, write) in
                [(STD_INPUT_HANDLE, "CONIN$", false), (STD_OUTPUT_HANDLE, "CONOUT$", true), (STD_ERROR_HANDLE, "CONOUT$", true)]
            {
                let current = GetStdHandle(std_handle);
                if current.is_null() || current as isize == -1 {
                    if let Ok(file) = OpenOptions::new().read(!write).write(write).open(device) {
                        SetStdHandle(std_handle, file.into_raw_handle() as Handle);
                    }
                }
            }
        }
    }
}

/// Process start, for `PATCHOPS_TRACE_STARTUP=1` (prints time-to-first-render to stderr).
pub static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

fn main() {
    START.get_or_init(std::time::Instant::now);
    #[cfg(feature = "debug-log")]
    env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(windows)]
    if !args.is_empty() {
        console::attach_parent();
    }
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
