use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, WebviewWindow};
use tauri_plugin_dialog::{DialogExt, FilePath};
use tauri_plugin_opener::OpenerExt;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowState {
    pub maximized: bool,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExternalTarget {
    SteamConsole,
    EnhancedGuide,
}

fn state(window: &WebviewWindow) -> Result<WindowState, String> {
    Ok(WindowState {
        maximized: window.is_maximized().map_err(|error| error.to_string())?,
    })
}

pub fn emit_window_state(window: &WebviewWindow) {
    if let Ok(state) = state(window) {
        let _ = window.emit("patchops-window-state", state);
    }
}

#[tauri::command]
pub fn get_platform() -> &'static str {
    match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        other => other,
    }
}

#[tauri::command]
pub fn get_window_state(window: WebviewWindow) -> Result<WindowState, String> {
    state(&window)
}

#[tauri::command]
pub fn minimize_window(window: WebviewWindow) -> Result<(), String> {
    window.minimize().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn toggle_maximize_window(window: WebviewWindow) -> Result<WindowState, String> {
    if window.is_maximized().map_err(|error| error.to_string())? {
        window.unmaximize().map_err(|error| error.to_string())?;
    } else {
        window.maximize().map_err(|error| error.to_string())?;
    }
    let state = state(&window)?;
    let _ = window.emit("patchops-window-state", state);
    Ok(state)
}

#[tauri::command]
pub fn close_window(window: WebviewWindow) -> Result<(), String> {
    window.close().map_err(|error| error.to_string())
}

fn pick_folder(app: &AppHandle, title: &str) -> Result<Option<String>, String> {
    match app.dialog().file().set_title(title).blocking_pick_folder() {
        Some(FilePath::Path(path)) => Ok(Some(path.to_string_lossy().into_owned())),
        Some(FilePath::Url(_)) => Err("Only local folders can be selected.".into()),
        None => Ok(None),
    }
}

#[tauri::command]
pub fn pick_game_directory(app: AppHandle) -> Result<Option<String>, String> {
    pick_folder(&app, "Select Black Ops III folder")
}

#[tauri::command]
pub fn pick_dump_source(app: AppHandle) -> Result<Option<String>, String> {
    pick_folder(&app, "Select extracted BO3 dump folder")
}

#[tauri::command]
pub fn pick_dump_archive(app: AppHandle) -> Result<Option<String>, String> {
    match app
        .dialog()
        .file()
        .set_title("Select BO3 DUMP.zip")
        .add_filter("ZIP archive", &["zip"])
        .blocking_pick_file()
    {
        Some(FilePath::Path(path)) => Ok(Some(path.to_string_lossy().into_owned())),
        Some(FilePath::Url(_)) => Err("Only local ZIP archives can be selected.".into()),
        None => Ok(None),
    }
}

#[tauri::command]
pub fn open_external(app: AppHandle, target: ExternalTarget) -> Result<(), String> {
    let url = match target {
        ExternalTarget::SteamConsole => "steam://open/console",
        ExternalTarget::EnhancedGuide => "https://youtu.be/rBZZTcSJ9_s",
    };
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| error.to_string())
}
