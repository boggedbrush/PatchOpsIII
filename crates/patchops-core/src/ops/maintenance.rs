//! Group `maintenance`: `reset-stock`, `update-check`.
//!
//! Legacy reference: `backend/api.py` (`reset_stock`, `_reset_to_stock`, `update_check`,
//! `_check_for_update`, `_fetch_release_for_channel`, `_select_update_asset`, `_version_sort_key`).
//!
//! `update-check` only *reports*: the legacy backend never downloads or applies an update, and the
//! Electron renderer ignored the `update` object too (it triggered the call from the title bar /
//! Updates card and showed the two log lines); the `update` payload is part of the API for any
//! future UI that wants to offer `pageUrl` / `asset`.

use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{reply, state_reply, Args, Group};
use crate::config_ini::ConfigValue;
use crate::context::Core;
use crate::error::{AppError, AppResult, OpError};
use crate::net::Net;
use crate::state::State;

pub const OPS: &[&str] = &["reset-stock", "update-check"];

pub const GROUP: Group = Group { name: "maintenance", ops: OPS, call };

fn call(core: &Core, op: &str, _args: &Args) -> Result<Value, OpError> {
    match op {
        "reset-stock" => state_reply(reset_stock(core)),
        "update-check" => reply(update_check(core)),
        _ => Err(OpError::UnknownOp(op.to_string())),
    }
}

// ---- reset-stock ----------------------------------------------------------------------------

/// Legacy `reset_stock`: every step is isolated and only logs a Warning when it fails.
pub fn reset_stock(core: &Core) -> AppResult<State> {
    let game_dir = core.game_dir().ok_or_else(|| AppError::new("Game directory is not set."))?;
    reset_to_stock(core, Path::new(&game_dir));
    Ok(State::build(core))
}

/// Legacy `_reset_to_stock`.
pub fn reset_to_stock(core: &Core, game: &Path) {
    use super::enhanced::{active_exe_profile, detect_enhanced_install, restore_current_exe, uninstall_enhanced};

    if detect_enhanced_install(game) {
        if let Err(e) = uninstall_enhanced(core, game) {
            core.log.warning(format!("Enhanced reset step failed: {e}"));
        }
    }

    let executable = crate::game::find_executable(game).unwrap_or_else(|| game.join("BlackOps3.exe"));
    let exe_step = if active_exe_profile(core, game) != "current" {
        restore_current_exe(core, game)
    } else {
        super::qol::restore_backup(core, &executable, "Restored original executable from backup.").map(|restored| {
            if restored {
                crate::game::write_exe_variant(game, "default");
            }
        })
    };
    if let Err(e) = exe_step {
        core.log.warning(format!("Executable reset step failed: {e}"));
    }

    if let Err(e) = super::t7::uninstall(core, game) {
        core.log.warning(format!("T7 Patch reset step failed: {}", e.message));
    }

    if let Err(e) = super::dxvk::uninstall(core, game) {
        core.log.warning(format!("DXVK reset step failed: {e}"));
    }

    let qol = super::qol::restore_backup(core, &game.join("d3dcompiler_46.dll"), "Restored d3dcompiler_46.dll.")
        .and_then(|_| super::qol::restore_intro_videos(core, game));
    if let Err(e) = qol {
        core.log.warning(format!("Quality of Life reset step failed: {e}"));
    }

    let stock: [(&str, &str, &str); 6] = [
        ("SmoothFramerate", "0", "0 or 1"),
        ("VideoMemory", "1", "0.75 to 1"),
        ("StreamMinResident", "0", "0 or 1"),
        ("MaxFrameLatency", "1", "0 to 4"),
        ("SerializeRender", "0", "0 to 2"),
        ("RestrictGraphicsOptions", "1", "0 or 1"),
    ];
    let updates: Vec<(String, ConfigValue, String)> =
        stock.iter().map(|(k, v, c)| (k.to_string(), ConfigValue::Str(v.to_string()), c.to_string())).collect();
    match crate::config_ini::write_values(game, &updates) {
        Ok(()) => core.log.success("Reset stock graphics and advanced config values."),
        Err(e) => core.log.warning(format!("Config reset step failed: {}", e.message)),
    }

    match crate::steam::launch_options::clear_launch_options(core) {
        Ok(_) => core.log.success("Cleared Steam launch options."),
        Err(e) => core.log.warning(format!("Launch options reset failed: {}", e.message)),
    }

    core.log.success("Reset to stock complete.");
}

// ---- update-check ---------------------------------------------------------------------------

const RELEASES_PATH: &str = "/repos/boggedbrush/PatchOpsIII/releases";
const RELEASE_PAGE_PATH: &str = "/boggedbrush/PatchOpsIII/releases/latest";

/// The installer asset chosen for the platform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAsset {
    pub name: String,
    pub url: String,
    pub size: Value,
    pub content_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub available: bool,
    pub channel: String,
    pub current_version: String,
    pub latest_version: String,
    pub name: String,
    pub body: String,
    pub page_url: String,
    pub asset: Option<UpdateAsset>,
}

/// `update-check` reply body: `{ok, update, state}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateReply {
    pub ok: bool,
    pub update: UpdateInfo,
    pub state: State,
}

/// Legacy `update_check` route against the real GitHub API.
pub fn update_check(core: &Core) -> AppResult<UpdateReply> {
    update_check_with(core, &Net::production())
}

pub fn update_check_with(core: &Core, net: &Net) -> AppResult<UpdateReply> {
    match check_for_update(core, net) {
        Ok(update) => {
            let label = title_case(&update.channel);
            if update.available {
                core.log.success(format!("{label} update available: {}", update.latest_version));
            } else {
                core.log.info(format!("No {} updates available.", label.to_lowercase()));
            }
            Ok(UpdateReply { ok: true, update, state: State::build(core) })
        }
        Err(e) => {
            core.log.error(format!("Update check failed: {e}"));
            Err(AppError::new(e))
        }
    }
}

fn title_case(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase()).unwrap_or_default()
}

/// Legacy `_fetch_release_for_channel`.
pub fn fetch_release_for_channel(net: &Net, channel: &str) -> Result<Value, String> {
    let timeout = Duration::from_secs(30);
    if channel == "stable" {
        let url = format!("{}{RELEASES_PATH}/latest", net.endpoints.github_api);
        return net.client.get_json(&url, timeout).map_err(|e| e.to_string());
    }
    let url = format!("{}{RELEASES_PATH}", net.endpoints.github_api);
    let releases = net.client.get_json(&url, timeout).map_err(|e| e.to_string())?;
    let Value::Array(list) = releases else { return Err("GitHub release list response was not valid.".into()) };
    let text = |r: &Value, k: &str| r.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let truthy = |r: &Value, k: &str| match r.get(k) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Null) | None => false,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    };
    for release in list {
        if !release.is_object() || truthy(&release, "draft") || !truthy(&release, "prerelease") {
            continue;
        }
        let identity = format!("{} {}", text(&release, "tag_name"), text(&release, "name")).to_lowercase();
        if identity.contains("beta") {
            return Ok(release);
        }
    }
    Err("No beta release was found.".into())
}

/// Legacy `_select_update_asset`: first expected name (in priority order) present with a download URL.
pub fn select_update_asset(release: &Value, windows: bool) -> Option<UpdateAsset> {
    let names: &[&str] = if windows {
        &["PatchOpsIII.msi", "PatchOpsIII-Beta.msi", "PatchOpsIII.exe", "PatchOpsIII-Beta.exe", "PatchOpsIII.zip", "PatchOpsIII-Beta.zip"]
    } else {
        &["PatchOpsIII.AppImage", "PatchOpsIII-Beta.AppImage"]
    };
    let assets = release.get("assets").and_then(Value::as_array)?;
    for expected in names {
        for asset in assets {
            let name = asset.get("name").and_then(Value::as_str).unwrap_or("");
            let url = asset.get("browser_download_url").and_then(Value::as_str).unwrap_or("");
            if name.to_lowercase() == expected.to_lowercase() && !url.is_empty() {
                let size = match asset.get("size") {
                    Some(v) if !matches!(v, Value::Null | Value::Bool(false)) && v.as_f64() != Some(0.0) => v.clone(),
                    _ => Value::from(0),
                };
                let ct = asset.get("content_type").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("application/octet-stream");
                return Some(UpdateAsset { name: name.to_string(), url: url.to_string(), size, content_type: ct.to_string() });
            }
        }
    }
    None
}

/// Legacy `_version_sort_key`: `(numeric base padded to 3, 1 for a final release / 0 for a
/// pre-release, last number in the pre-release suffix)`; compared as a tuple.
pub fn version_sort_key(value: &str) -> (Vec<u128>, u8, u128) {
    let normalized = value.trim().trim_start_matches(['v', 'V']);
    // Python `str.strip()` also strips unicode whitespace; `trim` does as well.
    let bytes = normalized.as_bytes();
    let mut end = 0;
    let mut base: Vec<u128> = Vec::new();
    // ^(\d+(?:\.\d+)*)
    let mut i = 0;
    loop {
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            break;
        }
        base.push(normalized[start..i].parse::<u128>().unwrap_or(u128::MAX));
        end = i;
        if i + 1 < bytes.len() && bytes[i] == b'.' && bytes[i + 1].is_ascii_digit() {
            i += 1;
        } else {
            break;
        }
    }
    let prerelease = if base.is_empty() { normalized } else { &normalized[end..] };
    while base.len() < 3 {
        base.push(0);
    }
    let rank = u8::from(prerelease.is_empty());
    let number = prerelease
        .split(|c: char| !c.is_ascii_digit())
        .rfind(|p| !p.is_empty())
        .map(|p| p.parse::<u128>().unwrap_or(u128::MAX))
        .unwrap_or(0);
    (base, rank, number)
}

/// Legacy `_check_for_update`.
pub fn check_for_update(core: &Core, net: &Net) -> Result<UpdateInfo, String> {
    let channel = core.settings().release_channel().to_string();
    let release = fetch_release_for_channel(net, &channel)?;
    let get = |k: &str| release.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
    let version = get("tag_name").or_else(|| get("name")).unwrap_or_else(|| "0.0.0".into());
    let page_url = get("html_url").unwrap_or_else(|| format!("{}{RELEASE_PAGE_PATH}", net.endpoints.github_web));
    let asset = select_update_asset(&release, cfg!(windows));
    let flag = |k: &str| release.get(k).and_then(Value::as_bool).unwrap_or(false);
    let available = asset.is_some()
        && !flag("draft")
        && (channel == "beta" || !flag("prerelease"))
        && version_sort_key(&version) > version_sort_key(&core.version);
    Ok(UpdateInfo {
        available,
        channel,
        current_version: core.version.clone(),
        latest_version: version,
        name: get("name").unwrap_or_else(|| "PatchOpsIII".into()),
        body: get("body").unwrap_or_default(),
        page_url,
        asset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::testserver::{serve, Route};
    use crate::paths::Paths;
    use serde_json::json;
    use std::collections::HashMap;

    fn mk(name: &str, version: &str) -> (Core, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("patchops-maint-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("home")).unwrap();
        (Core::with_paths(Paths::with_app_data(root.join("home"), root.join("appdata")), version.into()), root)
    }

    #[test]
    fn version_keys_order_like_python() {
        let k = version_sort_key;
        assert_eq!(k("v1.2.3"), (vec![1, 2, 3], 1, 0));
        assert_eq!(k("1.2"), (vec![1, 2, 0], 1, 0));
        assert_eq!(k("V2"), (vec![2, 0, 0], 1, 0));
        assert_eq!(k("1.2.3-beta.4"), (vec![1, 2, 3], 0, 4));
        assert_eq!(k("1.2.3-beta"), (vec![1, 2, 3], 0, 0));
        assert_eq!(k("1.2.3.4"), (vec![1, 2, 3, 4], 1, 0));
        assert_eq!(k("nightly-7"), (vec![0, 0, 0], 0, 7));
        assert_eq!(k(""), (vec![0, 0, 0], 1, 0));
        assert_eq!(k("  v0.9.1 "), (vec![0, 9, 1], 1, 0));
        assert!(k("1.10.0") > k("1.9.9"));
        assert!(k("1.2.3") > k("1.2.3-beta.9"));
        assert!(k("1.2.3-beta.2") > k("1.2.3-beta.1"));
        assert!(k("1.2.4-beta.1") > k("1.2.3"));
        assert!(k("1.2.3.1") > k("1.2.3"));
    }

    #[test]
    fn asset_selection_by_platform_and_priority() {
        let rel = json!({"assets": [
            {"name": "patchopsiii-beta.appimage", "browser_download_url": "u2", "size": 5},
            {"name": "PatchOpsIII.AppImage", "browser_download_url": "u1", "size": 9, "content_type": "application/x-executable"},
            {"name": "PatchOpsIII.exe", "browser_download_url": "", "size": 1},
            {"name": "PatchOpsIII.zip", "browser_download_url": "z"},
        ]});
        let a = select_update_asset(&rel, false).unwrap();
        assert_eq!((a.name.as_str(), a.url.as_str(), a.content_type.as_str()), ("PatchOpsIII.AppImage", "u1", "application/x-executable"));
        assert_eq!(a.size, json!(9));
        let w = select_update_asset(&rel, true).unwrap();
        assert_eq!((w.url.as_str(), w.size.clone(), w.content_type.as_str()), ("z", json!(0), "application/octet-stream"));
        assert!(select_update_asset(&json!({"assets": []}), false).is_none());
        assert!(select_update_asset(&json!({}), false).is_none());
    }

    fn release(tag: &str, prerelease: bool, draft: bool) -> Value {
        json!({"tag_name": tag, "name": format!("PatchOpsIII {tag}"), "prerelease": prerelease, "draft": draft, "body": "notes",
               "html_url": format!("https://example.test/{tag}"),
               "assets": [{"name": "PatchOpsIII.AppImage", "browser_download_url": format!("https://example.test/{tag}.AppImage"), "size": 10},
                          {"name": "PatchOpsIII.msi", "browser_download_url": format!("https://example.test/{tag}.msi"), "size": 11}]})
    }

    #[test]
    fn stable_and_beta_channels_against_local_server() {
        let (core, root) = mk("channels", "1.0.0");
        let mut routes = HashMap::new();
        routes.insert("/repos/boggedbrush/PatchOpsIII/releases/latest".to_string(), Route::json(&release("v1.1.0", false, false)));
        routes.insert(
            "/repos/boggedbrush/PatchOpsIII/releases".to_string(),
            Route::json(&json!([release("v2.0.0-rc1", true, false), release("v1.2.0-beta.3", true, true), release("v1.2.0-beta.2", true, false), release("v1.1.0", false, false)])),
        );
        let srv = serve(routes);
        let net = Net::local(&srv.base);

        let r = update_check_with(&core, &net).unwrap();
        assert!(r.ok && r.update.available);
        assert_eq!((r.update.channel.as_str(), r.update.latest_version.as_str(), r.update.current_version.as_str()), ("stable", "v1.1.0", "1.0.0"));
        assert_eq!(r.update.page_url, "https://example.test/v1.1.0");
        assert_eq!(r.update.asset.as_ref().unwrap().name, if cfg!(windows) { "PatchOpsIII.msi" } else { "PatchOpsIII.AppImage" });
        assert_eq!(core.log.recent(1)[0].message, "Stable update available: v1.1.0");

        core.update_settings(|s| s.set_str("release_channel", "beta")).unwrap();
        let r = update_check_with(&core, &net).unwrap();
        // The draft and the non-"beta" prerelease are skipped; first matching beta wins.
        assert_eq!(r.update.latest_version, "v1.2.0-beta.2");
        assert!(r.update.available);
        assert_eq!(core.log.recent(1)[0].message, "Beta update available: v1.2.0-beta.2");

        // Same version: no update.
        let (core2, root2) = mk("same", "v1.1.0");
        let r = update_check_with(&core2, &net).unwrap();
        assert!(!r.update.available);
        assert_eq!(core2.log.recent(1)[0].message, "No stable updates available.");
        let json = serde_json::to_value(&r.update).unwrap();
        assert_eq!(json["currentVersion"], "v1.1.0");
        assert_eq!(json["asset"]["contentType"], "application/octet-stream");
        let _ = (std::fs::remove_dir_all(root), std::fs::remove_dir_all(root2));
    }

    #[test]
    fn errors_are_logged_and_returned() {
        let (core, root) = mk("errors", "1.0.0");
        let mut routes = HashMap::new();
        routes.insert("/repos/boggedbrush/PatchOpsIII/releases".to_string(), Route::json(&json!([release("v1.0.1", false, false)])));
        let srv = serve(routes);
        let net = Net::local(&srv.base);
        // stable endpoint missing -> 404 text
        let e = update_check_with(&core, &net).unwrap_err();
        assert!(e.message.starts_with("404 Client Error: Not Found for url: "), "{}", e.message);
        assert_eq!(core.log.recent(1)[0].message, format!("Update check failed: {}", e.message));
        core.update_settings(|s| s.set_str("release_channel", "beta")).unwrap();
        assert_eq!(update_check_with(&core, &net).unwrap_err().message, "No beta release was found.");
        let mut routes = HashMap::new();
        routes.insert("/repos/boggedbrush/PatchOpsIII/releases".to_string(), Route::json(&json!({"not": "a list"})));
        let srv = serve(routes);
        assert_eq!(update_check_with(&core, &Net::local(&srv.base)).unwrap_err().message, "GitHub release list response was not valid.");
        let _ = std::fs::remove_dir_all(root);
    }
}
