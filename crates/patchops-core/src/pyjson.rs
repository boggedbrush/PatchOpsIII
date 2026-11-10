//! `json.dumps(value, indent=2)` as Python writes it (`ensure_ascii=True`, no trailing newline),
//! so files written by the port are byte-identical to the legacy ones.

use serde_json::Value;

pub fn dumps_indent2(v: &Value) -> String {
    let mut out = String::new();
    write(&mut out, v, 0);
    out
}

fn indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str("  ");
    }
}

fn write(out: &mut String, v: &Value, level: usize) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if let Some(f) = n.as_f64().filter(|_| n.is_f64()) {
                out.push_str(&crate::pyfmt::float_str(f));
            } else {
                out.push_str(&n.to_string());
            }
        }
        Value::String(s) => write_str(out, s),
        Value::Array(a) => {
            if a.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push_str("[\n");
            for (i, item) in a.iter().enumerate() {
                if i > 0 {
                    out.push_str(",\n");
                }
                indent(out, level + 1);
                write(out, item, level + 1);
            }
            out.push('\n');
            indent(out, level);
            out.push(']');
        }
        Value::Object(m) => {
            if m.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push_str("{\n");
            for (i, (k, item)) in m.iter().enumerate() {
                if i > 0 {
                    out.push_str(",\n");
                }
                indent(out, level + 1);
                write_str(out, k);
                out.push_str(": ");
                write(out, item, level + 1);
            }
            out.push('\n');
            indent(out, level);
            out.push('}');
        }
    }
}

fn write_str(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if (c as u32) <= 0x7f => out.push(c),
            c => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{:04x}", u));
                }
            }
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn matches_python() {
        let v = json!({"game_dir": "/a/\u{e9}", "n": [1, 2], "e": {}, "l": []});
        assert_eq!(
            dumps_indent2(&v),
            "{\n  \"game_dir\": \"/a/\\u00e9\",\n  \"n\": [\n    1,\n    2\n  ],\n  \"e\": {},\n  \"l\": []\n}"
        );
        assert_eq!(dumps_indent2(&json!({})), "{}");
    }
}
