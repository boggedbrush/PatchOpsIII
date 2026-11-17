//! JSON-lines RPC transport (the contract in `bench/OPS.md` section 1), shared by the GUI binary
//! (`patchops --rpc-stdio`, in-GUI bench RPC) and the GUI-free `patchops-rpc` dev binary.
//!
//! One JSON object per line in, one per line out:
//! `{"id":1,"op":"health","args":{}}` -> `{"id":1,"ok":true,"result":{...}}`
//! or `{"id":1,"ok":false,"error":"validation: ..."}`.
//!
//! Only protocol lines are written to stdout; diagnostics go to stderr.

use std::io::{BufRead, Write};

use serde_json::{json, Value};

use crate::Core;

/// Handle one request line; always returns a single-line JSON response.
pub fn handle_line(core: &Core, line: &str) -> String {
    let req: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        // The line is not a JSON document `serde_json` accepts (e.g. a `NaN` literal, a truncated tail). If it still
        // looks like an object, echo its `id` so an id-matching client is not left waiting for the answer.
        Err(e) => return json!({"id": salvage_id(line), "ok": false, "error": format!("validation: bad json: {e}")}).to_string(),
    };
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let Some(op) = req.get("op").and_then(Value::as_str) else {
        return json!({"id": id, "ok": false, "error": "validation: missing `op`"}).to_string();
    };
    let args = req.get("args").unwrap_or(&Value::Null);
    match crate::ops::dispatch(core, op, args) {
        Ok(result) => json!({"id": id, "ok": true, "result": result}),
        Err(e) => json!({"id": id, "ok": false, "error": e.to_string()}),
    }
    .to_string()
}

/// The value of a top-level `"id"` member of a line that is at least shaped like a JSON object, found by a
/// tolerant scan (values are skipped, not validated); `null` when there is none or the line is not an object.
fn salvage_id(line: &str) -> Value {
    let b = line.as_bytes();
    let skip_ws = |mut i: usize| {
        while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | b'\r') {
            i += 1;
        }
        i
    };
    // end (exclusive) of the string starting at `i` (`b[i] == '"'`)
    let string_end = |mut i: usize| -> Option<usize> {
        i += 1;
        while i < b.len() {
            match b[i] {
                b'\\' => i += 2,
                b'"' => return Some(i + 1),
                _ => i += 1,
            }
        }
        None
    };
    // end (exclusive) of the value starting at `i`
    let value_end = |start: usize| -> Option<usize> {
        let mut i = start;
        let mut depth = 0usize;
        while i < b.len() {
            match b[i] {
                b'"' => i = string_end(i)?,
                b'{' | b'[' => {
                    depth += 1;
                    i += 1;
                }
                b'}' | b']' if depth > 0 => {
                    depth -= 1;
                    i += 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                b',' | b'}' | b']' if depth == 0 => return Some(i),
                _ => i += 1,
            }
        }
        (depth == 0).then_some(i)
    };
    let mut i = skip_ws(0);
    if b.get(i) != Some(&b'{') {
        return Value::Null;
    }
    i += 1;
    let mut id = Value::Null;
    loop {
        i = skip_ws(i);
        if b.get(i) != Some(&b'"') {
            break;
        }
        let Some(kend) = string_end(i) else { break };
        let key = serde_json::from_str::<String>(&line[i..kend]).unwrap_or_default();
        i = skip_ws(kend);
        if b.get(i) != Some(&b':') {
            break;
        }
        i = skip_ws(i + 1);
        let Some(vend) = value_end(i) else { break };
        if key == "id" {
            if let Ok(v) = serde_json::from_str::<Value>(line[i..vend].trim()) {
                id = v;
            }
        }
        i = skip_ws(vend);
        if b.get(i) == Some(&b',') {
            i += 1;
        } else {
            break;
        }
    }
    id
}

/// Blocking stdin -> stdout loop. Returns at stdin EOF (or when stdout is closed).
pub fn serve_stdio(core: &Core) {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    let mut input = stdin.lock();
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match input.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let resp = handle_line(core, line);
        if writeln!(out, "{resp}").and_then(|_| out.flush()).is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn salvage_finds_the_id_of_unparsable_objects() {
        assert_eq!(salvage_id(r#"{"id": 7, "op": "x", "args": {"v": NaN}}"#), json!(7));
        assert_eq!(salvage_id(r#"{"op": "config", "args": {"a": [1, {"id": 9}], "s": "}\"id\":5"}, "id": "abc" , "z": Infinity}"#), json!("abc"));
        assert_eq!(salvage_id(r#"  {"id":null,"op":"x""#), Value::Null);
        assert_eq!(salvage_id(r#"{"id": 12, "op": "x" "#), json!(12));
        assert_eq!(salvage_id("not json"), Value::Null);
        assert_eq!(salvage_id("[1,2]"), Value::Null);
        assert_eq!(salvage_id(r#"{"id": NaN, "op": "x"}"#), Value::Null);
        assert_eq!(salvage_id(r#"{"id": {"a": 1}, "args": nope}"#), json!({"a": 1}));
    }

    #[test]
    fn out_of_range_number_literal_is_accepted_and_bad_lines_echo_their_id() {
        let root = std::env::temp_dir().join(format!("patchops-rpc-test-{}", std::process::id()));
        let core = crate::Core::with_paths(crate::paths::Paths::with_app_data(root.join("home"), root.join("app")), "t".into());
        let ok: Value = serde_json::from_str(&handle_line(&core, r#"{"id": 3, "op": "health", "args": {"x": 1e400}}"#)).unwrap();
        assert_eq!(ok["id"], json!(3));
        assert_eq!(ok["ok"], json!(true));
        let bad: Value = serde_json::from_str(&handle_line(&core, r#"{"id": 4, "op": "health", "args": {"x": NaN}}"#)).unwrap();
        assert_eq!(bad["id"], json!(4));
        assert_eq!(bad["ok"], json!(false));
        let junk: Value = serde_json::from_str(&handle_line(&core, "garbage")).unwrap();
        assert_eq!(junk["id"], Value::Null);
    }
}
