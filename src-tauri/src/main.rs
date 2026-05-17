#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod sidecar;
mod window;

use std::sync::Mutex;

use tauri::Manager;

use sidecar::BackendSupervisor;

pub struct AppState {
    backend_url: String,
    backend: Mutex<BackendSupervisor>,
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            backend_url: sidecar::backend_url(),
            backend: Mutex::new(BackendSupervisor::default()),
        })
        .setup(|app| {
            let state = app.state::<AppState>();
            if let Err(error) = state
                .backend
                .lock()
                .expect("backend supervisor poisoned")
                .start(app.handle())
            {
                eprintln!("failed to start backend: {error}");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::backend_url,
            commands::open_external_url,
            commands::platform,
            commands::pick_game_directory,
            window::window_state,
            window::window_minimize,
            window::window_toggle_maximize,
            window::window_close
        ])
        .on_window_event(|window, event| {
            if matches!(
                event,
                tauri::WindowEvent::Resized(_) | tauri::WindowEvent::ScaleFactorChanged { .. }
            ) {
                window::emit_window_state(window);
            }
            if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                let state = window.state::<AppState>();
                state
                    .backend
                    .lock()
                    .expect("backend supervisor poisoned")
                    .stop();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building PatchOpsIII Tauri app")
        .run(|app, event| {
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                let state = app.state::<AppState>();
                state
                    .backend
                    .lock()
                    .expect("backend supervisor poisoned")
                    .stop();
            }
        });
}
