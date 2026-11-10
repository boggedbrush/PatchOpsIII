//! Windows Steam root discovery (legacy `utils._find_windows_steam_root`).
//!
//! Order, exactly as the Python: registry `HKCU\Software\Valve\Steam` `SteamPath`, then
//! `HKLM\SOFTWARE\WOW6432Node\Valve\Steam` `InstallPath`, then `HKLM\SOFTWARE\Valve\Steam`
//! `InstallPath`, then `%PROGRAMFILES(X86)%\Steam`, `%PROGRAMFILES%\Steam` and the two hard-coded
//! `C:\Program Files...` locations. The first candidate (after `normpath`) that contains
//! `steam.exe` wins. Steam on `D:\Steam` is only discoverable through the registry.
//!
//! The ordering/selection logic is plain Rust ([`pick_root`], unit-tested on every platform);
//! only [`read_string`] (raw `RegGetValueW` FFI, no extra crates) is Windows-only.

use std::path::{Path, PathBuf};

use crate::pyfmt::normpath;

/// The candidate list in legacy order: registry values (already read, empty ones skipped by the
/// caller), then the Program Files locations.
pub fn candidates(registry: Vec<String>, program_files_x86: Option<String>, program_files: Option<String>) -> Vec<String> {
    let mut out: Vec<String> = registry.into_iter().filter(|s| !s.is_empty()).collect();
    for root in [program_files_x86, program_files].into_iter().flatten() {
        if !root.is_empty() {
            out.push(Path::new(&root).join("Steam").to_string_lossy().into_owned());
        }
    }
    out.push(r"C:\Program Files (x86)\Steam".into());
    out.push(r"C:\Program Files\Steam".into());
    out
}

/// First candidate whose normalised path contains `steam.exe` according to `exists`.
pub fn pick_root(cands: &[String], exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    for c in cands {
        if c.is_empty() {
            continue;
        }
        let normalized = normpath(c);
        if exists(&Path::new(&normalized).join("steam.exe")) {
            return Some(PathBuf::from(normalized));
        }
    }
    None
}

/// The three registry locations of the legacy lookup: (hive, subkey, value).
#[cfg(any(windows, test))]
pub const REGISTRY_TARGETS: [(Hive, &str, &str); 3] = [
    (Hive::CurrentUser, r"Software\Valve\Steam", "SteamPath"),
    (Hive::LocalMachine, r"SOFTWARE\WOW6432Node\Valve\Steam", "InstallPath"),
    (Hive::LocalMachine, r"SOFTWARE\Valve\Steam", "InstallPath"),
];

#[cfg(any(windows, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hive {
    CurrentUser,
    LocalMachine,
}

#[cfg(windows)]
pub fn windows_steam_root() -> Option<PathBuf> {
    let registry: Vec<String> = REGISTRY_TARGETS.iter().filter_map(|(h, k, v)| read_string(*h, k, v)).collect();
    let env = |n: &str| std::env::var(n).ok();
    pick_root(&candidates(registry, env("PROGRAMFILES(X86)"), env("PROGRAMFILES")), |p| p.exists())
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

/// `RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ`: strings only; the API NUL-terminates and expands `%VAR%` for the latter.
#[cfg(windows)]
const RRF_RT_STRING: u32 = 0x0000_0002 | 0x0000_0004;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_order_and_skips() {
        let c = candidates(
            vec!["d:/Steam".into(), String::new(), r"E:\Alt".into()],
            Some(r"C:\PF86".into()),
            None,
        );
        assert_eq!(c[0], "d:/Steam");
        assert_eq!(c[1], r"E:\Alt");
        assert!(c[2].ends_with("Steam") && c[2].starts_with(r"C:\PF86"));
        assert_eq!(&c[c.len() - 2..], [r"C:\Program Files (x86)\Steam", r"C:\Program Files\Steam"]);
        assert_eq!(c.len(), 5);
    }

    #[test]
    fn registry_beats_program_files_when_both_exist() {
        let c = candidates(vec!["/reg/steam".into()], Some("/pf86".into()), Some("/pf".into()));
        let both = |p: &Path| p.ends_with("steam.exe");
        assert_eq!(pick_root(&c, both), Some(PathBuf::from("/reg/steam")));
        // Only the Program Files install has the exe: it is used.
        let only_pf = |p: &Path| p.starts_with("/pf86");
        assert!(pick_root(&c, only_pf).unwrap().starts_with("/pf86"));
        assert_eq!(pick_root(&c, |_| false), None);
    }

    #[test]
    fn registry_targets_match_legacy() {
        assert_eq!(REGISTRY_TARGETS[0], (Hive::CurrentUser, r"Software\Valve\Steam", "SteamPath"));
        assert_eq!(REGISTRY_TARGETS[1].1, r"SOFTWARE\WOW6432Node\Valve\Steam");
        assert_eq!(REGISTRY_TARGETS[2], (Hive::LocalMachine, r"SOFTWARE\Valve\Steam", "InstallPath"));
    }
}
