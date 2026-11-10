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

pub struct Logger {
    path: PathBuf,
    ring: Mutex<VecDeque<LogEntry>>,
    subscribers: Mutex<Vec<Subscriber>>,
}

impl Logger {
    pub fn new(path: PathBuf) -> Logger {
        Logger { path, ring: Mutex::new(VecDeque::with_capacity(RING_CAPACITY)), subscribers: Mutex::new(Vec::new()) }
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
        match std::fs::OpenOptions::new().create(true).append(true).open(&self.path) {
            Ok(mut f) => {
                let mut buf = Vec::with_capacity(line.len() + 1);
                buf.extend_from_slice(line.as_bytes());
                buf.push(b'\n');
                if let Err(e) = f.write_all(&buf) {
                    eprintln!("Failed to write log entry to {}: {e}", self.path.display());
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

    /// Legacy `manage_log_retention_on_launch(threshold)`: count launches in `counter_path` and
    /// truncate the log file on every `threshold`-th launch (counter then restarts at 0).
    /// `threshold <= 0` disables it. Returns whether the log was cleared.
    pub fn manage_retention(&self, counter_path: &std::path::Path, threshold: i64) -> bool {
        if threshold <= 0 {
            return false;
        }
        let count = std::fs::read_to_string(counter_path).ok().and_then(|t| t.trim().parse::<i64>().ok()).unwrap_or(0) + 1;
        let (cleared, count) = if count >= threshold { (self.clear_file(), 0) } else { (false, count) };
        if let Some(dir) = counter_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(counter_path, count.to_string());
        cleared
    }

    /// Whole log file as text (lossy UTF-8), empty if unreadable.
    pub fn read_file(&self) -> String {
        std::fs::read(&self.path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_clears_every_third_launch() {
        let d = std::env::temp_dir().join(format!("patchops-logret-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let log = Logger::new(d.join("x.log"));
        let counter = d.join("launch_counter.txt");
        log.log_file_only(Category::Info, "keep me");
        assert!(!log.manage_retention(&counter, 3));
        assert!(!log.manage_retention(&counter, 3));
        assert!(log.read_file().contains("keep me"));
        assert!(log.manage_retention(&counter, 3));
        assert_eq!(log.read_file(), "");
        assert_eq!(std::fs::read_to_string(&counter).unwrap(), "0");
        assert!(!log.manage_retention(&counter, 0));
        let _ = std::fs::remove_dir_all(&d);
    }
}
