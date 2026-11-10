//! Child-process helpers shared by every module that spawns something.
//!
//! * [`command`] is the one place `Command::new` is called: on Windows it adds
//!   `CREATE_NO_WINDOW` so a GUI-subsystem process never flashes a console window when it runs
//!   `powershell`, `taskkill`, `cmd`, ... (the legacy Python app was a console process).
//! * [`run_bounded`] runs a child to completion for at most a given time and then *detaches*
//!   it (reaped by a helper thread) instead of blocking forever.
//!
//! Audit of waits on children in this crate (kept here so it does not rot):
//! `steam::client::run_bounded` users are bounded (10 s); `spawn_settled` waits at most the settle time;
//! `t7_install::powershell` is a short Defender query; the one *unbounded* wait is the elevated
//! helper in [`crate::elevate`] (`WaitForSingleObject(INFINITE)`), which is legitimately long (UAC
//! prompt + download + install) and is left as is. Nothing else calls `wait()`/`output()`.

use std::ffi::OsStr;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// `CREATE_NO_WINDOW`.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// `Command::new(program)` that never shows a console window on Windows.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

/// Outcome of [`run_bounded`].
#[derive(Debug)]
pub enum Bounded {
    /// The child exited within the limit.
    Exited(ExitStatus),
    /// Still running after the limit: it was detached (not killed) and is reaped in the background.
    StillRunning,
}

/// Spawn `cmd` (null stdio), poll `try_wait` until it exits or `limit` has passed, then detach.
pub fn run_bounded(mut cmd: Command, limit: Duration) -> std::io::Result<Bounded> {
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = cmd.spawn()?;
    let end = Instant::now() + limit;
    loop {
        if let Some(st) = child.try_wait()? {
            return Ok(Bounded::Exited(st));
        }
        if Instant::now() >= end {
            std::thread::Builder::new().name("child-reaper".into()).spawn(move || {
                let _ = child.wait();
            })?;
            return Ok(Bounded::StillRunning);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn exits_quickly() {
        let mut c = command("sh");
        c.args(["-c", "exit 3"]);
        match run_bounded(c, Duration::from_secs(5)).unwrap() {
            Bounded::Exited(st) => assert_eq!(st.code(), Some(3)),
            Bounded::StillRunning => panic!("should have exited"),
        }
    }

    #[test]
    fn long_runner_is_detached_not_waited_for() {
        let mut c = command("sh");
        c.args(["-c", "sleep 3"]);
        let t = Instant::now();
        assert!(matches!(run_bounded(c, Duration::from_millis(200)).unwrap(), Bounded::StillRunning));
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    }

    #[test]
    fn missing_program_is_an_error() {
        assert!(run_bounded(command("/nonexistent/prog-xyz"), Duration::from_secs(1)).is_err());
    }
}
