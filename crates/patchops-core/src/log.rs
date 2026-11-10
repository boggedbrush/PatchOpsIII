//! Application log: `<app_data>/PatchOpsIII.log` (append-only), an in-memory ring of the last
//! 300 entries, and subscribers for live tailing in the GUI.
//!
//! File line format (legacy `utils.write_log`): `YYYY-MM-DD HH:MM:SS - <Category>: <message>`.

use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom, Write};
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
/// the elevated install child, the panic hook). Appends are `O_APPEND` single writes, nothing else
/// (no stat, no re-check). A trim
/// (a) is exclusive between processes through an OS advisory lock on a persistent `<log>.lock`
/// (`flock` / `LockFileEx`, non-blocking: a busy lock skips this trim, the kernel releases it if the
/// holder dies), (b) first deletes the temps a crashed trim left behind, (c) streams the old head
/// into `.1`'s temp (memory stays around the retained tail, whatever the log size), (d) holds the
/// old file open while it works and carries every whole line appended in the meantime into the new
/// log, before and for a short grace period after the renames, so a line is not lost, and (e)
/// replaces `.1` only after the new log is fully prepared and the log was not truncated behind its
/// back. Accepted limits: a line from ANOTHER process that is stalled for more than ~16 ms between
/// open and write at the instant of a trim can be lost; each logger counts only its own appends
/// toward the cap; the append that crosses the cap performs the trim inline.
///
/// Lock order, everywhere: the in-process [`FileLog`] mutex first, then the OS lock (a leaf: nothing
/// is acquired while holding it). The only kind of process that trims at start-up is the GUI
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

/// `<log>.lock`: advisory OS lock (`flock` / `LockFileEx`) held while a trim, or a Clear Logs next
/// to a possible trim, runs. The file persists (it carries no data); the kernel drops the lock when
/// the holder exits or crashes. Created only when a trim is actually attempted.
struct TrimLock(std::fs::File);

enum Lock {
    Held(TrimLock),
    /// Another handle (process, or logger instance) holds it.
    Busy,
    /// The lock file cannot be opened (absent when only an existing one was asked for, or the
    /// directory is not writable).
    Unavailable,
}

impl TrimLock {
    fn path(log: &Path) -> PathBuf {
        let mut p = log.as_os_str().to_os_string();
        p.push(".lock");
        PathBuf::from(p)
    }

    /// Try to take the lock; with `wait` keep trying (polling) for that long. `create`: make the
    /// lock file when it does not exist yet.
    fn acquire(log: &Path, create: bool, wait: Option<std::time::Duration>) -> Lock {
        let mut o = std::fs::OpenOptions::new();
        o.read(true);
        if create {
            o.write(true).create(true).truncate(false);
        }
        let Ok(f) = o.open(Self::path(log)) else { return Lock::Unavailable };
        let deadline = wait.map(|w| std::time::Instant::now() + w);
        loop {
            match os_lock::try_lock(&f) {
                Ok(true) => return Lock::Held(TrimLock(f)),
                Ok(false) => {}
                Err(_) => return Lock::Unavailable,
            }
            match deadline {
                Some(d) if std::time::Instant::now() < d => std::thread::sleep(std::time::Duration::from_millis(5)),
                _ => return Lock::Busy,
            }
        }
    }
}

impl Drop for TrimLock {
    fn drop(&mut self) {
        // Closing the handle releases it; unlocking first makes that immediate everywhere.
        os_lock::unlock(&self.0);
    }
}

#[cfg(unix)]
mod os_lock {
    use std::os::unix::io::AsRawFd;

    /// `Ok(true)` locked, `Ok(false)` held by someone else.
    pub fn try_lock(f: &std::fs::File) -> std::io::Result<bool> {
        loop {
            // SAFETY: plain syscall on a descriptor that `f` keeps open.
            if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(true);
            }
            let e = std::io::Error::last_os_error();
            match e.kind() {
                std::io::ErrorKind::WouldBlock => return Ok(false),
                std::io::ErrorKind::Interrupted => continue,
                _ => return Err(e),
            }
        }
    }

    pub fn unlock(f: &std::fs::File) {
        // SAFETY: as above.
        unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_UN) };
    }
}

/// Raw `kernel32` declarations (no windows crate), like `elevate.rs`. Type-checked, not run on a
/// Windows machine yet.
#[cfg(windows)]
mod os_lock {
    use std::os::windows::io::AsRawHandle;

    type Handle = *mut core::ffi::c_void;

    #[repr(C)]
    struct Overlapped {
        internal: usize,
        internal_high: usize,
        offset: u32,
        offset_high: u32,
        event: Handle,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn LockFileEx(file: Handle, flags: u32, reserved: u32, lo: u32, hi: u32, overlapped: *mut Overlapped) -> i32;
        fn UnlockFileEx(file: Handle, reserved: u32, lo: u32, hi: u32, overlapped: *mut Overlapped) -> i32;
    }

    const LOCKFILE_FAIL_IMMEDIATELY: u32 = 1;
    const LOCKFILE_EXCLUSIVE_LOCK: u32 = 2;
    const ERROR_LOCK_VIOLATION: i32 = 33;

    fn overlapped() -> Overlapped {
        Overlapped { internal: 0, internal_high: 0, offset: 0, offset_high: 0, event: std::ptr::null_mut() }
    }

    /// Locks byte 0 of the lock file. `Ok(true)` locked, `Ok(false)` held by someone else.
    pub fn try_lock(f: &std::fs::File) -> std::io::Result<bool> {
        let mut ov = overlapped();
        // SAFETY: valid handle owned by `f`; `ov` outlives the (synchronous) call.
        let ok = unsafe { LockFileEx(f.as_raw_handle() as Handle, LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY, 0, 1, 0, &mut ov) };
        if ok != 0 {
            return Ok(true);
        }
        let e = std::io::Error::last_os_error();
        if e.raw_os_error() == Some(ERROR_LOCK_VIOLATION) {
            Ok(false)
        } else {
            Err(e)
        }
    }

    pub fn unlock(f: &std::fs::File) {
        let mut ov = overlapped();
        // SAFETY: as above.
        unsafe { UnlockFileEx(f.as_raw_handle() as Handle, 0, 1, 0, &mut ov) };
    }
}

#[cfg(not(any(unix, windows)))]
mod os_lock {
    pub fn try_lock(_: &std::fs::File) -> std::io::Result<bool> {
        Ok(true)
    }
    pub fn unlock(_: &std::fs::File) {}
}

/// Delete the `.<log>.tmp-*` / `.<log>.1.tmp-*` leftovers of a crashed trim (regular files only;
/// exact prefix). Called with the trim lock held, so no live trim owns any of them.
fn sweep_stale_temps(log: &Path) {
    let (Some(dir), Some(name)) = (log.parent(), log.file_name()) else { return };
    let name = name.to_string_lossy();
    let prefixes = [format!(".{name}.tmp-"), format!(".{name}.1.tmp-")];
    let Ok(rd) = std::fs::read_dir(if dir.as_os_str().is_empty() { Path::new(".") } else { dir }) else { return };
    for e in rd.flatten() {
        let n = e.file_name();
        let n = n.to_string_lossy();
        if prefixes.iter().any(|p| n.starts_with(p.as_str())) && e.file_type().is_ok_and(|t| t.is_file()) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

static TMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Fill a fresh sibling temp of `dest` through `fill`, with the permission bits of `like` (the log:
/// a 0600 log gives a 0600 `.1`, never briefly wider), fsynced. Returns the temp path.
fn write_like(dest: &Path, like: &std::fs::Metadata, fill: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>) -> Option<PathBuf> {
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
        fill(&mut f)?;
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

/// Copy the first `len` bytes of `src` into `out` through a bounded buffer (own handle, so the
/// trimmer's read position on the old file is untouched).
fn copy_prefix(src: &Path, len: u64, out: &mut std::fs::File) -> std::io::Result<()> {
    let input = std::fs::File::open(src)?;
    let copied = std::io::copy(&mut input.take(len), out)?;
    if copied == len {
        Ok(())
    } else {
        Err(std::io::ErrorKind::UnexpectedEof.into())
    }
}

/// Remove and return the part of `buf` up to and including its last `\n` (empty when there is none).
fn take_whole_lines(buf: &mut Vec<u8>) -> Vec<u8> {
    match buf.iter().rposition(|&b| b == b'\n') {
        Some(i) => {
            let rest = buf.split_off(i + 1);
            std::mem::replace(buf, rest)
        }
        None => Vec::new(),
    }
}

/// One line in the file format of [`Logger`].
fn format_line(category: Category, message: &str) -> String {
    format!("{} - {}: {}", crate::clock::now().format(), category.as_str(), message)
}

/// Append one line to the log at `path` and nothing else: no stat, no trim, no ring, no lock. For contexts that must not wait on or disturb anything (the panic hook, which can
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

#[cfg(test)]
thread_local! {
    /// Test hook: `stat`s of the log made by this thread (an append under the cap must make none).
    static STAT_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
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
        // Lock order: the `inner` mutex (held by the caller), then the OS lock.
        let Lock::Held(_lock) = TrimLock::acquire(&self.path, true, None) else {
            // Another process is trimming right now (or no lock file can be made); look again later.
            inner.trim_floor = len + RETRY_STEP;
            return len;
        };
        sweep_stale_temps(&self.path);
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
                // Geometric, so a huge log on a full disk is not re-read (even partly) every MiB.
                inner.trim_floor = now + (now / 2).max(1024 * 1024);
                now
            }
        }
    }

    /// The trim proper, holding the cross-process lock. Memory: the retained tail (~512 KiB) and
    /// a copy buffer, never the log.
    fn trim_exclusive(&self) -> Trim {
        let cur = || self.stat().map_or(0, |(l, _)| l);
        let mut f = match std::fs::File::open(&self.path) {
            Ok(f) => f,
            Err(_) => return Trim::Failed(cur()),
        };
        let Ok(meta) = f.metadata() else { return Trim::Failed(cur()) };
        let real = meta.len();
        if real <= LOG_MAX_BYTES {
            return Trim::NotNeeded(real);
        }
        // Read only the tail: from one byte before the cut (so a cut exactly on a line start keeps
        // that line) to the end.
        let cut = real - LOG_KEEP_BYTES;
        let mut buf = Vec::with_capacity(LOG_KEEP_BYTES as usize + 4096);
        if f.seek(SeekFrom::Start(cut - 1)).is_err() || f.read_to_end(&mut buf).is_err() {
            return Trim::Failed(real);
        }
        let mut consumed = cut - 1 + buf.len() as u64;
        // First line start at or after `cut`; no newline at all in the tail (one giant line)
        // retains nothing: the whole content goes to `.1`.
        let (start, tail_from) = match buf.iter().position(|&b| b == b'\n') {
            Some(i) => (cut + i as u64, i + 1),
            None => (consumed, buf.len()),
        };
        let mut tail_len = (buf.len() - tail_from) as u64;
        // Both files are prepared (and fsynced) before anything visible changes. The head (the big
        // one) first: when it cannot be written nothing else was done.
        let Some(head_tmp) = write_like(&rotated_path(&self.path), &meta, |o| copy_prefix(&self.path, start, o)) else {
            return Trim::Failed(cur());
        };
        let Some(tail_tmp) = write_like(&self.path, &meta, |o| o.write_all(&buf[tail_from..])) else {
            let _ = std::fs::remove_file(&head_tmp);
            return Trim::Failed(cur());
        };
        let discard = |why: Trim| {
            let _ = std::fs::remove_file(&head_tmp);
            let _ = std::fs::remove_file(&tail_tmp);
            why
        };
        // Lines appended while we were working (by other processes, or other loggers): carry them
        // over into the new log, then re-check until the old file is quiet.
        // Only whole lines are carried: a read can catch a line another writer is still storing
        // (page by page), and the rest of it must not end up after somebody else's line.
        let mut pending: Vec<u8> = Vec::new();
        for _ in 0..4 {
            let mut extra = Vec::new();
            if f.read_to_end(&mut extra).is_err() {
                return discard(Trim::Failed(cur()));
            }
            if extra.is_empty() {
                break;
            }
            consumed += extra.len() as u64;
            pending.extend_from_slice(&extra);
            let whole = take_whole_lines(&mut pending);
            if whole.is_empty() {
                continue;
            }
            match std::fs::OpenOptions::new().append(true).open(&tail_tmp).and_then(|mut t| t.write_all(&whole)) {
                Ok(()) => tail_len += whole.len() as u64,
                Err(_) => return discard(Trim::Failed(cur())),
            }
        }
        drop(buf);
        // Truncated behind our back (an external edit; Clear Logs takes the lock, so not that):
        // our copy is stale. Nothing visible has changed yet, so just give up for now.
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
            let _ = f.read_to_end(&mut late);
            pending.extend_from_slice(&late);
            let mut whole = take_whole_lines(&mut pending);
            if round == 2 && !pending.is_empty() {
                // A line still unfinished after the grace period: keep what there is as a line of
                // its own.
                whole.append(&mut pending);
                whole.push(b'\n');
            }
            if !whole.is_empty() {
                if let Ok(mut n) = std::fs::OpenOptions::new().create(true).append(true).open(&self.path) {
                    if n.write_all(&whole).is_ok() {
                        tail_len += whole.len() as u64;
                    }
                }
            }
        }
        Trim::Done(tail_len)
    }

    fn stat(&self) -> Option<FileKey> {
        #[cfg(test)]
        STAT_CALLS.with(|c| c.set(c.get() + 1));
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

    /// Truncate the log file (legacy `clear_log_file`); `false` on failure. Takes the trim lock
    /// (mutex first, then the OS lock, like every trim), waiting briefly for a trim in flight, so a
    /// trim of the old content can never finish after the clear and bring it back. The lock file is
    /// only created when the log is over the cap (a trim could start); with no lock file and a
    /// small log nothing can be trimming.
    pub fn clear_file(&self) -> bool {
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut inner = self.file.lock();
        let wait = Some(std::time::Duration::from_secs(10));
        let _lock = match TrimLock::acquire(&self.path, false, wait) {
            Lock::Held(l) => Some(l),
            Lock::Busy => {
                eprintln!("Failed to clear log file at {}: a trim is still running", self.path.display());
                return false;
            }
            Lock::Unavailable => {
                let over = std::fs::metadata(&self.path).is_ok_and(|m| m.len() > LOG_MAX_BYTES);
                match if over { TrimLock::acquire(&self.path, true, wait) } else { Lock::Unavailable } {
                    Lock::Held(l) => Some(l),
                    Lock::Busy => {
                        eprintln!("Failed to clear log file at {}: a trim is still running", self.path.display());
                        return false;
                    }
                    Lock::Unavailable => None,
                }
            }
        };
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

    #[test]
    fn two_loggers_on_one_file_lose_no_line() {
        // Seeded just under the cap, then ~0.6 MB of racing appends = exactly one trim, so `.1` plus
        // the log hold every line without snapshotting generations. Several rounds for race coverage.
        for round in 0..6 {
            let d = tmp(&format!("two{round}"));
            let p = d.join("a.log");
            let msg = "q".repeat(200);
            let seed: String = (0..8000).map(|i| format!("2026-01-01 00:00:00 - Info: seed-{i:06} {msg}\n")).collect();
            assert!((seed.len() as u64) < LOG_MAX_BYTES);
            fs::write(&p, &seed).unwrap();
            let a = Arc::new(Logger::new(p.clone()));
            let b = Arc::new(Logger::new(p.clone())); // separate mutex: a second process / instance
            let per = 600;
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
            assert!(rotated_path(&p).exists(), "the run crossed the cap");
            let texts = vec![fs::read_to_string(rotated_path(&p)).unwrap(), fs::read_to_string(&p).unwrap()];
            assert_eq!(ids(&texts).len(), 8000 + 4 * per, "round {round}: every line survives in .1 + the log");
            let total: usize = texts.iter().map(|t| t.lines().count()).sum();
            assert_eq!(total, 8000 + 4 * per, "round {round}: no duplicate line");
            assert!(texts.iter().flat_map(|t| t.lines()).all(|l| l.ends_with(&msg)), "no torn line");
            assert!(matches!(TrimLock::acquire(&p, false, None), Lock::Held(_)), "trim lock released");
            let names: Vec<_> = fs::read_dir(&d).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
            assert!(names.iter().all(|n| ["a.log", "a.log.1", "a.log.lock"].contains(&n.as_str())), "no temp left: {names:?}");
            let _ = fs::remove_dir_all(&d);
        }
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
        assert!(!TrimLock::path(&p).exists(), "no lock file unless a trim is attempted");
        // Another holder has the trim lock: this one skips, loses nothing, and retries after growth.
        let log = Logger::new(p.clone());
        let held = match TrimLock::acquire(&p, true, None) {
            Lock::Held(l) => l,
            _ => panic!("lock"),
        };
        log.trim_if_needed();
        assert!(!rotated_path(&p).exists());
        assert!(fs::metadata(&p).unwrap().len() > LOG_MAX_BYTES);
        drop(held);
        // Not retried before the file grew by the back-off step ...
        log.log_file_only(Category::Info, "small");
        assert!(!rotated_path(&p).exists());
        // ... and retried (and done) once it has.
        let msg = "z".repeat(1000);
        for _ in 0..80 {
            log.log_file_only(Category::Info, &msg);
        }
        assert!(rotated_path(&p).exists(), "trim retried after the lock came free");
        assert!(fs::metadata(&p).unwrap().len() <= LOG_MAX_BYTES);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn lock_file_only_exists_once_a_trim_was_attempted() {
        let d = tmp("lockfile");
        let p = d.join("a.log");
        let log = Logger::new(p.clone());
        for i in 0..50 {
            log.log_file_only(Category::Info, format!("line {i}"));
        }
        assert!(log.clear_file());
        assert_eq!(fs::read_dir(&d).unwrap().count(), 1, "only the log");
        fs::write(&p, lines(30_000, 90)).unwrap();
        log.trim_if_needed();
        assert!(TrimLock::path(&p).exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn streamed_trim_equals_the_whole_file_result() {
        // The reference is the old read-everything algorithm: cut = len - KEEP, forward to a line start.
        for (n, width) in [(30_000usize, 90usize), (45_017, 133), (70_000, 31)] {
            let d = tmp(&format!("stream{n}"));
            let p = d.join("a.log");
            let text = lines(n, width);
            fs::write(&p, &text).unwrap();
            let log = Logger::new(p.clone());
            log.trim_if_needed();
            let bytes = text.as_bytes();
            let cut = bytes.len() - LOG_KEEP_BYTES as usize;
            let start = bytes[cut - 1..].iter().position(|&b| b == b'\n').map_or(bytes.len(), |i| cut + i);
            assert_eq!(fs::read(&p).unwrap(), &bytes[start..], "n={n}");
            assert_eq!(fs::read(rotated_path(&p)).unwrap(), &bytes[..start], "n={n}");
            let _ = fs::remove_dir_all(&d);
        }
        // Cut exactly on a line start keeps that line (width 7: 8-byte lines, KEEP is a multiple of 8).
        let d = tmp("streamedge");
        let p = d.join("a.log");
        let text: String = (0..(3 * 1024 * 1024 / 8)).map(|i| format!("{:06}\n", i % 1_000_000).replace('\n', "x\n")).collect();
        fs::write(&p, &text).unwrap();
        Logger::new(p.clone()).trim_if_needed();
        assert_eq!(fs::metadata(&p).unwrap().len(), LOG_KEEP_BYTES);
        assert_eq!(fs::read_to_string(rotated_path(&p)).unwrap() + &fs::read_to_string(&p).unwrap(), text);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn stale_temps_of_a_crashed_trim_are_swept() {
        let d = tmp("sweep");
        let p = d.join("a.log");
        fs::write(&p, lines(30_000, 90)).unwrap();
        let stale = [".a.log.tmp-999-0", ".a.log.1.tmp-999-1"];
        let keep = [".a.log.tmpx", ".a.logx.tmp-1-1", "a.log.tmp-1-1", ".other.log.tmp-1-1", ".a.log.2.tmp-1-1"];
        for n in stale.iter().chain(keep.iter()) {
            fs::write(d.join(n), b"junk").unwrap();
        }
        fs::create_dir(d.join(".a.log.tmp-dir")).unwrap(); // not a regular file
        Logger::new(p.clone()).trim_if_needed();
        for n in stale {
            assert!(!d.join(n).exists(), "{n} swept");
        }
        for n in keep {
            assert!(d.join(n).exists(), "{n} untouched");
        }
        assert!(d.join(".a.log.tmp-dir").is_dir());
        assert!(rotated_path(&p).exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn append_under_the_cap_does_not_stat_the_file() {
        let d = tmp("nostat");
        let p = d.join("a.log");
        let log = Logger::new(p.clone());
        log.log_file_only(Category::Info, "warm-up (first append learns the size)");
        let before = STAT_CALLS.with(|c| c.get());
        for i in 0..20 {
            log.log_file_only(Category::Info, format!("line {i}"));
        }
        append_line(&p, Category::Error, "hook");
        assert_eq!(STAT_CALLS.with(|c| c.get()), before, "appends under the cap never stat the log");
        assert_eq!(fs::read_to_string(&p).unwrap().lines().count(), 22);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn clear_waits_for_a_trim_in_flight_and_is_never_undone() {
        // A trim (lock held by another handle) is running; Clear Logs must wait for it, then clear.
        let d = tmp("clearorder");
        let p = d.join("a.log");
        fs::write(&p, lines(30_000, 90)).unwrap();
        let held = match TrimLock::acquire(&p, true, None) {
            Lock::Held(l) => l,
            _ => panic!("lock"),
        };
        let log = Arc::new(Logger::new(p.clone()));
        let l2 = log.clone();
        let h = std::thread::spawn(move || l2.clear_file());
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert!(!h.is_finished(), "clear waits for the lock");
        assert!(fs::metadata(&p).unwrap().len() > LOG_MAX_BYTES, "log untouched while the lock is held");
        // The "trim" finishes its renames, then releases.
        fs::write(rotated_path(&p), b"old head\n").unwrap();
        drop(held);
        assert!(h.join().unwrap());
        assert_eq!(fs::metadata(&p).unwrap().len(), 0);
        assert!(!rotated_path(&p).exists(), "clear came after the trim and removed its result");
        // And against real trims on a second logger: nothing resurrects.
        for _ in 0..8 {
            fs::write(&p, lines(30_000, 90)).unwrap();
            let t = Logger::new(p.clone());
            let c = Logger::new(p.clone());
            std::thread::scope(|s| {
                s.spawn(|| t.trim_if_needed());
                s.spawn(|| {
                    std::thread::sleep(std::time::Duration::from_millis(3));
                    assert!(c.clear_file());
                });
            });
            // Either the trim finished before the clear (everything gone), or the clear was first and
            // the trim found a small file: never old content left.
            let any = |f: &Path| fs::read_to_string(f).unwrap_or_default().contains("xxxx");
            assert!(!any(&p) && !any(&rotated_path(&p)), "cleared content resurrected");
        }
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

    #[test]
    fn whole_lines_are_split_off_the_front() {
        let mut b = b"a\nbb\ncc".to_vec();
        assert_eq!(take_whole_lines(&mut b), b"a\nbb\n");
        assert_eq!(b, b"cc");
        assert!(take_whole_lines(&mut b).is_empty());
        assert_eq!(b, b"cc");
        b.extend_from_slice(b"\n");
        assert_eq!(take_whole_lines(&mut b), b"cc\n");
        assert!(b.is_empty());
    }
}
