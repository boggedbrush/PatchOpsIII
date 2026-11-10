//! Steam client process control: close, open, launch a game, open a URL.
//!
//! Legacy reference: `utils.py` (`close_steam`, `open_steam`, `launch_game_via_steam`,
//! `_linux_running_pids`) and `backend/api.py` (`_open_external_url`).
//!
//! # Waiting model (the point of this module)
//! The legacy code slept a fixed 5 s after `pkill -x steam` and then polled until Steam had been
//! present for 2 s (about 7.1 s per cycle, independent of the machine). Here every wait is
//! event driven and bounded:
//!
//! * **close**: nothing running -> return at once. Otherwise send SIGTERM (Steam shuts down
//!   cleanly and rewrites `localconfig.vdf` on the way out) and poll the process table every
//!   25 ms until no live `steam` process is left. Only then may the caller edit
//!   `localconfig.vdf`; returning earlier would let the exiting client overwrite our edit, which
//!   is the one real hazard of the whole cycle. Steam is *never* killed (a hard kill during cloud
//!   sync / config flush can corrupt its own files): it gets up to [`CLOSE_TIMEOUT`] (45 s) to
//!   exit and, if it is still alive, [`close_steam`] returns [`STEAM_CLOSE_TIMEOUT_MSG`] and the
//!   callers abort without touching any Steam file. On Windows the request is `steam.exe -shutdown`
//!   (or a plain, non-forced `taskkill` when the exe is unknown); the legacy code used `taskkill /F`.
//! * **open**: spawn detached and return as soon as the process has been *continuously* present
//!   for [`OPEN_STABLE`] (300 ms; catches a bootstrapper that dies immediately), bounded by
//!   [`OPEN_TIMEOUT`]. The legacy 2 s stability window protected nothing real: the result is
//!   only a log line, and nothing downstream depends on Steam being fully initialised.
//!
//! Process discovery reads `/proc/<pid>/comm` (what `pkill -x`/`pgrep -x` match) and skips
//! zombies, without spawning `pgrep`/`ps`. Children are reaped by a helper thread so no zombie is
//! left behind, and are started in their own process group so they outlive/ignore the caller.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use crate::context::Core;
use crate::proc;
#[cfg(unix)]
use crate::pyfmt::io_error_text;

/// Polling interval for process table checks.
const POLL: Duration = Duration::from_millis(25);
/// How long a clean shutdown may take (Steam can be syncing the cloud / flushing its config).
/// The wait ends the instant the process is gone.
pub const CLOSE_TIMEOUT: Duration = Duration::from_secs(45);
/// Error text when Steam did not exit in [`CLOSE_TIMEOUT`]; nothing was modified.
pub const STEAM_CLOSE_TIMEOUT_MSG: &str = "Steam did not close in time. Close Steam manually and try again.";
/// Upper bound for the `xdg-open steam://...` hand-off (legacy: 10 s), then it is detached.
pub const XDG_OPEN_TIMEOUT: Duration = Duration::from_secs(10);
/// Grace period for hand-off launchers (`steam -applaunch`, `xdg-open`) to finish.
const HANDOFF_SETTLE: Duration = Duration::from_millis(250);
/// Total time `open_steam` waits for the process to show up (legacy: 15 s).
pub const OPEN_TIMEOUT: Duration = Duration::from_secs(15);
/// The process must be present this long without interruption.
pub const OPEN_STABLE: Duration = Duration::from_millis(300);

// ---- PATH lookup --------------------------------------------------------------------------

/// `shutil.which`: first executable `name` on `PATH`.
pub fn which(name: &str) -> Option<PathBuf> {
    if name.contains('/') || (cfg!(windows) && name.contains('\\')) {
        let p = PathBuf::from(name);
        return is_executable(&p).then_some(p);
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let cand = dir.join(name);
        if is_executable(&cand) {
            return Some(cand);
        }
        #[cfg(windows)]
        {
            let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
            for ext in exts.split(';').filter(|e| !e.is_empty()) {
                let c = dir.join(format!("{name}{ext}"));
                if is_executable(&c) {
                    return Some(c);
                }
            }
        }
    }
    None
}

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// The Steam launcher: `steam` on `PATH` (Linux/macOS), `<root>/steam.exe` (Windows).
pub fn steam_command(core: &Core) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let exe = super::userdata_dir(core).parent().map(|p| p.join("steam.exe"));
        exe.filter(|p| p.exists())
    }
    #[cfg(not(windows))]
    {
        let _ = core;
        which("steam")
    }
}

// ---- spawning -----------------------------------------------------------------------------

/// Start `cmd args` detached (null stdio, own process group / detached console) and reap it from
/// a helper thread so it never becomes a zombie. Returns once the process has been created.
pub fn spawn_detached<I, S>(cmd: &Path, args: I) -> std::io::Result<()>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    spawn_settled(cmd, args, Duration::ZERO)
}

/// Like [`spawn_detached`], but also gives a short-lived launcher up to `settle` to exit before
/// returning (a hand-off such as `steam -applaunch` / `xdg-open` finishes in a few ms; a process
/// that becomes the long-running client simply outlives the grace period). This makes the effects
/// of the hand-off (e.g. the recorded call) visible to whoever looks right after we return.
pub fn spawn_settled<I, S>(cmd: &Path, args: I, settle: Duration) -> std::io::Result<()>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut c = proc::command(cmd);
    c.args(args.into_iter().map(Into::into)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW
        c.creation_flags(0x0000_0008 | 0x0000_0200 | proc::CREATE_NO_WINDOW);
    }
    let mut child = c.spawn()?;
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    std::thread::Builder::new().name("steam-reaper".into()).spawn(move || {
        let _ = child.wait();
        let _ = tx.send(());
    })?;
    if !settle.is_zero() {
        let _ = rx.recv_timeout(settle);
    }
    Ok(())
}

/// Run a hand-off launcher (`xdg-open steam://...`) with null stdio for at most `limit`.
/// `Ok(Some(code))` (`-1` when killed by a signal) if it exited, `Ok(None)` if it was still
/// running (some setups run the URL handler in the foreground for Steam's whole lifetime): it is
/// detached, not killed.
#[cfg(unix)]
fn run_handoff(cmd: &Path, args: &[&str], limit: Duration) -> std::io::Result<Option<i32>> {
    use std::os::unix::process::CommandExt;
    let mut c = proc::command(cmd);
    c.args(args).process_group(0);
    Ok(match proc::run_bounded(c, limit)? {
        proc::Bounded::Exited(st) => Some(st.code().unwrap_or(-1)),
        proc::Bounded::StillRunning => None,
    })
}

// ---- process table ------------------------------------------------------------------------

/// Pids of live (non-zombie) processes whose name is exactly `steam`
/// (`steam.exe` on Windows).
pub fn steam_pids() -> Vec<u32> {
    #[cfg(target_os = "linux")]
    {
        let Ok(rd) = std::fs::read_dir("/proc") else { return Vec::new() };
        let mut out = Vec::new();
        for e in rd.flatten() {
            let name = e.file_name();
            let Some(pid) = name.to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
            let base = e.path();
            let Ok(comm) = std::fs::read_to_string(base.join("comm")) else { continue };
            if comm.trim_end_matches('\n') != "steam" {
                continue;
            }
            // `pid (comm) S ...`: the state follows the last ')'.
            let Ok(stat) = std::fs::read_to_string(base.join("stat")) else { continue };
            let state = stat.rsplit_once(')').and_then(|(_, rest)| rest.trim_start().chars().next());
            if matches!(state, None | Some('Z') | Some('X')) {
                continue;
            }
            out.push(pid);
        }
        out
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        let Ok(o) = proc::command("pgrep").args(["-x", "steam"]).stderr(Stdio::null()).output() else { return Vec::new() };
        String::from_utf8_lossy(&o.stdout).lines().filter_map(|l| l.trim().parse().ok()).collect()
    }
    #[cfg(windows)]
    {
        win::pids_by_exe_name("steam.exe")
    }
}

/// Process listing through the ToolHelp snapshot API: a few hundred microseconds, no child
/// process (the previous `tasklist` spawn was polled every 25 ms and flashed a console window).
#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    /// `PROCESSENTRY32W` (the `ULONG_PTR` heap id makes the layout differ between 32/64 bit;
    /// `repr(C)` reproduces the C padding).
    #[repr(C)]
    struct ProcessEntry32W {
        dw_size: u32,
        cnt_usage: u32,
        th32_process_id: u32,
        th32_default_heap_id: usize,
        th32_module_id: u32,
        cnt_threads: u32,
        th32_parent_process_id: u32,
        pc_pri_class_base: i32,
        dw_flags: u32,
        sz_exe_file: [u16; 260],
    }

    const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
    const INVALID_HANDLE_VALUE: isize = -1;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> *mut c_void;
        fn Process32FirstW(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
        fn Process32NextW(snapshot: *mut c_void, entry: *mut ProcessEntry32W) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    /// Pids of all processes whose image name equals `exe` (ASCII case-insensitive).
    pub fn pids_by_exe_name(exe: &str) -> Vec<u32> {
        let mut out = Vec::new();
        // SAFETY: the snapshot handle is checked, closed exactly once, and `entry` is a valid,
        // correctly sized PROCESSENTRY32W whose `dw_size` is set as the API requires.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap as isize == INVALID_HANDLE_VALUE || snap.is_null() {
                return out;
            }
            let mut entry: ProcessEntry32W = std::mem::zeroed();
            entry.dw_size = std::mem::size_of::<ProcessEntry32W>() as u32;
            let mut ok = Process32FirstW(snap, &mut entry);
            while ok != 0 {
                let len = entry.sz_exe_file.iter().position(|&c| c == 0).unwrap_or(entry.sz_exe_file.len());
                if String::from_utf16_lossy(&entry.sz_exe_file[..len]).eq_ignore_ascii_case(exe) {
                    out.push(entry.th32_process_id);
                }
                ok = Process32NextW(snap, &mut entry);
            }
            CloseHandle(snap);
        }
        out
    }
}

/// Whether a live Steam client process exists.
pub fn is_steam_running() -> bool {
    !steam_pids().is_empty()
}

#[cfg(unix)]
fn signal(pids: &[u32], sig: i32) {
    for &p in pids {
        // SAFETY: plain kill(2) call; a stale/foreign pid just fails.
        unsafe {
            libc::kill(p as libc::pid_t, sig);
        }
    }
}

/// Poll until no steam process is left; `false` on timeout.
fn wait_gone(timeout: Duration) -> bool {
    let end = Instant::now() + timeout;
    loop {
        if !is_steam_running() {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(POLL);
    }
}

/// Ask Steam to exit and wait up to `timeout` until it is really gone. Never kills it.
/// `false` if a Steam process is still alive at the end.
#[cfg(unix)]
fn terminate_and_wait_for(pids: &[u32], timeout: Duration) -> bool {
    signal(pids, libc::SIGTERM);
    wait_gone(timeout)
}

#[cfg(unix)]
fn terminate_and_wait(core: &Core, pids: &[u32]) -> Result<(), String> {
    if terminate_and_wait_for(pids, CLOSE_TIMEOUT) {
        return Ok(());
    }
    core.log.error(STEAM_CLOSE_TIMEOUT_MSG);
    Err(STEAM_CLOSE_TIMEOUT_MSG.to_string())
}

// ---- close / open -------------------------------------------------------------------------

/// Legacy `close_steam(log_widget)`: ask the client to exit and return once it has. Logs
/// `Steam was not running.` (Info) when there was nothing to close.
///
/// `Err(STEAM_CLOSE_TIMEOUT_MSG)` (also logged) when Steam is still running after
/// [`CLOSE_TIMEOUT`]; the process is left alone and callers must not edit any Steam file.
/// Other legacy failures (control command missing / failing) are logged and, as before, do not
/// stop the caller *unless Steam is still running afterwards* (then it is the same error).
pub fn close_steam(core: &Core) -> Result<(), String> {
    #[cfg(windows)]
    {
        if !is_steam_running() {
            core.log.info("Steam was not running.");
            return Ok(());
        }
        // `-shutdown` is Steam's own graceful exit; without the exe fall back to a non-forced taskkill.
        let launched = match steam_command(core) {
            Some(exe) => proc::run_bounded({
                let mut c = proc::command(exe);
                c.arg("-shutdown");
                c
            }, Duration::from_secs(10)).map(|_| ()),
            None => proc::run_bounded({
                let mut c = proc::command("taskkill");
                c.args(["/IM", "steam.exe"]);
                c
            }, Duration::from_secs(10)).map(|_| ()),
        };
        if let Err(e) = launched {
            core.log.error(format!("Steam control command not found: {e}"));
        }
        if !wait_gone(CLOSE_TIMEOUT) {
            core.log.error(STEAM_CLOSE_TIMEOUT_MSG);
            return Err(STEAM_CLOSE_TIMEOUT_MSG.to_string());
        }
        Ok(())
    }
    #[cfg(unix)]
    {
        let pids = steam_pids();
        if pids.is_empty() {
            core.log.info("Steam was not running.");
            return Ok(());
        }
        terminate_and_wait(core, &pids)
    }
}

/// Legacy `open_steam(log_widget)`: start the client detached and return once it is running.
///
/// Log sequence (Linux): `[Closing Steam...]`, `Setting launch options...`, `Opening Steam...`,
/// then `Steam launched successfully` (Success) or `Steam did not launch successfully` (Error;
/// Warning on Windows).
pub fn open_steam(core: &Core) {
    #[cfg(windows)]
    {
        core.log.info("Opening Steam...");
        let launched = match steam_command(core) {
            Some(exe) => spawn_detached(&exe, ["-silent"]).is_ok(),
            None => {
                core.log.warning("Steam executable path not found; attempting to use system URI handler.");
                spawn_detached(Path::new("cmd"), ["/c", "start", "", "steam://"]).is_ok()
            }
        };
        finish_open(core, launched);
    }
    #[cfg(not(windows))]
    {
        if is_steam_running() {
            core.log.info("Closing Steam...");
            if terminate_and_wait(core, &steam_pids()).is_err() {
                // Still running: starting another client would be pointless.
                return;
            }
        }
        core.log.info("Setting launch options...");
        core.log.info("Opening Steam...");

        let mut launched = false;
        let mut errors: Vec<String> = Vec::new();
        if let Some(cmd) = steam_command(core) {
            match spawn_detached(&cmd, ["-silent"]) {
                Ok(()) => launched = true,
                Err(e) => errors.push(format!("{} -silent failed: {}", cmd.display(), io_error_text(&e, Some(&cmd)))),
            }
        }
        if !launched {
            if let Some(xdg) = which("xdg-open") {
                match run_handoff(&xdg, &["steam://open/main"], XDG_OPEN_TIMEOUT) {
                    // `None`: the handler is still running in the foreground (Steam's own
                    // lifetime); `finish_open` observes whether the client really came up.
                    Ok(Some(0)) | Ok(None) => launched = true,
                    Ok(Some(code)) => errors.push(format!("{} exited with code {code}", xdg.display())),
                    Err(e) => errors.push(format!("{} failed: {}", xdg.display(), io_error_text(&e, Some(&xdg)))),
                }
            }
        }
        if !launched {
            let detail = if errors.is_empty() { "no launch command available".to_string() } else { errors.join("; ") };
            core.log.error(format!("Steam launch command failed: {detail}"));
        }
        finish_open(core, launched);
    }
}

/// Wait for the client to be observed and log the outcome.
fn finish_open(core: &Core, launched: bool) {
    // Nothing was started: no point polling for 15 s (legacy did, and then failed anyway).
    let up = if launched { wait_stable(OPEN_TIMEOUT, OPEN_STABLE) } else { is_steam_running() };
    if up {
        core.log.success("Steam launched successfully");
    } else if cfg!(windows) {
        core.log.warning("Steam did not launch successfully");
    } else {
        core.log.error("Steam did not launch successfully");
    }
}

/// `true` once a steam process has been present continuously for `stable`, within `timeout`.
fn wait_stable(timeout: Duration, stable: Duration) -> bool {
    let end = Instant::now() + timeout;
    let mut since: Option<Instant> = None;
    loop {
        let now = Instant::now();
        if is_steam_running() {
            let s = *since.get_or_insert(now);
            if now.duration_since(s) >= stable {
                return true;
            }
        } else {
            since = None;
        }
        if now >= end {
            return false;
        }
        std::thread::sleep(POLL);
    }
}

// ---- launching the game / URLs ------------------------------------------------------------

/// Legacy `launch_game_via_steam(app_id)`: failure is only logged.
pub fn launch_game(core: &Core, app_id: &str) {
    let uri = format!("steam://rungameid/{app_id}");
    let ok_msg = |how: &str| core.log.success(format!("Launched Black Ops III via Steam (AppID: {app_id}) using {how}"));
    #[cfg(windows)]
    {
        let r = match steam_command(core) {
            Some(exe) => spawn_settled(&exe, ["-applaunch", app_id], HANDOFF_SETTLE),
            None => {
                core.log.warning("Steam executable path not found; attempting to use system URI handler.");
                spawn_detached(Path::new("cmd"), ["/c", "start", "", &uri])
            }
        };
        match r {
            Ok(()) => core.log.success(format!("Launched Black Ops III via Steam (AppID: {app_id})")),
            Err(e) => core.log.error(format!("Error launching game via Steam: {e}")),
        }
        let _ = ok_msg;
    }
    #[cfg(target_os = "macos")]
    {
        let _ = ok_msg;
        match spawn_settled(Path::new("open"), [uri.as_str()], HANDOFF_SETTLE) {
            Ok(()) => core.log.success(format!("Launched Black Ops III via Steam (AppID: {app_id})")),
            Err(e) => core.log.error(format!("Error launching game via Steam: {e}")),
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut last_error: Option<String> = None;
        let steam = steam_command(core);
        let xdg = which("xdg-open");
        // (command, description, wait-for-exit-and-check)
        let mut commands: Vec<(PathBuf, Vec<String>, String, bool)> = Vec::new();
        if let Some(s) = &steam {
            let base = s.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            commands.push((s.clone(), vec!["-applaunch".into(), app_id.into()], format!("{base} -applaunch"), false));
        }
        if let Some(x) = &xdg {
            commands.push((x.clone(), vec![uri.clone()], "xdg-open steam URI".into(), true));
        }
        if commands.is_empty() {
            commands.push((PathBuf::from("steam"), vec![uri.clone()], "steam URI fallback".into(), false));
        }
        for (cmd, args, desc, check) in commands {
            let res: std::io::Result<bool> = if check {
                let a: Vec<&str> = args.iter().map(String::as_str).collect();
                run_handoff(&cmd, &a, XDG_OPEN_TIMEOUT).map(|code| match code {
                    None | Some(0) => true,
                    Some(code) => {
                        last_error = Some(format!("{} exited with code {code}", cmd.display()));
                        false
                    }
                })
            } else {
                spawn_settled(&cmd, args.iter().map(String::as_str), HANDOFF_SETTLE).map(|_| true)
            };
            match res {
                Ok(true) => {
                    ok_msg(&desc);
                    return;
                }
                Ok(false) => continue,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    last_error = Some(format!("{} not found", cmd.display()));
                }
                Err(e) => last_error = Some(e.to_string()),
            }
        }
        core.log.error(format!(
            "Error launching game via Steam: {}",
            last_error.unwrap_or_else(|| "No suitable launcher command found".into())
        ));
    }
}

/// Legacy `_open_external_url(url)`; `Err` carries the legacy exception text.
pub fn open_external_url(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        spawn_detached(Path::new("cmd"), ["/c", "start", "", url]).map_err(|e| e.to_string())
    }
    #[cfg(target_os = "macos")]
    {
        spawn_detached(Path::new("open"), [url]).map_err(|e| e.to_string())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let launcher = which("xdg-open").or_else(|| which("steam")).ok_or_else(|| "No system URL launcher was found.".to_string())?;
        spawn_settled(&launcher, [url], HANDOFF_SETTLE).map_err(|e| io_error_text(&e, Some(&launcher)))
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::process::Child;
    use std::sync::Mutex;

    /// `steam_pids()` is process-global: serialise the tests that spawn a fake `steam`.
    static LOCK: Mutex<()> = Mutex::new(());

    /// A process whose `comm` is `steam`: a symlink to `sh` run with a script.
    fn fake_steam(dir: &Path, script: &str) -> Child {
        let link = dir.join("steam");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink("/bin/sh", &link).unwrap();
        let c = std::process::Command::new(&link).args(["-c", script]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        // Wait until the exec happened (comm is `steam`).
        let end = Instant::now() + Duration::from_secs(5);
        while !steam_pids().contains(&c.id()) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(10));
        }
        c
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("patchops-client-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn stubborn_steam_is_never_killed_and_close_fails() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if !steam_pids().is_empty() {
            eprintln!("skipped: a real/fake steam process is running");
            return;
        }
        let d = tmp("stubborn");
        let ready = d.join("ready");
        let mut c = fake_steam(&d, &format!("trap '' TERM; : > '{}'; while :; do sleep 0.1; done", ready.display()));
        // SIGTERM must not arrive before the trap is installed.
        let end = Instant::now() + Duration::from_secs(5);
        while !ready.exists() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(10));
        }
        let pids = steam_pids();
        assert!(pids.contains(&c.id()));
        let t = Instant::now();
        assert!(!terminate_and_wait_for(&pids, Duration::from_millis(400)));
        assert!(t.elapsed() >= Duration::from_millis(400));
        // Not killed: still alive and running.
        assert!(c.try_wait().unwrap().is_none(), "steam must not be SIGKILLed");
        assert!(steam_pids().contains(&c.id()));
        let _ = c.kill();
        let _ = c.wait();
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn graceful_steam_returns_as_soon_as_it_is_gone() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if !steam_pids().is_empty() {
            eprintln!("skipped: a real/fake steam process is running");
            return;
        }
        let d = tmp("graceful");
        let mut c = fake_steam(&d, "while :; do sleep 0.1; done");
        let reaper = std::thread::spawn(move || {
            let _ = c.wait();
        });
        let t = Instant::now();
        assert!(terminate_and_wait_for(&steam_pids(), Duration::from_secs(20)));
        assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
        reaper.join().unwrap();
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn close_steam_error_is_the_documented_text() {
        assert_eq!(STEAM_CLOSE_TIMEOUT_MSG, "Steam did not close in time. Close Steam manually and try again.");
        assert_eq!(CLOSE_TIMEOUT, Duration::from_secs(45));
    }

    #[test]
    fn xdg_open_style_foreground_handler_is_detached_after_the_limit() {
        let t = Instant::now();
        let r = run_handoff(Path::new("/bin/sh"), &["-c", "sleep 3"], Duration::from_millis(200)).unwrap();
        assert_eq!(r, None);
        assert!(t.elapsed() < Duration::from_secs(2));
        assert_eq!(run_handoff(Path::new("/bin/sh"), &["-c", "exit 4"], Duration::from_secs(5)).unwrap(), Some(4));
    }
}
