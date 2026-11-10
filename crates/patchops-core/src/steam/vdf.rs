//! Minimal Valve KeyValues ("VDF") text parser + writer.
//!
//! Written because Steam library discovery (`libraryfolders.vdf`), workshop state
//! (`appworkshop_311210.acf`) and launch-option reads (`localconfig.vdf`) need it for `status`.
//!
//! Semantics follow Python `vdf` 3.x (`vdf.load`, `merge_duplicate_keys=True`, `escaped=True`):
//! dict-like insertion (a repeated key keeps its first position; repeated sections merge, a
//! repeated string value replaces the old one; a key used as both string and section is an
//! error, where python-vdf raises too), `//` comments, BOM stripped, `[$COND]` suffixes
//! ignored, escapes `\n \t \v \b \r \f \a \\ \? \" \'` decoded in quoted strings.
//!
//! The *writer* ([`dump_pretty`]) is byte-identical to `vdf.dump(obj, fp, pretty=True)`: tab
//! indent and a **single space** between key and value (`"key" "value"`). (Steam itself writes
//! `"key"\t\t"value"`, which the parser accepts; the `\t\t` form seen in the original files and
//! in `localconfig_backup.vdf` is the *input*, the rewritten goldens use one space.) Checked
//! against `bench/golden/080-*.json` and the vdf 3.4 source.

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Str(String),
    Obj(Obj),
}

/// Ordered string-keyed map.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Obj(pub Vec<(String, Node)>);

impl Obj {
    pub fn get(&self, key: &str) -> Option<&Node> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Node> {
        self.0.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    pub fn get_obj(&self, key: &str) -> Option<&Obj> {
        match self.get(key) {
            Some(Node::Obj(o)) => Some(o),
            _ => None,
        }
    }
    pub fn get_str(&self, key: &str) -> Option<&str> {
        match self.get(key) {
            Some(Node::Str(s)) => Some(s),
            _ => None,
        }
    }
    /// Walk nested sections: `get_path(&["Software", "Valve"])`.
    pub fn get_path(&self, path: &[&str]) -> Option<&Node> {
        let (last, parents) = path.split_last()?;
        let mut cur = self;
        for p in parents {
            cur = cur.get_obj(p)?;
        }
        cur.get(last)
    }
    /// Python-dict style set: keeps the position of an existing key.
    pub fn set(&mut self, key: &str, node: Node) {
        match self.get_mut(key) {
            Some(slot) => *slot = node,
            None => self.0.push((key.to_string(), node)),
        }
    }
    /// Get-or-create a section (replacing a string value with a section, like the parser).
    pub fn section_mut(&mut self, key: &str) -> &mut Obj {
        if !matches!(self.get(key), Some(Node::Obj(_))) {
            self.set(key, Node::Obj(Obj::default()));
        }
        match self.get_mut(key) {
            Some(Node::Obj(o)) => o,
            _ => unreachable!(),
        }
    }
}

#[derive(Debug)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "vdf: {}", self.0)
    }
}
impl std::error::Error for ParseError {}

/// Sections nested deeper than this are refused (real files nest < 10 levels); keeps recursion
/// bounded on hostile input.
const MAX_DEPTH: usize = 128;

enum Tok<'a> {
    Str(std::borrow::Cow<'a, str>),
    Open,
    Close,
}

fn unescape_byte(c: u8) -> Option<char> {
    Some(match c {
        b'n' => '\n',
        b't' => '\t',
        b'v' => '\u{b}',
        b'b' => '\u{8}',
        b'r' => '\r',
        b'f' => '\u{c}',
        b'a' => '\u{7}',
        b'\\' => '\\',
        b'?' => '?',
        b'"' => '"',
        b'\'' => '\'',
        _ => return None,
    })
}

/// Single-pass tokenizer over the UTF-8 bytes (no `Vec<char>`); quoted strings without escapes
/// are borrowed from the input.
struct Lexer<'a> {
    text: &'a str,
    b: &'a [u8],
    i: usize,
}

impl<'a> Lexer<'a> {
    fn new(text: &'a str) -> Self {
        let text = text.trim_start_matches(['\u{feff}', '\u{fffe}']);
        Lexer { text, b: text.as_bytes(), i: 0 }
    }

    /// `(is_whitespace, byte length)` of the char starting at `i` (a char boundary), with the
    /// semantics of `char::is_whitespace`.
    fn char_at(&self, i: usize) -> (bool, usize) {
        let c = self.b[i];
        if c < 0x80 {
            return (matches!(c, 9..=13 | 32), 1);
        }
        match self.text[i..].chars().next() {
            Some(ch) => (ch.is_whitespace(), ch.len_utf8()),
            None => (false, 1),
        }
    }

    fn next(&mut self) -> Result<Option<Tok<'a>>, ParseError> {
        let n = self.b.len();
        while self.i < n {
            let c = self.b[self.i];
            let (is_ws, clen) = self.char_at(self.i);
            if is_ws {
                self.i += clen;
            } else if c == b'/' && self.b.get(self.i + 1) == Some(&b'/') {
                while self.i < n && self.b[self.i] != b'\n' {
                    self.i += 1;
                }
            } else if c == b'{' {
                self.i += 1;
                return Ok(Some(Tok::Open));
            } else if c == b'}' {
                self.i += 1;
                return Ok(Some(Tok::Close));
            } else if c == b'"' {
                self.i += 1;
                return self.quoted().map(|s| Some(Tok::Str(s)));
            } else if c == b'[' {
                // conditional like [$WIN32]: ignore
                while self.i < n && self.b[self.i] != b']' {
                    self.i += 1;
                }
                self.i += 1;
            } else {
                let start = self.i;
                while self.i < n && !matches!(self.b[self.i], b'{' | b'}' | b'"') {
                    let (is_ws, clen) = self.char_at(self.i);
                    if is_ws {
                        break;
                    }
                    self.i += clen;
                }
                return Ok(Some(Tok::Str(self.text[start..self.i].into())));
            }
        }
        Ok(None)
    }

    /// Body of a quoted string (opening quote already consumed).
    fn quoted(&mut self) -> Result<std::borrow::Cow<'a, str>, ParseError> {
        let eof = || ParseError("unexpected EOF (open quote)".into());
        let start = self.i;
        let n = self.b.len();
        // Fast path: no escapes.
        let mut j = start;
        while j < n && self.b[j] != b'"' && self.b[j] != b'\\' {
            j += 1;
        }
        if j >= n {
            return Err(eof());
        }
        if self.b[j] == b'"' {
            self.i = j + 1;
            return Ok(self.text[start..j].into());
        }
        let mut s = String::from(&self.text[start..j]);
        let mut seg = j; // start of the pending literal run
        loop {
            if j >= n {
                return Err(eof());
            }
            match self.b[j] {
                b'"' => {
                    s.push_str(&self.text[seg..j]);
                    self.i = j + 1;
                    return Ok(s.into());
                }
                b'\\' => match self.b.get(j + 1).and_then(|&e| unescape_byte(e)) {
                    Some(u) => {
                        s.push_str(&self.text[seg..j]);
                        s.push(u);
                        j += 2;
                        seg = j;
                    }
                    None => j += 1, // lone backslash stays literal
                },
                _ => j += 1,
            }
        }
    }
}

/// Parse-time section: insertion-ordered items plus a key index (built once a section grows past
/// a handful of keys) so duplicate-key handling is O(1) instead of a linear scan.
#[derive(Default)]
struct Sec {
    items: Vec<(String, PNode)>,
    index: Option<std::collections::HashMap<String, usize>>,
}

enum PNode {
    Str(String),
    Sec(Sec),
}

const INDEX_AT: usize = 12;

impl Sec {
    fn find(&self, key: &str) -> Option<usize> {
        match &self.index {
            Some(ix) => ix.get(key).copied(),
            None => self.items.iter().position(|(k, _)| k == key),
        }
    }

    fn push(&mut self, key: String, node: PNode) {
        if let Some(ix) = &mut self.index {
            ix.insert(key.clone(), self.items.len());
        }
        self.items.push((key, node));
        if self.index.is_none() && self.items.len() > INDEX_AT {
            self.index = Some(self.items.iter().enumerate().map(|(i, (k, _))| (k.clone(), i)).collect());
        }
    }

    /// Dict-style insert with python-vdf's merge semantics: sections merge recursively, a repeated
    /// string replaces in place (keeping the first position). A string/section clash for one key
    /// is an error (python-vdf raises there; we must not silently drop data and rewrite the file).
    fn insert(&mut self, key: String, node: PNode) -> Result<(), ParseError> {
        let Some(i) = self.find(&key) else {
            self.push(key, node);
            return Ok(());
        };
        match (&mut self.items[i].1, node) {
            (PNode::Str(old), PNode::Str(new)) => *old = new,
            (PNode::Sec(old), PNode::Sec(new)) => {
                for (k, v) in new.items {
                    old.insert(k, v)?;
                }
            }
            _ => return Err(ParseError(format!("key {key:?} is used both as a value and as a section"))),
        }
        Ok(())
    }

    fn into_obj(self) -> Obj {
        Obj(self.items.into_iter().map(|(k, v)| (k, v.into_node())).collect())
    }
}

impl PNode {
    fn into_node(self) -> Node {
        match self {
            PNode::Str(s) => Node::Str(s),
            PNode::Sec(s) => Node::Obj(s.into_obj()),
        }
    }
}

/// Parse a VDF document into its top-level section (linear time).
pub fn parse(text: &str) -> Result<Obj, ParseError> {
    let mut lx = Lexer::new(text);
    Ok(parse_sec(&mut lx, 0)?.into_obj())
}

fn parse_sec(lx: &mut Lexer<'_>, depth: usize) -> Result<Sec, ParseError> {
    if depth > MAX_DEPTH {
        return Err(ParseError("sections nested too deeply".into()));
    }
    let top = depth == 0;
    let mut sec = Sec::default();
    loop {
        match lx.next()? {
            None => {
                return if top { Ok(sec) } else { Err(ParseError("unclosed section (EOF)".into())) };
            }
            Some(Tok::Close) => {
                return if top { Err(ParseError("one too many closing braces".into())) } else { Ok(sec) };
            }
            Some(Tok::Open) => return Err(ParseError("unexpected opening brace".into())),
            Some(Tok::Str(key)) => {
                let key = key.into_owned();
                match lx.next()? {
                    Some(Tok::Open) => {
                        let child = parse_sec(lx, depth + 1)?;
                        sec.insert(key, PNode::Sec(child))?;
                    }
                    Some(Tok::Str(v)) => sec.insert(key, PNode::Str(v.into_owned()))?,
                    _ => return Err(ParseError(format!("missing value for key {key:?}"))),
                }
            }
        }
    }
}

fn escape_into(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\u{b}' => out.push_str("\\v"),
            '\u{8}' => out.push_str("\\b"),
            '\r' => out.push_str("\\r"),
            '\u{c}' => out.push_str("\\f"),
            '\u{7}' => out.push_str("\\a"),
            '\\' => out.push_str("\\\\"),
            '?' => out.push_str("\\?"),
            '"' => out.push_str("\\\""),
            '\'' => out.push_str("\\'"),
            c => out.push(c),
        }
    }
}

/// `vdf.dumps(obj, pretty=True)`.
pub fn dump_pretty(obj: &Obj) -> String {
    let mut out = String::new();
    dump_level(&mut out, obj, 0);
    out
}

fn indent(out: &mut String, level: usize) {
    out.extend(std::iter::repeat_n('\t', level));
}

fn dump_level(out: &mut String, obj: &Obj, level: usize) {
    for (k, v) in &obj.0 {
        indent(out, level);
        out.push('"');
        escape_into(out, k);
        out.push('"');
        match v {
            Node::Obj(o) => {
                out.push('\n');
                indent(out, level);
                out.push_str("{\n");
                dump_level(out, o, level + 1);
                indent(out, level);
                out.push_str("}\n");
            }
            Node::Str(s) => {
                out.push_str(" \"");
                escape_into(out, s);
                out.push_str("\"\n");
            }
        }
    }
}

/// Parse a UTF-8 VDF file (Python `open(.., encoding="utf-8")` + `vdf.load`). The error is the
/// text to log.
pub fn load_file(path: &std::path::Path) -> Result<Obj, String> {
    let bytes = std::fs::read(path).map_err(|e| crate::pyfmt::io_error_text(&e, Some(path)))?;
    let text = String::from_utf8(bytes).map_err(|e| {
        let u = e.utf8_error();
        let at = u.valid_up_to();
        let byte = e.as_bytes()[at];
        format!("'utf-8' codec can't decode byte 0x{byte:02x} in position {at}: invalid start byte")
    })?;
    parse(&text).map_err(|e| e.0)
}

/// Write `obj` with [`dump_pretty`], atomically (temp file + rename; permission bits kept).
pub fn write_file(path: &std::path::Path, obj: &Obj) -> std::io::Result<()> {
    crate::fsutil::atomic_write(path, dump_pretty(obj).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_library_folders() {
        let t = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"/a b\"\n\t}\n\t\"contentstatsid\" \"7\"\n}\n";
        let o = parse(t).unwrap();
        let lf = o.get_obj("libraryfolders").unwrap();
        assert_eq!(lf.get_obj("0").unwrap().get_str("path"), Some("/a b"));
        assert_eq!(lf.get_str("contentstatsid"), Some("7"));
    }

    #[test]
    fn escapes_and_comments() {
        let t = "// c\n\"A\" { \"LaunchOptions\" \"WINE=\\\"x\\\" %command%\" }";
        let o = parse(t).unwrap();
        assert_eq!(o.get_path(&["A", "LaunchOptions"]), Some(&Node::Str("WINE=\"x\" %command%".into())));
        assert!(dump_pretty(&o).contains("\\\"x\\\""));
    }

    #[test]
    fn merges_duplicate_sections() {
        let o = parse("\"a\" { \"x\" \"1\" } \"a\" { \"y\" \"2\" }").unwrap();
        let a = o.get_obj("a").unwrap();
        assert_eq!(a.get_str("x"), Some("1"));
        assert_eq!(a.get_str("y"), Some("2"));
    }

    #[test]
    fn errors() {
        assert!(parse("\"a\" { \"x\" \"1\"").is_err());
        assert!(parse("}").is_err());
    }

    #[test]
    fn dump_matches_python_vdf() {
        let o = parse("\"A\"\n{\n\t\"k\"\t\t\"v\"\n\t\"S\"\n\t{\n\t\t\"x\" \"a?b'c\\\\d\"\n\t}\n}\n").unwrap();
        assert_eq!(dump_pretty(&o), "\"A\"\n{\n\t\"k\" \"v\"\n\t\"S\"\n\t{\n\t\t\"x\" \"a\\?b\\'c\\\\d\"\n\t}\n}\n");
    }

    #[test]
    fn round_trip_is_stable() {
        let t = "\"R\"\n{\n\t\"a\" \"line\\nbreak \\\"q\\\" tab\\t\"\n\t\"n\"\n\t{\n\t\t\"\" \"\"\n\t}\n}\n";
        let o = parse(t).unwrap();
        assert_eq!(dump_pretty(&o), t);
        assert_eq!(parse(&dump_pretty(&o)).unwrap(), o);
    }

    #[test]
    fn duplicate_string_replaces_in_place() {
        let o = parse("\"a\" \"1\" \"b\" \"2\" \"a\" \"3\"").unwrap();
        assert_eq!(o.0[0], ("a".to_string(), Node::Str("3".into())));
        assert_eq!(o.0.len(), 2);
    }

    #[test]
    fn string_section_conflicts_are_errors() {
        for t in [
            "\"a\" \"1\" \"a\" { \"x\" \"y\" }",
            "\"a\" { \"x\" \"y\" } \"a\" \"1\"",
            "\"a\" { \"x\" \"y\" } \"a\" { \"x\" { } }",
            "\"a\" { \"x\" { } } \"a\" { \"x\" \"y\" }",
        ] {
            assert!(parse(t).is_err(), "{t}");
        }
    }

    #[test]
    fn large_sections_with_duplicates_merge_in_order() {
        let mut t = String::from("\"r\" {");
        for i in 0..100 {
            t.push_str(&format!("\"k{i}\" {{ \"a\" \"1\" }} "));
        }
        for i in (0..100).step_by(7) {
            t.push_str(&format!("\"k{i}\" {{ \"b\" \"2\" }} "));
        }
        t.push_str("\"k3\" {\"a\" \"9\"} }");
        let o = parse(&t).unwrap();
        let r = o.get_obj("r").unwrap();
        assert_eq!(r.0.len(), 100);
        assert_eq!(r.0[7].0, "k7");
        assert_eq!(r.get_path(&["k7", "b"]), Some(&Node::Str("2".into())));
        assert_eq!(r.get_path(&["k3", "a"]), Some(&Node::Str("9".into())));
        assert_eq!(r.get_obj("k3").unwrap().0.len(), 1);
    }

    #[test]
    fn deep_nesting_is_an_error_not_a_stack_overflow() {
        let t = "\"a\" {".repeat(100_000);
        assert!(parse(&t).is_err());
    }

    #[test]
    fn unicode_whitespace_and_escapes() {
        let o = parse("\"a\"\u{a0}\"x\\ty\\q\u{e9}\" \u{85}\"b\" \"c\"").unwrap();
        assert_eq!(o.get_str("a"), Some("x\ty\\q\u{e9}"));
        assert_eq!(o.get_str("b"), Some("c"));
    }

    #[test]
    fn bare_multibyte_tokens_do_not_split_chars() {
        let o = parse("k\u{e9}y v\u{e4}l\u{a0}\u{4e2d}\u{6587} \u{1f600}").unwrap();
        assert_eq!(o.get_str("k\u{e9}y"), Some("v\u{e4}l"));
        assert_eq!(o.get_str("\u{4e2d}\u{6587}"), Some("\u{1f600}"));
    }

    #[test]
    fn write_file_is_atomic_and_keeps_mode() {
        let d = std::env::temp_dir().join(format!("patchops-vdfw-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("x.vdf");
        std::fs::write(&p, "old").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o640)).unwrap();
        }
        write_file(&p, &parse("\"a\" \"b\"").unwrap()).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "\"a\" \"b\"\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o640);
        }
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn bom_and_conditionals() {
        let o = parse("\u{feff}\"a\" { \"k\" \"v\" [$WIN32] }").unwrap();
        assert_eq!(o.get_path(&["a", "k"]), Some(&Node::Str("v".into())));
    }
}
