use std::{
    cmp::Ordering,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

use serde_json::Value;
use tauri::State;

use crate::{
    app::{self, AppState},
    dxvk, enhanced, exe,
    models::{
        CompatibleExeResult, DepotStatus, DxvkSettings, EnhancedValidation, MaintenanceState,
        ModsState, PatchOpsState,
    },
    steam, t7,
};

async fn blocking<T, F>(operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|error| format!("background task failed: {error}"))?
}

fn detected_game_dir(state: &AppState) -> Option<PathBuf> {
    let settings = state.load_settings();
    steam::find_game_directory(settings.game_dir.as_deref())
}

fn required_game_dir(state: &AppState) -> Result<PathBuf, String> {
    detected_game_dir(state)
        .ok_or_else(|| "Game directory is not set or could not be detected.".into())
}

pub fn current_state(state: &AppState) -> Result<PatchOpsState, String> {
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
        platform: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        game_dir: game_dir
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        game_detected: game_dir.is_some(),
        config_exists: game_dir
            .as_deref()
            .is_some_and(|path| app::config_path(path).is_file()),
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
        },
        mods,
        logs: state.recent_logs(),
    })
}

#[tauri::command]
pub async fn get_state(state: State<'_, AppState>) -> Result<PatchOpsState, String> {
    let state = state.inner().clone();
    let result = blocking(move || current_state(&state)).await;
    if std::env::var_os("PATCHOPSIII_BENCHMARK").is_some() {
        eprintln!("PATCHOPSIII_BENCHMARK_INTERACTIVE");
    }
    result
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

fn check_update(state: &AppState) -> Result<(), String> {
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
        .map_err(|error| error.to_string())?
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
        .map_err(|error| error.to_string())?;
    if body.len() as u64 > MAX_BODY {
        return Err("Update metadata is unexpectedly large.".into());
    }
    let root: Value = serde_json::from_slice(&body).map_err(|error| error.to_string())?;
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
    let latest = release
        .get("tag_name")
        .or_else(|| release.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("0.0.0");
    let expected_extension = if cfg!(windows) { ".msi" } else { ".AppImage" };
    let has_package = release
        .get("assets")
        .and_then(Value::as_array)
        .is_some_and(|assets| {
            assets.iter().any(|asset| {
                asset
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| name.ends_with(expected_extension))
                    && asset
                        .get("browser_download_url")
                        .and_then(Value::as_str)
                        .is_some()
            })
        });
    if has_package && compare_versions(latest, app::APP_VERSION).is_gt() {
        state.log(
            "Success",
            format!(
                "{} update available: {latest}",
                if channel == "beta" { "Beta" } else { "Stable" }
            ),
        );
    } else {
        state.log("Info", format!("No {channel} updates available."));
    }
    Ok(())
}

#[tauri::command]
pub async fn check_for_updates(state: State<'_, AppState>) -> Result<PatchOpsState, String> {
    let state = state.inner().clone();
    blocking(move || {
        if let Err(error) = check_update(&state) {
            state.log("Error", &error);
            return Err(error);
        }
        current_state(&state)
    })
    .await
}

macro_rules! state_command {
    ($name:ident $(, $arg:ident: $ty:ty)*, |$app:ident| $body:block) => {
        #[tauri::command]
        pub async fn $name(state: State<'_, AppState>, $($arg: $ty),*) -> Result<PatchOpsState, String> {
            let $app = state.inner().clone();
            blocking(move || {
                let _operation = $app.lock_operation()?;
                $body;
                current_state(&$app)
            })
            .await
        }
    };
}

#[tauri::command]
pub async fn set_release_channel(
    state: State<'_, AppState>,
    channel: String,
) -> Result<String, String> {
    let state = state.inner().clone();
    blocking(move || {
        let _operation = state.lock_operation()?;
        app::set_release_channel(&state, &channel)?;
        Ok(channel)
    })
    .await
}

state_command!(set_game_directory, path: String, |state| {
    app::set_game_directory(&state, &path)?;
});

#[tauri::command]
pub async fn activate_compatible_exe(
    state: State<'_, AppState>,
) -> Result<CompatibleExeResult, String> {
    let state = state.inner().clone();
    blocking(move || {
        let _operation = state.lock_operation()?;
        let game_dir = required_game_dir(&state)?;
        let depot_command = match exe::activate_compatible(&state, &game_dir) {
            Ok(()) => None,
            Err(error) if error == exe::COMPATIBLE_DEPOT_REQUIRED_MESSAGE => {
                Some(exe::COMPATIBLE_DEPOT_COMMAND.into())
            }
            Err(error) => return Err(error),
        };
        Ok(CompatibleExeResult {
            state: current_state(&state)?,
            depot_command,
        })
    })
    .await
}

#[tauri::command]
pub async fn get_compatible_depot_status() -> Result<DepotStatus, String> {
    blocking(move || {
        Ok(DepotStatus {
            available: exe::compatible_depot_available(),
        })
    })
    .await
}

state_command!(activate_current_exe, |app| {
    exe::activate_current(&app, &required_game_dir(&app)?)?;
});
state_command!(activate_enhanced_exe, |app| {
    exe::activate_enhanced(&app, &required_game_dir(&app)?)?;
});

state_command!(set_config_value, key: String, value: Value, |state| {
    app::set_config_value(&state, &required_game_dir(&state)?, &key, value)?;
});

state_command!(apply_launch_profile, profile_id: String, |state| {
    steam::apply_launch_profile(&state, &profile_id)?;
});

state_command!(install_workshop_profile, profile_id: String, |state| {
    steam::install_workshop_profile(&state, &profile_id)?;
});

state_command!(set_intro_skip, enabled: bool, |state| {
    app::set_intro_skip(&state, &required_game_dir(&state)?, enabled)?;
});

state_command!(set_d3dcompiler_workaround, enabled: bool, |state| {
    app::set_d3dcompiler(&state, &required_game_dir(&state)?, enabled)?;
});

state_command!(set_all_intro_skip, enabled: bool, |state| {
    app::set_all_intro_skip(&state, &required_game_dir(&state)?, enabled)?;
});

state_command!(set_all_qol, enabled: bool, |state| {
    let game_dir = required_game_dir(&state)?;
    let previous = app::qol_state(Some(&game_dir));
    let mut applied = Vec::new();
    let result = (|| {
        if previous.d3dcompiler != enabled {
            app::set_d3dcompiler(&state, &game_dir, enabled)?;
            applied.push("d3d");
        }
        if previous.intro != enabled {
            app::set_intro_skip(&state, &game_dir, enabled)?;
            applied.push("intro");
        }
        if previous.all_intros != enabled {
            app::set_all_intro_skip(&state, &game_dir, enabled)?;
            applied.push("all");
        }
        Ok(())
    })();
    if let Err(error) = result {
        for action in applied.into_iter().rev() {
            let _ = match action {
                "d3d" => app::set_d3dcompiler(&state, &game_dir, previous.d3dcompiler),
                "intro" => app::set_intro_skip(&state, &game_dir, previous.intro),
                _ => app::set_all_intro_skip(&state, &game_dir, previous.all_intros),
            };
        }
        return Err(error);
    }
});

state_command!(configure_t7, gamertag: Option<String>, color_code: Option<String>, network_password: Option<String>, friends_only: Option<bool>, |state| {
    t7::configure(
        &state,
        &required_game_dir(&state)?,
        gamertag.as_deref(),
        color_code.as_deref().unwrap_or(""),
        network_password.as_deref(),
        friends_only,
    )?;
});

state_command!(apply_preset, name: String, |state| {
    app::apply_preset(&state, &required_game_dir(&state)?, &name)?;
});

state_command!(install_t7, |state| {
    let game_dir = required_game_dir(&state)?;
    let profile = exe::status(&state.load_settings(), Some(&game_dir)).profile;
    t7::install(&state, &game_dir, &profile)?;
});

state_command!(uninstall_t7, |app| {
    t7::uninstall(&app, &required_game_dir(&app)?)?;
});

#[tauri::command]
pub async fn validate_enhanced_source(
    state: State<'_, AppState>,
    dump_source: String,
) -> Result<EnhancedValidation, String> {
    let state = state.inner().clone();
    blocking(move || {
        let _operation = state.lock_operation()?;
        let valid = enhanced::validate_and_remember_dump_source(&state, Path::new(&dump_source))?;
        let message = if valid {
            "Ready"
        } else {
            "Required BO3 dump files were not found"
        }
        .into();
        Ok(EnhancedValidation {
            valid,
            message,
            state: current_state(&state)?,
        })
    })
    .await
}

state_command!(install_enhanced, dump_source: String, |state| {
    enhanced::install(&state, &required_game_dir(&state)?, Path::new(&dump_source))?;
});

state_command!(uninstall_enhanced, |app| {
    enhanced::uninstall(&app, &required_game_dir(&app)?)?;
});

state_command!(configure_dxvk, settings: DxvkSettings, |state| {
    dxvk::configure(&state, &required_game_dir(&state)?, &settings)?;
});

state_command!(install_dxvk, settings: DxvkSettings, |state| {
    dxvk::install(&state, &required_game_dir(&state)?, &settings)?;
});

state_command!(uninstall_dxvk, |app| {
    dxvk::uninstall(&app, &required_game_dir(&app)?)?;
});

state_command!(set_config_readonly, enabled: bool, |state| {
    app::set_config_readonly(&state, &required_game_dir(&state)?, enabled)?;
});

state_command!(set_vram_target, limited: bool, target: i32, |state| {
    app::set_vram_target(&state, &required_game_dir(&state)?, limited, target)?;
});

#[tauri::command]
pub async fn get_log_payload(state: State<'_, AppState>) -> Result<String, String> {
    let state = state.inner().clone();
    blocking(move || Ok(app::log_payload(&state))).await
}

state_command!(clear_logs, |app| {
    app.clear_logs()?;
    app.log("Success", "Logs cleared.");
});
state_command!(clear_mod_files, |app| {
    app::clear_mod_files(&app)?;
});

fn reset_to_stock_inner(state: &AppState) -> Result<(), String> {
    let game_dir = required_game_dir(state)?;
    let mut errors = Vec::new();
    if enhanced::detect_install(&game_dir) || enhanced::has_owned_install(state, &game_dir) {
        if let Err(error) = enhanced::uninstall(state, &game_dir) {
            errors.push(format!("Enhanced: {error}"));
        }
    }
    if exe::status(&state.load_settings(), Some(&game_dir)).profile != exe::CURRENT_EXE_ID {
        if let Err(error) = exe::activate_current(state, &game_dir) {
            errors.push(format!("EXE: {error}"));
        }
    }
    if let Err(error) = t7::uninstall(state, &game_dir) {
        errors.push(format!("T7 Patch: {error}"));
    }
    if let Err(error) = dxvk::uninstall(state, &game_dir) {
        errors.push(format!("DXVK: {error}"));
    }
    let qol = app::qol_state(Some(&game_dir));
    if qol.d3dcompiler {
        if let Err(error) = app::set_d3dcompiler(state, &game_dir, false) {
            errors.push(format!("d3dcompiler: {error}"));
        }
    }
    if qol.all_intros {
        if let Err(error) = app::set_all_intro_skip(state, &game_dir, false) {
            errors.push(format!("intros: {error}"));
        }
    } else if qol.intro {
        if let Err(error) = app::set_intro_skip(state, &game_dir, false) {
            errors.push(format!("intro: {error}"));
        }
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

state_command!(reset_to_stock, |app| {
    reset_to_stock_inner(&app)?;
});
state_command!(launch_game, |app| {
    steam::launch_game(&app)?;
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_order_matches_stable_and_beta_channels() {
        assert!(compare_versions("v1.3.1", "1.3.0-beta3").is_gt());
        assert!(compare_versions("1.3.0", "1.3.0-beta3").is_gt());
        assert!(compare_versions("1.3.0-beta4", "1.3.0-beta3").is_gt());
        assert_eq!(compare_versions("v1.3", "1.3.0"), Ordering::Equal);
    }
}
