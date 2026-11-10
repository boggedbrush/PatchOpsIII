//! Helpers that reproduce Python's text formatting where the legacy API leaked it into
//! user-visible strings (error messages, `str(float)`, `str(Path)`, `repr(str)`).

use std::path::{Component, Path, PathBuf};

/// Python `str(Path(p))`: collapses repeated separators, drops `.` components and a trailing
/// separator. No symlink/`..` resolution.
pub fn path_str(p: impl AsRef<Path>) -> String {
    let mut out = PathBuf::new();
    for c in p.as_ref().components() {
        if matches!(c, Component::CurDir) {
            continue;
        }
        out.push(c.as_os_str());
    }
    if out.as_os_str().is_empty() {
        ".".to_string()
    } else {
        out.to_string_lossy().into_owned()
    }
}

/// Python's `OSError` text for `Path.rename` / `os.rename`: `[Errno 13] Permission denied: 'a' -> 'b'`.
pub fn rename_error_text(e: &std::io::Error, src: &Path, dst: &Path) -> String {
    format!("{}: {} -> {}", io_error_text(e, None), repr_str(&src.to_string_lossy()), repr_str(&dst.to_string_lossy()))
}

/// Python `os.path.normpath` (lexical; POSIX flavour, also fine for Windows drive paths).
pub fn normpath(p: &str) -> String {
    if p.is_empty() {
        return ".".into();
    }
    let sep = if cfg!(windows) { '\\' } else { '/' };
    let norm: String = p.chars().map(|c| if cfg!(windows) && c == '/' { '\\' } else { c }).collect();
    let rooted = norm.starts_with(sep);
    let mut parts: Vec<&str> = Vec::new();
    for comp in norm.split(sep) {
        if comp.is_empty() || comp == "." {
            continue;
        }
        if comp == ".." {
            if parts.last().is_some_and(|l| *l != "..") {
                parts.pop();
                continue;
            }
            if rooted {
                continue;
            }
        }
        parts.push(comp);
    }
    let body = parts.join(&sep.to_string());
    match (rooted, body.is_empty()) {
        (true, _) => format!("{sep}{body}"),
        (false, true) => ".".into(),
        (false, false) => body,
    }
}

/// Python `os.path.abspath` (joins the cwd for relative paths, then normalises).
pub fn abspath(p: &str) -> String {
    if Path::new(p).is_absolute() {
        normpath(p)
    } else {
        let cwd = std::env::current_dir().unwrap_or_default();
        normpath(&cwd.join(p).to_string_lossy())
    }
}

/// Python `Path.resolve()` (non-strict): canonical path, falling back to the input when it
/// cannot be resolved. Strips the Windows verbatim prefix.
pub fn resolve(p: &Path) -> PathBuf {
    match std::fs::canonicalize(p) {
        Ok(c) => {
            let s = c.to_string_lossy();
            match s.strip_prefix(r"\\?\") {
                Some(rest) if cfg!(windows) && !rest.starts_with("UNC") => PathBuf::from(rest),
                _ => c,
            }
        }
        Err(_) => PathBuf::from(normpath(&p.to_string_lossy())),
    }
}

/// Python `Path.expanduser()` for `~` and `~/...` using `home`.
pub fn expanduser(p: &str, home: &Path) -> PathBuf {
    if p == "~" {
        home.to_path_buf()
    } else if let Some(rest) = p.strip_prefix("~/").or_else(|| if cfg!(windows) { p.strip_prefix("~\\") } else { None }) {
        home.join(rest)
    } else {
        PathBuf::from(p)
    }
}

/// Python `repr(str)`.
pub fn repr_str(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Python `str(float)` for finite values (`1.0`, `0.8`, `1e-05`, `1e+16`).
pub fn float_str(v: f64) -> String {
    if v.is_nan() {
        return "nan".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if v == 0.0 {
        return if v.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    let abs = v.abs();
    if (1e-4..1e16).contains(&abs) {
        let s = format!("{v}");
        if s.contains('.') {
            s
        } else {
            format!("{s}.0")
        }
    } else {
        // scientific: Rust gives `1e16`/`1.5e-5`; Python wants `1e+16`/`1.5e-05`.
        let s = format!("{v:e}");
        let (mant, exp) = s.split_once('e').unwrap_or((&s, "0"));
        let exp: i32 = exp.parse().unwrap_or(0);
        format!("{mant}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
    }
}

/// Python's `OSError` text: `[Errno 13] Permission denied: '/path'`.
pub fn io_error_text(e: &std::io::Error, path: Option<&Path>) -> String {
    let Some(code) = e.raw_os_error() else {
        return e.to_string();
    };
    let full = e.to_string();
    let msg = match full.rfind(" (os error") {
        Some(i) => &full[..i],
        None => full.as_str(),
    };
    match path {
        Some(p) => format!("[Errno {code}] {msg}: {}", repr_str(&p.to_string_lossy())),
        None => format!("[Errno {code}] {msg}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats() {
        assert_eq!(float_str(1.0), "1.0");
        assert_eq!(float_str(0.8), "0.8");
        assert_eq!(float_str(144.0), "144.0");
        assert_eq!(float_str(1e16), "1e+16");
        assert_eq!(float_str(0.00001), "1e-05");
    }

    #[test]
    fn paths() {
        assert_eq!(path_str("/a//b/./c/"), "/a/b/c");
        assert_eq!(path_str("./a"), "a");
    }

    #[test]
    fn norm() {
        if !cfg!(windows) {
            assert_eq!(normpath("/a/b/../c//d/."), "/a/c/d");
            assert_eq!(normpath("a/../.."), "..");
            assert_eq!(normpath("/.."), "/");
            assert_eq!(normpath(""), ".");
        }
    }

    #[test]
    fn repr() {
        assert_eq!(repr_str("a'b"), "\"a'b\"");
        assert_eq!(repr_str("a\\b"), "'a\\\\b'");
    }
}
