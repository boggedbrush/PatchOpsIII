//! Op registry and JSON dispatch.
//!
//! # Layout
//! One file per op group, each file-disjoint so groups can be ported in parallel:
//!
//! | file | ops |
//! |---|---|
//! | `basics` | health, status, browse, game-directory, release-channel, logs/payload, logs/clear, mod-files/clear |
//! | `config` | config, vram-target, presets/apply, config-readonly (+ `graphics`/`advanced` state) |
//! | `qol` | intro-skip, all-intros-skip, d3dcompiler (+ `qol` state) |
//! | `dxvk` | dxvk-config, dxvk-install, dxvk-uninstall (+ `dxvk` state) |
//! | `steam_ops` | launch, launch-options, workshop-install (+ launch profile state) |
//! | `t7` | t7-config, t7-install, t7-uninstall (+ `t7` state) |
//! | `enhanced` | enhanced-validate/install/uninstall, exe-swap/* (+ `enhanced`, `exeSwap` state) |
//! | `maintenance` | reset-stock, update-check |
//!
//! # Adding / changing a handler
//! 1. Write a *typed* function in your group file, e.g.
//!    `pub fn set_intro_skip(core: &Core, enabled: bool) -> AppResult<State>`. The GUI calls these
//!    directly. Success returns the data (usually the fresh `State::build(core)`), an application
//!    failure returns `Err(AppError::new("message"))` (JSON: `{"ok":false,"error":...}`).
//! 2. In your file's `call` function, validate arguments with [`Args`] (all fields are validated
//!    *before* the typed function runs) and map the result with [`state_reply`] (state-returning
//!    ops) or [`reply`] (anything serialisable).
//! 3. Op names are listed in the group's `OPS` const; `dispatch` does the routing. Do not edit
//!    this file when porting a group.

pub mod args;
pub mod basics;
pub mod config;
pub mod dxvk;
pub mod enhanced;
pub mod maintenance;
pub mod qol;
pub mod steam_ops;
pub mod t7;

use serde::Serialize;
use serde_json::{json, Value};

use crate::context::Core;
use crate::error::{AppResult, OpError};
use crate::state::State;

pub use args::Args;

/// A routable set of ops.
pub struct Group {
    pub name: &'static str,
    pub ops: &'static [&'static str],
    pub call: fn(&Core, &str, &Args) -> Result<Value, OpError>,
}

/// All groups. Every op of `bench/OPS.md` section 3 must appear in exactly one `ops` list
/// (checked by the `registry_is_complete` test).
pub const GROUPS: &[Group] = &[
    basics::GROUP,
    config::GROUP,
    qol::GROUP,
    dxvk::GROUP,
    steam_ops::GROUP,
    t7::GROUP,
    enhanced::GROUP,
    maintenance::GROUP,
];

/// Route `op` with JSON `args` (a JSON object, or `null` for none) to its handler and return the
/// legacy JSON body. Envelope-level failures are [`OpError`]s; application failures are
/// `{"ok": false, "error": ...}` bodies.
pub fn dispatch(core: &Core, op: &str, args: &Value) -> Result<Value, OpError> {
    for g in GROUPS {
        if g.ops.contains(&op) {
            let a = Args::new(args)?;
            return (g.call)(core, op, &a);
        }
    }
    Err(OpError::UnknownOp(op.to_string()))
}

/// Names of all registered ops.
pub fn all_ops() -> impl Iterator<Item = &'static str> {
    GROUPS.iter().flat_map(|g| g.ops.iter().copied())
}

/// `Ok(state)` -> `{"ok": true, "state": {...}}`, `Err(e)` -> `e.to_json()`.
pub fn state_reply(r: AppResult<State>) -> Result<Value, OpError> {
    match r {
        Ok(s) => Ok(json!({"ok": true, "state": to_json(&s)?})),
        Err(e) => Ok(e.to_json()),
    }
}

/// `Ok(v)` -> `v` serialised as the whole body, `Err(e)` -> `e.to_json()`.
pub fn reply<T: Serialize>(r: AppResult<T>) -> Result<Value, OpError> {
    match r {
        Ok(v) => to_json(&v),
        Err(e) => Ok(e.to_json()),
    }
}

/// Serialise a typed value into a JSON body.
pub fn to_json<T: Serialize>(v: &T) -> Result<Value, OpError> {
    serde_json::to_value(v).map_err(|e| OpError::Internal(format!("serialize: {e}")))
}

/// For the `call` fn of groups that are not ported yet.
pub fn not_implemented(_core: &Core, op: &str, _args: &Args) -> Result<Value, OpError> {
    Err(OpError::not_implemented(op))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_complete() {
        const EXPECTED: &[&str] = &[
            "health", "status", "browse", "game-directory", "launch", "launch-options", "workshop-install", "update-check",
            "release-channel", "config", "vram-target", "presets/apply", "intro-skip", "all-intros-skip", "d3dcompiler",
            "config-readonly", "dxvk-config", "dxvk-install", "dxvk-uninstall", "t7-config", "t7-install", "t7-uninstall",
            "enhanced-validate", "enhanced-install", "enhanced-uninstall", "exe-swap/compatible", "exe-swap/compatible-depot",
            "exe-swap/current", "exe-swap/enhanced", "logs/payload", "logs/clear", "mod-files/clear", "reset-stock",
        ];
        let mut got: Vec<&str> = all_ops().collect();
        got.sort_unstable();
        let mut want: Vec<&str> = EXPECTED.to_vec();
        want.sort_unstable();
        assert_eq!(got, want, "ops registered in groups must equal the OPS.md catalogue (no duplicates)");
    }
}
