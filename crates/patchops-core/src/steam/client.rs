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
//!   is the one real hazard of the whole cycle. If Steam is still alive after
//!   [`CLOSE_TIMEOUT`] it is killed with SIGKILL (it can no longer write anything) and we wait
//!   again, so the file is never edited while a client may still write it.
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
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::context::Core;
use crate::pyfmt::io_error_text;

/// Polling interval for process table checks.
const POLL: Duration = Duration::from_millis(25);
/// How long a clean (SIGTERM) shutdown may take before the client is killed.
pub const CLOSE_TIMEOUT: Duration = Duration::from_secs(10);
/// Extra wait after SIGKILL.
const KILL_TIMEOUT: Duration = Duration::from_secs(3);
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
    let mut c = Command::new(cmd);
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
        c.creation_flags(0x0000_0008 | 0x0000_0200 | 0x0800_0000);
    }
    let mut child = c.spawn()?;
    std::thread::Builder::new().name("steam-reaper".into()).spawn(move || {
        let _ = child.wait();
    })?;
    Ok(())
}

/// Run to completion with null stdio; `Ok(exit code)` (`-1` when killed by a signal).
fn run_status(cmd: &Path, args: &[&str]) -> std::io::Result<i32> {
    let st = Command::new(cmd).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status()?;
    Ok(st.code().unwrap_or(-1))
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
        let Ok(o) = Command::new("pgrep").args(["-x", "steam"]).stderr(Stdio::null()).output() else { return Vec::new() };
        String::from_utf8_lossy(&o.stdout).lines().filter_map(|l| l.trim().parse().ok()).collect()
    }
    #[cfg(windows)]
    {
        let Ok(o) = Command::new("tasklist").args(["/FI", "IMAGENAME eq steam.exe", "/FO", "CSV", "/NH"]).stderr(Stdio::null()).output() else {
            return Vec::new();
        };
        String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| l.to_ascii_lowercase().contains("steam.exe"))
            .filter_map(|l| l.split("\",\"").nth(1).and_then(|p| p.trim_matches('"').parse().ok()))
            .collect()
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

/// Ask Steam to terminate and wait until it is really gone; escalates to a hard kill after
/// [`CLOSE_TIMEOUT`]. Returns `false` if a Steam process is still alive at the end.
#[cfg(unix)]
fn terminate_and_wait(core: &Core, pids: &[u32]) -> bool {
    signal(pids, libc::SIGTERM);
    if wait_gone(CLOSE_TIMEOUT) {
        return true;
    }
    core.log.warning("Steam did not exit in time; forcing it to close.");
    signal(&steam_pids(), libc::SIGKILL);
    wait_gone(KILL_TIMEOUT)
}

// ---- close / open -------------------------------------------------------------------------

/// Legacy `close_steam(log_widget)`: terminate the client and return once it has exited.
/// Logs `Steam was not running.` (Info) when there was nothing to close.
pub fn close_steam(core: &Core) {
    #[cfg(windows)]
    {
        let out = Command::new("taskkill").args(["/F", "/IM", "steam.exe"]).stdin(Stdio::null()).output();
        match out {
            Ok(o) if o.status.success() => {
                if !wait_gone(CLOSE_TIMEOUT) {
                    core.log.warning("Steam did not exit in time.");
                }
            }
            Ok(o) => {
                if !is_steam_running() {
                    core.log.info("Steam was not running.");
                } else {
                    let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
                    let msg = if err.is_empty() { String::from_utf8_lossy(&o.stdout).trim().to_string() } else { err };
                    core.log.error(format!("Failed to close Steam: {msg}"));
                }
            }
            Err(e) => core.log.error(format!("Steam control command not found: {e}")),
        }
    }
    #[cfg(unix)]
    {
        let pids = steam_pids();
        if pids.is_empty() {
            core.log.info("Steam was not running.");
            return;
        }
        if !terminate_and_wait(core, &pids) {
            core.log.error("Failed to close Steam (it is still running).");
        }
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
            terminate_and_wait(core, &steam_pids());
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
                match run_status(&xdg, &["steam://open/main"]) {
                    Ok(0) => launched = true,
                    Ok(code) => errors.push(format!("{} exited with code {code}", xdg.display())),
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
            Some(exe) => spawn_detached(&exe, ["-applaunch", app_id]),
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
        match spawn_detached(Path::new("open"), [uri.as_str()]) {
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
                run_status(&cmd, &a).map(|code| {
                    if code != 0 {
                        last_error = Some(format!("{} exited with code {code}", cmd.display()));
                    }
                    code == 0
                })
            } else {
                spawn_detached(&cmd, args.iter().map(String::as_str)).map(|_| true)
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
        spawn_detached(&launcher, [url]).map_err(|e| io_error_text(&e, Some(&launcher)))
    }
}
