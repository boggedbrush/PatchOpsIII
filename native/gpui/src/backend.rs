//! Blocking HTTP stays on one worker thread. The UI never waits for game operations.
use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use serde_json::Value;
use std::{sync::mpsc, thread, time::Duration};

pub struct Request {
    pub path: String,
    pub body: Option<Value>,
}

pub struct Reply {
    pub state: Option<Value>,
    pub message: String,
    pub failed: bool,
    pub is_status: bool,
}

pub struct Backend {
    pub requests: mpsc::Sender<Request>,
    pub replies: mpsc::Receiver<Reply>,
}

pub fn validate_url(url: &str) -> Result<String> {
    let parsed = reqwest::Url::parse(url)?;
    if parsed.scheme() != "http"
        || parsed.host_str() != Some("127.0.0.1")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        bail!("GPUI backend must be an HTTP origin on 127.0.0.1");
    }
    Ok(url.trim_end_matches('/').to_owned())
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
        "Connected to the local service".to_owned()
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

fn execute(client: &Client, url: &str, request: Request) -> Result<Reply> {
    let path = request.path;
    let builder = match request.body {
        Some(body) => client.post(format!("{url}{path}")).json(&body),
        None => client
            .get(format!("{url}{path}"))
            .timeout(Duration::from_secs(30)),
    };
    let value = builder
        .send()
        .context("Local service request failed")?
        .error_for_status()
        .context("Local service returned an HTTP error")?
        .json::<Value>()
        .context("Local service returned invalid JSON")?;
    decode_reply(&path, value)
}

impl Backend {
    pub fn start(url: String) -> Result<Self> {
        let url = validate_url(&url)?;
        // Loopback requests must not use a developer's system HTTP proxy.
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(600))
            .build()?;
        let (requests, incoming) = mpsc::channel::<Request>();
        let (outgoing, replies) = mpsc::channel();
        thread::Builder::new()
            .name("patchops-gpui-api".into())
            .spawn(move || {
                for request in incoming {
                    let is_status = request.path == "/api/status";
                    let reply = execute(&client, &url, request).unwrap_or_else(|error| Reply {
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
        Ok(Self { requests, replies })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accepts_only_loopback_origins() {
        assert_eq!(
            validate_url("http://127.0.0.1:8767/").unwrap(),
            "http://127.0.0.1:8767"
        );
        for url in [
            "https://127.0.0.1",
            "http://example.com",
            "http://localhost",
            "http://127.0.0.1/api",
            "http://user@127.0.0.1",
            "http://127.0.0.1?x=1",
        ] {
            assert!(validate_url(url).is_err(), "{url}");
        }
    }

    #[test]
    fn handles_http_200_operation_failures_and_depot_instructions() {
        let error = decode_reply("/api/exe-swap/compatible", json!({"ok": false, "error": "Download depot first", "depotCommand": "download_depot 311210 311211 123"})).err().unwrap();
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
    fn worker_posts_exact_payload_and_surfaces_http_errors() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            let headers = String::from_utf8(request).unwrap();
            assert!(headers.starts_with("POST /api/config HTTP/1.1"));
            let length: usize = headers
                .lines()
                .find_map(|line| {
                    line.to_lowercase()
                        .strip_prefix("content-length:")
                        .map(|s| s.trim().parse().unwrap())
                })
                .unwrap();
            let mut body = vec![0; length];
            stream.read_exact(&mut body).unwrap();
            assert_eq!(
                serde_json::from_slice::<Value>(&body).unwrap(),
                json!({"key": "MaxFPS", "value": 144})
            );
            stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        let backend = Backend::start(url).unwrap();
        backend
            .requests
            .send(Request {
                path: "/api/config".into(),
                body: Some(json!({"key": "MaxFPS", "value": 144})),
            })
            .unwrap();
        let reply = backend
            .replies
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(reply.failed);
        assert!(reply.message.contains("503"));
        assert!(reply.state.is_none());
        server.join().unwrap();
    }
}
