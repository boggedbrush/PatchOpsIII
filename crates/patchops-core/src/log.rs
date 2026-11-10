//! Application log: `<app_data>/PatchOpsIII.log` (append-only), an in-memory ring of the last
//! 300 entries, and subscribers for live tailing in the GUI.
//!
//! File line format (legacy `utils.write_log`): `YYYY-MM-DD HH:MM:SS - <Category>: <message>`.

use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

pub const RING_CAPACITY: usize = 300;
/// Entries embedded in `State.logs`.
pub const STATE_TAIL: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Info,
    Success,
    Warning,
    Error,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Info => "Info",
            Category::Success => "Success",
            Category::Warning => "Warning",
            Category::Error => "Error",
        }
    }
}

/// One log entry as exposed to the UI / `state.logs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    /// Plain message (no timestamp/category).
    pub message: String,
    pub category: String,
    /// Full file line without trailing newline.
    pub line: String,
}

pub type Subscriber = Arc<dyn Fn(&LogEntry) + Send + Sync>;

/// Size bound of the app log. Once the file exceeds [`LOG_MAX_BYTES`] the newest
/// [`LOG_KEEP_BYTES`] (cut forward to a whole line) stay in `PatchOpsIII.log` and everything
/// older moves to `PatchOpsIII.log.1` (one generation, replaced on each trim, never part of the
/// copy-logs payload). 2 MiB is ~20k lines (many sessions of history, far more than a bug report
/// needs); keeping 512 KiB leaves ~1.5 MiB of headroom, so a trim happens at most once per ~1.5 MiB
/// of logging instead of on every append, and a freshly trimmed log still holds several sessions.
/// (Legacy's unused every-3rd-launch wipe was intentionally not adopted.)
pub const LOG_MAX_BYTES: u64 = 2 * 1024 * 1024;
pub const LOG_KEEP_BYTES: u64 = 512 * 1024;

/// File-side state shared (via `Arc`) between the [`Logger`] and lazy payload handles.
pub struct FileLog {
    path: PathBuf,
    inner: Mutex<FileInner>,
}

/// `(len, mtime)` of the log file.
type FileKey = (u64, Option<std::time::SystemTime>);

#[derive(Default)]
struct FileInner {
    /// Tracked log size; `None` until the first stat.
    size: Option<u64>,
    /// Do not trim before the file grows past this (set after a failed trim to avoid retrying on
    /// every append).
    trim_floor: u64,
    /// Formatted payload with the `(len, mtime)` of the file it was built from.
    payload: Option<(FileKey, Arc<str>)>,
}

fn rotated_path(path: &std::path::Path) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".1");
    PathBuf::from(s)
}

impl FileLog {
    fn lock(&self) -> std::sync::MutexGuard<'_, FileInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Trim when `len` exceeds the cap. Returns the resulting size. Any failure leaves the log
    /// untouched (no line is ever lost: the old content is written to `.1` before the log is
    /// replaced, and a failed replace, e.g. a Windows rename against an open handle, keeps the
    /// original file).
    fn trim_locked(&self, inner: &mut FileInner, len: u64) -> u64 {
        if len <= LOG_MAX_BYTES.max(inner.trim_floor) {
            return len;
        }
        let bytes = match std::fs::read(&self.path) {
            Ok(b) => b,
            Err(_) => {
                inner.trim_floor = len + 1024 * 1024;
                return len;
            }
        };
        let real = bytes.len() as u64;
        if real <= LOG_MAX_BYTES {
            return real; // tracked size was stale (cleared / trimmed by someone else)
        }
        let cut = (real - LOG_KEEP_BYTES) as usize;
        // First line start at or after `cut`; no newline at all in the tail (one giant line)
        // retains nothing: the whole content goes to `.1`.
        let start = bytes[cut - 1..].iter().position(|&b| b == b'\n').map_or(bytes.len(), |i| cut + i);
        let ok = crate::fsutil::atomic_write(&rotated_path(&self.path), &bytes[..start]).is_ok()
            && crate::fsutil::atomic_write(&self.path, &bytes[start..]).is_ok();
        inner.payload = None;
        if ok {
            inner.trim_floor = 0;
            (bytes.len() - start) as u64
        } else {
            inner.trim_floor = real + 1024 * 1024;
            real
        }
    }

    fn stat(&self) -> Option<FileKey> {
        std::fs::metadata(&self.path).ok().map(|m| (m.len(), m.modified().ok()))
    }

    /// Formatted payload for the current file contents; `make` receives the log text (lossy
    /// UTF-8, untrimmed) and is only called on a cache miss. One `stat` per call validates the
    /// cache against external writers (elevated child, second instance).
    pub fn payload(&self, make: impl FnOnce(&str) -> String) -> Arc<str> {
        let mut inner = self.lock();
        let mut key = self.stat();
        if let Some((len, _)) = key {
            if len > LOG_MAX_BYTES.max(inner.trim_floor) {
                let size = self.trim_locked(&mut inner, len);
                inner.size = Some(size);
                key = self.stat();
            }
        }
        if let (Some(k), Some((ck, text))) = (key, inner.payload.as_ref()) {
            if *ck == k {
                return text.clone();
            }
        }
        let body = std::fs::read(&self.path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        let text: Arc<str> = make(&body).into();
        inner.payload = key.map(|k| (k, text.clone()));
        text
    }
}

pub struct Logger {
    path: PathBuf,
    file: Arc<FileLog>,
    ring: Mutex<VecDeque<LogEntry>>,
    subscribers: Mutex<Vec<Subscriber>>,
}

impl Logger {
    pub fn new(path: PathBuf) -> Logger {
        let file = Arc::new(FileLog { path: path.clone(), inner: Mutex::new(FileInner::default()) });
        {
            // Start-up check: one stat, a trim only above the cap.
            let mut inner = file.lock();
            if let Some((len, _)) = file.stat() {
                let size = file.trim_locked(&mut inner, len);
                inner.size = Some(size);
            }
        }
        Logger { path, file, ring: Mutex::new(VecDeque::with_capacity(RING_CAPACITY)), subscribers: Mutex::new(Vec::new()) }
    }

    /// Shared file-side handle (payload cache), for lazy payload values.
    pub fn file(&self) -> Arc<FileLog> {
        self.file.clone()
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Write to the file, the ring and all subscribers (legacy `write_log(msg, cat, log_target)`).
    pub fn log(&self, category: Category, message: impl AsRef<str>) {
        let entry = self.append_file(category, message.as_ref());
        {
            let mut ring = self.ring.lock().unwrap_or_else(|e| e.into_inner());
            if ring.len() == RING_CAPACITY {
                ring.pop_front();
            }
            ring.push_back(entry.clone());
        }
        let subs: Vec<Subscriber> = self.subscribers.lock().unwrap_or_else(|e| e.into_inner()).clone();
        for s in subs {
            s(&entry);
        }
    }

    /// Write to the file only (legacy `write_log(msg, cat, None)`: not shown in the UI).
    pub fn log_file_only(&self, category: Category, message: impl AsRef<str>) {
        self.append_file(category, message.as_ref());
    }

    pub fn info(&self, m: impl AsRef<str>) {
        self.log(Category::Info, m)
    }
    pub fn success(&self, m: impl AsRef<str>) {
        self.log(Category::Success, m)
    }
    pub fn warning(&self, m: impl AsRef<str>) {
        self.log(Category::Warning, m)
    }
    pub fn error(&self, m: impl AsRef<str>) {
        self.log(Category::Error, m)
    }

    fn append_file(&self, category: Category, message: &str) -> LogEntry {
        let line = format!("{} - {}: {}", crate::clock::now().format(), category.as_str(), message);
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // Appends are serialised so lines never interleave and the size/cache bookkeeping is exact.
        let mut inner = self.file.lock();
        match std::fs::OpenOptions::new().create(true).append(true).open(&self.path) {
            Ok(mut f) => {
                let mut buf = Vec::with_capacity(line.len() + 1);
                buf.extend_from_slice(line.as_bytes());
                buf.push(b'\n');
                match f.write_all(&buf) {
                    Ok(()) => {
                        drop(f);
                        inner.payload = None;
                        let size = match inner.size {
                            Some(s) => s + buf.len() as u64,
                            None => self.file.stat().map_or(0, |(l, _)| l),
                        };
                        let size = self.file.trim_locked(&mut inner, size);
                        inner.size = Some(size);
                    }
                    Err(e) => {
                        inner.size = None;
                        eprintln!("Failed to write log entry to {}: {e}", self.path.display());
                    }
                }
            }
            Err(e) => eprintln!("Failed to write log entry to {}: {e}", self.path.display()),
        }
        LogEntry { message: message.to_string(), category: category.as_str().to_string(), line }
    }

    /// Truncate the log file (legacy `clear_log_file`); `false` on failure.
    pub fn clear_file(&self) -> bool {
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut inner = self.file.lock();
        inner.payload = None;
        inner.size = Some(0);
        inner.trim_floor = 0;
        match std::fs::File::create(&self.path) {
            Ok(_) => true,
            Err(e) => {
                eprintln!("Failed to clear log file at {}: {e}", self.path.display());
                false
            }
        }
    }

    pub fn clear_ring(&self) {
        self.ring.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    /// Last `n` entries, oldest first.
    pub fn recent(&self, n: usize) -> Vec<LogEntry> {
        let ring = self.ring.lock().unwrap_or_else(|e| e.into_inner());
        let skip = ring.len().saturating_sub(n);
        ring.iter().skip(skip).cloned().collect()
    }

    /// Register a callback invoked (on the logging thread) for every new ring entry.
    /// Marshal to the UI thread inside the callback; keep it fast.
    pub fn subscribe(&self, f: impl Fn(&LogEntry) + Send + Sync + 'static) {
        self.subscribers.lock().unwrap_or_else(|e| e.into_inner()).push(Arc::new(f));
    }

    /// Whole log file as text (lossy UTF-8), empty if unreadable.
    pub fn read_file(&self) -> String {
        std::fs::read(&self.path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("patchops-log-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn lines(n: usize, width: usize) -> String {
        (0..n).map(|i| format!("{i:08} {}\n", "x".repeat(width))).collect()
    }

    #[test]
    fn startup_trim_keeps_newest_whole_lines_and_rotates_rest() {
        let d = tmp("trim");
        let p = d.join("a.log");
        let text = lines(30_000, 90); // ~2.9 MB
        fs::write(&p, &text).unwrap();
        let log = Logger::new(p.clone());
        let kept = fs::read_to_string(&p).unwrap();
        let old = fs::read_to_string(rotated_path(&p)).unwrap();
        assert!(kept.len() as u64 <= LOG_KEEP_BYTES && kept.len() as u64 > LOG_KEEP_BYTES - 200);
        assert!(kept.starts_with("000"), "starts on a line start");
        assert!(kept.contains("00029999 "), "newest line retained");
        assert_eq!(format!("{old}{kept}"), text, "nothing lost, order kept");
        assert!(text.ends_with(&kept));
        drop(log);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn under_cap_is_a_strict_noop() {
        let d = tmp("noop");
        let p = d.join("a.log");
        fs::write(&p, lines(20_000, 90)).unwrap(); // ~1.9 MB
        let before = fs::metadata(&p).unwrap();
        let bytes = fs::read(&p).unwrap();
        let log = Logger::new(p.clone());
        let _ = log.file().payload(|b| b.to_string());
        let after = fs::metadata(&p).unwrap();
        assert_eq!(before.modified().unwrap(), after.modified().unwrap());
        assert_eq!(bytes, fs::read(&p).unwrap());
        assert!(!rotated_path(&p).exists());
        assert_eq!(fs::read_dir(&d).unwrap().count(), 1);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn append_crossing_cap_trims_once() {
        let d = tmp("append");
        let p = d.join("a.log");
        let log = Logger::new(p.clone());
        let msg = "m".repeat(1000);
        let mut rotations = 0;
        let mut last_old = 0;
        for _ in 0..3000 {
            log.log_file_only(Category::Info, &msg);
            let r = fs::metadata(rotated_path(&p)).map(|m| m.len()).unwrap_or(0);
            if r != last_old {
                rotations += 1;
                last_old = r;
            }
        }
        assert_eq!(rotations, 1, "one trim for ~3 MB of appends");
        let kept = fs::metadata(&p).unwrap().len();
        assert!(kept <= LOG_MAX_BYTES && kept > 0);
        let all = fs::read_to_string(rotated_path(&p)).unwrap() + &fs::read_to_string(&p).unwrap();
        assert_eq!(all.lines().count(), 3000);
        assert!(all.lines().all(|l| l.ends_with(&msg)));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn giant_single_line_and_invalid_utf8() {
        let d = tmp("giant");
        let p = d.join("a.log");
        let mut data = vec![0xffu8; 3 * 1024 * 1024];
        data.extend_from_slice(b"\nlast line\n");
        fs::write(&p, &data).unwrap();
        let log = Logger::new(p.clone());
        assert_eq!(fs::read_to_string(&p).unwrap(), "last line\n");
        assert_eq!(fs::read(rotated_path(&p)).unwrap().len(), 3 * 1024 * 1024 + 1);
        // one giant line and no newline: whole content rotates, log stays usable
        let mut one = vec![b'a'; 3 * 1024 * 1024];
        one.push(0xfe);
        fs::write(&p, &one).unwrap();
        let log2 = Logger::new(p.clone());
        assert_eq!(fs::metadata(&p).unwrap().len(), 0);
        log2.log_file_only(Category::Info, "after");
        assert!(fs::read_to_string(&p).unwrap().ends_with("Info: after\n"));
        drop(log);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_and_unwritable_do_not_panic() {
        let d = tmp("missing");
        let log = Logger::new(d.join("nope").join("a.log"));
        assert_eq!(log.read_file(), "");
        assert_eq!(&*log.file().payload(|b| format!("[{b}]")), "[]");
        // log path is a directory: append fails (eprintln only)
        let dirlog = d.join("dir.log");
        fs::create_dir(&dirlog).unwrap();
        let log = Logger::new(dirlog);
        log.log_file_only(Category::Error, "x");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn payload_cache_invalidates_on_append_clear_trim_and_external_change() {
        let d = tmp("cache");
        let p = d.join("a.log");
        let log = Logger::new(p.clone());
        let calls = std::cell::Cell::new(0);
        let get = |log: &Logger| {
            log.file().payload(|b| {
                calls.set(calls.get() + 1);
                b.to_string()
            })
        };
        log.log_file_only(Category::Info, "one");
        let a = get(&log);
        let b = get(&log);
        assert_eq!(calls.get(), 1, "second use is a cache hit");
        assert!(Arc::ptr_eq(&a, &b));
        log.log_file_only(Category::Info, "two");
        assert!(get(&log).contains("two"));
        assert_eq!(calls.get(), 2);
        // external writer
        let mut f = fs::OpenOptions::new().append(true).open(&p).unwrap();
        f.write_all(b"external\n").unwrap();
        drop(f);
        assert!(get(&log).contains("external"));
        assert_eq!(calls.get(), 3);
        assert!(log.clear_file());
        assert_eq!(&*get(&log), "");
        assert_eq!(calls.get(), 4);
        // trim invalidates and the payload is bounded
        fs::write(&p, lines(30_000, 90)).unwrap();
        let t = get(&log);
        assert!((t.len() as u64) <= LOG_KEEP_BYTES, "over-cap file is trimmed before the payload is built");
        assert!(rotated_path(&p).exists());
        let _ = fs::remove_dir_all(&d);
    }
}
