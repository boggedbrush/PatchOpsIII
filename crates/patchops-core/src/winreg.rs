//! Minimal registry reader: raw `RegGetValueW` FFI (no extra crates). Windows-only; shared by the Steam
//! root lookup and the legacy-install detection.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum Hive {
    CurrentUser,
    LocalMachine,
}

#[cfg(windows)]
use std::ffi::c_void;

#[cfg(windows)]
#[link(name = "advapi32")]
extern "system" {
    /// `LSTATUS RegGetValueW(HKEY, LPCWSTR subkey, LPCWSTR value, DWORD flags, LPDWORD type, PVOID data, LPDWORD cb)`;
    /// `cb` is in **bytes**.
    fn RegGetValueW(hkey: isize, sub_key: *const u16, value: *const u16, flags: u32, ty: *mut u32, data: *mut c_void, cb_data: *mut u32) -> i32;
}

/// `RRF_RT_REG_SZ` alone: strings only; the API NUL-terminates. Without `RRF_NOEXPAND` a
/// `REG_EXPAND_SZ` value is expanded first and then reported as `REG_SZ`, so it passes this
/// restriction too. (Adding `RRF_RT_REG_EXPAND_SZ` without `RRF_NOEXPAND` is the combination
/// Wine / older Windows reject with `ERROR_INVALID_PARAMETER`; the MSDN page does not state the rule.)
#[cfg(windows)]
const RRF_RT_STRING: u32 = 0x0000_0002;
#[cfg(windows)]
const ERROR_SUCCESS: i32 = 0;
#[cfg(windows)]
const ERROR_MORE_DATA: i32 = 234;

#[cfg(windows)]
fn hive_handle(h: Hive) -> isize {
    // HKEY_CURRENT_USER = (HKEY)(ULONG_PTR)((LONG)0x80000001): sign-extended on 64-bit.
    match h {
        Hive::CurrentUser => 0x8000_0001u32 as i32 as isize,
        Hive::LocalMachine => 0x8000_0002u32 as i32 as isize,
    }
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A `REG_SZ`/`REG_EXPAND_SZ` value, `None` when the key/value is missing or not a string.
#[cfg(windows)]
pub fn read_string(hive: Hive, subkey: &str, value: &str) -> Option<String> {
    let (sub, val) = (wide(subkey), wide(value));
    let mut cap_bytes: u32 = 0;
    // SAFETY: first call passes a null data pointer to query the required size in bytes; `sub` and `val`
    // are NUL-terminated and outlive the calls.
    let mut rc = unsafe { RegGetValueW(hive_handle(hive), sub.as_ptr(), val.as_ptr(), RRF_RT_STRING, std::ptr::null_mut(), std::ptr::null_mut(), &mut cap_bytes) };
    // The value may grow between the size query and the read: retry a few times on ERROR_MORE_DATA.
    for _ in 0..4 {
        if rc != ERROR_SUCCESS && rc != ERROR_MORE_DATA {
            return None;
        }
        // Buffer in u16 units: size in bytes rounded up, plus room for a terminator.
        let mut buf = vec![0u16; (cap_bytes as usize).div_ceil(2) + 1];
        let mut cb = (buf.len() * 2) as u32; // bytes available
        // SAFETY: `buf` is valid for `cb` bytes; the API writes at most `cb` bytes and updates `cb`.
        rc = unsafe { RegGetValueW(hive_handle(hive), sub.as_ptr(), val.as_ptr(), RRF_RT_STRING, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut cb) };
        if rc == ERROR_SUCCESS {
            let units = (cb as usize / 2).min(buf.len());
            let end = buf[..units].iter().position(|&c| c == 0).unwrap_or(units);
            return Some(String::from_utf16_lossy(&buf[..end]));
        }
        cap_bytes = cb;
    }
    None
}
