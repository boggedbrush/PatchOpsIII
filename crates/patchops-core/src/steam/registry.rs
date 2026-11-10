//! Windows Steam root discovery (legacy `utils._find_windows_steam_root`).
//!
//! Order, exactly as the Python: registry `HKCU\Software\Valve\Steam` `SteamPath`, then
//! `HKLM\SOFTWARE\WOW6432Node\Valve\Steam` `InstallPath`, then `HKLM\SOFTWARE\Valve\Steam`
//! `InstallPath`, then `%PROGRAMFILES(X86)%\Steam`, `%PROGRAMFILES%\Steam` and the two hard-coded
//! `C:\Program Files...` locations. The first candidate (after `normpath`) that contains
//! `steam.exe` wins. Steam on `D:\Steam` is only discoverable through the registry.
//!
//! The ordering/selection logic is plain Rust ([`pick_root`], unit-tested on every platform);
//! only the registry read ([`crate::winreg::read_string`]) is Windows-only.

use std::path::{Path, PathBuf};

use crate::pyfmt::normpath;
#[cfg(any(windows, test))]
pub use crate::winreg::Hive;
#[cfg(windows)]
use crate::winreg::read_string;

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

#[cfg(windows)]
pub fn windows_steam_root() -> Option<PathBuf> {
    let registry: Vec<String> = REGISTRY_TARGETS.iter().filter_map(|(h, k, v)| read_string(*h, k, v)).collect();
    let env = |n: &str| std::env::var(n).ok();
    pick_root(&candidates(registry, env("PROGRAMFILES(X86)"), env("PROGRAMFILES")), |p| p.exists())
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
