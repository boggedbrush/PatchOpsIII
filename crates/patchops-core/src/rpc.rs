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
        Err(e) => return json!({"id": null, "ok": false, "error": format!("validation: bad json: {e}")}).to_string(),
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
