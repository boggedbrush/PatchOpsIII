//! Small filesystem helpers: atomic writes and `shutil.copy2`-style copies.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Sibling temp path `<dir>/.<name>.tmp-<pid>-<n>`.
fn tmp_sibling(path: &Path) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = format!(".{name}.tmp-{}-{n}", std::process::id());
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.join(tmp),
        _ => PathBuf::from(tmp),
    }
}

/// Follow `path` through symbolic links (final component only; parent directories are resolved by the OS).
/// A dangling link resolves to the path it points at, like `open(path, "w")` which creates the target.
/// More than 40 hops fail with `ELOOP` (unix) as the kernel would.
pub(crate) fn resolve_link_chain(path: &Path) -> io::Result<PathBuf> {
    let mut cur = path.to_path_buf();
    for _ in 0..40 {
        match fs::symlink_metadata(&cur) {
            Ok(m) if m.file_type().is_symlink() => {
                let target = fs::read_link(&cur)?;
                cur = if target.is_absolute() {
                    target
                } else {
                    match cur.parent() {
                        Some(p) => p.join(target),
                        None => target,
                    }
                };
            }
            _ => return Ok(cur),
        }
    }
    #[cfg(unix)]
    {
        Err(io::Error::from_raw_os_error(libc::ELOOP))
    }
    #[cfg(not(unix))]
    {
        Err(io::Error::new(io::ErrorKind::InvalidInput, "too many levels of symbolic links"))
    }
}

/// More than one directory entry names this file (unix only; elsewhere assumed 1).
fn has_other_hard_links(meta: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.nlink() > 1
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        false
    }
}

/// The failures after which an in-place write may still work: the directory refuses a new entry or
/// the rename (read-only/immutable parent, sticky bit) although the file itself is writable.
fn is_dir_permission_error(e: &io::Error) -> bool {
    #[cfg(unix)]
    if matches!(e.raw_os_error(), Some(libc::EACCES | libc::EPERM | libc::EROFS)) {
        return true;
    }
    e.kind() == io::ErrorKind::PermissionDenied
}

/// What Python's `open(path, "w")` does: truncate and write the existing file, fsync.
fn write_in_place(dest: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut f = fs::OpenOptions::new().write(true).open(dest)?;
    f.set_len(0)?;
    f.write_all(bytes)?;
    f.flush()?;
    f.sync_all()
}

/// Write `bytes` to `path` with the semantics of the legacy `open(path, "w")` but crash-safe where possible.
///
/// * The destination is resolved through symbolic links first (a dangling link resolves to its
///   target, which is created, as `open` does), so a symlinked `config.ini` keeps being a symlink to
///   the updated file.
/// * An existing file must be writable by the caller (`[Errno 13] Permission denied` otherwise and
///   the file stays untouched), exactly as the legacy in-place open failed.
/// * Normally: temp file next to the real file, fsync, copy the existing permission bits (a new file
///   gets the normal create mode), rename over.
/// * Falls back to truncate-and-write of the existing file (fsync after), as legacy always did,
///   when the file has other hard links (a rename would detach them) or when the directory refuses
///   the temp file / the rename (EACCES, EPERM, EROFS: read-only parent directory).
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dest = resolve_link_chain(path)?;
    let Some(meta) = fs::metadata(&dest).ok().filter(|m| m.is_file()) else {
        return write_via_temp(&dest, bytes, None);
    };
    // The legacy in-place open: fails for a read-only file before anything is touched. The probe
    // handle is closed again (a Windows rename cannot replace a file that is open).
    drop(fs::OpenOptions::new().write(true).open(&dest)?);
    if has_other_hard_links(&meta) {
        return write_in_place(&dest, bytes);
    }
    match write_via_temp(&dest, bytes, Some(&meta)) {
        Err(e) if is_dir_permission_error(&e) => write_in_place(&dest, bytes),
        r => r,
    }
}

fn write_via_temp(dest: &Path, bytes: &[u8], existing: Option<&fs::Metadata>) -> io::Result<()> {
    let tmp = tmp_sibling(dest);
    let res = (|| {
        let mut f = fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(bytes)?;
        f.flush()?;
        if let Some(m) = existing {
            fs::set_permissions(&tmp, m.permissions())?;
        }
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, dest)
    })();
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res
}

/// Copy contents, permissions and mtime (like Python `shutil.copy2`).
pub fn copy2(src: &Path, dst: &Path) -> io::Result<()> {
    fs::copy(src, dst)?; // copies contents + permission bits
    let mtime = fs::metadata(src)?.modified()?;
    let f = fs::OpenOptions::new().write(true).open(dst)?;
    f.set_modified(mtime)?;
    Ok(())
}

/// Copy `src` over `dst` with `shutil.copy2` semantics, crash-safe where possible: a symlinked `dst`
/// is followed (legacy wrote through the link into the target, which stays a link); a file with
/// other hard links, or a directory that refuses the temp file / the rename, is overwritten in place
/// (what legacy did); otherwise `dst.tmp-…` is prepared and renamed over.
pub fn atomic_copy(src: &Path, dst: &Path) -> io::Result<()> {
    let dest = resolve_link_chain(dst)?;
    let meta = fs::metadata(&dest).ok().filter(|m| m.is_file());
    if meta.as_ref().is_some_and(has_other_hard_links) {
        return copy2(src, &dest);
    }
    let tmp = tmp_sibling(&dest);
    let res = copy2(src, &tmp).and_then(|_| fs::rename(&tmp, &dest));
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
        if meta.is_some() && res.as_ref().err().is_some_and(is_dir_permission_error) {
            return copy2(src, &dest);
        }
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("patchops-fsutil-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn atomic_write_creates_and_replaces() {
        let d = dir("aw");
        let p = d.join("f.txt");
        atomic_write(&p, b"one").unwrap();
        atomic_write(&p, b"two").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        assert_eq!(fs::read_dir(&d).unwrap().count(), 1, "no temp left behind");
        let _ = fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_preserves_mode() {
        use std::os::unix::fs::PermissionsExt;
        let d = dir("mode");
        let p = d.join("f");
        fs::write(&p, b"x").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o751)).unwrap();
        atomic_write(&p, b"y").unwrap();
        assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o7777, 0o751);
        let _ = fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_new_file_has_plain_create_mode() {
        use std::os::unix::fs::PermissionsExt;
        let d = dir("newmode");
        let a = d.join("a");
        let b = d.join("b");
        atomic_write(&a, b"x").unwrap();
        fs::write(&b, b"x").unwrap();
        assert_eq!(
            fs::metadata(&a).unwrap().permissions().mode(),
            fs::metadata(&b).unwrap().permissions().mode()
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn atomic_write_failure_cleans_temp() {
        let d = dir("fail");
        let p = d.join("target");
        fs::create_dir(&p).unwrap(); // rename file over a directory fails
        assert!(atomic_write(&p, b"x").is_err());
        assert_eq!(fs::read_dir(&d).unwrap().count(), 1);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn copy2_keeps_mtime_and_atomic_copy_replaces() {
        let d = dir("copy");
        let s = d.join("s");
        let t = d.join("t");
        fs::write(&s, b"data").unwrap();
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
        fs::OpenOptions::new().write(true).open(&s).unwrap().set_modified(old).unwrap();
        copy2(&s, &t).unwrap();
        assert_eq!(fs::metadata(&t).unwrap().modified().unwrap(), old);
        fs::write(&t, b"zzz").unwrap();
        atomic_copy(&s, &t).unwrap();
        assert_eq!(fs::read(&t).unwrap(), b"data");
        assert_eq!(fs::read_dir(&d).unwrap().count(), 2);
        let _ = fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    fn is_root() -> bool {
        // SAFETY: geteuid has no preconditions and cannot fail.
        unsafe { libc::geteuid() == 0 }
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_follows_relative_symlink_and_keeps_it() {
        use std::os::unix::fs::symlink;
        let d = dir("symrel");
        fs::write(d.join("real.ini"), b"old").unwrap();
        symlink("real.ini", d.join("link.ini")).unwrap();
        atomic_write(&d.join("link.ini"), b"new").unwrap();
        assert!(fs::symlink_metadata(d.join("link.ini")).unwrap().file_type().is_symlink());
        assert_eq!(fs::read(d.join("real.ini")).unwrap(), b"new");
        assert_eq!(fs::read(d.join("link.ini")).unwrap(), b"new");
        assert_eq!(fs::read_dir(&d).unwrap().count(), 2, "no temp left behind");
        let _ = fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_follows_absolute_symlink_into_another_directory() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let d = dir("symabs");
        fs::create_dir(d.join("dotfiles")).unwrap();
        fs::create_dir(d.join("game")).unwrap();
        let real = d.join("dotfiles/config.ini");
        fs::write(&real, b"old").unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o640)).unwrap();
        symlink(&real, d.join("game/config.ini")).unwrap();
        atomic_write(&d.join("game/config.ini"), b"new").unwrap();
        assert!(fs::symlink_metadata(d.join("game/config.ini")).unwrap().file_type().is_symlink());
        assert_eq!(fs::read(&real).unwrap(), b"new");
        assert_eq!(fs::metadata(&real).unwrap().permissions().mode() & 0o7777, 0o640);
        assert_eq!(fs::read_dir(d.join("dotfiles")).unwrap().count(), 1, "temp is created next to the real file and renamed");
        assert_eq!(fs::read_dir(d.join("game")).unwrap().count(), 1);
        let _ = fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_follows_symlink_chains_and_fails_on_loops() {
        use std::os::unix::fs::symlink;
        let d = dir("symchain");
        fs::write(d.join("real"), b"old").unwrap();
        symlink("real", d.join("b")).unwrap();
        symlink("b", d.join("a")).unwrap();
        atomic_write(&d.join("a"), b"new").unwrap();
        assert_eq!(fs::read(d.join("real")).unwrap(), b"new");
        symlink("loop2", d.join("loop1")).unwrap();
        symlink("loop1", d.join("loop2")).unwrap();
        let e = atomic_write(&d.join("loop1"), b"x").unwrap_err();
        assert_eq!(e.raw_os_error(), Some(libc::ELOOP));
        let _ = fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_keeps_hard_links_in_sync() {
        let d = dir("hard");
        fs::write(d.join("a"), b"old").unwrap();
        fs::hard_link(d.join("a"), d.join("b")).unwrap();
        atomic_write(&d.join("a"), b"newer content").unwrap();
        assert_eq!(fs::read(d.join("a")).unwrap(), b"newer content");
        assert_eq!(fs::read(d.join("b")).unwrap(), b"newer content", "the other name sees the new content");
        use std::os::unix::fs::MetadataExt;
        assert_eq!(fs::metadata(d.join("a")).unwrap().ino(), fs::metadata(d.join("b")).unwrap().ino(), "still one inode");
        // shrinking must truncate in place too
        atomic_write(&d.join("b"), b"s").unwrap();
        assert_eq!(fs::read(d.join("a")).unwrap(), b"s");
        assert_eq!(fs::read_dir(&d).unwrap().count(), 2);
        let _ = fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_in_read_only_parent_dir_writes_in_place() {
        use std::os::unix::fs::PermissionsExt;
        if is_root() {
            return; // root ignores directory permissions: nothing to observe
        }
        let d = dir("rodir");
        let p = d.join("config.ini");
        fs::write(&p, b"old").unwrap();
        fs::set_permissions(&d, fs::Permissions::from_mode(0o555)).unwrap();
        let r = atomic_write(&p, b"new");
        fs::set_permissions(&d, fs::Permissions::from_mode(0o755)).unwrap();
        r.unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"new");
        assert_eq!(fs::read_dir(&d).unwrap().count(), 1);
        // a missing file in a read-only directory still fails (nothing to write in place)
        fs::set_permissions(&d, fs::Permissions::from_mode(0o555)).unwrap();
        let r = atomic_write(&d.join("missing"), b"x");
        fs::set_permissions(&d, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(r.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        let _ = fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_read_only_file_is_permission_denied_and_untouched() {
        use std::os::unix::fs::PermissionsExt;
        if is_root() {
            return;
        }
        let d = dir("rofile");
        let p = d.join("f");
        fs::write(&p, b"old").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o444)).unwrap();
        let e = atomic_write(&p, b"new").unwrap_err();
        assert_eq!(e.raw_os_error(), Some(libc::EACCES));
        assert_eq!(fs::read(&p).unwrap(), b"old");
        assert_eq!(fs::read_dir(&d).unwrap().count(), 1);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn atomic_write_missing_file_is_created_and_missing_parent_fails() {
        let d = dir("missing");
        atomic_write(&d.join("new.txt"), b"x").unwrap();
        assert_eq!(fs::read(d.join("new.txt")).unwrap(), b"x");
        let e = atomic_write(&d.join("nodir/new.txt"), b"x").unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::NotFound);
        let _ = fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_dangling_symlink_creates_the_target_like_open_w() {
        use std::os::unix::fs::symlink;
        let d = dir("dangling");
        symlink("target.ini", d.join("link.ini")).unwrap();
        atomic_write(&d.join("link.ini"), b"fresh").unwrap();
        assert!(fs::symlink_metadata(d.join("link.ini")).unwrap().file_type().is_symlink());
        assert_eq!(fs::read(d.join("target.ini")).unwrap(), b"fresh");
        // dangling into a missing directory: ENOENT, the link is left alone
        symlink("nodir/t.ini", d.join("bad.ini")).unwrap();
        assert_eq!(atomic_write(&d.join("bad.ini"), b"x").unwrap_err().kind(), io::ErrorKind::NotFound);
        assert!(fs::symlink_metadata(d.join("bad.ini")).unwrap().file_type().is_symlink());
        let _ = fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_copy_writes_through_symlink_and_hard_links() {
        use std::os::unix::fs::symlink;
        let d = dir("copylink");
        fs::write(d.join("src.exe"), b"NEWEXE").unwrap();
        fs::write(d.join("real.exe"), b"old").unwrap();
        symlink("real.exe", d.join("BlackOps3.exe")).unwrap();
        atomic_copy(&d.join("src.exe"), &d.join("BlackOps3.exe")).unwrap();
        assert!(fs::symlink_metadata(d.join("BlackOps3.exe")).unwrap().file_type().is_symlink());
        assert_eq!(fs::read(d.join("real.exe")).unwrap(), b"NEWEXE");
        fs::write(d.join("h1.exe"), b"old").unwrap();
        fs::hard_link(d.join("h1.exe"), d.join("h2.exe")).unwrap();
        atomic_copy(&d.join("src.exe"), &d.join("h1.exe")).unwrap();
        assert_eq!(fs::read(d.join("h2.exe")).unwrap(), b"NEWEXE");
        assert_eq!(fs::read_dir(&d).unwrap().count(), 5, "no temp left behind");
        let _ = fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_copy_in_read_only_dir_writes_in_place() {
        use std::os::unix::fs::PermissionsExt;
        if is_root() {
            return;
        }
        let d = dir("copyrodir");
        fs::write(d.join("src"), b"NEW").unwrap();
        fs::create_dir(d.join("game")).unwrap();
        fs::write(d.join("game/x.exe"), b"old").unwrap();
        fs::set_permissions(d.join("game"), fs::Permissions::from_mode(0o555)).unwrap();
        let r = atomic_copy(&d.join("src"), &d.join("game/x.exe"));
        fs::set_permissions(d.join("game"), fs::Permissions::from_mode(0o755)).unwrap();
        r.unwrap();
        assert_eq!(fs::read(d.join("game/x.exe")).unwrap(), b"NEW");
        let _ = fs::remove_dir_all(&d);
    }
}
