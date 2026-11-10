//! JSON-lines RPC over stdin/stdout.
//!
//! One JSON object per line in, one per line out:
//! `{"id":1,"op":"health"}` -> `{"id":1,"ok":true,"result":{...}}`
//!
//! Two entry points:
//! * `run_headless` (`patchops --rpc-stdio`): no GPUI, no display, no GPU.
//! * `spawn_in_gui` (env `PATCHOPS_BENCH_RPC=1`): same loop on a background
//!   thread inside the GUI process, talking to the live `SharedState`.

use std::io::{BufRead, Write};
use std::sync::Arc;

use serde_json::{json, Value};

use crate::state::{SharedState, UiEvent};

/// Handle one request line; always returns a single-line JSON response.
pub fn handle_line(state: &SharedState, line: &str) -> String {
    let req: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return json!({"ok": false, "error": format!("bad json: {e}")}).to_string(),
    };
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let op = req.get("op").and_then(Value::as_str).unwrap_or("");
    let result: Result<Value, String> = match op {
        "health" => Ok(json!({
            "version": patchops_core::version(),
            "mode": if state.is_gui() { "gui" } else { "headless" },
            "pid": std::process::id(),
        })),
        "echo" => Ok(req.get("data").cloned().unwrap_or(Value::Null)),
        "get_state" => {
            let d = state.snapshot();
            Ok(json!({
                "counter": d.counter, "toggle": d.toggle, "slider": d.slider,
                "text": d.text, "log_len": d.log.len(),
            }))
        }
        "click" => {
            let n = state.with(|d| {
                d.counter += 1;
                d.counter
            });
            state.notify_ui(UiEvent::Refresh);
            Ok(json!({ "counter": n }))
        }
        "set_toggle" => {
            let v = req.get("value").and_then(Value::as_bool).unwrap_or(false);
            state.with(|d| d.toggle = v);
            state.notify_ui(UiEvent::Refresh);
            Ok(json!({ "toggle": v }))
        }
        "set_slider" => match req.get("value").and_then(Value::as_f64) {
            Some(v) => {
                let v = (v as f32).clamp(0.0, 1.0);
                state.with(|d| d.slider = v);
                state.notify_ui(UiEvent::Refresh);
                Ok(json!({ "slider": v }))
            }
            None => Err("missing numeric `value`".into()),
        },
        "push_log" => {
            let line = req.get("line").and_then(Value::as_str).unwrap_or("");
            state.push_log(line);
            Ok(json!({ "log_len": state.log_len() }))
        }
        "navigate" => match req.get("index").and_then(Value::as_u64) {
            Some(i) => {
                state.notify_ui(UiEvent::Navigate(i as usize));
                Ok(json!({ "index": i }))
            }
            None => Err("missing integer `index`".into()),
        },
        other => Err(format!("unknown op `{other}`")),
    };
    match result {
        Ok(r) => json!({"id": id, "ok": true, "result": r}),
        Err(e) => json!({"id": id, "ok": false, "error": e}),
    }
    .to_string()
}

/// Blocking stdin -> stdout loop. Returns at EOF.
pub fn serve_stdio(state: &SharedState) {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let resp = handle_line(state, &line);
        if writeln!(out, "{resp}").and_then(|_| out.flush()).is_err() {
            break;
        }
    }
}

/// `patchops --rpc-stdio`: never touches GPUI.
pub fn run_headless() {
    serve_stdio(&SharedState::new());
}

/// Run the same loop on a background thread inside the GUI process.
pub fn spawn_in_gui(state: Arc<SharedState>) {
    std::thread::Builder::new()
        .name("patchops-rpc".into())
        .spawn(move || serve_stdio(&state))
        .expect("spawn rpc thread");
}
