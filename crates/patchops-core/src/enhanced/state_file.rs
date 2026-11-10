//! `<app_data>/bo3_enhanced_state.json` (legacy `bo3_enhanced.load_state/save_state`) and the
//! derived `status_summary`.

use std::path::PathBuf;

use serde_json::{Map, Value};

use crate::context::Core;
use crate::ops::enhanced::detect_enhanced_install;

pub const STATE_FILENAME: &str = "bo3_enhanced_state.json";

pub type StateMap = Map<String, Value>;

pub fn state_path(core: &Core) -> PathBuf {
    core.paths.app_data.join(STATE_FILENAME)
}

/// Any read/parse error (or a non-object) yields an empty map.
pub fn load_state(core: &Core) -> StateMap {
    match std::fs::read(state_path(core)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
        Some(Value::Object(m)) => m,
        _ => StateMap::new(),
    }
}

/// Failures are only logged (file-only warning), like the legacy `save_state`.
pub fn save_state(core: &Core, state: &StateMap) {
    let path = state_path(core);
    let r = std::fs::create_dir_all(&core.paths.app_data)
        .and_then(|_| std::fs::write(&path, crate::pyjson::dumps_indent2(&Value::Object(state.clone()))));
    if let Err(e) = r {
        core.log.log_file_only(crate::log::Category::Warning, format!("Failed to persist BO3 Enhanced state: {}", crate::pyfmt::io_error_text(&e, Some(&path))));
    }
}

/// Python truthiness of a JSON value.
pub fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// `installed_files` as a list of strings (non-list -> empty; non-string items are dropped).
pub fn installed_files(state: &StateMap) -> Vec<String> {
    match state.get("installed_files") {
        Some(Value::Array(a)) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
        _ => Vec::new(),
    }
}

/// Number of entries in `installed_files` (any JSON array item counts, like `len()`).
pub fn installed_files_len(state: &StateMap) -> usize {
    match state.get("installed_files") {
        Some(Value::Array(a)) => a.len(),
        _ => 0,
    }
}

pub fn clear_enhanced_state(core: &Core) {
    let mut state = load_state(core);
    state.insert("installed".into(), Value::Bool(false));
    state.insert("installed_files".into(), Value::Array(vec![]));
    save_state(core, &state);
}

#[derive(Debug, Clone)]
pub struct Summary {
    pub installed: bool,
    pub detected_at: Option<String>,
    pub acknowledged_at: Option<String>,
}

/// Legacy `status_summary` (including its side effect of clearing a stale `installed` flag).
pub fn status_summary(core: &Core, game_dir: &std::path::Path) -> Summary {
    let state = load_state(core);
    let mut detected = detect_enhanced_install(game_dir);
    if !detected && !truthy(state.get("installed_files")) {
        if truthy(state.get("installed")) {
            clear_enhanced_state(core);
        }
        detected = false;
    } else {
        detected = detected || truthy(state.get("installed"));
    }
    Summary {
        installed: detected,
        detected_at: state.get("detected_at").and_then(Value::as_str).map(str::to_string),
        acknowledged_at: state.get("acknowledged_at").and_then(Value::as_str).map(str::to_string),
    }
}

/// `time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())`
pub fn utc_timestamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    format_utc(secs)
}

pub fn format_utc(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_format() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(1_709_210_096), "2024-02-29T12:34:56Z");
    }
}
