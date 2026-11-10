//! `<game>/players/config.ini` access.
//!
//! The *read* side (lines like `Key = "value" // comment`) is used by `status` for the
//! `graphics`/`advanced` state. The *write* side ([`write_values`]) reproduces the legacy
//! `_write_config_values` (`re.sub` with a template replacement, see `bench/OPS.md`, "Config
//! writes") without a regex engine: the pattern is fixed (`^\s*KEY\s*=.*$`, MULTILINE), so it is
//! matched by hand, and the replacement template is parsed like CPython's `re` does.

use std::path::Path;

use crate::error::{AppError, AppResult};
use crate::pyfmt::{io_error_text, path_str, repr_str};

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
        Ok(bytes) => normalize_newlines(&decode_ignore(&bytes)),
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
    let chars: Vec<char> = content.chars().collect();
    let key: Vec<char> = key.chars().collect();
    let skip_ws = |mut i: usize| {
        while i < chars.len() && py_isspace(chars[i]) {
            i += 1;
        }
        i
    };
    (0..=chars.len())
        .filter(|&s| s == 0 || chars[s - 1] == '\n')
        .find_map(|s| {
            let ws_end = skip_ws(s);
            (s..=ws_end).rev().find_map(|q| {
                if !chars[q..].starts_with(&key) {
                    return None;
                }
                let r = skip_ws(q + key.len());
                if chars.get(r) != Some(&'=') {
                    return None;
                }
                let r = skip_ws(r + 1);
                if chars.get(r) != Some(&'"') {
                    return None;
                }
                // `"([^"]+)"`: the value runs to the next quote and must be non-empty
                let start = r + 1;
                let end = start + chars[start..].iter().position(|&c| c == '"')?;
                (end > start).then(|| chars[start..end].iter().collect())
            })
        })
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

/// Apply `(key, value, comment)` updates to `<game>/players/config.ini` (legacy
/// `_write_config_values`). The file is read once, every update is applied in order and the result
/// is written once, so a failing update leaves the file untouched.
pub fn write_values(game_dir: &Path, updates: &[(String, ConfigValue, String)]) -> AppResult<()> {
    let config = crate::game::config_path(game_dir);
    let shown = path_str(&config);
    if !config.exists() {
        return Err(AppError::new(format!("config.ini not found at {shown}")));
    }
    let io_err = |e: std::io::Error| AppError::new(io_error_text(&e, Some(config.as_path())));
    let bytes = std::fs::read(&config).map_err(io_err)?;
    let mut text = normalize_newlines(&decode_ignore(&bytes));
    for (key, value, comment) in updates {
        let replacement = format!("{key} = \"{}\" // {comment}", value.py_str());
        text = apply_update(&text, key, &replacement).map_err(AppError::new)?;
    }
    // `atomic_write` would happily rename over a read-only file; an in-place open must fail first
    // exactly as the legacy `open(path, "w")` did (`[Errno 13] Permission denied`), leaving the file untouched.
    std::fs::OpenOptions::new().write(true).open(&config).map_err(io_err)?;
    crate::fsutil::atomic_write(&config, text.as_bytes()).map_err(io_err)
}

/// Python universal newlines on read: `\r\n` and lone `\r` become `\n`.
pub fn normalize_newlines(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

/// Legacy text transform for one update. Every line matching `^\s*KEY\s*=.*$` (MULTILINE) is
/// replaced by `replacement`, which is a `re.sub` template (escapes and `\g<0>` are processed).
/// When no line matches, `replacement` is appended after the right-stripped text.
/// Errors are the Python `re` error texts (`bad escape \q at position 12`).
pub fn apply_update(text: &str, key: &str, replacement: &str) -> Result<String, String> {
    let chars: Vec<char> = text.chars().collect();
    let key: Vec<char> = key.chars().collect();
    let Some(first) = find_match(&chars, 0, &key) else {
        return Ok(format!("{}\n{replacement}\n", text.trim_end_matches(py_isspace)));
    };
    let template = parse_template(&replacement.chars().collect::<Vec<char>>())?;
    let mut out = String::with_capacity(text.len() + replacement.len());
    let mut pos = 0;
    let mut hit = Some(first);
    while let Some((start, end)) = hit {
        out.extend(&chars[pos..start]);
        for part in &template {
            match part {
                Part::Lit(s) => out.push_str(s),
                Part::Group0 => out.extend(&chars[start..end]),
            }
        }
        pos = end;
        hit = find_match(&chars, pos, &key);
    }
    out.extend(&chars[pos..]);
    Ok(out)
}

/// Python `str.isspace()`, which is also what the `\s` class matches for str patterns.
pub fn py_isspace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0b}' | '\u{0c}' | '\r' | ' ' | '\u{1c}'..='\u{1f}' | '\u{85}' | '\u{a0}' | '\u{1680}'
            | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
    )
}

/// First match of `^\s*KEY\s*=.*$` starting at or after `from`, as `(start, end)` char indices.
/// `start` is a line start; `end` is the end of the line containing the `=`.
fn find_match(chars: &[char], from: usize, key: &[char]) -> Option<(usize, usize)> {
    (from..=chars.len())
        .filter(|&s| s == 0 || chars[s - 1] == '\n')
        .find_map(|s| match_at(chars, s, key).map(|end| (s, end)))
}

/// Match the pattern with its `^` at `s`. The leading `\s*` may run over blank lines.
fn match_at(chars: &[char], s: usize, key: &[char]) -> Option<usize> {
    let n = chars.len();
    let mut ws_end = s;
    while ws_end < n && py_isspace(chars[ws_end]) {
        ws_end += 1;
    }
    // `\s*` is greedy: try the longest whitespace prefix first, then shorter ones.
    for q in (s..=ws_end).rev() {
        if !chars[q..].starts_with(key) {
            continue;
        }
        let mut r = q + key.len();
        while r < n && py_isspace(chars[r]) {
            r += 1;
        }
        if r < n && chars[r] == '=' {
            let mut e = r + 1;
            while e < n && chars[e] != '\n' {
                e += 1;
            }
            return Some(e);
        }
    }
    None
}

#[derive(Debug, PartialEq)]
enum Part {
    Lit(String),
    /// `\g<0>` (the only valid group: the pattern has no capture groups).
    Group0,
}

/// Port of CPython's `re._parser.parse_template` for a pattern with no groups, including its
/// tokenizer (one token of look-ahead) so that error positions match.
fn parse_template(src: &[char]) -> Result<Vec<Part>, String> {
    let mut s = Tokenizer::new(src)?;
    let mut out = Vec::new();
    let mut lit = String::new();
    while let Some(this) = s.get()? {
        if !this.starts_with('\\') {
            lit.push_str(&this);
            continue;
        }
        let c = this.chars().nth(1).unwrap_or('\\');
        let tok_len = this.chars().count() as isize;
        if c == 'g' {
            if !s.matches('<')? {
                return Err(s.error("missing <", 0));
            }
            let name = s.getuntil('>', "group name")?;
            let name_len = name.chars().count() as isize;
            if !name.chars().all(|ch| ch.is_ascii_digit()) {
                if !is_identifier(&name) {
                    return Err(s.error(&format!("bad character in group name {}", repr_str(&name)), name_len + 1));
                }
                // Named groups do not exist: CPython raises IndexError here (no position).
                return Err(format!("unknown group name {}", repr_str(&name)));
            }
            let index = name.trim_start_matches('0');
            if !index.is_empty() {
                return Err(s.error(&format!("invalid group reference {index}"), name_len + 1));
            }
            flush(&mut lit, &mut out);
            out.push(Part::Group0);
        } else if c == '0' {
            // `\0`, `\0o`, `\0oo`: octal, up to three digits in total.
            let mut digits = String::from("0");
            if let Some(d) = s.next_char().filter(is_oct) {
                s.get()?;
                digits.push(d);
                if let Some(d) = s.next_char().filter(is_oct) {
                    s.get()?;
                    digits.push(d);
                }
            }
            let v = u32::from_str_radix(&digits, 8).unwrap_or(0) & 0xff;
            lit.push(char::from_u32(v).unwrap_or('\0'));
        } else if c.is_ascii_digit() {
            let mut this_digits = String::from(c);
            if let Some(d) = s.next_char().filter(|d| d.is_ascii_digit()) {
                s.get()?;
                this_digits.push(d);
                if is_oct(&c) && is_oct(&d) && s.next_char().as_ref().is_some_and(is_oct) {
                    let d3 = s.next_char().unwrap_or('0');
                    s.get()?;
                    this_digits.push(d3);
                    let v = u32::from_str_radix(&this_digits, 8).unwrap_or(0);
                    if v > 0o377 {
                        let shown = format!("\\{this_digits}");
                        return Err(s.error(&format!("octal escape value {shown} outside of range 0-0o377"), 4));
                    }
                    lit.push(char::from_u32(v).unwrap_or('\0'));
                    continue;
                }
            }
            // Every numbered group reference is invalid: the pattern has no capture groups.
            // CPython: `addgroup(int(this[1:]), len(this) - 1)`, i.e. the digit count as offset.
            let index = this_digits.trim_start_matches('0');
            let digits = this_digits.chars().count() as isize;
            return Err(s.error(&format!("invalid group reference {index}"), digits));
        } else {
            match escape_char(&this) {
                Some(ch) => lit.push(ch),
                None if c.is_ascii_alphabetic() => {
                    return Err(s.error(&format!("bad escape {this}"), tok_len));
                }
                None => lit.push_str(&this),
            }
        }
    }
    flush(&mut lit, &mut out);
    Ok(out)
}

fn flush(lit: &mut String, out: &mut Vec<Part>) {
    if !lit.is_empty() {
        out.push(Part::Lit(std::mem::take(lit)));
    }
}

/// Escapes CPython's template parser turns into a literal character (`re._parser.ESCAPES`).
fn escape_char(tok: &str) -> Option<char> {
    Some(match tok {
        "\\a" => '\u{07}',
        "\\b" => '\u{08}',
        "\\f" => '\u{0c}',
        "\\n" => '\n',
        "\\r" => '\r',
        "\\t" => '\t',
        "\\v" => '\u{0b}',
        "\\\\" => '\\',
        _ => return None,
    })
}

fn is_oct(c: &char) -> bool {
    ('0'..='7').contains(c)
}

fn is_identifier(s: &str) -> bool {
    let mut cs = s.chars();
    matches!(cs.next(), Some(c) if c == '_' || c.is_alphabetic()) && cs.all(|c| c == '_' || c.is_alphanumeric())
}

/// `[Errno N] msg at position P`, plus ` (line L, column C)` when the template spans lines.
fn err_at(src: &[char], msg: &str, pos: isize) -> String {
    let mut text = format!("{msg} at position {pos}");
    if src.contains(&'\n') {
        let upto = pos.clamp(0, src.len() as isize) as usize;
        let line = src[..upto].iter().filter(|&&c| c == '\n').count() + 1;
        let last = src[..upto].iter().rposition(|&c| c == '\n').map_or(-1, |i| i as isize);
        text.push_str(&format!(" (line {line}, column {})", pos - last));
    }
    text
}

/// Tokenizer of CPython's `re._parser`: a token is one character, or a backslash plus the next
/// character. `next` is the look-ahead token; `index` points past it.
struct Tokenizer<'a> {
    src: &'a [char],
    index: usize,
    next: Option<String>,
}

impl<'a> Tokenizer<'a> {
    fn new(src: &'a [char]) -> Result<Self, String> {
        let mut t = Tokenizer { src, index: 0, next: None };
        t.advance()?;
        Ok(t)
    }

    fn advance(&mut self) -> Result<(), String> {
        let mut index = self.index;
        let Some(&c) = self.src.get(index) else {
            self.next = None;
            return Ok(());
        };
        let mut tok = String::from(c);
        if c == '\\' {
            index += 1;
            match self.src.get(index) {
                Some(&d) => tok.push(d),
                None => return Err(err_at(self.src, "bad escape (end of pattern)", self.src.len() as isize - 1)),
            }
        }
        self.index = index + 1;
        self.next = Some(tok);
        Ok(())
    }

    /// The look-ahead token when it is a single character (CPython's `s.next in OCTDIGITS` etc.).
    fn next_char(&self) -> Option<char> {
        let mut it = self.next.as_deref()?.chars();
        let c = it.next()?;
        it.next().is_none().then_some(c)
    }

    fn get(&mut self) -> Result<Option<String>, String> {
        let this = self.next.take();
        self.advance()?;
        Ok(this)
    }

    fn matches(&mut self, ch: char) -> Result<bool, String> {
        if self.next_char() == Some(ch) {
            self.advance()?;
            return Ok(true);
        }
        Ok(false)
    }

    fn tell(&self) -> isize {
        let len = self.next.as_ref().map_or(0, |t| t.chars().count());
        self.index as isize - len as isize
    }

    fn error(&self, msg: &str, offset: isize) -> String {
        err_at(self.src, msg, self.tell() - offset)
    }

    fn getuntil(&mut self, term: char, name: &str) -> Result<String, String> {
        let mut result = String::new();
        loop {
            let c = self.next.clone();
            self.advance()?;
            match c {
                None if result.is_empty() => return Err(self.error(&format!("missing {name}"), 0)),
                None => {
                    let len = result.chars().count() as isize;
                    return Err(self.error(&format!("missing {term}, unterminated name"), len));
                }
                Some(c) if self.is_single(&c, term) => {
                    if result.is_empty() {
                        return Err(self.error(&format!("missing {name}"), 1));
                    }
                    return Ok(result);
                }
                Some(c) => result.push_str(&c),
            }
        }
    }

    fn is_single(&self, tok: &str, ch: char) -> bool {
        tok.chars().count() == 1 && tok.starts_with(ch)
    }
}

#[cfg(test)]
mod write_tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn write_values_is_atomic_and_keeps_the_file_mode() {
        use std::os::unix::fs::PermissionsExt;
        let game = std::env::temp_dir().join(format!("patchops-ini-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&game);
        let cfg = crate::game::config_path(&game);
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        std::fs::write(&cfg, "FOV = \"80\" // x\n").unwrap();
        std::fs::set_permissions(&cfg, std::fs::Permissions::from_mode(0o640)).unwrap();
        write_values(&game, &[("FOV".into(), ConfigValue::Str("90".into()), "c".into())]).unwrap();
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "FOV = \"90\" // c\n");
        assert_eq!(std::fs::metadata(&cfg).unwrap().permissions().mode() & 0o777, 0o640);
        assert_eq!(std::fs::read_dir(cfg.parent().unwrap()).unwrap().count(), 1, "no temp file left");
        let _ = std::fs::remove_dir_all(&game);
    }

    /// Same replacement string as the legacy `_write_config_values` (comment fixed to `c`).
    fn set(text: &str, key: &str, value: &str) -> Result<String, String> {
        apply_update(text, key, &format!("{key} = \"{value}\" // c"))
    }

    #[test]
    fn replaces_every_matching_line_and_keeps_others() {
        let t = "DrawFPS = \"0\" // a\nX = 1\nDrawFPS = \"1\" // b\n";
        assert_eq!(set(t, "DrawFPS", "9").unwrap(), "DrawFPS = \"9\" // c\nX = 1\nDrawFPS = \"9\" // c\n");
    }

    #[test]
    fn key_must_be_followed_by_equals_and_is_not_a_prefix() {
        // the prefix line and the commented line do not match; the indented one does, and its
        // leading indentation is part of the replaced text
        let t = "VsyncMode = \"adaptive\"\n// Vsync = \"1\"\n  Vsync  =  \"0\"\n";
        assert_eq!(set(t, "Vsync", "2").unwrap(), "VsyncMode = \"adaptive\"\n// Vsync = \"1\"\nVsync = \"2\" // c\n");
    }

    #[test]
    fn blank_lines_before_a_key_are_eaten() {
        assert_eq!(set("x\n\n\nA = 1\n", "A", "2").unwrap(), "x\nA = \"2\" // c\n");
    }

    #[test]
    fn no_match_appends_after_rstrip() {
        assert_eq!(set("a = 1  \n\n\t", "B", "2").unwrap(), "a = 1\nB = \"2\" // c\n");
        assert_eq!(set("", "B", "2").unwrap(), "\nB = \"2\" // c\n");
    }

    #[test]
    fn equals_may_sit_on_the_next_line() {
        // `\s*=` crosses a newline, and `.*$` then runs to the end of that line
        assert_eq!(set("K\n= 1 x\nZ\n", "K", "2").unwrap(), "K = \"2\" // c\nZ\n");
    }

    #[test]
    fn template_escapes_are_processed_on_replace() {
        assert_eq!(set("E = \"old\"\n", "E", "a\\nb").unwrap(), "E = \"a\nb\" // c\n");
        assert_eq!(set("E = 1\n", "E", "\\\\").unwrap(), "E = \"\\\" // c\n");
        assert_eq!(set("E = 1\n", "E", "\\g<0>").unwrap(), "E = \"E = 1\" // c\n");
        assert_eq!(set("E = 1\n", "E", "\\g<00>").unwrap(), "E = \"E = 1\" // c\n");
        assert_eq!(set("E = 1\n", "E", "\\g<0>\\g<0>").unwrap(), "E = \"E = 1E = 1\" // c\n");
        assert_eq!(set("E = 1\n", "E", "\\.").unwrap(), "E = \"\\.\" // c\n");
        assert_eq!(set("E = 1\n", "E", "x\\").unwrap(), "E = \"x\\\" // c\n");
        assert_eq!(set("E = 1\n", "E", "\\0").unwrap(), "E = \"\u{0}\" // c\n");
        assert_eq!(set("E = 1\n", "E", "\\101\\0777").unwrap(), "E = \"A?7\" // c\n");
        assert_eq!(set("E = 1\n", "E", "\\0777").unwrap(), "E = \"?7\" // c\n");
        assert_eq!(
            set("E = 1\n", "E", "\\\\\\n\\t\\b\\a\\f\\r\\v").unwrap(),
            "E = \"\\\n\t\u{8}\u{7}\u{c}\r\u{b}\" // c\n"
        );
        assert_eq!(set("E = 1\n", "E", "\\\n").unwrap(), "E = \"\\\n\" // c\n");
        assert_eq!(set("E = 1\n", "E", "é\\é").unwrap(), "E = \"é\\é\" // c\n");
    }

    #[test]
    fn append_path_does_not_parse_the_template() {
        assert_eq!(set("a\n", "B", "\\q").unwrap(), "a\nB = \"\\q\" // c\n");
    }

    #[test]
    fn bad_escape_texts_and_positions_match_cpython() {
        let t = "E = 1\n";
        assert_eq!(set(t, "E", "\\q").unwrap_err(), "bad escape \\q at position 5");
        assert_eq!(set(t, "E", "\\q\\").unwrap_err(), "bad escape \\q at position 5");
        assert_eq!(set(t, "E", "\\s").unwrap_err(), "bad escape \\s at position 5");
        assert_eq!(set(t, "E", "\\0\\x41\\.").unwrap_err(), "bad escape \\x at position 7");
        assert_eq!(set(t, "E", "a\n\\q").unwrap_err(), "bad escape \\q at position 7 (line 2, column 1)");
        assert_eq!(set(t, "E", "\\400").unwrap_err(), "octal escape value \\400 outside of range 0-0o377 at position 5");
        assert_eq!(set(t, "E", "\\g<1>").unwrap_err(), "invalid group reference 1 at position 8");
        assert_eq!(set(t, "E", "\\1").unwrap_err(), "invalid group reference 1 at position 6");
        assert_eq!(set(t, "E", "\\12").unwrap_err(), "invalid group reference 12 at position 6");
        assert_eq!(set(t, "E", "\\89").unwrap_err(), "invalid group reference 89 at position 6");
        assert_eq!(set(t, "E", "\\8").unwrap_err(), "invalid group reference 8 at position 6");
        assert_eq!(set(t, "E", "\\g<x>").unwrap_err(), "unknown group name 'x'");
        assert_eq!(set(t, "E", "\\g<>").unwrap_err(), "missing group name at position 8");
        assert_eq!(set(t, "E", "\\g<1x>").unwrap_err(), "bad character in group name '1x' at position 8");
        assert_eq!(set(t, "E", "\\g<1").unwrap_err(), "missing >, unterminated name at position 8");
        assert_eq!(set(t, "E", "\\g").unwrap_err(), "missing < at position 7");
        assert_eq!(set(t, "E", "\\gx").unwrap_err(), "missing < at position 7");
    }

    #[test]
    fn crlf_and_lone_cr_normalise_to_lf() {
        assert_eq!(normalize_newlines("a\r\nb\rc\n"), "a\nb\nc\n");
    }

    #[test]
    fn py_isspace_matches_cpython_set() {
        for c in ['\t', '\n', '\u{0b}', '\u{0c}', '\r', ' ', '\u{1c}', '\u{1f}', '\u{85}', '\u{a0}', '\u{3000}'] {
            assert!(py_isspace(c), "{c:?}");
        }
        assert!(!py_isspace('a') && !py_isspace('\u{200b}') && !py_isspace('\u{feff}'));
    }
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
    fn extract_matches_python_regex() {
        // expectations from `re.search(r'^\s*K\s*=\s*"([^"]+)"', s, re.M)` (CPython 3.13)
        assert_eq!(extract("\u{a0}MaxFPS = \"165\"", "MaxFPS").as_deref(), Some("165"));
        assert_eq!(extract("MaxFPS\n = \"9\"", "MaxFPS").as_deref(), Some("9"));
        assert_eq!(extract("X = \"a\"\"", "X").as_deref(), Some("a"));
        assert_eq!(extract("X = \"\"", "X"), None);
        assert_eq!(extract("X = \"a", "X"), None);
        assert_eq!(extract("  \n\n  X\t=\t\"v w\" // c", "X").as_deref(), Some("v w"));
        assert_eq!(extract("VsyncMode = \"x\"\nVsync = \"0\"", "Vsync").as_deref(), Some("0"));
        assert_eq!(extract("// FOV = \"70\"\nFOV = \"80\"", "FOV").as_deref(), Some("80"));
        assert_eq!(extract("A\r\nB = \"1\"", "B").as_deref(), Some("1"));
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
