//! SHA-256 of large game files with a process-wide cache.
//!
//! `status` hashes `BlackOps3.exe` (100+ MB) on every call; the legacy Python re-hashes each
//! time. Here the digest is remembered per path and revalidated against a cheap signature
//! (size, mtime, and inode + ctime on unix / creation time on Windows), so an unchanged exe
//! costs one `stat`. [`HashScope`] additionally memoises digests for the duration of one op.
//! Operations that decide about replacing files use [`file_sha256_fresh`] and call
//! [`invalidate`] after renames/copies.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

#[derive(Clone, PartialEq, Eq)]
struct Sig {
    len: u64,
    mtime_ns: u128,
    #[cfg(unix)]
    ino: u64,
    #[cfg(unix)]
    ctime: (i64, i64),
    /// Windows: creation time (a replacement file that kept size and mtime still gets a new one).
    #[cfg(windows)]
    created: u64,
}

fn signature(path: &Path) -> Option<Sig> {
    let m = std::fs::metadata(path).ok()?;
    if !m.is_file() {
        return None;
    }
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    #[cfg(windows)]
    use std::os::windows::fs::MetadataExt as _;
    Some(Sig {
        len: m.len(),
        mtime_ns: m.modified().ok()?.duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0),
        #[cfg(unix)]
        ino: m.ino(),
        #[cfg(unix)]
        ctime: (m.ctime(), m.ctime_nsec()),
        #[cfg(windows)]
        created: m.creation_time(),
    })
}

type Cache = Mutex<HashMap<PathBuf, (Sig, String)>>;

fn cache() -> &'static Cache {
    static C: OnceLock<Cache> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Lower-case hex digest, `None` when the file is missing/unreadable. Cached.
pub fn file_sha256_cached(path: &Path) -> Option<String> {
    let sig = signature(path)?;
    {
        let c = cache().lock().unwrap_or_else(|e| e.into_inner());
        if let Some((s, h)) = c.get(path) {
            if *s == sig {
                return Some(h.clone());
            }
        }
    }
    let h = crate::hash::file_sha256(path)?;
    // Only trust the digest if the file did not change while it was being read.
    if signature(path).as_ref() == Some(&sig) {
        cache().lock().unwrap_or_else(|e| e.into_inner()).insert(path.to_path_buf(), (sig, h.clone()));
    }
    Some(h)
}

/// Uncached digest (also refreshes the cache entry).
pub fn file_sha256_fresh(path: &Path) -> Option<String> {
    invalidate(path);
    file_sha256_cached(path)
}

/// Forget the remembered digest of `path`.
pub fn invalidate(path: &Path) {
    cache().lock().unwrap_or_else(|e| e.into_inner()).remove(path);
}

/// Per-operation memo in front of the global cache: the first lookup of a path goes through
/// [`file_sha256_cached`], later ones in the same scope are plain map hits (no `stat`, no
/// re-validation). Create one per op/`status` call and drop it afterwards; do not keep it across
/// file modifications (use [`file_sha256_fresh`] + [`invalidate`] around those).
#[derive(Default)]
pub struct HashScope {
    memo: std::cell::RefCell<HashMap<PathBuf, Option<String>>>,
}

impl HashScope {
    pub fn new() -> HashScope {
        HashScope::default()
    }

    /// Cached digest of `path` (`None`: missing/unreadable).
    pub fn sha256(&self, path: &Path) -> Option<String> {
        if let Some(v) = self.memo.borrow().get(path) {
            return v.clone();
        }
        let v = file_sha256_cached(path);
        self.memo.borrow_mut().insert(path.to_path_buf(), v.clone());
        v
    }

    /// Forget one path (after the op changed that file).
    pub fn forget(&self, path: &Path) {
        self.memo.borrow_mut().remove(path);
        invalidate(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caches_and_detects_change() {
        let dir = std::env::temp_dir().join(format!("patchops-hc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("f.bin");
        std::fs::write(&p, b"abc").unwrap();
        let h1 = file_sha256_cached(&p).unwrap();
        assert_eq!(h1, crate::hash::sha256_hex(b"abc"));
        assert_eq!(file_sha256_cached(&p).unwrap(), h1);
        std::fs::write(&p, b"abcd").unwrap();
        assert_eq!(file_sha256_cached(&p).unwrap(), crate::hash::sha256_hex(b"abcd"));
        std::fs::remove_file(&p).unwrap();
        assert!(file_sha256_cached(&p).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scope_memoises_within_scope_only() {
        let dir = std::env::temp_dir().join(format!("patchops-hcs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("f.bin");
        std::fs::write(&p, b"one").unwrap();
        let scope = HashScope::new();
        let h = scope.sha256(&p).unwrap();
        assert_eq!(h, crate::hash::sha256_hex(b"one"));
        std::fs::write(&p, b"two!").unwrap();
        assert_eq!(scope.sha256(&p).unwrap(), h, "memoised inside the scope");
        assert_eq!(HashScope::new().sha256(&p).unwrap(), crate::hash::sha256_hex(b"two!"));
        scope.forget(&p);
        assert_eq!(scope.sha256(&p).unwrap(), crate::hash::sha256_hex(b"two!"));
        assert!(scope.sha256(&dir.join("missing")).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn same_size_replacement_with_preserved_mtime_is_detected() {
        let dir = std::env::temp_dir().join(format!("patchops-hcr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("f.bin");
        std::fs::write(&p, b"aaaa").unwrap();
        let mtime = std::fs::metadata(&p).unwrap().modified().unwrap();
        let h1 = file_sha256_cached(&p).unwrap();
        // Replace via a new file (new inode / creation time), same size, same mtime.
        let q = dir.join("g.bin");
        std::fs::write(&q, b"bbbb").unwrap();
        std::fs::OpenOptions::new().write(true).open(&q).unwrap().set_modified(mtime).unwrap();
        std::fs::rename(&q, &p).unwrap();
        assert_ne!(file_sha256_cached(&p).unwrap(), h1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
