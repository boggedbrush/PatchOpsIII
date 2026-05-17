use std::process::{Command, Stdio};

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::AppState;

#[tauri::command]
pub fn backend_url(state: State<'_, AppState>) -> String {
    state.backend_url.clone()
}

#[tauri::command]
pub fn platform() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "win32"
    }
    #[cfg(target_os = "macos")]
    {
        "darwin"
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        "linux"
    }
}

#[tauri::command]
pub async fn pick_game_directory(app: AppHandle) -> Option<String> {
    app.dialog()
        .file()
        .set_title("Select Black Ops III folder")
        .blocking_pick_folder()
        .map(|path| path.to_string())
}

#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    let trimmed = url.trim();
    if !is_allowed_external_url(trimmed) {
        return Err("unsupported external URL scheme".to_string());
    }

    let mut command = external_open_command(trimmed);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command.spawn().map_err(|error| error.to_string())?;
    Ok(())
}

fn is_allowed_external_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    ["https://", "http://", "steam://"]
        .iter()
        .any(|scheme| lower.starts_with(scheme))
}

fn external_open_command(url: &str) -> Command {
    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("rundll32");
        command.args(["url.dll,FileProtocolHandler", url]);
        command
    }

    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("open");
        command.arg(url);
        command
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    }
}

#[cfg(test)]
mod tests {
    use super::is_allowed_external_url;

    #[test]
    fn allows_expected_external_url_schemes() {
        assert!(is_allowed_external_url("https://example.com"));
        assert!(is_allowed_external_url("http://example.com"));
        assert!(is_allowed_external_url("steam://open/console"));
    }

    #[test]
    fn rejects_local_or_script_urls() {
        assert!(!is_allowed_external_url(
            "file:///C:/Windows/system32/calc.exe"
        ));
        assert!(!is_allowed_external_url("javascript:alert(1)"));
        assert!(!is_allowed_external_url(""));
    }
}
