use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    config_reader::read_config,
    error::{CoreError, Result},
    file_integrity::hash_path,
    game_scan::status,
    steam_detect::scan_steam,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GameDirPayload {
    game_dir: PathBuf,
}

#[derive(Debug, Deserialize)]
struct HashPayload {
    path: PathBuf,
}

fn parse_payload<T: for<'de> Deserialize<'de>>(payload: Value) -> Result<T> {
    serde_json::from_value(payload).map_err(CoreError::from)
}

pub fn handle_command(command: &str, payload: Value) -> Result<Value> {
    let output = match command {
        "status" => {
            let payload: GameDirPayload = parse_payload(payload)?;
            serde_json::to_value(status(&payload.game_dir)?)?
        }
        "hash" => {
            let payload: HashPayload = parse_payload(payload)?;
            serde_json::to_value(hash_path(&payload.path)?)?
        }
        "scan-steam" => serde_json::to_value(scan_steam())?,
        "read-config" => {
            let payload: GameDirPayload = parse_payload(payload)?;
            serde_json::to_value(read_config(&payload.game_dir)?)?
        }
        other => return Err(CoreError::UnknownCommand(other.to_string())),
    };

    Ok(match output {
        Value::Object(_) => output,
        _ => json!({ "ok": true, "value": output }),
    })
}
