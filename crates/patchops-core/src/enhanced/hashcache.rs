//! SHA-256 of large game files with a process-wide cache.
//!
//! `status` hashes `BlackOps3.exe` (100+ MB) on every call; the legacy Python re-hashes each
//! time. Here the digest is remembered per path and revalidated against a cheap signature
//! (size, mtime, and on unix inode + ctime), so an unchanged exe costs one `stat`.
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
}

fn signature(path: &Path) -> Option<Sig> {
    let m = std::fs::metadata(path).ok()?;
    if !m.is_file() {
        return None;
    }
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    Some(Sig {
        len: m.len(),
        mtime_ns: m.modified().ok()?.duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0),
        #[cfg(unix)]
        ino: m.ino(),
        #[cfg(unix)]
        ctime: (m.ctime(), m.ctime_nsec()),
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
}
