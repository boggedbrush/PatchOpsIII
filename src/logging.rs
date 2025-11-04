use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Local;
use flume::Sender;
use once_cell::sync::OnceCell;
use parking_lot::Mutex;

#[derive(Clone, Debug)]
pub struct Logger {
    inner: Arc<LoggerInner>,
}

#[derive(Debug)]
struct LoggerInner {
    file_path: PathBuf,
    file: Mutex<File>,
}

impl Logger {
    pub fn initialize(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            inner: Arc::new(LoggerInner {
                file_path: path,
                file: Mutex::new(file),
            }),
        })
    }

    pub fn log(&self, category: LogCategory, message: impl AsRef<str>) {
        let timestamp = Local::now();
        let formatted = timestamp.format("%Y-%m-%d %H:%M:%S");
        let line = format!(
            "{} - {}: {}\n",
            formatted,
            category.as_str(),
            message.as_ref()
        );
        if let Ok(mut handle) = self
            .inner
            .file
            .lock()
            .try_lock_for(std::time::Duration::from_secs(1))
        {
            let _ = handle.write_all(line.as_bytes());
        }
        if let Some(sender) = LOG_CHANNEL.get() {
            let _ = sender.send(LogEntry {
                timestamp: formatted.to_string(),
                category,
                message: message.as_ref().to_string(),
            });
        }
    }

    pub fn path(&self) -> &Path {
        &self.inner.file_path
    }
}

#[derive(Debug, Clone, Copy)]
pub enum LogCategory {
    Info,
    Warning,
    Success,
    Error,
}

impl LogCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            LogCategory::Info => "Info",
            LogCategory::Warning => "Warning",
            LogCategory::Success => "Success",
            LogCategory::Error => "Error",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub category: LogCategory,
    pub message: String,
}

static LOGGER: OnceCell<Logger> = OnceCell::new();
static LOG_CHANNEL: OnceCell<Sender<LogEntry>> = OnceCell::new();

pub fn init_global_logger(path: impl AsRef<Path>) -> std::io::Result<()> {
    let logger = Logger::initialize(path)?;
    LOGGER.set(logger).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "Logger already initialized",
        )
    })
}

pub fn global_logger() -> Option<Logger> {
    LOGGER.get().cloned()
}

pub fn set_channel(sender: Sender<LogEntry>) {
    let _ = LOG_CHANNEL.set(sender);
}

pub fn log(category: LogCategory, message: impl AsRef<str>) {
    if let Some(logger) = global_logger() {
        logger.log(category, message);
    }
}
