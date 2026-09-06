mod app;
#[cfg(test)]
mod benchmark;
mod commands;
mod desktop;
mod dxvk;
mod enhanced;
mod exe;
mod fs_ops;
mod models;
mod steam;
mod t7;

use app::AppState;
use tauri::{Emitter, Manager};

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = AppState::new(app.handle().clone()).map_err(std::io::Error::other)?;
            state.log("Info", "PatchOpsIII started.");
            app.manage(state);

            let window = app
                .get_webview_window("main")
                .ok_or_else(|| std::io::Error::other("main window was not created"))?;
            let event_window = window.clone();
            window.on_window_event(move |event| match event {
                tauri::WindowEvent::Resized(_) => desktop::emit_window_state(&event_window),
                tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, position }) => {
                    let paths = paths
                        .iter()
                        .map(|path| path.to_string_lossy().into_owned())
                        .collect::<Vec<_>>();
                    let _ = event_window.emit(
                        "patchops-file-drop",
                        serde_json::json!({ "paths": paths, "position": position }),
                    );
                }
                _ => {}
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::check_for_updates,
            commands::set_release_channel,
            commands::set_game_directory,
            commands::activate_compatible_exe,
            commands::get_compatible_depot_status,
            commands::activate_current_exe,
            commands::activate_enhanced_exe,
            commands::set_config_value,
            commands::apply_launch_profile,
            commands::install_workshop_profile,
            commands::set_intro_skip,
            commands::set_d3dcompiler_workaround,
            commands::set_all_intro_skip,
            commands::set_all_qol,
            commands::configure_t7,
            commands::apply_preset,
            commands::install_t7,
            commands::uninstall_t7,
            commands::validate_enhanced_source,
            commands::install_enhanced,
            commands::uninstall_enhanced,
            commands::configure_dxvk,
            commands::install_dxvk,
            commands::uninstall_dxvk,
            commands::set_config_readonly,
            commands::set_vram_target,
            commands::get_log_payload,
            commands::clear_logs,
            commands::clear_mod_files,
            commands::reset_to_stock,
            commands::launch_game,
            desktop::get_platform,
            desktop::get_window_state,
            desktop::minimize_window,
            desktop::toggle_maximize_window,
            desktop::close_window,
            desktop::pick_game_directory,
            desktop::pick_dump_source,
            desktop::pick_dump_archive,
            desktop::open_external,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run PatchOpsIII");
}
