//! Windows administrator elevation for the T7 Patch install (legacy `is_admin`,
//! `_run_elevated_and_wait`, `_elevated_backend_command_args`).
//!
//! The legacy backend re-launched itself with `ShellExecuteEx(verb="runas")`, hidden, waited for the
//! process and read its exit code. Here the "backend" is the application binary itself and the
//! elevated process runs the hidden CLI command parsed by [`crate::cli`]. On other platforms
//! [`run_elevated_and_wait`] always fails with the legacy "only available on Windows" error and
//! [`is_admin`] is `false`.
//!
//! Only std + raw `extern "system"` declarations (no windows crate). The Windows code has not been
//! compiled or run on a Windows machine in this repository's CI so far.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElevationError {
    /// Legacy `ElevationRequired` (text is the legacy message).
    Required(String),
    /// Legacy `ElevationCancelled`: the user dismissed the UAC prompt.
    Cancelled,
}

impl fmt::Display for ElevationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ElevationError::Required(m) => f.write_str(m),
            ElevationError::Cancelled => f.write_str("Windows administrator approval was cancelled."),
        }
    }
}

/// Python `subprocess.list2cmdline`: quote arguments for a Windows command line.
pub fn list2cmdline(args: &[String]) -> String {
    let mut out = String::new();
    for (n, arg) in args.iter().enumerate() {
        if n > 0 {
            out.push(' ');
        }
        let needs_quote = arg.is_empty() || arg.contains(' ') || arg.contains('\t');
        if needs_quote {
            out.push('"');
        }
        let mut backslashes = 0usize;
        for c in arg.chars() {
            if c == '\\' {
                backslashes += 1;
            } else if c == '"' {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            } else {
                out.push_str(&"\\".repeat(backslashes));
                backslashes = 0;
                out.push(c);
            }
        }
        if backslashes > 0 {
            out.push_str(&"\\".repeat(backslashes));
        }
        if needs_quote {
            out.push_str(&"\\".repeat(backslashes));
            out.push('"');
        }
    }
    out
}

#[cfg(windows)]
mod imp {
    use super::{list2cmdline, ElevationError};
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;

    #[repr(C)]
    struct ShellExecuteInfoW {
        cb_size: u32,
        f_mask: u32,
        hwnd: *mut c_void,
        lp_verb: *const u16,
        lp_file: *const u16,
        lp_parameters: *const u16,
        lp_directory: *const u16,
        n_show: i32,
        h_inst_app: *mut c_void,
        lp_id_list: *mut c_void,
        lp_class: *const u16,
        hkey_class: *mut c_void,
        dw_hot_key: u32,
        h_icon: *mut c_void,
        h_process: *mut c_void,
    }

    #[link(name = "shell32")]
    extern "system" {
        fn IsUserAnAdmin() -> i32;
        fn ShellExecuteExW(info: *mut ShellExecuteInfoW) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn WaitForSingleObject(handle: *mut c_void, millis: u32) -> u32;
        fn GetExitCodeProcess(handle: *mut c_void, code: *mut u32) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
        fn GetLastError() -> u32;
    }

    const SEE_MASK_NOCLOSEPROCESS: u32 = 0x40;
    const SW_HIDE: i32 = 0;
    const WAIT_OBJECT_0: u32 = 0;
    const INFINITE: u32 = 0xFFFF_FFFF;
    const ERROR_CANCELLED: u32 = 1223;

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    pub fn is_admin() -> bool {
        // SAFETY: plain FFI call without arguments.
        unsafe { IsUserAnAdmin() != 0 }
    }

    pub fn run_elevated_and_wait(args: &[String]) -> Result<u32, ElevationError> {
        let exe = std::env::current_exe().map_err(|e| ElevationError::Required(format!("Could not locate the application: {e}")))?;
        let dir = exe.parent().map(|p| p.to_path_buf());
        let verb = wide(std::ffi::OsStr::new("runas"));
        let file = wide(exe.as_os_str());
        let params = wide(std::ffi::OsStr::new(&list2cmdline(args)));
        let dir_w = dir.as_ref().map(|d| wide(d.as_os_str()));

        let mut info = ShellExecuteInfoW {
            cb_size: std::mem::size_of::<ShellExecuteInfoW>() as u32,
            f_mask: SEE_MASK_NOCLOSEPROCESS,
            hwnd: std::ptr::null_mut(),
            lp_verb: verb.as_ptr(),
            lp_file: file.as_ptr(),
            lp_parameters: params.as_ptr(),
            lp_directory: dir_w.as_ref().map_or(std::ptr::null(), |d| d.as_ptr()),
            n_show: SW_HIDE,
            h_inst_app: std::ptr::null_mut(),
            lp_id_list: std::ptr::null_mut(),
            lp_class: std::ptr::null(),
            hkey_class: std::ptr::null_mut(),
            dw_hot_key: 0,
            h_icon: std::ptr::null_mut(),
            h_process: std::ptr::null_mut(),
        };
        // SAFETY: `info` and the wide strings it points to outlive the calls; the process handle
        // returned through SEE_MASK_NOCLOSEPROCESS is closed exactly once below.
        unsafe {
            if ShellExecuteExW(&mut info) == 0 {
                let code = GetLastError();
                if code == ERROR_CANCELLED {
                    return Err(ElevationError::Cancelled);
                }
                return Err(ElevationError::Required(format!("Windows administrator approval failed with code {code}.")));
            }
            let handle = info.h_process;
            let result = (|| {
                let waited = WaitForSingleObject(handle, INFINITE);
                if waited != WAIT_OBJECT_0 {
                    return Err(ElevationError::Required(format!("Elevated helper did not finish normally ({waited}).")));
                }
                let mut code = 0u32;
                if GetExitCodeProcess(handle, &mut code) == 0 {
                    return Err(ElevationError::Required("Could not read elevated helper result.".into()));
                }
                Ok(code)
            })();
            if !handle.is_null() {
                CloseHandle(handle);
            }
            result
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::ElevationError;

    pub fn is_admin() -> bool {
        false
    }

    pub fn run_elevated_and_wait(_args: &[String]) -> Result<u32, ElevationError> {
        Err(ElevationError::Required("Administrator approval is only available on Windows.".into()))
    }
}

/// Windows: `IsUserAnAdmin()`; elsewhere `false`.
pub fn is_admin() -> bool {
    imp::is_admin()
}

/// Re-launch the running binary elevated (UAC) with `args`, hidden, wait for it and return its exit code.
pub fn run_elevated_and_wait(args: &[String]) -> Result<u32, ElevationError> {
    imp::run_elevated_and_wait(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmdline_quoting_matches_python() {
        let v = |a: &[&str]| list2cmdline(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(v(&["--install-t7-patch", r"C:\Games\Black Ops III"]), r#"--install-t7-patch "C:\Games\Black Ops III""#);
        assert_eq!(v(&["a", ""]), r#"a """#);
        assert_eq!(v(&[r#"say "hi""#]), r#""say \"hi\"""#);
        assert_eq!(v(&[r#"q"x"#]), r#"q\"x"#);
        assert_eq!(v(&[r"C:\dir with space\"]), r#""C:\dir with space\\""#);
        assert_eq!(v(&[r"C:\plain\"]), r"C:\plain\");
    }

    #[cfg(not(windows))]
    #[test]
    fn not_windows() {
        assert!(!is_admin());
        assert_eq!(
            run_elevated_and_wait(&[]).unwrap_err().to_string(),
            "Administrator approval is only available on Windows."
        );
    }
}
