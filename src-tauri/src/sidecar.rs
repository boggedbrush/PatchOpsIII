#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::{
    env, fs,
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use tauri::{AppHandle, Manager};

const DEFAULT_BACKEND_HOST: &str = "127.0.0.1";
const DEFAULT_BACKEND_PORT: u16 = 8765;
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

struct BackendEnv {
    host: String,
    port: u16,
    version: String,
    parent_pid: u32,
    shutdown_token: String,
    core_binary: Option<PathBuf>,
    resource_dir: Option<PathBuf>,
}

#[derive(Clone, Debug)]
struct BackendShutdown {
    host: String,
    port: u16,
    token: String,
}

pub fn backend_host() -> String {
    env::var("PATCHOPSIII_BACKEND_HOST")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_BACKEND_HOST.to_string())
}

pub fn backend_port() -> u16 {
    env::var("PATCHOPSIII_BACKEND_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(DEFAULT_BACKEND_PORT)
}

pub fn backend_url() -> String {
    format!("http://{}:{}", backend_host(), backend_port())
}

#[derive(Default)]
pub struct BackendSupervisor {
    child: Option<Child>,
    shutdown: Option<BackendShutdown>,
}

impl BackendSupervisor {
    pub fn start(&mut self, app: &AppHandle) -> Result<(), String> {
        if self.child.is_some() {
            return Ok(());
        }

        let root = app_root(app);
        let host = backend_host();
        let port = backend_port();
        let backend_binary = if cfg!(debug_assertions) {
            None
        } else {
            find_binary(app, "patchops-backend")
        };
        let mut command = if let Some(binary) = backend_binary {
            Command::new(binary)
        } else {
            let mut command = Command::new(python_command(root.as_deref()));
            command.args([
                "-m",
                "uvicorn",
                "backend.api:app",
                "--host",
                &host,
                "--port",
                &port.to_string(),
            ]);
            command
        };

        if let Some(root) = root.as_ref() {
            command.current_dir(root);
        }

        let backend_env = BackendEnv {
            host,
            port,
            version: app_version(app, root.as_deref()),
            parent_pid: std::process::id(),
            shutdown_token: shutdown_token(),
            core_binary: find_binary(app, "patchops-core"),
            resource_dir: resource_root(app),
        };
        let shutdown = BackendShutdown {
            host: backend_env.host.clone(),
            port: backend_env.port,
            token: backend_env.shutdown_token.clone(),
        };

        apply_backend_env(&mut command, &backend_env);
        configure_backend_process(&mut command);

        self.child = Some(command.spawn().map_err(|error| error.to_string())?);
        self.shutdown = Some(shutdown);
        Ok(())
    }

    pub fn stop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };

        if let Some(shutdown) = self.shutdown.take() {
            let _ = request_backend_shutdown(&shutdown);
            if wait_for_exit(&mut child, Duration::from_secs(4)) {
                let _ = child.wait();
                return;
            }
        }

        #[cfg(target_os = "windows")]
        {
            let _ = Command::new("taskkill")
                .args(["/pid", &child.id().to_string(), "/t"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();

            if !wait_for_exit(&mut child, Duration::from_secs(2)) {
                let _ = Command::new("taskkill")
                    .args(["/pid", &child.id().to_string(), "/t", "/f"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = Command::new("kill")
                .args(["-TERM", &child.id().to_string()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();

            if !wait_for_exit(&mut child, Duration::from_secs(3)) {
                let _ = child.kill();
            }
        }

        let _ = child.wait();
    }
}

fn apply_backend_env(command: &mut Command, backend_env: &BackendEnv) {
    if let Some(core_binary) = backend_env.core_binary.as_ref() {
        command.env("PATCHOPSIII_CORE_BINARY", core_binary);
    }
    if let Some(resource_dir) = backend_env.resource_dir.as_ref() {
        command.env("PATCHOPSIII_RESOURCE_DIR", resource_dir);
    }

    command
        .env("PATCHOPSIII_BACKEND_HOST", &backend_env.host)
        .env("PATCHOPSIII_BACKEND_PORT", backend_env.port.to_string())
        .env("PATCHOPSIII_PARENT_PID", backend_env.parent_pid.to_string())
        .env("PATCHOPSIII_SHUTDOWN_TOKEN", &backend_env.shutdown_token)
        .env("PATCHOPSIII_VERSION", &backend_env.version)
        .env("PYTHONUNBUFFERED", "1");
}

fn shutdown_token() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{}-{nanos}", std::process::id())
}

fn request_backend_shutdown(shutdown: &BackendShutdown) -> std::io::Result<()> {
    let mut stream = TcpStream::connect((&*shutdown.host, shutdown.port))?;
    stream.set_read_timeout(Some(Duration::from_secs(1)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;

    let request = format!(
        "POST /api/shutdown HTTP/1.1\r\nHost: {}:{}\r\nContent-Length: 0\r\nX-Patchopsiii-Shutdown-Token: {}\r\nConnection: close\r\n\r\n",
        shutdown.host, shutdown.port, shutdown.token
    );
    stream.write_all(request.as_bytes())?;
    stream.flush()?;

    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    if response.starts_with("HTTP/1.1 200") || response.starts_with("HTTP/1.0 200") {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "backend rejected shutdown request",
        ))
    }
}

fn wait_for_exit(child: &mut Child, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            _ => return false,
        }
    }
}

fn configure_backend_process(command: &mut Command) {
    command.stdin(Stdio::null());

    #[cfg(target_os = "windows")]
    {
        if cfg!(debug_assertions) {
            command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
        } else {
            command
                .creation_flags(CREATE_NO_WINDOW)
                .stdout(Stdio::null())
                .stderr(Stdio::null());
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    }
}

impl Drop for BackendSupervisor {
    fn drop(&mut self) {
        self.stop();
    }
}

fn app_version(app: &AppHandle, app_root: Option<&Path>) -> String {
    if let Ok(value) = env::var("PATCHOPSIII_VERSION") {
        if !value.trim().is_empty() {
            return value;
        }
    }

    let mut candidates = Vec::new();
    if let Some(root) = app_root {
        candidates.push(root.join("package.json"));
    }
    if let Some(resource_dir) = resource_root(app) {
        candidates.push(resource_dir.join("package.json"));
    }

    for candidate in candidates {
        let Ok(content) = fs::read_to_string(candidate) else {
            continue;
        };
        let Ok(package) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        if let Some(version) = package.get("version").and_then(|value| value.as_str()) {
            if !version.trim().is_empty() {
                return version.to_string();
            }
        }
    }

    if let Some(version) = option_env!("PATCHOPSIII_PACKAGE_VERSION") {
        if !version.trim().is_empty() {
            return version.to_string();
        }
    }

    app.package_info().version.to_string()
}

fn python_command(app_root: Option<&Path>) -> String {
    if let Ok(value) = env::var("PATCHOPSIII_PYTHON") {
        if !value.trim().is_empty() {
            return value;
        }
    }

    let root = app_root.unwrap_or_else(|| Path::new("."));
    let venv = if cfg!(target_os = "windows") {
        root.join(".venv").join("Scripts").join("python.exe")
    } else {
        root.join(".venv").join("bin").join("python")
    };
    if venv.is_file() {
        return venv.to_string_lossy().into_owned();
    }

    if cfg!(target_os = "windows") {
        "python".to_string()
    } else {
        "python3".to_string()
    }
}

fn app_root(app: &AppHandle) -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        let current = env::current_dir().ok()?;
        if current.join("backend").is_dir() {
            return Some(current);
        }
        if let Some(parent) = current.parent() {
            if parent.join("backend").is_dir() {
                return Some(parent.to_path_buf());
            }
        }
        return Some(current);
    }
    resource_root(app).or_else(|| app.path().resource_dir().ok())
}

fn resource_root(app: &AppHandle) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir.join("_up_"));
        candidates.push(resource_dir);
    }
    if let Ok(current_exe) = env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            candidates.push(parent.join("_up_"));
            candidates.push(parent.to_path_buf());
        }
    }

    candidates.into_iter().find(|candidate| {
        candidate.join("presets.json").is_file() || candidate.join("package.json").is_file()
    })
}

fn find_binary(app: &AppHandle, name: &str) -> Option<PathBuf> {
    let mut directories = Vec::new();
    if let Some(root) = app_root(app) {
        directories.push(root.join("src-tauri").join("binaries"));
        directories.push(root.join("dist").join("backend"));
        directories.push(root.join("target").join("release"));
        directories.push(root.join("target").join("debug"));
    }
    if let Ok(resource_dir) = app.path().resource_dir() {
        directories.push(resource_dir);
    }
    if let Ok(current_exe) = env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            directories.push(parent.to_path_buf());
        }
    }

    directories
        .into_iter()
        .find_map(|directory| find_binary_in(&directory, name))
}

fn find_binary_in(directory: &Path, name: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(directory).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let lower = file_name.to_ascii_lowercase();
        if lower.starts_with(name) && (!cfg!(target_os = "windows") || lower.ends_with(".exe")) {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, ffi::OsString};

    use super::{apply_backend_env, BackendEnv, Command, PathBuf};

    fn command_env(command: &Command) -> HashMap<String, OsString> {
        command
            .get_envs()
            .filter_map(|(key, value)| {
                value.map(|value| (key.to_string_lossy().into_owned(), value.to_os_string()))
            })
            .collect()
    }

    #[test]
    fn applies_backend_sidecar_environment() {
        let mut command = if cfg!(target_os = "windows") {
            Command::new("cmd")
        } else {
            Command::new("sh")
        };
        let backend_env = BackendEnv {
            host: "127.0.0.1".to_string(),
            port: 8767,
            version: "v1.3.0-beta3".to_string(),
            parent_pid: 4242,
            shutdown_token: "shutdown-token-test".to_string(),
            core_binary: Some(PathBuf::from("patchops-core-test")),
            resource_dir: Some(PathBuf::from("resource-root-test")),
        };

        apply_backend_env(&mut command, &backend_env);

        let env = command_env(&command);
        assert_eq!(env["PATCHOPSIII_BACKEND_HOST"], "127.0.0.1");
        assert_eq!(env["PATCHOPSIII_BACKEND_PORT"], "8767");
        assert_eq!(env["PATCHOPSIII_PARENT_PID"], "4242");
        assert_eq!(env["PATCHOPSIII_SHUTDOWN_TOKEN"], "shutdown-token-test");
        assert_eq!(env["PATCHOPSIII_VERSION"], "v1.3.0-beta3");
        assert_eq!(env["PYTHONUNBUFFERED"], "1");
        assert_eq!(
            env["PATCHOPSIII_CORE_BINARY"],
            OsString::from("patchops-core-test")
        );
        assert_eq!(
            env["PATCHOPSIII_RESOURCE_DIR"],
            OsString::from("resource-root-test")
        );
    }

    #[test]
    fn omits_optional_sidecar_environment_when_paths_are_missing() {
        let mut command = if cfg!(target_os = "windows") {
            Command::new("cmd")
        } else {
            Command::new("sh")
        };
        let backend_env = BackendEnv {
            host: "127.0.0.1".to_string(),
            port: 8765,
            version: "1.3.0".to_string(),
            parent_pid: 2424,
            shutdown_token: "shutdown-token-test".to_string(),
            core_binary: None,
            resource_dir: None,
        };

        apply_backend_env(&mut command, &backend_env);

        let env = command_env(&command);
        assert!(!env.contains_key("PATCHOPSIII_CORE_BINARY"));
        assert!(!env.contains_key("PATCHOPSIII_RESOURCE_DIR"));
        assert_eq!(env["PATCHOPSIII_BACKEND_HOST"], "127.0.0.1");
        assert_eq!(env["PATCHOPSIII_BACKEND_PORT"], "8765");
        assert_eq!(env["PATCHOPSIII_PARENT_PID"], "2424");
        assert_eq!(env["PATCHOPSIII_SHUTDOWN_TOKEN"], "shutdown-token-test");
    }
}
