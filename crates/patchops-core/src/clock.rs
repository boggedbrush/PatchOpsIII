//! Local wall-clock time without a date/time dependency, plus `uname` for the log payload.

/// Broken-down local time.
#[derive(Debug, Clone, Copy)]
pub struct LocalTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl LocalTime {
    /// `YYYY-MM-DD HH:MM:SS` (Python `strftime("%Y-%m-%d %H:%M:%S")`).
    pub fn format(&self) -> String {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}

#[cfg(unix)]
pub fn now() -> LocalTime {
    // SAFETY: plain libc calls with valid out-pointers.
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        LocalTime {
            year: tm.tm_year + 1900,
            month: (tm.tm_mon + 1) as u32,
            day: tm.tm_mday as u32,
            hour: tm.tm_hour as u32,
            minute: tm.tm_min as u32,
            second: tm.tm_sec as u32,
        }
    }
}

#[cfg(windows)]
pub fn now() -> LocalTime {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        millis: u16,
    }
    extern "system" {
        fn GetLocalTime(st: *mut SystemTime);
    }
    let mut st = SystemTime::default();
    // SAFETY: valid out-pointer to a SYSTEMTIME-compatible struct.
    unsafe { GetLocalTime(&mut st) };
    LocalTime {
        year: st.year as i32,
        month: st.month as u32,
        day: st.day as u32,
        hour: st.hour as u32,
        minute: st.minute as u32,
        second: st.second as u32,
    }
}

#[cfg(not(any(unix, windows)))]
pub fn now() -> LocalTime {
    LocalTime { year: 1970, month: 1, day: 1, hour: 0, minute: 0, second: 0 }
}

/// `(release, machine)` as in Python's `platform.release()` / `platform.machine()`.
#[cfg(unix)]
pub fn uname() -> (String, String) {
    use std::ffi::CStr;
    // SAFETY: uname fills a zeroed utsname; fields are NUL-terminated.
    unsafe {
        let mut u: libc::utsname = std::mem::zeroed();
        if libc::uname(&mut u) != 0 {
            return (String::new(), String::new());
        }
        (
            CStr::from_ptr(u.release.as_ptr()).to_string_lossy().into_owned(),
            CStr::from_ptr(u.machine.as_ptr()).to_string_lossy().into_owned(),
        )
    }
}

#[cfg(not(unix))]
pub fn uname() -> (String, String) {
    let machine = match std::env::consts::ARCH {
        "x86_64" => "AMD64",
        other => other,
    };
    // TODO(windows): Python reports the NT release (e.g. "10"/"11"); needs RtlGetVersion.
    (String::from("10"), machine.to_string())
}
