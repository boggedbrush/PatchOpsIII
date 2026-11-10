//! Minimal Valve KeyValues ("VDF") text parser + writer.
//!
//! Written because Steam library discovery (`libraryfolders.vdf`), workshop state
//! (`appworkshop_311210.acf`) and launch-option reads (`localconfig.vdf`) need it for `status`.
//!
//! Semantics follow Python `vdf` 3.x (`vdf.load`, `merge_duplicate_keys=True`, `escaped=True`):
//! dict-like insertion (a repeated key keeps its first position; repeated sections merge, a
//! repeated string value replaces the old one), `//` comments, BOM stripped, `[$COND]` suffixes
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

#[derive(Debug, PartialEq)]
enum Tok {
    Str(String),
    Open,
    Close,
}

fn unescape_char(c: char) -> Option<char> {
    Some(match c {
        'n' => '\n',
        't' => '\t',
        'v' => '\u{b}',
        'b' => '\u{8}',
        'r' => '\r',
        'f' => '\u{c}',
        'a' => '\u{7}',
        '\\' => '\\',
        '?' => '?',
        '"' => '"',
        '\'' => '\'',
        _ => return None,
    })
}

fn tokenize(text: &str) -> Result<Vec<Tok>, ParseError> {
    let text = text.trim_start_matches(['\u{feff}', '\u{fffe}']);
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '{' {
            out.push(Tok::Open);
            i += 1;
        } else if c == '}' {
            out.push(Tok::Close);
            i += 1;
        } else if c == '"' {
            i += 1;
            let mut s = String::new();
            loop {
                let Some(&ch) = chars.get(i) else {
                    return Err(ParseError("unexpected EOF (open quote)".into()));
                };
                i += 1;
                if ch == '"' {
                    break;
                }
                if ch == '\\' {
                    if let Some(&n) = chars.get(i) {
                        if let Some(u) = unescape_char(n) {
                            s.push(u);
                            i += 1;
                            continue;
                        }
                    }
                }
                s.push(ch);
            }
            out.push(Tok::Str(s));
        } else if c == '[' {
            // conditional like [$WIN32]: ignore
            while i < chars.len() && chars[i] != ']' {
                i += 1;
            }
            i += 1;
        } else {
            let start = i;
            while i < chars.len() && !chars[i].is_whitespace() && !matches!(chars[i], '{' | '}' | '"') {
                i += 1;
            }
            out.push(Tok::Str(chars[start..i].iter().collect()));
        }
    }
    Ok(out)
}

/// Parse a VDF document into its top-level section.
pub fn parse(text: &str) -> Result<Obj, ParseError> {
    let toks = tokenize(text)?;
    let mut pos = 0;
    let obj = parse_obj(&toks, &mut pos, true)?;
    Ok(obj)
}

fn parse_obj(toks: &[Tok], pos: &mut usize, top: bool) -> Result<Obj, ParseError> {
    let mut obj = Obj::default();
    loop {
        match toks.get(*pos) {
            None => {
                return if top { Ok(obj) } else { Err(ParseError("unclosed section (EOF)".into())) };
            }
            Some(Tok::Close) => {
                *pos += 1;
                return if top { Err(ParseError("one too many closing braces".into())) } else { Ok(obj) };
            }
            Some(Tok::Open) => return Err(ParseError("unexpected opening brace".into())),
            Some(Tok::Str(key)) => {
                *pos += 1;
                match toks.get(*pos) {
                    Some(Tok::Open) => {
                        *pos += 1;
                        let child = parse_obj(toks, pos, false)?;
                        merge_node(&mut obj, key, Node::Obj(child));
                    }
                    Some(Tok::Str(v)) => {
                        *pos += 1;
                        obj.set(key, Node::Str(v.clone()));
                    }
                    _ => return Err(ParseError(format!("missing value for key {key:?}"))),
                }
            }
        }
    }
}

/// Dict-style insert: sections merge recursively, anything else replaces in place.
fn merge_node(base: &mut Obj, key: &str, node: Node) {
    let idx = base.0.iter().position(|(k, _)| k == key);
    match (idx, node) {
        (Some(i), Node::Obj(new)) => match &mut base.0[i].1 {
            Node::Obj(existing) => {
                for (k, v) in new.0 {
                    merge_node(existing, &k, v);
                }
            }
            slot => *slot = Node::Obj(new),
        },
        (Some(i), n) => base.0[i].1 = n,
        (None, n) => base.0.push((key.to_string(), n)),
    }
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
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
    out
}

/// `vdf.dumps(obj, pretty=True)`.
pub fn dump_pretty(obj: &Obj) -> String {
    let mut out = String::new();
    dump_level(&mut out, obj, 0);
    out
}

fn dump_level(out: &mut String, obj: &Obj, level: usize) {
    let ind = "\t".repeat(level);
    for (k, v) in &obj.0 {
        match v {
            Node::Obj(o) => {
                out.push_str(&format!("{ind}\"{}\"\n{ind}{{\n", escape(k)));
                dump_level(out, o, level + 1);
                out.push_str(&format!("{ind}}}\n"));
            }
            Node::Str(s) => out.push_str(&format!("{ind}\"{}\" \"{}\"\n", escape(k), escape(s))),
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

/// Write `obj` with [`dump_pretty`] (truncating, like Python's `open(.., "w")`).
pub fn write_file(path: &std::path::Path, obj: &Obj) -> std::io::Result<()> {
    std::fs::write(path, dump_pretty(obj))
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
    fn duplicate_string_replaces_in_place_and_str_to_section() {
        let o = parse("\"a\" \"1\" \"b\" \"2\" \"a\" \"3\" \"b\" { \"x\" \"y\" }").unwrap();
        assert_eq!(o.0[0], ("a".to_string(), Node::Str("3".into())));
        assert!(matches!(o.get("b"), Some(Node::Obj(_))));
    }

    #[test]
    fn bom_and_conditionals() {
        let o = parse("\u{feff}\"a\" { \"k\" \"v\" [$WIN32] }").unwrap();
        assert_eq!(o.get_path(&["a", "k"]), Some(&Node::Str("v".into())));
    }
}
