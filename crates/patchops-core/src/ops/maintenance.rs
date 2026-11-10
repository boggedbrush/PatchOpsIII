//! Group `maintenance`: `reset-stock`, `update-check`.
//!
//! TODO(maintenance agent) - port from `backend/api.py`: `reset_stock` + `_reset_to_stock`
//! (orchestrates the qol/dxvk/t7/enhanced/steam_ops/config helpers - call their typed fns; each
//! step is its own try/except that only logs a Warning), `update_check` + `_check_for_update`,
//! `_fetch_release_for_channel`, `_select_update_asset`, `_version_sort_key` (network).

use serde_json::Value;

use super::{not_implemented, Args, Group};
use crate::context::Core;
use crate::error::OpError;

pub const OPS: &[&str] = &["reset-stock", "update-check"];

pub const GROUP: Group = Group { name: "maintenance", ops: OPS, call };

fn call(core: &Core, op: &str, args: &Args) -> Result<Value, OpError> {
    // TODO(maintenance agent)
    not_implemented(core, op, args)
}
