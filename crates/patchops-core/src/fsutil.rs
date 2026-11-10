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

/// Write `bytes` to `path` atomically: temp file in the same directory, fsync, copy the
/// existing file's permission bits (a new file gets the normal create mode), rename over.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let existing = fs::metadata(path).ok().filter(|m| m.is_file());
    let tmp = tmp_sibling(path);
    let res = (|| {
        let mut f = fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(bytes)?;
        f.flush()?;
        if let Some(m) = &existing {
            fs::set_permissions(&tmp, m.permissions())?;
        }
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)
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

/// Copy `src` to `dst.tmp-…` (copy2 semantics) and rename over `dst`.
pub fn atomic_copy(src: &Path, dst: &Path) -> io::Result<()> {
    let tmp = tmp_sibling(dst);
    let res = copy2(src, &tmp).and_then(|_| fs::rename(&tmp, dst));
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
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
}
