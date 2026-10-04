//! In-process Rust backend hosted on one worker thread.
use anyhow::{Result, bail};
use patchops_core::{AppState, Engine, EventCallback, models::LogEntry};
use serde_json::Value;
use std::{sync::mpsc, thread};

pub use patchops_core::models::OperationProgress;

pub struct Request {
    pub path: String,
    pub body: Option<Value>,
}

#[derive(Debug)]
pub struct Reply {
    pub state: Option<Value>,
    pub message: String,
    pub failed: bool,
    pub is_status: bool,
}

/// Streaming events emitted while a long-running operation is executing.
///
/// The current UI obtains the same entries in the next state response. A UI
/// can subscribe to Backend::events to render logs/progress immediately.
#[derive(Clone, Debug)]
#[allow(dead_code)] // UI event drain is wired separately from the backend port.
pub enum BackendEvent {
    Log(LogEntry),
    Progress(OperationProgress),
}

pub struct Backend {
    pub requests: mpsc::Sender<Request>,
    pub replies: mpsc::Receiver<Reply>,
    #[allow(dead_code)] // Public subscription hook for the UI.
    pub events: mpsc::Receiver<BackendEvent>,
}

pub fn decode_reply(path: &str, value: Value) -> Result<Reply> {
    if value.get("ok").and_then(Value::as_bool) == Some(false)
        || value.get("valid").and_then(Value::as_bool) == Some(false)
    {
        let mut message = value["error"]
            .as_str()
            .or(value["message"].as_str())
            .unwrap_or("Operation failed")
            .to_owned();
        if let Some(command) = value["depotCommand"].as_str() {
            message.push_str(&format!("\nSteam console command: {command}"));
        }
        bail!("{message}");
    }
    if path != "/api/status" && value.get("ok").and_then(Value::as_bool) != Some(true) {
        bail!("Backend returned an invalid operation result");
    }
    let state = if path == "/api/status" {
        if !value["appVersion"].is_string() || !value["gameDetected"].is_boolean() {
            bail!("Backend returned an invalid status document");
        }
        Some(value.clone())
    } else {
        value.get("state").cloned()
    };
    let message = if path == "/api/status" {
        "Connected to the in-process Rust backend".to_owned()
    } else if let Some(update) = value.get("update") {
        format!("Update check: {update}")
    } else {
        value["message"]
            .as_str()
            .unwrap_or("Operation completed")
            .to_owned()
    };
    Ok(Reply {
        state,
        message,
        failed: false,
        is_status: path == "/api/status",
    })
}

fn execute(engine: &Engine, request: Request) -> Result<Reply> {
    let path = request.path;
    let value = engine
        .dispatch(&path, request.body)
        .map_err(anyhow::Error::msg)?;
    decode_reply(&path, value)
}

impl Backend {
    pub fn start() -> Result<Self> {
        Self::start_with(|callback| AppState::for_desktop(Some(callback)))
    }

    fn start_with(
        factory: impl FnOnce(EventCallback) -> std::result::Result<AppState, String> + Send + 'static,
    ) -> Result<Self> {
        let (requests, incoming) = mpsc::channel::<Request>();
        let (outgoing, replies) = mpsc::channel();
        let (event_sender, events) = mpsc::channel();
        thread::Builder::new()
            .name("patchops-gpui-core".into())
            .spawn(move || {
                let progress_sender = event_sender.clone();
                let callback = std::sync::Arc::new(move |entry| {
                    let _ = event_sender.send(BackendEvent::Log(entry));
                });
                let state = match factory(callback) {
                    Ok(state) => state,
                    Err(error) => {
                        for request in incoming {
                            let is_status = request.path == "/api/status";
                            if outgoing
                                .send(Reply {
                                    state: None,
                                    message: error.clone(),
                                    failed: true,
                                    is_status,
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                        return;
                    }
                };
                state.set_progress_callback(std::sync::Arc::new(move |progress| {
                    let _ = progress_sender.send(BackendEvent::Progress(progress));
                }));
                let engine = Engine::new(state);
                for request in incoming {
                    let is_status = request.path == "/api/status";
                    let reply = execute(&engine, request).unwrap_or_else(|error| Reply {
                        state: None,
                        message: format!("{error:#}"),
                        failed: true,
                        is_status,
                    });
                    if outgoing.send(reply).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            requests,
            replies,
            events,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn handles_operation_failures_and_depot_instructions() {
        let error = decode_reply(
            "/api/exe-swap/compatible",
            json!({
                "ok": false,
                "error": "Download depot first",
                "depotCommand": "download_depot 311210 311211 123"
            }),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("download_depot 311210 311211 123")
        );
        assert!(decode_reply("/api/status", json!({"ok": true})).is_err());
        assert!(decode_reply("/api/config", json!({"state": {}})).is_err());
        assert!(
            decode_reply(
                "/api/enhanced-validate",
                json!({"ok": true, "valid": false, "message": "Missing files"})
            )
            .is_err()
        );
    }

    #[test]
    fn preserves_backend_state_without_fabricating_success() {
        let state = json!({"appVersion": "v1.3.4", "gameDetected": false});
        assert_eq!(
            decode_reply("/api/status", state.clone()).unwrap().state,
            Some(state.clone())
        );
        assert_eq!(
            decode_reply("/api/config", json!({"ok": true, "state": state}))
                .unwrap()
                .state,
            Some(state)
        );
    }

    #[test]
    fn worker_returns_in_process_status() {
        let root =
            std::env::temp_dir().join(format!("patchops-worker-test-{}", std::process::id()));
        let data = root.join("data");
        let game = root.join("game");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(game.join("players")).unwrap();
        std::fs::write(game.join("BlackOps3.exe"), b"fake game").unwrap();
        std::fs::write(game.join("players/config.ini"), b"FOV = \"90\"\n").unwrap();
        std::fs::write(
            data.join("electron-settings.json"),
            json!({"game_dir": game}).to_string(),
        )
        .unwrap();
        let backend =
            Backend::start_with(move |callback| AppState::new(data, None, Some(callback))).unwrap();
        backend
            .requests
            .send(Request {
                path: "/api/status".into(),
                body: None,
            })
            .unwrap();
        let reply = backend
            .replies
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert!(!reply.failed, "{}", reply.message);
        let state = reply.state.unwrap();
        assert!(state["appVersion"].is_string());
        assert_eq!(state["gameDetected"], true);
        assert_eq!(state["graphics"]["fov"], 90);
        drop(backend);
        std::fs::remove_dir_all(root).unwrap();
    }
}
