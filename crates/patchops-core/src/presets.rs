//! Graphics presets: `presets.json` (`{name: {Key: [value, comment]}}`, order significant).
//!
//! The repo-root `presets.json` is embedded in the binary; `PATCHOPS_PRESETS=<file>` overrides it.

use std::sync::OnceLock;

use serde_json::{Map, Value};

use crate::context::Core;

const EMBEDDED: &str = include_str!("../../../presets.json");

fn embedded() -> &'static Map<String, Value> {
    static CELL: OnceLock<Map<String, Value>> = OnceLock::new();
    CELL.get_or_init(|| match serde_json::from_str::<Value>(EMBEDDED) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    })
}

/// All presets in file order.
pub fn load(core: &Core) -> Map<String, Value> {
    if let Some(p) = &core.paths.presets_override {
        if let Ok(text) = std::fs::read_to_string(p) {
            if let Ok(Value::Object(m)) = serde_json::from_str::<Value>(&text) {
                return m;
            }
        }
    }
    embedded().clone()
}

/// Preset names in file order (legacy `_preset_names`).
pub fn names(core: &Core) -> Vec<String> {
    if core.paths.presets_override.is_some() {
        return load(core).keys().cloned().collect();
    }
    embedded().keys().cloned().collect()
}
