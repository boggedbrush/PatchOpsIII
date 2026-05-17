use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow, Window};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowState {
    maximized: bool,
}

fn main_window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    app.get_webview_window("main")
}

fn state_for(window: &WebviewWindow) -> WindowState {
    WindowState {
        maximized: window.is_maximized().unwrap_or(false),
    }
}

fn state_for_window(window: &Window) -> WindowState {
    WindowState {
        maximized: window.is_maximized().unwrap_or(false),
    }
}

pub fn emit_window_state(window: &Window) {
    let _ = window.emit("desktop:window-state", state_for_window(window));
}

#[tauri::command]
pub fn window_state(app: AppHandle) -> WindowState {
    main_window(&app)
        .map(|window| state_for(&window))
        .unwrap_or(WindowState { maximized: false })
}

#[tauri::command]
pub fn window_minimize(app: AppHandle) {
    if let Some(window) = main_window(&app) {
        let _ = window.minimize();
    }
}

#[tauri::command]
pub fn window_toggle_maximize(app: AppHandle) -> WindowState {
    let Some(window) = main_window(&app) else {
        return WindowState { maximized: false };
    };

    let maximized = window.is_maximized().unwrap_or(false);
    if maximized {
        let _ = window.unmaximize();
    } else {
        let _ = window.maximize();
    }

    let state = state_for(&window);
    let _ = window.emit("desktop:window-state", state.clone());
    state
}

#[tauri::command]
pub fn window_close(app: AppHandle) {
    if let Some(window) = main_window(&app) {
        let _ = window.close();
    }
}
