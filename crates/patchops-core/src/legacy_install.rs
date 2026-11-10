//! Detection of a leftover install made by the old Electron-era NSIS installer.
//!
//! Releases 1.3.0-1.3.4 shipped a per-machine MSI *and* an NSIS installer named `PatchOpsIII.exe`
//! (electron-builder, `oneClick: false`, `perMachine: false`: per-user at
//! `%LOCALAPPDATA%\Programs\PatchOpsIII` by default, optionally all-users at
//! `%ProgramFiles%\PatchOpsIII`). Someone who installed that and now installs the new MSI ends up
//! with two installs. The app only *tells* the user (it never runs an uninstaller itself).
//!
//! The NSIS uninstall entry is `Software\Microsoft\Windows\CurrentVersion\Uninstall\<key>` in HKCU
//! (per-user) or HKLM (all-users), 64-bit view, where `<key>` is the uuid5 of the app id
//! `com.patchopsiii.app` under electron-builder's namespace ([`UNINSTALL_GUID`]). electron-builder
//! writes it without braces (`UNINSTALL_APP_KEY` in its NSIS templates); the braced form is probed
//! too in case that ever changes.
//!
//! This is deliberately outside `State`, op responses and the log: it only feeds a GUI notice.
//!
//! If the running exe lives inside the recorded install dir, the entry is ignored. That is the
//! all-users case: the new per-machine MSI installs into the very same `%ProgramFiles%\PatchOpsIII`,
//! overwrote the NSIS files, and the NSIS uninstall entry is merely stale. Telling the user to
//! "remove the old install" there would be wrong and destructive (the NSIS uninstaller would delete
//! the folder holding the new app), so no notice is shown.

use std::path::{Path, PathBuf};

/// uuid5(`com.patchopsiii.app`, electron-builder namespace `50e065bc-3134-11e6-9bab-38c9862bdaf3`).
pub const UNINSTALL_GUID: &str = "a0f3b457-ab4b-5b70-97b3-7be0d5217bca";

const UNINSTALL_ROOT: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Installed for the current user only (HKCU).
    User,
    /// Installed for all users (HKLM).
    Machine,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyInstall {
    pub scope: Scope,
    pub display_name: Option<String>,
    pub install_dir: Option<PathBuf>,
}

/// Registry hive as seen by the injected reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Root {
    CurrentUser,
    LocalMachine,
}

fn canon(p: &Path) -> PathBuf {
    let c = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    PathBuf::from(c.to_string_lossy().to_lowercase().replace('/', "\\"))
}

fn inside(exe: &Path, dir: &Path) -> bool {
    // Windows paths are compared lower-cased with backslashes (case-insensitive file system).
    // Normalise both the same way; `Path::starts_with` is component-wise on Windows only, so
    // compare on a trailing-separator string for portability of the unit tests.
    let norm = |p: PathBuf| p.to_string_lossy().trim_end_matches('\\').to_string();
    let (e, d) = (norm(canon(exe)), norm(canon(dir)));
    !d.is_empty() && (e == d || e.starts_with(&format!("{d}\\")))
}

/// Pure detection. `read(root, subkey, value)` reads a registry string; `exe` is the running executable.
pub fn detect_with(read: impl Fn(Root, &str, &str) -> Option<String>, exe: Option<&Path>) -> Option<LegacyInstall> {
    for (root, scope) in [(Root::CurrentUser, Scope::User), (Root::LocalMachine, Scope::Machine)] {
        for key in [UNINSTALL_GUID.to_string(), format!("{{{UNINSTALL_GUID}}}")] {
            let sub = format!(r"{UNINSTALL_ROOT}\{key}");
            let get = |v: &str| read(root, &sub, v).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
            let (name, dir, uninst) = (get("DisplayName"), get("InstallLocation"), get("UninstallString"));
            // No identifying value at all: the key is absent (or not ours).
            if name.is_none() && dir.is_none() && uninst.is_none() {
                continue;
            }
            let install_dir = dir.map(PathBuf::from);
            if let (Some(exe), Some(d)) = (exe, install_dir.as_deref()) {
                if inside(exe, d) {
                    // Same folder as the running (new) app: stale entry, see module docs. The other
                    // hive may still hold a genuinely separate install, so keep looking.
                    break;
                }
            }
            return Some(LegacyInstall { scope, display_name: name, install_dir });
        }
    }
    None
}

/// Real registry read on Windows, `None` elsewhere.
pub fn detect() -> Option<LegacyInstall> {
    #[cfg(windows)]
    {
        use crate::winreg::{read_string, Hive};
        let exe = std::env::current_exe().ok();
        detect_with(
            |root, sub, val| {
                read_string(
                    match root {
                        Root::CurrentUser => Hive::CurrentUser,
                        Root::LocalMachine => Hive::LocalMachine,
                    },
                    sub,
                    val,
                )
            },
            exe.as_deref(),
        )
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    type Reg = HashMap<(Root, String, String), String>;

    fn put(r: &mut Reg, root: Root, key: &str, name: Option<&str>, dir: Option<&str>) {
        let sub = format!(r"{UNINSTALL_ROOT}\{key}");
        if let Some(n) = name {
            r.insert((root, sub.clone(), "DisplayName".into()), n.into());
        }
        if let Some(d) = dir {
            r.insert((root, sub.clone(), "InstallLocation".into()), d.into());
        }
        r.insert((root, sub, "UninstallString".into()), r#""x\Uninstall PatchOpsIII.exe" /currentuser"#.into());
    }

    fn run(r: &Reg, exe: Option<&str>) -> Option<LegacyInstall> {
        detect_with(|h, s, v| r.get(&(h, s.to_string(), v.to_string())).cloned(), exe.map(Path::new))
    }

    fn uuid5(name: &str, ns: &str) -> String {
        let mut data: Vec<u8> = (0..32).step_by(2).map(|i| u8::from_str_radix(&ns.replace('-', "")[i..i + 2], 16).unwrap()).collect();
        data.extend_from_slice(name.as_bytes());
        let d = ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, &data);
        let mut b: [u8; 16] = d.as_ref()[..16].try_into().unwrap();
        b[6] = (b[6] & 0x0f) | 0x50;
        b[8] = (b[8] & 0x3f) | 0x80;
        let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
        format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
    }

    #[test]
    fn guid_is_uuid5_of_app_id_under_electron_builder_namespace() {
        assert_eq!(uuid5("com.patchopsiii.app", "50e065bc-3134-11e6-9bab-38c9862bdaf3"), UNINSTALL_GUID);
    }

    #[test]
    fn none_when_empty() {
        assert_eq!(run(&Reg::new(), Some(r"C:\Apps\PatchOpsIII.exe")), None);
    }

    #[test]
    fn user_only() {
        let mut r = Reg::new();
        put(&mut r, Root::CurrentUser, UNINSTALL_GUID, Some("PatchOpsIII 1.3.4"), Some(r"C:\Users\a\AppData\Local\Programs\PatchOpsIII"));
        let l = run(&r, Some(r"D:\portable\PatchOpsIII.exe")).unwrap();
        assert_eq!(l.scope, Scope::User);
        assert_eq!(l.display_name.as_deref(), Some("PatchOpsIII 1.3.4"));
        assert_eq!(l.install_dir, Some(PathBuf::from(r"C:\Users\a\AppData\Local\Programs\PatchOpsIII")));
    }

    #[test]
    fn machine_only_braced_key() {
        let mut r = Reg::new();
        put(&mut r, Root::LocalMachine, &format!("{{{UNINSTALL_GUID}}}"), Some("PatchOpsIII 1.3.3"), None);
        let l = run(&r, None).unwrap();
        assert_eq!(l.scope, Scope::Machine);
        assert_eq!(l.install_dir, None);
    }

    #[test]
    fn both_prefers_user() {
        let mut r = Reg::new();
        put(&mut r, Root::LocalMachine, UNINSTALL_GUID, Some("m"), None);
        put(&mut r, Root::CurrentUser, UNINSTALL_GUID, Some("u"), None);
        assert_eq!(run(&r, None).unwrap().scope, Scope::User);
    }

    #[test]
    fn missing_optional_values() {
        let mut r = Reg::new();
        put(&mut r, Root::CurrentUser, UNINSTALL_GUID, None, None);
        let l = run(&r, None).unwrap();
        assert_eq!((l.display_name, l.install_dir), (None, None));
        // Blank values count as absent.
        let mut r = Reg::new();
        r.insert((Root::CurrentUser, format!(r"{UNINSTALL_ROOT}\{UNINSTALL_GUID}"), "DisplayName".into()), "  ".into());
        assert_eq!(run(&r, None), None);
    }

    #[test]
    fn own_exe_inside_install_dir_is_ignored_case_insensitively() {
        let mut r = Reg::new();
        put(&mut r, Root::LocalMachine, UNINSTALL_GUID, Some("PatchOpsIII 1.3.4"), Some(r"C:\Program Files\PatchOpsIII"));
        assert_eq!(run(&r, Some(r"c:\program files\patchopsiii\PatchOpsIII.exe")), None);
        // A sibling folder with the same prefix is not "inside".
        assert!(run(&r, Some(r"C:\Program Files\PatchOpsIII-portable\PatchOpsIII.exe")).is_some());
        // A separate per-user entry is still reported (HKCU is checked first, and the HKLM entry is skipped).
        put(&mut r, Root::CurrentUser, UNINSTALL_GUID, Some("u"), Some(r"C:\Users\a\AppData\Local\Programs\PatchOpsIII"));
        assert_eq!(run(&r, Some(r"C:\Program Files\PatchOpsIII\PatchOpsIII.exe")).unwrap().scope, Scope::User);
    }

    #[test]
    fn non_windows_detect_is_none() {
        #[cfg(not(windows))]
        assert_eq!(detect(), None);
    }
}
