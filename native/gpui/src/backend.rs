//! Blocking HTTP stays on one worker thread. The UI never waits for game operations.
use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use serde_json::Value;
use std::{
    env,
    ffi::OsString,
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const BACKEND_HOST: &str = "127.0.0.1";
const BACKEND_START_TIMEOUT: Duration = Duration::from_secs(30);

struct LaunchSpec {
    command: PathBuf,
    args: Vec<OsString>,
    app_root: PathBuf,
    packaged: bool,
}

/// Owns the local API process for the lifetime of the desktop application.
///
/// `PATCHOPSIII_GPUI_BACKEND_URL` remains an escape hatch for the development
/// launcher and tests. In that mode the process that supplied the URL retains
/// ownership of the service.
pub struct BackendService {
    url: String,
    child: Option<Child>,
}

impl BackendService {
    pub fn launch() -> Result<Self> {
        if let Ok(url) = env::var("PATCHOPSIII_GPUI_BACKEND_URL") {
            return Ok(Self {
                url: validate_url(&url)?,
                child: None,
            });
        }

        let spec = resolve_launch_spec()?;
        let port = reserve_loopback_port()?;
        let url = format!("http://{BACKEND_HOST}:{port}");
        let mut command = Command::new(&spec.command);
        command
            .args(&spec.args)
            .current_dir(&spec.app_root)
            .env("PATCHOPSIII_BACKEND_HOST", BACKEND_HOST)
            .env("PATCHOPSIII_BACKEND_PORT", port.to_string())
            .env("PYTHONUNBUFFERED", "1")
            .stdin(Stdio::null());
        if let Some(version) = application_version(&spec.app_root) {
            command.env("PATCHOPSIII_VERSION", version);
        }
        if spec.packaged {
            command.stdout(Stdio::null()).stderr(Stdio::null());
        } else {
            command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
        }
        configure_child_process(&mut command);

        let mut child = command.spawn().with_context(|| {
            format!(
                "Unable to start the PatchOpsIII backend at {}",
                spec.command.display()
            )
        })?;
        if let Err(error) = wait_until_healthy(&mut child, &url, BACKEND_START_TIMEOUT) {
            stop_child(&mut child);
            return Err(error);
        }
        Ok(Self {
            url,
            child: Some(child),
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for BackendService {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            stop_child(child);
        }
    }
}

fn backend_executable_name() -> &'static str {
    if cfg!(windows) {
        "patchops-backend.exe"
    } else {
        "patchops-backend"
    }
}

fn resolve_launch_spec() -> Result<LaunchSpec> {
    if let Some(path) = env::var_os("PATCHOPSIII_BACKEND_PATH").map(PathBuf::from) {
        let path = if path.is_absolute() {
            path
        } else {
            env::current_dir()
                .context("Unable to resolve PATCHOPSIII_BACKEND_PATH")?
                .join(path)
        };
        if !path.is_file() {
            bail!(
                "PATCHOPSIII_BACKEND_PATH does not name a file: {}",
                path.display()
            );
        }
        return packaged_launch(path);
    }

    let executable = env::current_exe().context("Unable to locate the GPUI executable")?;
    let executable_dir = executable
        .parent()
        .context("The GPUI executable has no parent directory")?;
    let name = backend_executable_name();
    let packaged_candidates = [
        executable_dir
            .join("resources")
            .join("backend-bin")
            .join(name),
        executable_dir.join("backend-bin").join(name),
        executable_dir
            .parent()
            .unwrap_or(executable_dir)
            .join("Resources")
            .join("backend-bin")
            .join(name),
    ];
    for path in packaged_candidates {
        if path.is_file() {
            return packaged_launch(path);
        }
    }

    let app_root = find_source_root(&executable)?;
    let python = python_command(&app_root);
    Ok(LaunchSpec {
        command: python,
        args: vec![app_root.join("backend").join("api.py").into_os_string()],
        app_root,
        packaged: false,
    })
}

fn packaged_launch(path: PathBuf) -> Result<LaunchSpec> {
    ensure_backend_executable(&path)?;
    let app_root = path
        .parent()
        .and_then(Path::parent)
        .context("Packaged backend has no resources directory")?
        .to_path_buf();
    Ok(LaunchSpec {
        command: path,
        args: Vec::new(),
        app_root,
        packaged: true,
    })
}

#[cfg(unix)]
fn ensure_backend_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    let mode = permissions.mode();
    if mode & 0o100 == 0 {
        permissions.set_mode(mode | 0o100);
        fs::set_permissions(path, permissions).with_context(|| {
            format!(
                "Unable to make the packaged backend executable: {}",
                path.display()
            )
        })?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_backend_executable(_path: &Path) -> Result<()> {
    Ok(())
}

fn find_source_root(executable: &Path) -> Result<PathBuf> {
    let current_dir = env::current_dir().context("Unable to read the working directory")?;
    for start in [Some(current_dir.as_path()), executable.parent()]
        .into_iter()
        .flatten()
    {
        for candidate in start.ancestors() {
            if candidate.join("backend").join("api.py").is_file()
                && candidate.join("presets.json").is_file()
            {
                return Ok(candidate.to_path_buf());
            }
        }
    }
    bail!(
        "PatchOpsIII backend not found. Install resources/backend-bin/{} next to the app, or run from the repository.",
        backend_executable_name()
    )
}

fn python_command(app_root: &Path) -> PathBuf {
    if let Some(python) = env::var_os("PATCHOPSIII_PYTHON") {
        return PathBuf::from(python);
    }
    let venv_python = if cfg!(windows) {
        app_root.join(".venv").join("Scripts").join("python.exe")
    } else {
        app_root.join(".venv").join("bin").join("python")
    };
    if venv_python.is_file() {
        venv_python
    } else {
        PathBuf::from(if cfg!(windows) { "python" } else { "python3" })
    }
}

fn application_version(app_root: &Path) -> Option<String> {
    if let Ok(version) = env::var("PATCHOPSIII_VERSION")
        && !version.trim().is_empty()
    {
        return Some(version);
    }
    let package = fs::read_to_string(app_root.join("package.json")).ok()?;
    serde_json::from_str::<Value>(&package).ok()?["version"]
        .as_str()
        .map(str::to_owned)
}

fn reserve_loopback_port() -> Result<u16> {
    let listener = TcpListener::bind((BACKEND_HOST, 0))
        .context("Unable to reserve a loopback port for the backend")?;
    Ok(listener.local_addr()?.port())
}

fn wait_until_healthy(child: &mut Child, url: &str, timeout: Duration) -> Result<()> {
    let client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(1))
        .timeout(Duration::from_secs(1))
        .build()?;
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().context("Unable to inspect the backend")? {
            bail!("PatchOpsIII backend exited during startup with {status}");
        }
        if let Ok(response) = client.get(format!("{url}/api/health")).send()
            && response.status().is_success()
            && response
                .json::<Value>()
                .ok()
                .and_then(|body| body["ok"].as_bool())
                == Some(true)
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    bail!("PatchOpsIII backend did not become healthy within 30 seconds")
}

#[cfg(windows)]
fn configure_child_process(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn configure_child_process(_command: &mut Command) {}

fn stop_child(child: &mut Child) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    #[cfg(windows)]
    {
        let pid = child.id().to_string();
        let _ = Command::new("taskkill")
            .args(["/pid", pid.as_str(), "/t", "/f"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(not(windows))]
    {
        let pid = child.id().to_string();
        let _ = Command::new("kill")
            .args(["-TERM", pid.as_str()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

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
