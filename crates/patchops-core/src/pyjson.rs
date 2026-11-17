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
        // `json.dumps`: ints of any size as `str(int)`, floats as `repr(float)`, non-finite floats as `Infinity` / `NaN`.
        Value::Number(n) => out.push_str(match crate::pyfmt::py_str_of_number(n).as_str() {
            "inf" => "Infinity",
            "-inf" => "-Infinity",
            "nan" => "NaN",
            other => other,
        }),
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
        let nums: Value = serde_json::from_str("[9223372036854775808, -0, 123456789.12345679, 1e400, 1.0, 1e2, 7]").unwrap();
        assert_eq!(dumps_indent2(&nums), "[\n  9223372036854775808,\n  0,\n  123456789.12345679,\n  Infinity,\n  1.0,\n  100.0,\n  7\n]");
    }
}
