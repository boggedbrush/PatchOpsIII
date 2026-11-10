// Release builds on Windows are GUI-subsystem (no console window behind the app, and none flashing
// for the hidden elevated `--install-t7-patch` child). The command-line modes (`--version`,
// `--rpc-stdio`, hidden CLI commands) still need stdio, so `console::attach_parent` re-attaches
// to the launching terminal when the process is started with arguments. Debug builds keep the
// console subsystem so `cargo run` output works as usual.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
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

/// gpui 0.2.2 exposes no fallible platform init (`current_platform` is crate-private and unwraps; with
/// neither DISPLAY nor WAYLAND_DISPLAY set it silently starts "headless" with no window). So before
/// handing over to gpui, check that the display socket gpui will pick is reachable and, if not, print a
/// one-line error and return it instead of letting gpui abort with a raw panic.
#[cfg(target_os = "linux")]
fn check_display() -> Result<(), String> {
    use std::os::linux::net::SocketAddrExt;
    use std::os::unix::net::{SocketAddr, UnixStream};
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    if let Some(w) = var("WAYLAND_DISPLAY") {
        let path = if w.starts_with('/') {
            std::path::PathBuf::from(&w)
        } else {
            std::path::Path::new(&var("XDG_RUNTIME_DIR").unwrap_or_default()).join(&w)
        };
        return UnixStream::connect(&path)
            .map(drop)
            .map_err(|e| format!("cannot connect to Wayland display {} ({e})", path.display()));
    }
    let Some(d) = var("DISPLAY") else {
        return Err("no display available: neither WAYLAND_DISPLAY nor DISPLAY is set".into());
    };
    // Only local displays (":N[.screen]") can be probed; remote ones are left to gpui.
    let Some(num) = d.strip_prefix(':').and_then(|r| r.split('.').next()).filter(|n| n.chars().all(|c| c.is_ascii_digit())) else {
        return Ok(());
    };
    let path = format!("/tmp/.X11-unix/X{num}");
    let file = UnixStream::connect(&path);
    let abstract_ok = || SocketAddr::from_abstract_name(path.as_bytes()).and_then(|a| UnixStream::connect_addr(&a)).is_ok();
    match file {
        Ok(_) => Ok(()),
        Err(_) if abstract_ok() => Ok(()),
        Err(e) => Err(format!("cannot connect to X display {d} ({path}: {e})")),
    }
}

/// Append panics (message, location, thread) to the PatchOpsIII log file before the default hook
/// runs, in GUI and RPC modes alike. Uses a fresh file-only `Logger` on the core's log path rather
/// than a `Core`: the hook can run while the panicking thread holds one of core's locks.
fn install_panic_hook() {
    let log_path = patchops_core::paths::Paths::from_env().log;
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "non-string panic payload".into());
        let at = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_else(|| "unknown location".into());
        let thread = std::thread::current();
        let line = format!("panic in thread '{}': {msg} at {at}", thread.name().unwrap_or("<unnamed>"));
        patchops_core::log::Logger::new(log_path.clone()).log_file_only(patchops_core::log::Category::Error, line);
        default(info);
    }));
}

/// Process start, for `PATCHOPS_TRACE_STARTUP=1` (prints time-to-first-render to stderr).
pub static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

fn main() {
    START.get_or_init(std::time::Instant::now);
    install_panic_hook();
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

    #[cfg(target_os = "linux")]
    if let Err(e) = check_display() {
        eprintln!("patchops: {e}");
        std::process::exit(1);
    }

    let core = Arc::new(Core::new());
    let (ui_tx, ui_rx) = async_channel::unbounded();
    if std::env::var_os("PATCHOPS_BENCH_RPC").is_some_and(|v| v == "1") {
        rpc::spawn_in_gui(core.clone(), ui_tx);
    }
    app::run(core, ui_rx);
}
