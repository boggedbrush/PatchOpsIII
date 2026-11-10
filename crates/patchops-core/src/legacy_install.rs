//! Detection of a leftover install made by the old Electron-era NSIS installer.
//!
//! Releases 1.3.0-1.3.4 shipped a per-machine MSI *and* an NSIS installer named `PatchOpsIII.exe`
//! (electron-builder, `oneClick: false`, `perMachine: false`: per-user at
//! `%LOCALAPPDATA%\Programs\PatchOpsIII` by default, optionally all-users at
//! `%ProgramFiles%\PatchOpsIII`). Someone who installed that and now installs the new MSI ends up
//! with two installs. The app only *tells* the user (it never runs an uninstaller itself).
//!
//! What the NSIS installer writes (electron-builder 26.8.1, `templates/nsis/include/installer.nsh`
//! `registryAddInstallInfo`, `multiUser.nsh`), in HKCU (per-user) or HKLM (all-users), 64-bit view:
//!
//! * `Software\Microsoft\Windows\CurrentVersion\Uninstall\<GUID>` ([`UNINSTALL_GUID`], the uuid5 of the
//!   app id `com.patchopsiii.app`, no braces): `DisplayName`, `UninstallString`
//!   (`"<dir>\Uninstall PatchOpsIII.exe" /currentuser|/allusers`), `QuietUninstallString`,
//!   `DisplayVersion`, `DisplayIcon`, `Publisher`, `EstimatedSize`. **No `InstallLocation`.**
//! * `Software\<GUID>` (`INSTALL_REGISTRY_KEY`): `InstallLocation` (= `$INSTDIR`), `KeepShortcuts`,
//!   `ShortcutName`, `MenuDirectory`.
//!
//! This is deliberately outside `State`, op responses and the log: it only feeds a GUI notice.
//!
//! # When the notice is shown
//!
//! The notice tells the user to run the old uninstaller, which does `RMDir /r $INSTDIR` and removes
//! its shortcuts. That must never hit the new app, so an entry is reported **only when all of**
//! these hold, and every doubtful case (unknown or unresolvable directory, canonicalisation
//! failure, shared folder) is silence:
//!
//! 1. the NSIS install directory is known: `InstallLocation` from `Software\<GUID>` of the same
//!    hive, else the directory of the quoted executable in `UninstallString`;
//! 2. that directory still holds `Uninstall PatchOpsIII.exe` (a stale key whose files are gone is not
//!    worth a notice);
//! 3. the running executable is not inside it (portable copy or the new install living there);
//! 4. the new MSI's install directory is not that directory and not inside it. The MSI directory is
//!    `HKLM\Software\PatchOpsIII` `InstallLocation` (written by the MSI) when that value is present
//!    and non-empty, and **only** that (see [`msi_candidates`]); the `%ProgramW6432%` /
//!    `%ProgramFiles%` `\PatchOpsIII` defaults are used only when the value is absent (an older MSI,
//!    or no MSI at all). So a new MSI in a custom folder no longer hides an all-users NSIS install
//!    in Program Files, which is then a separate install worth reporting.
//!
//! The common trap: an all-users NSIS install is in `%ProgramFiles%\PatchOpsIII`, exactly where the
//! MSI installs. The MSI overwrites the NSIS files and the HKLM NSIS entry is merely stale; running
//! its uninstaller would delete the new app and its all-users shortcuts. The app shows nothing there
//! (cosmetic: one extra entry in Settings -> Apps).
//!
//! Known, accepted false negative: an all-users NSIS install in Program Files plus a portable-only
//! user (the new MSI is not installed, so no `InstallLocation`) is indistinguishable, without
//! calling the Windows Installer API, from the trap above (NSIS install overwritten by the MSI),
//! so it stays silent. That errs in the safe direction: a missed notice, never an uninstaller
//! pointed at the new app. No MSI-API FFI is used on purpose.

use std::path::{Path, PathBuf};

/// uuid5(`com.patchopsiii.app`, electron-builder namespace `50e065bc-3134-11e6-9bab-38c9862bdaf3`).
pub const UNINSTALL_GUID: &str = "a0f3b457-ab4b-5b70-97b3-7be0d5217bca";

const UNINSTALL_ROOT: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
/// `INSTALL_REGISTRY_KEY` parent: `Software\<GUID>`.
const INSTALL_ROOT: &str = r"Software";
/// File name of the NSIS uninstaller (`UNINSTALL_FILENAME` = `Uninstall ${PRODUCT_FILENAME}.exe`).
const UNINSTALLER_NAME: &str = "Uninstall PatchOpsIII.exe";
/// Key the new MSI writes its install folder to (`HKLM`, value `InstallLocation`; see the `.wxs`).
pub const MSI_KEY: &str = r"Software\PatchOpsIII";

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

/// Everything besides the registry that the decision depends on (injected so it is testable).
pub struct Env<'a> {
    /// The running executable.
    pub exe: Option<&'a Path>,
    /// Candidate install directories of the new MSI (registered one and the Program Files default).
    pub msi_dirs: &'a [PathBuf],
    /// Is this an existing regular file?
    pub is_file: &'a dyn Fn(&Path) -> bool,
    /// Canonical form of an existing path (`None` = cannot be resolved).
    pub canon: &'a dyn Fn(&Path) -> Option<PathBuf>,
}

/// Lower-cased, backslash-separated, no trailing separator, no `\\?\` prefix.
fn normalize(p: &Path) -> String {
    let s = p.to_string_lossy().replace('/', "\\").to_lowercase();
    let s = if let Some(r) = s.strip_prefix(r"\\?\unc\") {
        format!(r"\\{r}")
    } else {
        s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s)
    };
    s.trim_end_matches('\\').to_string()
}

/// `a` equals `b` or lies inside it (both normalised).
fn within(a: &str, b: &str) -> bool {
    !b.is_empty() && (a == b || a.starts_with(&format!("{b}\\")))
}

/// Directory of the executable named by an `UninstallString`: `"<dir>\Uninstall X.exe" /switch`
/// (quoted) or `<dir>\Uninstall X.exe /switch` (unquoted, up to the first `.exe`).
fn dir_from_uninstall_string(s: &str) -> Option<String> {
    let s = s.trim();
    let exe = if let Some(rest) = s.strip_prefix('"') {
        rest.split_once('"')?.0.trim()
    } else {
        let i = s.to_ascii_lowercase().find(".exe")?;
        s[..i + 4].trim()
    };
    let (dir, file) = exe.rsplit_once(['\\', '/'])?;
    let dir = dir.trim();
    // Absolute only: `X:\...` or `\\server\share`.
    let b = dir.as_bytes();
    let absolute = (b.len() >= 3 && b[1] == b':' && b[0].is_ascii_alphabetic()) || dir.starts_with(r"\\");
    (absolute && !file.is_empty()).then(|| dir.to_string())
}

fn join_win(dir: &str, name: &str) -> PathBuf {
    PathBuf::from(format!("{}\\{name}", dir.trim_end_matches(['\\', '/'])))
}

/// Is it safe to tell the user to run the old uninstaller for the NSIS install in `dir`? See the
/// module docs; `false` whenever in doubt.
fn safe_to_report(dir: &str, env: &Env) -> bool {
    let nsis = Path::new(dir);
    if !(env.is_file)(&join_win(dir, UNINSTALLER_NAME)) {
        return false;
    }
    let Some(nsis) = (env.canon)(nsis).map(|c| normalize(&c)) else { return false };
    if nsis.is_empty() {
        return false;
    }
    // The running exe must resolve, and must not live in the NSIS folder.
    let Some(exe) = env.exe.and_then(|e| (env.canon)(e)).map(|c| normalize(&c)) else { return false };
    if within(&exe, &nsis) {
        return false;
    }
    // The new MSI's folder must not be (or sit inside) the NSIS folder. A candidate that does not
    // exist cannot be the NSIS folder (which exists), so compare it lexically.
    for m in env.msi_dirs {
        let m = (env.canon)(m).map(|c| normalize(&c)).unwrap_or_else(|| normalize(m));
        if within(&m, &nsis) {
            return false;
        }
    }
    true
}

/// Pure detection. `read(root, subkey, value)` reads a registry string.
pub fn detect_with(read: impl Fn(Root, &str, &str) -> Option<String>, env: &Env) -> Option<LegacyInstall> {
    for (root, scope) in [(Root::CurrentUser, Scope::User), (Root::LocalMachine, Scope::Machine)] {
        for key in [UNINSTALL_GUID.to_string(), format!("{{{UNINSTALL_GUID}}}")] {
            let get = |sub: &str, v: &str| read(root, sub, v).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
            let sub = format!(r"{UNINSTALL_ROOT}\{key}");
            let (name, uninst) = (get(&sub, "DisplayName"), get(&sub, "UninstallString"));
            // No identifying value at all: the key is absent (or not ours).
            if name.is_none() && uninst.is_none() {
                continue;
            }
            let dir = get(&format!(r"{INSTALL_ROOT}\{key}"), "InstallLocation")
                .map(|d| d.trim_matches('"').trim().to_string())
                .filter(|d| !d.is_empty())
                .or_else(|| uninst.as_deref().and_then(dir_from_uninstall_string));
            // Unknown directory, or one we must not send the user to uninstall: say nothing about
            // this entry (the other hive may still hold a separate, safe one).
            let Some(dir) = dir.filter(|d| safe_to_report(d, env)) else { break };
            return Some(LegacyInstall { scope, display_name: name, install_dir: Some(PathBuf::from(dir)) });
        }
    }
    None
}

/// Directories the new MSI may be installed in: the registered `InstallLocation` alone when it is
/// present and non-empty, else the Program Files defaults (older MSIs, no MSI).
pub fn msi_candidates(registered: Option<&str>, defaults: &[PathBuf]) -> Vec<PathBuf> {
    match registered.map(|d| d.trim().trim_matches('"').trim()).filter(|d| !d.is_empty()) {
        Some(d) => vec![PathBuf::from(d)],
        None => defaults.to_vec(),
    }
}

/// Real registry / file system read on Windows, `None` elsewhere.
pub fn detect() -> Option<LegacyInstall> {
    #[cfg(windows)]
    {
        use crate::winreg::{read_string, Hive};
        let exe = std::env::current_exe().ok();
        let defaults: Vec<PathBuf> = ["ProgramW6432", "ProgramFiles"]
            .iter()
            .filter_map(|var| std::env::var_os(var))
            .map(|pf| Path::new(&pf).join("PatchOpsIII"))
            .collect();
        let msi_dirs = msi_candidates(read_string(Hive::LocalMachine, MSI_KEY, "InstallLocation").as_deref(), &defaults);
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
            &Env { exe: exe.as_deref(), msi_dirs: &msi_dirs, is_file: &|p| p.is_file(), canon: &|p| std::fs::canonicalize(p).ok() },
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
    use std::collections::{HashMap, HashSet};

    type Reg = HashMap<(Root, String, String), String>;

    const USER_DIR: &str = r"C:\Users\a\AppData\Local\Programs\PatchOpsIII";
    const PF_DIR: &str = r"C:\Program Files\PatchOpsIII";

    fn uninstaller(dir: &str) -> String {
        format!(r"{dir}\Uninstall PatchOpsIII.exe")
    }

    /// What electron-builder 26.8.1 writes: the uninstall key has no `InstallLocation`, which lives
    /// under `Software\<GUID>`.
    fn put(r: &mut Reg, root: Root, key: &str, name: Option<&str>, install_location: Option<&str>, uninstall_string: Option<&str>) {
        let sub = format!(r"{UNINSTALL_ROOT}\{key}");
        if let Some(n) = name {
            r.insert((root, sub.clone(), "DisplayName".into()), n.into());
        }
        if let Some(u) = uninstall_string {
            r.insert((root, sub.clone(), "UninstallString".into()), u.into());
        }
        if let Some(d) = install_location {
            r.insert((root, format!(r"Software\{key}"), "InstallLocation".into()), d.into());
        }
    }

    /// A complete, well-formed install in `dir`.
    fn put_install(r: &mut Reg, root: Root, dir: &str) {
        let sw = if root == Root::CurrentUser { "/currentuser" } else { "/allusers" };
        put(r, root, UNINSTALL_GUID, Some("PatchOpsIII 1.3.4"), Some(dir), Some(&format!(r#""{}" {sw}"#, uninstaller(dir))));
    }

    struct World {
        files: HashSet<PathBuf>,
        msi: Vec<PathBuf>,
    }

    impl World {
        fn new(files_in: &[&str]) -> World {
            World { files: files_in.iter().map(|d| PathBuf::from(uninstaller(d))).collect(), msi: vec![PathBuf::from(PF_DIR)] }
        }
    }

    fn run(r: &Reg, w: &World, exe: Option<&str>) -> Option<LegacyInstall> {
        let exe = exe.map(Path::new);
        detect_with(
            |h, s, v| r.get(&(h, s.to_string(), v.to_string())).cloned(),
            &Env { exe, msi_dirs: &w.msi, is_file: &|p| w.files.contains(p), canon: &|p| Some(p.to_path_buf()) },
        )
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
        assert_eq!(run(&Reg::new(), &World::new(&[]), Some(r"C:\Apps\PatchOpsIII.exe")), None);
    }

    #[test]
    fn per_user_install_with_files_is_reported() {
        let mut r = Reg::new();
        put_install(&mut r, Root::CurrentUser, USER_DIR);
        let l = run(&r, &World::new(&[USER_DIR]), Some(PF_DIR.to_string().as_str()).map(|d| format!(r"{d}\PatchOpsIII.exe")).as_deref()).unwrap();
        assert_eq!(l.scope, Scope::User);
        assert_eq!(l.display_name.as_deref(), Some("PatchOpsIII 1.3.4"));
        assert_eq!(l.install_dir, Some(PathBuf::from(USER_DIR)));
    }

    #[test]
    fn per_user_install_whose_files_are_gone_is_silent() {
        let mut r = Reg::new();
        put_install(&mut r, Root::CurrentUser, USER_DIR);
        assert_eq!(run(&r, &World::new(&[]), Some(r"D:\portable\PatchOpsIII.exe")), None);
    }

    #[test]
    fn all_users_entry_in_the_msi_folder_is_silent() {
        let mut r = Reg::new();
        put_install(&mut r, Root::LocalMachine, PF_DIR);
        let w = World::new(&[PF_DIR]);
        // Running the new MSI app, a portable copy elsewhere, or no exe at all: never a notice.
        assert_eq!(run(&r, &w, Some(r"C:\Program Files\PatchOpsIII\PatchOpsIII.exe")), None);
        assert_eq!(run(&r, &w, Some(r"D:\portable\PatchOpsIII.exe")), None);
        assert_eq!(run(&r, &w, None), None);
        // Case and trailing separators do not matter.
        let mut r2 = Reg::new();
        put_install(&mut r2, Root::LocalMachine, r"c:\PROGRAM FILES\patchopsiii\");
        let mut w2 = World::new(&[]);
        w2.files.insert(PathBuf::from(r"c:\PROGRAM FILES\patchopsiii\Uninstall PatchOpsIII.exe"));
        assert_eq!(run(&r2, &w2, Some(r"D:\x\PatchOpsIII.exe")), None);
    }

    #[test]
    fn msi_registered_in_a_custom_folder_is_never_the_nsis_folder() {
        // The MSI wizard lets the user pick a folder; its registered InstallLocation is checked too.
        let custom = r"D:\Games\Tools\PatchOpsIII";
        let mut r = Reg::new();
        put_install(&mut r, Root::LocalMachine, custom);
        let mut w = World::new(&[custom]);
        assert!(run(&r, &w, Some(r"D:\x\PatchOpsIII.exe")).is_some(), "no MSI there: reported");
        w.msi.push(PathBuf::from(custom));
        assert_eq!(run(&r, &w, Some(r"D:\x\PatchOpsIII.exe")), None);
        // An MSI folder *inside* the NSIS folder would be deleted with it.
        w.msi = vec![PathBuf::from(format!(r"{custom}\app"))];
        assert_eq!(run(&r, &w, Some(r"D:\x\PatchOpsIII.exe")), None);
    }

    #[test]
    fn msi_candidates_use_the_registered_folder_alone() {
        let defaults = [PathBuf::from(PF_DIR), PathBuf::from(r"C:\Program Files (x86)\PatchOpsIII")];
        // Registered (quotes and blanks tolerated): only it.
        assert_eq!(msi_candidates(Some(r#" "D:\Apps\PatchOpsIII\" "#), &defaults), vec![PathBuf::from(r"D:\Apps\PatchOpsIII\")]);
        // Absent or empty: the defaults.
        assert_eq!(msi_candidates(None, &defaults), defaults.to_vec());
        assert_eq!(msi_candidates(Some("  "), &defaults), defaults.to_vec());
        assert_eq!(msi_candidates(Some(r#""""#), &defaults), defaults.to_vec());
    }

    #[test]
    fn case_d_nsis_in_program_files_with_the_msi_in_a_custom_folder_is_reported() {
        // The new MSI recorded D:\Apps\PatchOpsIII, so Program Files holds a genuinely separate NSIS install.
        let mut r = Reg::new();
        put_install(&mut r, Root::LocalMachine, PF_DIR);
        let mut w = World::new(&[PF_DIR]);
        w.msi = msi_candidates(Some(r"D:\Apps\PatchOpsIII"), &[PathBuf::from(PF_DIR)]);
        let l = run(&r, &w, Some(r"D:\Apps\PatchOpsIII\PatchOpsIII.exe")).unwrap();
        assert_eq!((l.scope, l.install_dir), (Scope::Machine, Some(PathBuf::from(PF_DIR))));
    }

    #[test]
    fn case_c_all_users_nsis_with_a_portable_exe_and_no_msi_stays_silent() {
        // Deliberate false negative (see the module docs): without an MSI InstallLocation this cannot
        // be told apart from an NSIS install the MSI overwrote, and silence is the safe side.
        let mut r = Reg::new();
        put_install(&mut r, Root::LocalMachine, PF_DIR);
        let mut w = World::new(&[PF_DIR]);
        w.msi = msi_candidates(None, &[PathBuf::from(PF_DIR)]);
        assert_eq!(run(&r, &w, Some(r"C:\Users\a\Downloads\PatchOpsIII.exe")), None);
    }

    #[test]
    fn all_users_entry_in_a_different_folder_is_reported() {
        let other = r"D:\Apps\PatchOpsIII-old";
        let mut r = Reg::new();
        put_install(&mut r, Root::LocalMachine, other);
        let l = run(&r, &World::new(&[other]), Some(r"C:\Program Files\PatchOpsIII\PatchOpsIII.exe")).unwrap();
        assert_eq!(l.scope, Scope::Machine);
        assert_eq!(l.install_dir, Some(PathBuf::from(other)));
        // A sibling with the same prefix as the MSI folder is not "the same folder".
        let sib = r"C:\Program Files\PatchOpsIII-old";
        let mut r = Reg::new();
        put_install(&mut r, Root::LocalMachine, sib);
        assert!(run(&r, &World::new(&[sib]), Some(r"D:\x\PatchOpsIII.exe")).is_some());
    }

    #[test]
    fn directory_from_uninstall_string_when_install_location_is_missing() {
        let mut r = Reg::new();
        put(&mut r, Root::CurrentUser, UNINSTALL_GUID, Some("PatchOpsIII"), None, Some(&format!(r#""{}" /currentuser"#, uninstaller(USER_DIR))));
        let w = World::new(&[USER_DIR]);
        assert_eq!(run(&r, &w, Some(r"D:\x\PatchOpsIII.exe")).unwrap().install_dir, Some(PathBuf::from(USER_DIR)));
        // Unquoted, with trailing switches.
        let mut r = Reg::new();
        put(&mut r, Root::CurrentUser, UNINSTALL_GUID, None, None, Some(&format!(r"{} /currentuser /S", uninstaller(USER_DIR))));
        assert_eq!(run(&r, &w, Some(r"D:\x\PatchOpsIII.exe")).unwrap().install_dir, Some(PathBuf::from(USER_DIR)));
        // InstallLocation wins over UninstallString, quotes around it are tolerated.
        let mut r = Reg::new();
        put(&mut r, Root::CurrentUser, UNINSTALL_GUID, Some("x"), Some(&format!("\"{USER_DIR}\"")), Some(r#""C:\elsewhere\Uninstall PatchOpsIII.exe" /currentuser"#));
        assert_eq!(run(&r, &w, Some(r"D:\x\PatchOpsIII.exe")).unwrap().install_dir, Some(PathBuf::from(USER_DIR)));
    }

    #[test]
    fn unparseable_or_missing_directory_is_silent() {
        let w = World::new(&[USER_DIR, ""]);
        for u in ["", "   ", r#""C:\unterminated\Uninstall PatchOpsIII.exe /currentuser"#, "Uninstall PatchOpsIII.exe", r#""Uninstall PatchOpsIII.exe" /x"#, r"C:\no\extension /x", r#""\Uninstall PatchOpsIII.exe""#] {
            let mut r = Reg::new();
            put(&mut r, Root::CurrentUser, UNINSTALL_GUID, Some("PatchOpsIII"), None, if u.is_empty() { None } else { Some(u) });
            assert_eq!(run(&r, &w, Some(r"D:\x\PatchOpsIII.exe")), None, "{u:?}");
        }
        // A DisplayName alone (no way to find the directory).
        let mut r = Reg::new();
        put(&mut r, Root::LocalMachine, &format!("{{{UNINSTALL_GUID}}}"), Some("PatchOpsIII 1.3.3"), None, None);
        assert_eq!(run(&r, &w, None), None);
    }

    #[test]
    fn running_exe_inside_the_nsis_folder_is_silent() {
        let mut r = Reg::new();
        put_install(&mut r, Root::CurrentUser, USER_DIR);
        let w = World::new(&[USER_DIR]);
        assert_eq!(run(&r, &w, Some(&format!(r"{USER_DIR}\PatchOpsIII.exe"))), None);
        assert_eq!(run(&r, &w, Some(&format!(r"{}\sub\PatchOpsIII.exe", USER_DIR.to_uppercase()))), None);
        // Unknown running exe: cannot prove anything.
        assert_eq!(run(&r, &w, None), None);
    }

    #[test]
    fn portable_exe_elsewhere_with_msi_in_the_nsis_folder_is_silent() {
        let mut r = Reg::new();
        put_install(&mut r, Root::LocalMachine, PF_DIR);
        assert_eq!(run(&r, &World::new(&[PF_DIR]), Some(r"E:\Downloads\PatchOpsIII.exe")), None);
    }

    #[test]
    fn unresolvable_paths_are_silent() {
        let mut r = Reg::new();
        put_install(&mut r, Root::CurrentUser, USER_DIR);
        let w = World::new(&[USER_DIR]);
        let read = |h: Root, s: &str, v: &str| r.get(&(h, s.to_string(), v.to_string())).cloned();
        let exe = Path::new(r"D:\x\PatchOpsIII.exe");
        let ok = |canon: &dyn Fn(&Path) -> Option<PathBuf>| {
            detect_with(read, &Env { exe: Some(exe), msi_dirs: &w.msi, is_file: &|p| w.files.contains(p), canon }).is_some()
        };
        assert!(ok(&|p| Some(p.to_path_buf())));
        assert!(!ok(&|_| None), "nothing resolves");
        assert!(!ok(&|p| (p != exe).then(|| p.to_path_buf())), "running exe does not resolve");
        assert!(!ok(&|p| (p != Path::new(USER_DIR)).then(|| p.to_path_buf())), "NSIS folder does not resolve");
        // An MSI candidate that does not exist is fine (compared lexically).
        assert!(ok(&|p| (p != Path::new(PF_DIR)).then(|| p.to_path_buf())));
        // `\\?\` prefixes of real canonicalisation are ignored.
        assert!(!detect_with(read, &Env { exe: Some(Path::new(r"\\?\C:\Users\a\AppData\Local\Programs\PatchOpsIII\PatchOpsIII.exe")), msi_dirs: &w.msi, is_file: &|p| w.files.contains(p), canon: &|p| Some(p.to_path_buf()) }).is_some());
    }

    #[test]
    fn both_hives_prefers_user_and_skips_a_stale_machine_entry() {
        let mut r = Reg::new();
        put_install(&mut r, Root::LocalMachine, PF_DIR);
        put_install(&mut r, Root::CurrentUser, USER_DIR);
        let w = World::new(&[PF_DIR, USER_DIR]);
        assert_eq!(run(&r, &w, Some(r"E:\x\PatchOpsIII.exe")).unwrap().scope, Scope::User);
        // Per-user files gone: the all-users entry is the MSI's folder, still silent.
        assert_eq!(run(&r, &World::new(&[PF_DIR]), Some(r"E:\x\PatchOpsIII.exe")), None);
        // A separate all-users install in another folder is reported when the per-user one is gone.
        let other = r"D:\Apps\Old";
        let mut r = Reg::new();
        put_install(&mut r, Root::LocalMachine, other);
        put_install(&mut r, Root::CurrentUser, USER_DIR);
        assert_eq!(run(&r, &World::new(&[other]), Some(r"E:\x\PatchOpsIII.exe")).unwrap().scope, Scope::Machine);
    }

    #[test]
    fn braced_key_is_probed() {
        let key = format!("{{{UNINSTALL_GUID}}}");
        let mut r = Reg::new();
        put(&mut r, Root::LocalMachine, &key, Some("PatchOpsIII 1.3.3"), Some(r"D:\Apps\Old"), Some(r#""D:\Apps\Old\Uninstall PatchOpsIII.exe" /allusers"#));
        assert_eq!(run(&r, &World::new(&[r"D:\Apps\Old"]), Some(r"E:\x\PatchOpsIII.exe")).unwrap().scope, Scope::Machine);
    }

    #[test]
    fn blank_values_count_as_absent() {
        let mut r = Reg::new();
        r.insert((Root::CurrentUser, format!(r"{UNINSTALL_ROOT}\{UNINSTALL_GUID}"), "DisplayName".into()), "  ".into());
        assert_eq!(run(&r, &World::new(&[]), None), None);
    }

    #[test]
    fn non_windows_detect_is_none() {
        #[cfg(not(windows))]
        assert_eq!(detect(), None);
    }
}
