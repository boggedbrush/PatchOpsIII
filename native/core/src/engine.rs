use std::{
    cmp::Ordering,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::{
    app::{self, AppState},
    dxvk, enhanced, exe,
    models::{DxvkSettings, MaintenanceState, ModsState, PatchOpsState},
    steam, t7,
};

/// Synchronous operations facade. Callers are expected to own it on a worker
/// thread; no method requires a GUI or async runtime.
pub struct Engine {
    state: AppState,
}

impl Engine {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub fn state(&self) -> &AppState {
        &self.state
    }

    pub fn current_state(&self) -> Result<PatchOpsState, String> {
        current_state(&self.state)
    }

    /// Dispatch the legacy local-HTTP paths without HTTP. The returned JSON
    /// intentionally retains the Python API's wire shapes so existing UI code
    /// can migrate independently from the backend.
    pub fn dispatch(&self, path: &str, body: Option<Value>) -> Result<Value, String> {
        let report = !matches!(
            path,
            "/api/status" | "/api/health" | "/api/logs/payload" | "/api/exe-swap/compatible-depot"
        );
        if report {
            self.state.begin_progress(path);
        }
        let requested_directory = body
            .as_ref()
            .and_then(|value| value["path"].as_str())
            .map(str::to_owned);
        let result = self.dispatch_inner(path, body);
        if report {
            let error = result.as_ref().err().map(String::as_str).or_else(|| {
                result
                    .as_ref()
                    .ok()
                    .filter(|value| value["ok"] == false || value["valid"] == false)
                    .and_then(|value| value["error"].as_str().or(value["message"].as_str()))
            });
            if let Some(error) = error.filter(|error| {
                // Python returns these input validation failures before its log/try block.
                !(path == "/api/t7-config"
                    && (error.starts_with("Gamertag ")
                        || error.starts_with("t7patch.conf was not found.")))
            }) {
                let (category, message) = match path {
                    "/api/game-directory" => (
                        "Error",
                        format!(
                            "Selected directory is not a Black Ops III install: {}",
                            requested_directory.as_deref().unwrap_or_default()
                        ),
                    ),
                    "/api/launch-options" if error == "Unsupported launch option." => {
                        ("Warning", error.to_owned())
                    }
                    "/api/t7-config" => (
                        "Error",
                        format!("Failed to update T7 Patch settings: {error}"),
                    ),
                    _ => ("Error", error.to_owned()),
                };
                self.state.log(category, message);
            }
            self.state.end_progress(error);
        }
        result
            .or_else(|error| {
                if path == "/api/launch-options" && error == "Unsupported launch option." {
                    Ok(json!({"ok": false, "error": error, "state": self.current_state()?}))
                } else if report {
                    Ok(json!({"ok": false, "error": error}))
                } else {
                    Err(error)
                }
            })
            .map(normalize_wire_value)
    }

    fn dispatch_inner(&self, path: &str, body: Option<Value>) -> Result<Value, String> {
        let body = body.unwrap_or_else(|| json!({}));
        match path {
            "/api/health" => Ok(json!({"ok": true, "version": app::APP_VERSION})),
            "/api/status" => serde_json::to_value(self.current_state()?).map_err(string_error),
            "/api/game-directory" => {
                let path = required_string(&body, "path")?;
                self.mutate(|| app::set_game_directory(&self.state, path))
            }
            "/api/launch" => self.mutate(|| steam::launch_game(&self.state)),
            "/api/launch-options" => {
                let options = required_string(&body, "options")?;
                let preserve = optional_bool(&body, "preserve_fs_game").unwrap_or(false);
                self.mutate(|| steam::apply_launch_options(&self.state, options, preserve))
            }
            "/api/workshop-install" => {
                let profile = required_string(&body, "profileId")?;
                self.mutate(|| steam::install_workshop_profile(&self.state, profile))
            }
            "/api/update-check" => self.update_check(),
            "/api/release-channel" => {
                let channel = required_string(&body, "channel")?;
                self.mutate(|| app::set_release_channel(&self.state, channel))
            }
            "/api/config" => {
                let key = required_string(&body, "key")?;
                let value = body
                    .get("value")
                    .cloned()
                    .ok_or_else(|| "Missing value.".to_string())?;
                self.mutate(|| {
                    app::set_config_value(
                        &self.state,
                        &required_game_dir(&self.state)?,
                        key,
                        value,
                        body["comment"].as_str(),
                    )
                })
            }
            "/api/dxvk-install" => {
                let settings: DxvkSettings = decode(body)?;
                self.mutate(|| {
                    dxvk::install(&self.state, &required_game_dir(&self.state)?, &settings)
                })
            }
            "/api/dxvk-uninstall" => {
                self.mutate(|| dxvk::uninstall(&self.state, &required_game_dir(&self.state)?))
            }
            "/api/dxvk-config" => {
                let settings: DxvkSettings = decode(body)?;
                self.mutate(|| {
                    dxvk::configure(&self.state, &required_game_dir(&self.state)?, &settings)
                })
            }
            "/api/t7-config" => {
                let gamertag = optional_string(&body, "gamertag");
                let color = optional_string(&body, "colorCode").unwrap_or_default();
                let password = optional_string(&body, "networkPassword");
                let friends = optional_bool(&body, "friendsOnly");
                self.mutate(|| {
                    t7::configure(
                        &self.state,
                        &required_game_dir(&self.state)?,
                        gamertag.as_deref(),
                        &color,
                        password.as_deref(),
                        friends,
                    )
                })
            }
            "/api/t7-install" => self.mutate(|| {
                let game_dir = required_game_dir(&self.state)?;
                let profile = exe::status(&self.state.load_settings(), Some(&game_dir)).profile;
                t7::install(&self.state, &game_dir, &profile)
            }),
            "/api/t7-uninstall" => {
                self.mutate(|| t7::uninstall(&self.state, &required_game_dir(&self.state)?))
            }
            "/api/enhanced-validate" => {
                let source = required_string(&body, "dumpSource")?.trim();
                if source.is_empty() {
                    return self.validation_response(false, "Select a dump source first.");
                }
                let valid =
                    enhanced::validate_and_remember_dump_source(&self.state, Path::new(source))?;
                self.validation_response(
                    valid,
                    if valid {
                        "Source looks ready."
                    } else {
                        "Source is missing required dump files."
                    },
                )
            }
            "/api/enhanced-install" => {
                let source = required_string(&body, "dumpSource")?;
                self.mutate(|| {
                    enhanced::install(
                        &self.state,
                        &required_game_dir(&self.state)?,
                        Path::new(source),
                    )
                })
            }
            "/api/enhanced-uninstall" => {
                self.mutate(|| enhanced::uninstall(&self.state, &required_game_dir(&self.state)?))
            }
            "/api/exe-swap/compatible" => self.activate_compatible(),
            "/api/exe-swap/compatible-depot" => Ok(json!({
                "ok": true,
                "available": exe::compatible_depot_available(),
                "state": self.current_state()?,
            })),
            "/api/exe-swap/current" => {
                self.mutate(|| exe::activate_current(&self.state, &required_game_dir(&self.state)?))
            }
            "/api/exe-swap/enhanced" => self
                .mutate(|| exe::activate_enhanced(&self.state, &required_game_dir(&self.state)?)),
            "/api/presets/apply" => {
                let name = required_string(&body, "name")?;
                self.mutate(|| {
                    app::apply_preset(&self.state, &required_game_dir(&self.state)?, name)
                })
            }
            "/api/intro-skip" => {
                let enabled = required_bool(&body, "enabled")?;
                self.mutate(|| {
                    app::set_intro_skip(&self.state, &required_game_dir(&self.state)?, enabled)
                })
            }
            "/api/d3dcompiler" => {
                let enabled = required_bool(&body, "enabled")?;
                self.mutate(|| {
                    app::set_d3dcompiler(&self.state, &required_game_dir(&self.state)?, enabled)
                })
            }
            "/api/all-intros-skip" => {
                let enabled = required_bool(&body, "enabled")?;
                self.mutate(|| {
                    app::set_all_intro_skip(&self.state, &required_game_dir(&self.state)?, enabled)
                })
            }
            "/api/config-readonly" => {
                let enabled = required_bool(&body, "enabled")?;
                self.mutate(|| {
                    app::set_config_readonly(&self.state, &required_game_dir(&self.state)?, enabled)
                })
            }
            "/api/vram-target" => {
                let limited = required_bool(&body, "limited")?;
                let target = required_i32(&body, "target")?;
                self.mutate(|| {
                    app::set_vram_target(
                        &self.state,
                        &required_game_dir(&self.state)?,
                        limited,
                        target,
                    )
                })
            }
            "/api/logs/payload" => Ok(json!({
                "ok": true,
                "payload": app::log_payload(&self.state),
            })),
            "/api/logs/clear" => self.mutate(|| {
                self.state.clear_logs()?;
                self.state.log("Success", "Logs cleared.");
                Ok(())
            }),
            "/api/mod-files/clear" => self.mutate(|| app::clear_mod_files(&self.state)),
            "/api/reset-stock" => self.mutate(|| reset_to_stock_inner(&self.state)),
            _ => Err(format!("Unsupported backend path: {path}")),
        }
    }

    fn mutate(&self, operation: impl FnOnce() -> Result<(), String>) -> Result<Value, String> {
        let _guard = self.state.lock_operation()?;
        operation()?;
        Ok(json!({"ok": true, "state": self.current_state()?}))
    }

    fn validation_response(&self, valid: bool, message: &str) -> Result<Value, String> {
        Ok(json!({
            "ok": true,
            "valid": valid,
            "message": message,
            "state": self.current_state()?,
        }))
    }

    fn activate_compatible(&self) -> Result<Value, String> {
        let _guard = self.state.lock_operation()?;
        let game_dir = required_game_dir(&self.state)?;
        match exe::activate_compatible(&self.state, &game_dir) {
            Ok(()) => Ok(json!({"ok": true, "state": self.current_state()?})),
            Err(error) if error == exe::COMPATIBLE_DEPOT_REQUIRED_MESSAGE => Ok(json!({
                "ok": false,
                "error": error,
                "depotRequired": true,
                "depotCommand": exe::COMPATIBLE_DEPOT_COMMAND,
                "state": self.current_state()?,
            })),
            Err(error) => Err(error),
        }
    }

    fn update_check(&self) -> Result<Value, String> {
        let _guard = self.state.lock_operation()?;
        let result = check_update(&self.state)?;
        Ok(json!({
            "ok": true,
            "update": result,
            "state": self.current_state()?,
        }))
    }
}

fn normalize_wire_value(mut value: Value) -> Value {
    // Python omits `path` for built-in profiles but includes null for Workshop.
    let state = if value.get("state").is_some() {
        &mut value["state"]
    } else {
        &mut value
    };
    if let Some(profiles) = state
        .get_mut("launchProfiles")
        .and_then(Value::as_array_mut)
    {
        for profile in profiles {
            if matches!(profile["id"].as_str(), Some("default" | "offline"))
                && let Some(profile) = profile.as_object_mut()
            {
                profile.remove("path");
            }
        }
    }
    value
}

fn current_state(state: &AppState) -> Result<PatchOpsState, String> {
    let settings = state.load_settings();
    let game_dir = steam::find_game_directory(settings.game_dir.as_deref());
    let current_launch_options = steam::current_launch_options();
    let launch_profiles = steam::launch_profiles(current_launch_options.as_deref());
    let active_launch_profile = launch_profiles
        .iter()
        .find(|profile| profile.active)
        .map(|profile| profile.id.clone())
        .unwrap_or_else(|| "custom".into());
    let exe_swap = exe::status(&settings, game_dir.as_deref());
    let t7_state = t7::status(game_dir.as_deref(), Some(&exe_swap.profile));
    let dxvk_state = dxvk::status(game_dir.as_deref());
    let enhanced_state = enhanced::status(
        state,
        game_dir.as_deref(),
        current_launch_options.as_deref(),
        settings.enhanced_dump_source.clone().unwrap_or_default(),
    );
    let config = game_dir
        .as_deref()
        .map(app::read_config)
        .unwrap_or_default();
    let qol = app::qol_state(game_dir.as_deref());
    let mods = ModsState {
        t7_patch: t7_state.installed,
        dxvk: dxvk_state.installed,
        enhanced: enhanced_state.installed,
    };

    Ok(PatchOpsState {
        app_version: app::APP_VERSION.into(),
        platform: app::platform_name().into(),
        game_dir: game_dir
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        game_detected: game_dir.is_some(),
        config_exists: !config.is_empty(),
        steam_user_id: steam::user_id(),
        log_path: state.log_path().to_string_lossy().into_owned(),
        presets: app::preset_names(),
        current_launch_options,
        active_launch_profile,
        release_channel: app::release_channel(&settings).into(),
        launch_profiles,
        enhanced: enhanced_state,
        exe_swap,
        t7: t7_state,
        dxvk: dxvk_state,
        qol,
        graphics: app::graphics_state(&config),
        advanced: app::advanced_state(game_dir.as_deref(), &config),
        maintenance: MaintenanceState {
            mod_files_dir: state.mod_files_dir().to_string_lossy().into_owned(),
            log_payload: app::log_payload(state),
        },
        mods,
        logs: state.recent_logs(),
    })
}

fn detected_game_dir(state: &AppState) -> Option<PathBuf> {
    steam::find_game_directory(state.load_settings().game_dir.as_deref())
}

fn required_game_dir(state: &AppState) -> Result<PathBuf, String> {
    detected_game_dir(state).ok_or_else(|| "Game directory is not set.".into())
}

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|error| format!("Invalid request payload: {error}"))
}

fn required_string<'a>(body: &'a Value, key: &str) -> Result<&'a str, String> {
    body.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Missing or invalid {key}."))
}

fn optional_string(body: &Value, key: &str) -> Option<String> {
    body.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn required_bool(body: &Value, key: &str) -> Result<bool, String> {
    body.get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("Missing or invalid {key}."))
}

fn optional_bool(body: &Value, key: &str) -> Option<bool> {
    body.get(key).and_then(Value::as_bool)
}

fn required_i32(body: &Value, key: &str) -> Result<i32, String> {
    body.get(key)
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| format!("Missing or invalid {key}."))
}

fn string_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn version_key(value: &str) -> (Vec<u64>, bool, u64) {
    let value = value.trim().trim_start_matches(['v', 'V']);
    let base_end = value
        .find(|character: char| !character.is_ascii_digit() && character != '.')
        .unwrap_or(value.len());
    let mut base = value[..base_end]
        .split('.')
        .filter_map(|part| part.parse().ok())
        .collect::<Vec<_>>();
    base.resize(3, 0);
    let suffix = &value[base_end..];
    let prerelease_number = suffix
        .split(|character: char| !character.is_ascii_digit())
        .filter_map(|part| part.parse().ok())
        .next_back()
        .unwrap_or(0);
    (base, suffix.is_empty(), prerelease_number)
}

fn compare_versions(left: &str, right: &str) -> Ordering {
    version_key(left).cmp(&version_key(right))
}

fn check_update(state: &AppState) -> Result<Value, String> {
    const MAX_BODY: u64 = 4 * 1024 * 1024;
    let settings = state.load_settings();
    let channel = app::release_channel(&settings);
    let endpoint = if channel == "beta" {
        "https://api.github.com/repos/boggedbrush/PatchOpsIII/releases?per_page=20"
    } else {
        "https://api.github.com/repos/boggedbrush/PatchOpsIII/releases/latest"
    };
    let response = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(string_error)?
        .get(endpoint)
        .header(reqwest::header::USER_AGENT, "PatchOpsIII")
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| format!("Update check failed: {error}"))?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_BODY)
    {
        return Err("Update metadata is unexpectedly large.".into());
    }
    let mut body = Vec::new();
    response
        .take(MAX_BODY + 1)
        .read_to_end(&mut body)
        .map_err(string_error)?;
    if body.len() as u64 > MAX_BODY {
        return Err("Update metadata is unexpectedly large.".into());
    }
    let root: Value = serde_json::from_slice(&body).map_err(string_error)?;
    let release = if channel == "beta" {
        root.as_array()
            .and_then(|releases| {
                releases.iter().find(|release| {
                    release.get("draft").and_then(Value::as_bool) != Some(true)
                        && release.get("prerelease").and_then(Value::as_bool) == Some(true)
                        && format!(
                            "{} {}",
                            release
                                .get("tag_name")
                                .and_then(Value::as_str)
                                .unwrap_or_default(),
                            release
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                        )
                        .to_ascii_lowercase()
                        .contains("beta")
                })
            })
            .ok_or_else(|| "No beta release was found.".to_string())?
    } else {
        &root
    };
    let result = update_metadata(release, channel, std::env::consts::OS);
    let available = result["available"].as_bool() == Some(true);
    state.log(
        if available { "Success" } else { "Info" },
        if available {
            format!(
                "{} update available: {}",
                title_case(channel),
                result["latestVersion"].as_str().unwrap_or_default()
            )
        } else {
            format!("No {channel} updates available.")
        },
    );
    Ok(result)
}

// Preserve Python's metadata shape but choose the native client artifact.
fn update_metadata(release: &Value, channel: &str, platform: &str) -> Value {
    let expected_name = match platform {
        "windows" => "PatchOpsIII-native-windows-x64.zip",
        "linux" => "PatchOpsIII-native-linux-x64.zip",
        _ => "",
    };
    let asset = release["assets"].as_array().and_then(|assets| {
        assets.iter().find_map(|asset| {
            let name = asset["name"].as_str()?;
            if expected_name.is_empty() || !name.eq_ignore_ascii_case(expected_name) {
                return None;
            }
            let url = asset["browser_download_url"]
                .as_str()
                .filter(|url| !url.is_empty())?;
            Some(json!({
                "name": name, "url": url,
                "size": asset["size"].as_u64().unwrap_or(0),
                "contentType": asset["content_type"].as_str().unwrap_or("application/octet-stream"),
            }))
        })
    });
    let latest = release["tag_name"]
        .as_str()
        .filter(|tag| !tag.is_empty())
        .or(release["name"].as_str())
        .unwrap_or("0.0.0");
    let available = asset.is_some()
        && release["draft"] != true
        && (channel == "beta" || release["prerelease"] != true)
        && compare_versions(latest, app::APP_VERSION).is_gt();
    json!({
        "available": available, "channel": channel, "currentVersion": app::APP_VERSION,
        "latestVersion": latest, "name": release["name"].as_str().unwrap_or("PatchOpsIII"),
        "body": release["body"].as_str().unwrap_or(""),
        "pageUrl": release["html_url"].as_str().unwrap_or("https://github.com/boggedbrush/PatchOpsIII/releases"),
        "asset": asset,
    })
}

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

fn reset_to_stock_inner(state: &AppState) -> Result<(), String> {
    let game_dir = required_game_dir(state)?;
    let mut errors = Vec::new();
    if (enhanced::detect_install(&game_dir) || enhanced::has_owned_install(state, &game_dir))
        && let Err(error) = enhanced::uninstall(state, &game_dir)
    {
        errors.push(format!("Enhanced: {error}"));
    }
    if exe::status(&state.load_settings(), Some(&game_dir)).profile != exe::CURRENT_EXE_ID
        && let Err(error) = exe::activate_current(state, &game_dir)
    {
        errors.push(format!("EXE: {error}"));
    }
    if let Err(error) = t7::uninstall(state, &game_dir) {
        errors.push(format!("T7 Patch: {error}"));
    }
    if let Err(error) = dxvk::uninstall(state, &game_dir) {
        errors.push(format!("DXVK: {error}"));
    }
    let qol = app::qol_state(Some(&game_dir));
    if qol.d3dcompiler
        && let Err(error) = app::set_d3dcompiler(state, &game_dir, false)
    {
        errors.push(format!("d3dcompiler: {error}"));
    }
    if qol.all_intros {
        if let Err(error) = app::set_all_intro_skip(state, &game_dir, false) {
            errors.push(format!("intros: {error}"));
        }
    } else if qol.intro
        && let Err(error) = app::set_intro_skip(state, &game_dir, false)
    {
        errors.push(format!("intro: {error}"));
    }
    if app::config_path(&game_dir).is_file() {
        let updates = vec![
            ("SmoothFramerate".into(), "0".into(), "0 or 1".into()),
            ("VideoMemory".into(), "1".into(), "0.75 to 1".into()),
            ("StreamMinResident".into(), "0".into(), "0 or 1".into()),
            ("MaxFrameLatency".into(), "1".into(), "0 to 4".into()),
            ("SerializeRender".into(), "0".into(), "0 to 2".into()),
            (
                "RestrictGraphicsOptions".into(),
                "1".into(),
                "0 or 1".into(),
            ),
        ];
        if let Err(error) = app::write_config_values(&game_dir, &updates) {
            errors.push(format!("config: {error}"));
        }
    }
    if let Err(error) = steam::apply_launch_options(state, "", false) {
        errors.push(format!("launch options: {error}"));
    }
    if errors.is_empty() {
        state.log("Success", "Reset to stock complete.");
        Ok(())
    } else {
        let message = format!("Reset completed with errors: {}", errors.join("; "));
        state.log("Error", &message);
        Err(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, sync::Arc};

    fn test_engine(name: &str) -> (Engine, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "patchops-core-engine-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        (
            Engine::new(AppState::new(root.join("data"), None, None).unwrap()),
            root,
        )
    }

    #[test]
    fn status_preserves_legacy_json_shape() {
        let (engine, root) = test_engine("status");
        let status = engine.dispatch("/api/status", None).unwrap();
        assert!(status["appVersion"].is_string());
        assert!(status["gameDetected"].is_boolean());
        assert!(status["exeSwap"]["currentBuildId"].is_string());
        assert!(status["launchProfiles"].is_array());
        assert_eq!(status["platform"], app::platform_name());
        assert!(status["maintenance"]["logPayload"].is_string());
        assert!(status["launchProfiles"][0].get("path").is_none());
        assert!(status["launchProfiles"][2].get("path").is_some());
        assert!(
            engine
                .dispatch("/api/health", None)
                .unwrap()
                .get("launchProfiles")
                .is_none()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn log_callback_is_toolkit_independent() {
        let root =
            std::env::temp_dir().join(format!("patchops-core-events-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let capture = seen.clone();
        let state = AppState::new(
            root.join("data"),
            None,
            Some(Arc::new(move |entry| capture.lock().unwrap().push(entry))),
        )
        .unwrap();
        state.log("Info", "hello");
        assert_eq!(seen.lock().unwrap()[0].message, "hello");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn updates_select_native_archives_and_preserve_python_metadata() {
        let release = json!({"tag_name": "v9.0.0", "name": "New release", "body": "Notes", "html_url": "https://example.invalid/release", "assets": [
            {"name": "PatchOpsIII.AppImage", "browser_download_url": "https://example.invalid/legacy"},
            {"name": "PatchOpsIII-native-linux-x64.zip", "browser_download_url": "https://example.invalid/linux", "size": 42, "content_type": "application/zip"},
            {"name": "PatchOpsIII-native-windows-x64.zip", "browser_download_url": "https://example.invalid/windows"}
        ]});
        let linux = update_metadata(&release, "stable", "linux");
        assert_eq!(linux["asset"]["url"], "https://example.invalid/linux");
        assert_eq!(linux["asset"]["size"], 42);
        assert_eq!(linux["body"], "Notes");
        assert_eq!(linux["available"], true);
        assert_eq!(
            update_metadata(&release, "stable", "windows")["asset"]["url"],
            "https://example.invalid/windows"
        );
        assert_eq!(
            update_metadata(&release, "stable", "macos")["available"],
            false
        );
        let mut beta = release.clone();
        beta["prerelease"] = json!(true);
        assert_eq!(
            update_metadata(&beta, "stable", "linux")["available"],
            false
        );
        assert_eq!(update_metadata(&beta, "beta", "linux")["available"], true);
        beta["draft"] = json!(true);
        assert_eq!(update_metadata(&beta, "beta", "linux")["available"], false);
        beta["assets"] = json!([]);
        assert!(update_metadata(&beta, "stable", "linux")["asset"].is_null());
    }

    #[test]
    fn progress_reports_failure_and_stays_quiet_during_status_polls() {
        let (engine, root) = test_engine("progress");
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let capture = seen.clone();
        engine
            .state()
            .set_progress_callback(Arc::new(move |event| capture.lock().unwrap().push(event)));
        engine.dispatch("/api/status", None).unwrap();
        assert!(seen.lock().unwrap().is_empty());
        assert_eq!(
            engine
                .dispatch("/api/config", Some(json!({"key": "FOV", "value": 80})))
                .unwrap()["ok"],
            false
        );
        let events = seen.lock().unwrap();
        assert_eq!(events.first().unwrap().stage, "started");
        assert_eq!(events.last().unwrap().stage, "failed");
        assert_eq!(events.last().unwrap().op, "/api/config");
        assert!(events.last().unwrap().fraction.is_none());
        drop(events);
        engine
            .dispatch("/api/release-channel", Some(json!({"channel": "beta"})))
            .unwrap();
        let events = seen.lock().unwrap();
        assert_eq!(events.last().unwrap().stage, "completed");
        assert_eq!(events.last().unwrap().op, "/api/release-channel");
        assert_eq!(events.last().unwrap().fraction, Some(1.0));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn version_order_matches_stable_and_beta_channels() {
        assert!(compare_versions("v1.3.1", "1.3.0-beta3").is_gt());
        assert!(compare_versions("1.3.0", "1.3.0-beta3").is_gt());
        assert!(compare_versions("1.3.0-beta4", "1.3.0-beta3").is_gt());
        assert_eq!(compare_versions("v1.3", "1.3.0"), Ordering::Equal);
    }
}
