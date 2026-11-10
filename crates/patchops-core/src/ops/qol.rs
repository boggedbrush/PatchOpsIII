//! Group `qol`: `intro-skip`, `all-intros-skip`, `d3dcompiler` plus the `qol` state builder
//! (implemented).
//!
//! TODO(qol agent) - port from `backend/api.py`: `intro_skip`, `all_intros_skip`, `d3dcompiler`
//! (+ `_restore_backup`, `_restore_intro_videos`, which `maintenance::reset-stock` also needs:
//! put shared helpers here as `pub fn` and call them from there). Use `crate::backup`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{not_implemented, Args, Group};
use crate::backup;
use crate::context::Core;
use crate::error::OpError;
use crate::state::StateCx;

pub const OPS: &[&str] = &["intro-skip", "all-intros-skip", "d3dcompiler"];

pub const GROUP: Group = Group { name: "qol", ops: OPS, call };

fn call(core: &Core, op: &str, args: &Args) -> Result<Value, OpError> {
    // TODO(qol agent)
    not_implemented(core, op, args)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QolState {
    pub d3dcompiler: bool,
    pub intro: bool,
    pub all_intros: bool,
}

pub fn qol_state(cx: &StateCx) -> QolState {
    let Some(game) = cx.game_dir.as_deref() else {
        return QolState { d3dcompiler: false, intro: false, all_intros: false };
    };
    let game = std::path::Path::new(game);
    let video = game.join("video");
    let mut all_intros = false;
    if video.exists() {
        if let Ok(rd) = std::fs::read_dir(&video) {
            let (mut mkv, mut bak) = (0usize, 0usize);
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.ends_with(".mkv") {
                    mkv += 1;
                }
                if name.ends_with(&format!(".mkv{}", backup::PATCHOPS_SUFFIX)) || name.ends_with(&format!(".mkv{}", backup::LEGACY_SUFFIX)) {
                    bak += 1;
                }
            }
            all_intros = bak > 0 && mkv == 0;
        }
    }
    QolState {
        d3dcompiler: backup::existing(&game.join("d3dcompiler_46.dll")).is_some(),
        intro: backup::existing(&video.join("BO3_Global_Logo_LogoSequence.mkv")).is_some(),
        all_intros,
    }
}
