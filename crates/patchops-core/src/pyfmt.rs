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

/// Python `str.isprintable()` for one character: false for the categories Cc, Cf, Co, Cn, Zl, Zp and Zs
/// (except U+0020). Exact for Unicode 15.1.0 (CPython 3.13) via the generated table in `pyunicode.rs`.
pub fn py_isprintable(c: char) -> bool {
    let cp = c as u32;
    if cp < 0x7f {
        return cp >= 0x20;
    }
    crate::pyunicode::NON_PRINTABLE
        .binary_search_by(|&(lo, hi)| {
            if cp < lo {
                std::cmp::Ordering::Greater
            } else if cp > hi {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_err()
}

/// Python `repr(str)`: the quote choice and escapes of `unicode_repr` (`\n \r \t`, `\xNN` / `\uNNNN` /
/// `\UNNNNNNNN` for non-printable characters, printable non-ASCII kept as is).
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
            c if py_isprintable(c) => out.push(c),
            c => {
                let cp = c as u32;
                if cp <= 0xff {
                    out.push_str(&format!("\\x{cp:02x}"));
                } else if cp <= 0xffff {
                    out.push_str(&format!("\\u{cp:04x}"));
                } else {
                    out.push_str(&format!("\\U{cp:08x}"));
                }
            }
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

// ---- Python string semantics -----------------------------------------------------------------

/// Python `str.isspace()` for one character (also what `\s` matches for `str` patterns and what
/// `str.strip()` removes): ASCII whitespace, U+001C..U+001F, U+0085, U+00A0 and the Unicode `Zs`/`Zl`/`Zp` spaces.
pub fn py_isspace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0b}' | '\u{0c}' | '\r' | ' ' | '\u{1c}'..='\u{1f}' | '\u{85}' | '\u{a0}' | '\u{1680}'
            | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
    )
}

/// Python `str.strip()` (no argument).
pub fn py_strip(s: &str) -> &str {
    s.trim_matches(py_isspace)
}

/// Python `str.rstrip()` (no argument).
pub fn py_rstrip(s: &str) -> &str {
    s.trim_end_matches(py_isspace)
}

/// Python `str.splitlines()` (no `keepends`): splits on `\n`, `\r`, `\r\n`, `\v`, `\f`, U+001C..U+001E,
/// U+0085, U+2028 and U+2029; no empty final element after a trailing line break.
pub fn py_splitlines(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut it = s.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if !matches!(c, '\n' | '\r' | '\u{0b}' | '\u{0c}' | '\u{1c}'..='\u{1e}' | '\u{85}' | '\u{2028}' | '\u{2029}') {
            continue;
        }
        out.push(&s[start..i]);
        start = i + c.len_utf8();
        if c == '\r' && matches!(it.peek(), Some(&(_, '\n'))) {
            it.next();
            start += 1;
        }
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

/// Python's universal-newlines translation for a file opened in text mode: `\r\n` and lone `\r` become `\n`.
pub fn universal_newlines(s: &str) -> String {
    if !s.contains('\r') {
        return s.to_string();
    }
    s.replace("\r\n", "\n").replace('\r', "\n")
}

// ---- Python numbers ---------------------------------------------------------------------------

/// A Python `int` of any size, kept as its canonical decimal text (no `+`, no leading zeros, `-0` is `0`).
/// Serialises as a JSON integer of any magnitude (serde_json `arbitrary_precision`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PyInt(String);

impl PyInt {
    /// From a validated sign and ASCII digit string.
    fn from_parts(neg: bool, digits: &str) -> PyInt {
        let d = digits.trim_start_matches('0');
        if d.is_empty() {
            PyInt("0".into())
        } else if neg {
            PyInt(format!("-{d}"))
        } else {
            PyInt(d.to_string())
        }
    }

    /// `int(trunc(f))` for a finite float (exact, any magnitude); `None` for inf/nan (Python raises).
    pub fn from_f64_trunc(f: f64) -> Option<PyInt> {
        if !f.is_finite() {
            return None;
        }
        let t = f.trunc();
        Some(PyInt::from_parts(t < 0.0, format!("{:.0}", t.abs()).as_str()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Saturating conversion.
    pub fn to_i64(&self) -> i64 {
        self.0.parse::<i64>().unwrap_or(if self.0.starts_with('-') { i64::MIN } else { i64::MAX })
    }

    pub fn to_f64(&self) -> f64 {
        self.0.parse::<f64>().unwrap_or(0.0)
    }
}

impl Default for PyInt {
    fn default() -> Self {
        PyInt("0".into())
    }
}

impl From<i64> for PyInt {
    fn from(v: i64) -> Self {
        PyInt(v.to_string())
    }
}

impl PartialEq<i64> for PyInt {
    fn eq(&self, other: &i64) -> bool {
        self.0 == other.to_string()
    }
}

impl std::fmt::Display for PyInt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl serde::Serialize for PyInt {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        match self.0.parse::<serde_json::Number>() {
            Ok(n) => n.serialize(ser),
            Err(_) => ser.serialize_str(&self.0),
        }
    }
}

impl<'de> serde::Deserialize<'de> for PyInt {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let n = serde_json::Number::deserialize(de)?;
        match py_int(&n.to_string()) {
            Some(i) => Ok(i),
            // a float spelling such as `5.0`: what `int(float)` would give
            None => n.as_f64().and_then(PyInt::from_f64_trunc).ok_or_else(|| serde::de::Error::custom("not an integer")),
        }
    }
}

/// Map Unicode decimal digits to ASCII and strip Python whitespace, as `int()` / `float()` prepare their
/// argument (`_PyUnicode_TransformDecimalAndSpaceToASCII` + strip). Other characters are left as they are
/// (and make the parse fail).
fn numeric_prepare(s: &str) -> String {
    // `int()` / `float()` skip the whitespace of `Py_ISSPACE`, which (unlike `str.strip()`) excludes U+001C..U+001F.
    s.trim_matches(|c| py_isspace(c) && !matches!(c, '\u{1c}'..='\u{1f}'))
        .chars()
        .map(|c| {
            if c.is_ascii() {
                return c;
            }
            let cp = c as u32;
            let zero = match crate::pyunicode::DECIMAL_ZEROS.binary_search(&cp) {
                Ok(i) => Some(crate::pyunicode::DECIMAL_ZEROS[i]),
                Err(0) => None,
                Err(i) => Some(crate::pyunicode::DECIMAL_ZEROS[i - 1]),
            };
            match zero {
                Some(z) if cp - z < 10 => char::from(b'0' + (cp - z) as u8),
                _ => c,
            }
        })
        .collect()
}

/// Remove the `_` separators of a Python numeric literal; `None` unless every `_` sits between two digits.
fn strip_digit_underscores(s: &str) -> Option<String> {
    let b = s.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        if c == b'_' && !(i > 0 && b[i - 1].is_ascii_digit() && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            return None;
        }
    }
    Some(s.replace('_', ""))
}

/// Python 3 `int(str)` (base 10): surrounding whitespace, an optional sign, Unicode decimal digits, single
/// underscores between digits, any size.
pub fn py_int(s: &str) -> Option<PyInt> {
    let t = numeric_prepare(s);
    let (neg, body) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.strip_prefix('+').unwrap_or(&t)),
    };
    let digits = strip_digit_underscores(body)?;
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(PyInt::from_parts(neg, &digits))
}

/// Python 3 `float(str)`: surrounding whitespace, Unicode decimal digits, underscores between digits, `inf` /
/// `infinity` / `nan` in any case with an optional sign, `5.`, `.5`, exponents. Out-of-range values give `inf`.
pub fn py_float(s: &str) -> Option<f64> {
    let t = numeric_prepare(s);
    let t = strip_digit_underscores(&t)?;
    // Rust's grammar is Python's for these inputs; what Rust accepts and Python does not (nothing known) is
    // not guarded against, what Python accepts and Rust does not (underscores, Unicode digits) is handled above.
    if t.is_empty() || !t.is_ascii() {
        return None;
    }
    t.parse::<f64>().ok()
}

/// Python `str(value)` of a JSON number as `json.loads` would have produced it: an integer literal is an
/// arbitrary-size `int` (`-0` is `0`), anything with a fraction or exponent a `float` (`1e400` is `inf`).
pub fn py_str_of_number(n: &serde_json::Number) -> String {
    let text = n.to_string();
    if text.contains(['.', 'e', 'E']) {
        return float_str(text.parse::<f64>().unwrap_or(f64::NAN));
    }
    match py_int(&text) {
        Some(i) => i.0,
        None => text,
    }
}

/// True for a JSON integer literal (no fraction / exponent), i.e. what Python parses as `int`.
pub fn number_is_int(n: &serde_json::Number) -> bool {
    !n.to_string().contains(['.', 'e', 'E'])
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
    fn repr_escapes_like_python() {
        assert_eq!(repr_str("a\u{200b}b"), "'a\\u200bb'");
        assert_eq!(repr_str("\u{a0}x\u{7f}\u{85}"), "'\\xa0x\\x7f\\x85'");
        assert_eq!(repr_str("\u{e9}\u{4e2d}\u{1f600}"), "'\u{e9}\u{4e2d}\u{1f600}'");
        assert_eq!(repr_str("\u{e0000}\u{10ffff}\u{378}"), "'\\U000e0000\\U0010ffff\\u0378'");
        assert_eq!(repr_str("\u{2028}\u{feff}\0"), "'\\u2028\\ufeff\\x00'");
        assert_eq!(repr_str("it's \"x\""), "'it\\'s \"x\"'");
    }

    #[test]
    fn strip_and_splitlines_match_python() {
        assert_eq!(py_strip("\u{1c}\u{a0} x y \u{2003}\u{85}\n"), "x y");
        assert_eq!(py_strip("\u{200b}x"), "\u{200b}x"); // ZWSP is not whitespace
        assert_eq!(py_rstrip(" a \u{1f}"), " a");
        assert_eq!(py_splitlines("a\r\nb\rc\nd\u{b}e\u{c}f\u{1c}g\u{1d}h\u{1e}i\u{85}j\u{2028}k\u{2029}l"), ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"]);
        assert_eq!(py_splitlines("a\n"), ["a"]);
        assert_eq!(py_splitlines("a\n\n"), ["a", ""]);
        assert_eq!(py_splitlines("\r\r\n"), ["", ""]);
        assert!(py_splitlines("").is_empty());
        assert_eq!(py_splitlines("x\u{1f}y"), ["x\u{1f}y"]);
        assert_eq!(universal_newlines("a\r\nb\rc\n\r\r\n"), "a\nb\nc\n\n\n");
    }

    #[test]
    fn python_int_and_float_parsing() {
        let i = |s: &str| py_int(s).map(|v| v.to_string());
        assert_eq!(i(" 1_0 ").as_deref(), Some("10"));
        assert_eq!(i("\u{ff11}\u{ff12}").as_deref(), Some("12"));
        assert_eq!(i("\u{663}\u{660}").as_deref(), Some("30"));
        assert_eq!(i("-0").as_deref(), Some("0"));
        assert_eq!(i("\u{85}\u{a0}\u{b}7\u{2003}").as_deref(), Some("7"));
        assert_eq!(i("+007").as_deref(), Some("7"));
        assert_eq!(i("123456789012345678901234567890").as_deref(), Some("123456789012345678901234567890"));
        for bad in ["", "1__0", "_1", "1_", "1.0", "1e3", "0x10", "--1", "1 2", "\u{200b}1", "١٢a", "+", "-", "\u{1c}1", "1\u{1f}"] {
            assert!(py_int(bad).is_none(), "{bad:?}");
        }
        assert_eq!(py_int("9223372036854775808").map(|v| v.to_i64()), Some(i64::MAX));
        let f = |s: &str| py_float(s);
        assert_eq!(f("\u{ff11}\u{ff12}"), Some(12.0));
        assert_eq!(f(" 1_0.5_0e1_0 "), Some(10.5e10));
        assert_eq!(f("5."), Some(5.0));
        assert_eq!(f(".5"), Some(0.5));
        assert_eq!(f("INF"), Some(f64::INFINITY));
        assert_eq!(f("-Infinity"), Some(f64::NEG_INFINITY));
        assert!(f("+NaN").is_some_and(f64::is_nan));
        assert_eq!(f("1e400"), Some(f64::INFINITY));
        for bad in ["", "1__0", "1_.5", "1._5", "1e_5", "_1", "0x1p3", "1e", "e1", ".", "in", "\u{200b}1", "nan(1)", "1 2", "\u{1d}1"] {
            assert!(f(bad).is_none(), "{bad:?}");
        }
        assert_eq!(PyInt::from_f64_trunc(9_223_372_036_854_775_808.0).map(|v| v.to_string()).as_deref(), Some("9223372036854775808"));
        assert_eq!(PyInt::from_f64_trunc(-1e30).map(|v| v.to_string()).as_deref(), Some("-1000000000000000019884624838656"));
        assert_eq!(PyInt::from_f64_trunc(-0.5).map(|v| v.to_string()).as_deref(), Some("0"));
        assert!(PyInt::from_f64_trunc(f64::NAN).is_none());
    }

    #[test]
    fn json_numbers_stringify_like_python() {
        let n = |s: &str| py_str_of_number(&serde_json::from_str::<serde_json::Number>(s).unwrap());
        assert_eq!(n("9223372036854775808"), "9223372036854775808");
        assert_eq!(n("-0"), "0");
        assert_eq!(n("123456789.12345679"), "123456789.12345679");
        assert_eq!(n("1e400"), "inf");
        assert_eq!(n("-1e400"), "-inf");
        assert_eq!(n("1.0"), "1.0");
        assert_eq!(n("1e2"), "100.0");
        assert_eq!(n("1E-7"), "1e-07");
        assert_eq!(n("-0.0"), "-0.0");
        assert_eq!(n("12345678901234567890123"), "12345678901234567890123");
        assert!(!number_is_int(&serde_json::from_str("1e2").unwrap()));
        assert!(number_is_int(&serde_json::from_str("-0").unwrap()));
    }

    #[test]
    fn pyint_serialises_as_json_integer() {
        let v = serde_json::to_string(&PyInt::from_f64_trunc(9_223_372_036_854_775_808.0).unwrap()).unwrap();
        assert_eq!(v, "9223372036854775808");
        let back: PyInt = serde_json::from_str("-12").unwrap();
        assert_eq!(back, -12);
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
