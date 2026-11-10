//! Application log: `<app_data>/PatchOpsIII.log` (append-only), an in-memory ring of the last
//! 300 entries, and subscribers for live tailing in the GUI.
//!
//! File line format (legacy `utils.write_log`): `YYYY-MM-DD HH:MM:SS - <Category>: <message>`.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
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
///
/// Concurrency: several processes may append to the same file (the GUI, an `--rpc-stdio` helper,
/// the elevated install child, the panic hook). Appends are `O_APPEND` single writes. A trim
/// (a) is exclusive between processes through a `<log>.trim` lock file (stale after 60 s),
/// (b) holds the old file open while it works and carries every line appended in the meantime into
/// the new log, so a line is never lost (a line written during the final rename may end up a few
/// lines out of order), and (c) replaces `.1` only after the new log is fully prepared and the log
/// was not truncated behind its back. The only kind of process that trims at start-up is the GUI
/// ([`Logger::trim_if_needed`], off the main thread); the hidden elevated child never trims
/// ([`Logger::disable_trim`]); the panic hook uses the append-only [`append_line`].
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
    /// Never trim (the elevated child, which only appends a few lines).
    trim_disabled: bool,
    /// Formatted payload with the `(len, mtime)` of the file it was built from.
    payload: Option<(FileKey, Arc<str>)>,
}

fn rotated_path(path: &std::path::Path) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".1");
    PathBuf::from(s)
}

/// Back-off after an abandoned trim (another trim in flight, log cleared mid-trim).
const RETRY_STEP: u64 = 64 * 1024;

enum Trim {
    /// The file was already at or below the cap (real size).
    NotNeeded(u64),
    /// Trimmed; size of the new log.
    Done(u64),
    /// Gave up without changing anything (current size); retry after some growth.
    Abandoned(u64),
    /// I/O failure (current size); retry after more growth.
    Failed(u64),
}

/// `<log>.trim`: exclusive between processes while a trim runs. Created with `create_new`; one
/// older than a minute is a leftover of a crashed trim and is taken over.
struct TrimLock(PathBuf);

impl TrimLock {
    fn acquire(log: &Path) -> Option<TrimLock> {
        let mut p = log.as_os_str().to_os_string();
        p.push(".trim");
        let p = PathBuf::from(p);
        for _ in 0..2 {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&p) {
                Ok(_) => return Some(TrimLock(p)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let stale = std::fs::metadata(&p)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|age| age > std::time::Duration::from_secs(60));
                    if !stale || std::fs::remove_file(&p).is_err() {
                        return None;
                    }
                }
                Err(_) => return None,
            }
        }
        None
    }
}

impl Drop for TrimLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

static TMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Write `bytes` to a fresh sibling temp of `dest`, with the permission bits of `like` (the log:
/// a 0600 log gives a 0600 `.1`, never briefly wider), fsynced. Returns the temp path.
fn write_like(dest: &Path, bytes: &[u8], like: &std::fs::Metadata) -> Option<PathBuf> {
    let n = TMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = dest.file_name()?.to_string_lossy().into_owned();
    let tmp = dest.with_file_name(format!(".{name}.tmp-{}-{n}", std::process::id()));
    let res = (|| {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            o.mode(like.permissions().mode() & 0o777);
        }
        let mut f = o.open(&tmp)?;
        f.write_all(bytes)?;
        f.flush()?;
        // After writing (a Windows read-only file could not be written); also exact bits despite the umask.
        std::fs::set_permissions(&tmp, like.permissions())?;
        f.sync_all()
    })();
    match res {
        Ok(()) => Some(tmp),
        Err(_) => {
            let _ = std::fs::remove_file(&tmp);
            None
        }
    }
}

/// One line in the file format of [`Logger`].
fn format_line(category: Category, message: &str) -> String {
    format!("{} - {}: {}", crate::clock::now().format(), category.as_str(), message)
}

/// Append one line to the log at `path` and nothing else: no stat, no trim, no ring, no lock held
/// across calls. For contexts that must not wait on or disturb anything (the panic hook, which can
/// run while another thread holds the logger's mutex). Failures are ignored.
pub fn append_line(path: &Path, category: Category, message: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut buf = format_line(category, message).into_bytes();
    buf.push(b'\n');
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(&buf);
    }
}

impl FileLog {
    fn lock(&self) -> std::sync::MutexGuard<'_, FileInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Trim when `len` exceeds the cap. Returns the resulting size. Any failure leaves the log
    /// untouched (no line is ever lost: see the notes on [`LOG_MAX_BYTES`]; `.1` is only replaced
    /// once the replacement log is fully written, and a failed replace, e.g. a Windows rename
    /// against an open handle, keeps the original file).
    fn trim_locked(&self, inner: &mut FileInner, len: u64) -> u64 {
        if inner.trim_disabled {
            return len;
        }
        if len <= LOG_MAX_BYTES {
            // Observed at or below the cap: any earlier back-off no longer applies.
            inner.trim_floor = 0;
            return len;
        }
        if len <= inner.trim_floor {
            return len;
        }
        let Some(_lock) = TrimLock::acquire(&self.path) else {
            // Another process is trimming right now; look again later.
            inner.trim_floor = len + RETRY_STEP;
            return len;
        };
        inner.payload = None;
        match self.trim_exclusive() {
            Trim::NotNeeded(real) => {
                // Cleared / trimmed by someone else: the tracked size was stale.
                inner.trim_floor = 0;
                real
            }
            Trim::Done(size) => {
                inner.trim_floor = 0;
                size
            }
            Trim::Abandoned(now) => {
                inner.trim_floor = now + RETRY_STEP;
                now
            }
            Trim::Failed(now) => {
                inner.trim_floor = now + 1024 * 1024;
                now
            }
        }
    }

    /// The trim proper, holding the cross-process lock.
    fn trim_exclusive(&self) -> Trim {
        let cur = || self.stat().map_or(0, |(l, _)| l);
        let mut f = match std::fs::File::open(&self.path) {
            Ok(f) => f,
            Err(_) => return Trim::Failed(cur()),
        };
        let mut bytes = Vec::new();
        if f.read_to_end(&mut bytes).is_err() {
            return Trim::Failed(cur());
        }
        let real = bytes.len() as u64;
        if real <= LOG_MAX_BYTES {
            return Trim::NotNeeded(real);
        }
        let Ok(meta) = f.metadata() else { return Trim::Failed(real) };
        let cut = (real - LOG_KEEP_BYTES) as usize;
        // First line start at or after `cut`; no newline at all in the tail (one giant line)
        // retains nothing: the whole content goes to `.1`.
        let start = bytes[cut - 1..].iter().position(|&b| b == b'\n').map_or(bytes.len(), |i| cut + i);
        let mut tail_len = (bytes.len() - start) as u64;
        let mut consumed = real;
        // Both files are prepared (and fsynced) before anything visible changes.
        let head = write_like(&rotated_path(&self.path), &bytes[..start], &meta);
        let tail = write_like(&self.path, &bytes[start..], &meta);
        let (Some(head_tmp), Some(tail_tmp)) = (head.clone(), tail.clone()) else {
            for x in [head, tail].into_iter().flatten() {
                let _ = std::fs::remove_file(x);
            }
            return Trim::Failed(real);
        };
        let discard = |why: Trim| {
            let _ = std::fs::remove_file(&head_tmp);
            let _ = std::fs::remove_file(&tail_tmp);
            why
        };
        // Lines appended while we were working (by other processes, or other loggers): carry them
        // over into the new log, then re-check until the old file is quiet.
        for _ in 0..4 {
            let mut extra = Vec::new();
            if f.read_to_end(&mut extra).is_err() {
                return discard(Trim::Failed(cur()));
            }
            if extra.is_empty() {
                break;
            }
            match std::fs::OpenOptions::new().append(true).open(&tail_tmp).and_then(|mut t| t.write_all(&extra)) {
                Ok(()) => {
                    consumed += extra.len() as u64;
                    tail_len += extra.len() as u64;
                }
                Err(_) => return discard(Trim::Failed(cur())),
            }
        }
        // Truncated behind our back (Clear Logs, an external edit): our copy is stale. Nothing
        // visible has changed yet, so just give up for now.
        if cur() < consumed {
            return discard(Trim::Abandoned(cur()));
        }
        // `.1` first (the old content is still complete in the log, so a failure here, or between
        // the two renames, can only duplicate lines, never lose them), then the new log.
        if std::fs::rename(&head_tmp, rotated_path(&self.path)).is_err() {
            return discard(Trim::Failed(real));
        }
        if std::fs::rename(&tail_tmp, &self.path).is_err() {
            return discard(Trim::Failed(real));
        }
        // Whatever reached the old file after the last read above is still readable through our
        // handle (the old inode lives until it is dropped): append it to the new log. An appender
        // that opened the old path just before the rename may only write a moment later (it can
        // be descheduled between open and write), so keep looking for a short grace period. A
        // writer stalled for longer than that could still lose its one line; that window is the
        // residual risk of path-based appends.
        for round in 0..3 {
            if round > 0 {
                std::thread::sleep(std::time::Duration::from_millis(8));
            }
            let mut late = Vec::new();
            if f.read_to_end(&mut late).is_ok() && !late.is_empty() {
                if let Ok(mut n) = std::fs::OpenOptions::new().create(true).append(true).open(&self.path) {
                    if n.write_all(&late).is_ok() {
                        tail_len += late.len() as u64;
                    }
                }
            }
        }
        Trim::Done(tail_len)
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
    /// A logger on `path`. Creating one touches nothing (in particular it does not trim): the
    /// GUI calls [`Logger::trim_if_needed`] from a background thread, appends enforce the cap.
    pub fn new(path: PathBuf) -> Logger {
        let file = Arc::new(FileLog { path: path.clone(), inner: Mutex::new(FileInner::default()) });
        Logger { path, file, ring: Mutex::new(VecDeque::with_capacity(RING_CAPACITY)), subscribers: Mutex::new(Vec::new()) }
    }

    /// Start-up check: one stat, and a trim only when the file is over the cap (a strict no-op
    /// otherwise). Blocking (reads/writes up to a few MiB): call off the UI thread.
    pub fn trim_if_needed(&self) {
        let mut inner = self.file.lock();
        if let Some((len, _)) = self.file.stat() {
            let size = self.file.trim_locked(&mut inner, len);
            inner.size = Some(size);
        }
    }

    /// Never trim from this logger (appends and payloads included). For the hidden elevated child,
    /// which appends a few lines and must not race the GUI's own trims.
    pub fn disable_trim(&self) {
        self.file.lock().trim_disabled = true;
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
        let line = format_line(category, message);
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
                        // While backing off after a failed trim, look at the real size (one stat) so
                        // a file that shrank behind our back clears the back-off.
                        let size = match inner.size {
                            Some(s) if inner.trim_floor == 0 => s + buf.len() as u64,
                            _ => self.file.stat().map_or(0, |(l, _)| l),
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
        // The previous generation goes too (Clear Logs means clear); it never exists in goldens.
        let _ = std::fs::remove_file(rotated_path(&self.path));
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
        assert_eq!(fs::read(&p).unwrap().len(), text.len(), "creating a logger never trims");
        log.trim_if_needed();
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
        log.trim_if_needed();
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
        log.trim_if_needed();
        assert_eq!(fs::read_to_string(&p).unwrap(), "last line\n");
        assert_eq!(fs::read(rotated_path(&p)).unwrap().len(), 3 * 1024 * 1024 + 1);
        // one giant line and no newline: whole content rotates, log stays usable
        let mut one = vec![b'a'; 3 * 1024 * 1024];
        one.push(0xfe);
        fs::write(&p, &one).unwrap();
        let log2 = Logger::new(p.clone());
        log2.trim_if_needed();
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

    /// Every unique payload id in `.1` (every generation captured by `snap`) plus the log.
    fn ids(texts: &[String]) -> std::collections::HashSet<String> {
        texts.iter().flat_map(|t| t.lines()).filter_map(|l| l.split(" - Info: ").nth(1)).map(|s| s.split(' ').next().unwrap().to_string()).collect()
    }

    #[cfg(unix)]
    #[test]
    fn two_loggers_on_one_file_lose_no_line() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let d = tmp("two");
        let p = d.join("a.log");
        let a = Arc::new(Logger::new(p.clone()));
        let b = Arc::new(Logger::new(p.clone())); // separate mutex: a second process / instance
        let gens = Arc::new(Mutex::new(Vec::<String>::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let poll = {
            let (gens, stop, rot) = (gens.clone(), stop.clone(), rotated_path(&p));
            std::thread::spawn(move || {
                use std::os::unix::fs::MetadataExt;
                let mut last = 0;
                while !stop.load(Ordering::Relaxed) {
                    if let Ok(m) = fs::metadata(&rot) {
                        if m.ino() != last {
                            if let Ok(x) = fs::read(&rot) {
                                last = m.ino();
                                gens.lock().unwrap().push(String::from_utf8_lossy(&x).into_owned());
                            }
                        }
                    }
                }
            })
        };
        let msg = "q".repeat(200);
        let per = 6000;
        let hs: Vec<_> = [("A", a), ("B", b)]
            .into_iter()
            .flat_map(|(n, l)| (0..2).map(move |t| (n, t, l.clone())))
            .map(|(n, t, l)| {
                let msg = msg.clone();
                std::thread::spawn(move || {
                    for i in 0..per {
                        l.log_file_only(Category::Info, format!("{n}{t}-{i:06} {msg}"));
                    }
                })
            })
            .collect();
        for h in hs {
            h.join().unwrap();
        }
        stop.store(true, Ordering::Relaxed);
        poll.join().unwrap();
        let mut texts = gens.lock().unwrap().clone();
        assert!(texts.len() >= 2, "the run crossed the cap more than once");
        texts.push(fs::read_to_string(&p).unwrap());
        assert_eq!(ids(&texts).len(), 4 * per, "every line survives in .1 generations + the log");
        assert!(texts.iter().flat_map(|t| t.lines()).all(|l| l.ends_with(&msg)), "no torn line");
        assert!(!d.join("a.log.trim").exists(), "trim lock released");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn trim_carries_lines_appended_while_it_runs() {
        // Simulate the window by appending from another handle between read and replace: the
        // held-handle recovery in `trim_exclusive` must keep them. Driven via many racing appends
        // from a raw second handle (no Logger mutex involved).
        let d = tmp("late");
        let p = d.join("a.log");
        fs::write(&p, lines(24_000, 90)).unwrap(); // ~2.3 MB, over the cap
        let log = Logger::new(p.clone());
        let raw = p.clone();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let s2 = stop.clone();
        let h = std::thread::spawn(move || {
            let mut n = 0u32;
            while !s2.load(std::sync::atomic::Ordering::Relaxed) {
                let mut f = fs::OpenOptions::new().create(true).append(true).open(&raw).unwrap();
                f.write_all(format!("raw-{n:06}\n").as_bytes()).unwrap();
                n += 1;
            }
            n
        });
        log.trim_if_needed();
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let n = h.join().unwrap();
        let all = fs::read_to_string(rotated_path(&p)).unwrap() + &fs::read_to_string(&p).unwrap();
        let got = all.lines().filter(|l| l.starts_with("raw-")).count();
        assert!(got >= n as usize, "raw appends: wrote {n}, found {got}");
        assert_eq!(all.lines().filter(|l| !l.starts_with("raw-")).count(), 24_000);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn disabled_logger_and_held_lock_do_not_trim() {
        let d = tmp("nolock");
        let p = d.join("a.log");
        fs::write(&p, lines(30_000, 90)).unwrap();
        let log = Logger::new(p.clone());
        log.disable_trim();
        log.trim_if_needed();
        log.log_file_only(Category::Info, "x");
        let _ = log.file().payload(|b| b.to_string());
        assert!(!rotated_path(&p).exists());
        // Another process holds the trim lock: this one backs off, loses nothing.
        let log = Logger::new(p.clone());
        fs::write(d.join("a.log.trim"), b"").unwrap();
        log.trim_if_needed();
        assert!(!rotated_path(&p).exists());
        assert!(fs::metadata(&p).unwrap().len() > LOG_MAX_BYTES);
        // A stale lock (crashed trim) is taken over.
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(300);
        fs::OpenOptions::new().write(true).open(d.join("a.log.trim")).unwrap().set_modified(old).unwrap();
        let log = Logger::new(p.clone());
        log.trim_if_needed();
        assert!(rotated_path(&p).exists());
        assert!(!d.join("a.log.trim").exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn append_line_is_append_only() {
        let d = tmp("appendline");
        let p = d.join("sub").join("a.log");
        append_line(&p, Category::Error, "boom");
        append_line(&p, Category::Error, "bang");
        let t = fs::read_to_string(&p).unwrap();
        assert_eq!(t.lines().count(), 2);
        assert!(t.lines().next().unwrap().ends_with(" - Error: boom"));
        // Over the cap: still just an append (no trim, no .1).
        fs::write(&p, lines(30_000, 90)).unwrap();
        let before = fs::metadata(&p).unwrap().len();
        append_line(&p, Category::Error, "x");
        assert!(fs::metadata(&p).unwrap().len() > before);
        assert!(!rotated_path(&p).exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn trim_backoff_clears_when_the_file_shrinks_behind_our_back() {
        let d = tmp("floor");
        let p = d.join("a.log");
        // `.1` is a directory: every trim fails and sets the back-off floor.
        fs::create_dir(rotated_path(&p)).unwrap();
        let log = Logger::new(p.clone());
        let msg = "w".repeat(1000);
        for _ in 0..2600 {
            log.log_file_only(Category::Info, &msg);
        }
        assert!(fs::metadata(&p).unwrap().len() > LOG_MAX_BYTES, "failed trim leaves the log alone");
        assert!(log.file().lock().trim_floor > LOG_MAX_BYTES);
        // Somebody empties the log and fixes `.1`: one append resyncs and drops the floor.
        fs::write(&p, b"").unwrap();
        fs::remove_dir(rotated_path(&p)).unwrap();
        log.log_file_only(Category::Info, "after");
        assert_eq!(log.file().lock().trim_floor, 0);
        assert!(log.file().lock().size.unwrap() < 1024);
        // The cap is the cap again: crossing 2 MiB trims right there, not at 5 MiB.
        for _ in 0..2300 {
            log.log_file_only(Category::Info, &msg);
        }
        assert!(rotated_path(&p).exists(), "trimmed at ~2 MiB");
        assert!(fs::metadata(&p).unwrap().len() <= LOG_MAX_BYTES);
        let _ = fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn rotated_file_gets_the_logs_permission_bits() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp("perm");
        let p = d.join("a.log");
        fs::write(&p, lines(30_000, 90)).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        let log = Logger::new(p.clone());
        log.trim_if_needed();
        let mode = |f: &Path| fs::metadata(f).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&p), 0o600);
        assert_eq!(mode(&rotated_path(&p)), 0o600);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn clear_file_removes_the_rotated_generation() {
        let d = tmp("clear1");
        let p = d.join("a.log");
        fs::write(&p, lines(30_000, 90)).unwrap();
        let log = Logger::new(p.clone());
        log.trim_if_needed();
        assert!(rotated_path(&p).exists());
        assert!(log.clear_file());
        assert!(!rotated_path(&p).exists());
        assert_eq!(fs::metadata(&p).unwrap().len(), 0);
        // Nothing to remove is fine.
        assert!(log.clear_file());
        let _ = fs::remove_dir_all(&d);
    }
}
