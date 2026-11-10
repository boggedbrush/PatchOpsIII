//! `<game>/players/config.ini` access.
//!
//! The *read* side (lines like `Key = "value" // comment`) is implemented because `status`
//! derives `graphics`/`advanced` from it. The *write* side ([`write_values`]) is the work of the
//! `config` op group: it must reproduce the legacy `_write_config_values` regex semantics
//! (see `bench/OPS.md`, "Config writes").

use std::path::Path;

use crate::error::{AppError, AppResult};

/// A value accepted by `config`/presets: Python's `str | int | float | bool`.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigValue {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

impl ConfigValue {
    /// From a JSON scalar (`str | int | float | bool`); `None` for null/array/object.
    pub fn from_json(v: &serde_json::Value) -> Option<ConfigValue> {
        use serde_json::Value;
        match v {
            Value::String(s) => Some(ConfigValue::Str(s.clone())),
            Value::Bool(b) => Some(ConfigValue::Bool(*b)),
            Value::Number(n) => match n.as_i64() {
                Some(i) => Some(ConfigValue::Int(i)),
                None => n.as_f64().map(ConfigValue::Float),
            },
            _ => None,
        }
    }

    /// Python `str(value)`.
    pub fn py_str(&self) -> String {
        match self {
            ConfigValue::Str(s) => s.clone(),
            ConfigValue::Int(i) => i.to_string(),
            ConfigValue::Float(f) => crate::pyfmt::float_str(*f),
            ConfigValue::Bool(true) => "True".into(),
            ConfigValue::Bool(false) => "False".into(),
        }
    }
}

/// Legacy `_read_config`: UTF-8 with invalid bytes dropped (not replaced), `""` when missing.
pub fn read(config_path: &Path) -> String {
    match std::fs::read(config_path) {
        Ok(bytes) => decode_ignore(&bytes),
        Err(_) => String::new(),
    }
}

/// `bytes.decode("utf-8", errors="ignore")`.
pub fn decode_ignore(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => {
            let mut out = String::with_capacity(bytes.len());
            let mut rest = bytes;
            loop {
                match std::str::from_utf8(rest) {
                    Ok(s) => {
                        out.push_str(s);
                        break;
                    }
                    Err(e) => {
                        let (good, bad) = rest.split_at(e.valid_up_to());
                        out.push_str(std::str::from_utf8(good).unwrap_or(""));
                        let skip = e.error_len().unwrap_or(bad.len());
                        rest = &bad[skip..];
                    }
                }
            }
            out
        }
    }
}

/// First `^\s*Key\s*=\s*"([^"]+)"` match in `content` (MULTILINE, so `\s*` may span blank
/// lines before the key and between `=` and the quote, as the Python regex does). The value
/// must be non-empty.
pub fn extract(content: &str, key: &str) -> Option<String> {
    let b = content.as_bytes();
    let k = key.as_bytes();
    let n = b.len();
    let is_ws = |c: u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
    // candidate line starts
    let mut line_start = 0usize;
    while line_start <= n {
        let mut i = line_start;
        while i < n && is_ws(b[i]) {
            i += 1;
        }
        if b[i.min(n)..].starts_with(k) && !k.is_empty() {
            let mut j = i + k.len();
            while j < n && is_ws(b[j]) {
                j += 1;
            }
            if j < n && b[j] == b'=' {
                j += 1;
                while j < n && is_ws(b[j]) {
                    j += 1;
                }
                if j < n && b[j] == b'"' {
                    let start = j + 1;
                    if let Some(len) = b[start..].iter().position(|&c| c == b'"') {
                        if len > 0 {
                            return Some(content[start..start + len].to_string());
                        }
                    }
                }
            }
        }
        // next line start
        match b[line_start..].iter().position(|&c| c == b'\n') {
            Some(p) => line_start += p + 1,
            None => break,
        }
    }
    None
}

/// Like [`extract`] with a default (legacy `_extract_config`).
pub fn extract_or(content: &str, key: &str, default: &str) -> String {
    extract(content, key).unwrap_or_else(|| default.to_string())
}

/// Python `int(float(s))` / `float(s)` parsing helpers for state fields.
pub fn parse_float(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok()
}

/// Legacy `_config_int`: `int(float(value))`, default on failure.
pub fn get_int(content: &str, key: &str, default: i64) -> i64 {
    extract(content, key).and_then(|v| parse_float(&v)).filter(|f| f.is_finite()).map(|f| f.trunc() as i64).unwrap_or(default)
}

/// Legacy `_config_float`.
pub fn get_float(content: &str, key: &str, default: f64) -> f64 {
    extract(content, key).and_then(|v| parse_float(&v)).unwrap_or(default)
}

/// Legacy `_config_bool(key, enabled_value, default)`.
pub fn get_flag(content: &str, key: &str, enabled: &str, default: &str) -> bool {
    extract_or(content, key, default) == enabled
}

/// Legacy `_config_is_readonly`: the file exists and the process cannot write it
/// (`not os.access(path, W_OK)`; always writable for root).
pub fn is_readonly(config_path: &Path) -> bool {
    if !config_path.exists() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let Ok(c) = std::ffi::CString::new(config_path.as_os_str().as_bytes()) else { return false };
        // SAFETY: valid NUL-terminated path.
        unsafe { libc::access(c.as_ptr(), libc::W_OK) != 0 }
    }
    #[cfg(not(unix))]
    {
        std::fs::metadata(config_path).map(|m| m.permissions().readonly()).unwrap_or(false)
    }
}

/// Apply `(key, value, comment)` updates to `<game>/players/config.ini`.
///
/// TODO(config group): port `_write_config_values` (see module docs). Stub: fails.
pub fn write_values(_game_dir: &Path, _updates: &[(String, ConfigValue, String)]) -> AppResult<()> {
    Err(AppError::new("internal: config write not implemented"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_basic() {
        let c = "// FOV = \"1\"\nMaxFPS = \"165\" // x\n  FOV = \"90\"\nVsyncMode = \"2\"\nVsync = \"0\"\n";
        assert_eq!(extract(c, "MaxFPS").as_deref(), Some("165"));
        assert_eq!(extract(c, "FOV").as_deref(), Some("90"));
        assert_eq!(extract(c, "Vsync").as_deref(), Some("0"));
        assert_eq!(extract(c, "Nope"), None);
        assert_eq!(extract("A = \"\"\nA = \"3\"", "A").as_deref(), Some("3"));
    }

    #[test]
    fn ignore_decode() {
        assert_eq!(decode_ignore(b"a\xE9b"), "ab");
    }

    #[test]
    fn numbers() {
        assert_eq!(get_int("X = \"59.94\"", "X", 1), 59);
        assert_eq!(get_int("X = \"abc\"", "X", 7), 7);
        assert_eq!(get_float("X = \"0.29\"", "X", 0.75), 0.29);
    }
}
