//! Blocking HTTP layer for the network ops (update check, DXVK / T7 / BO3 Enhanced downloads).
//!
//! * `ureq` with rustls (ring) and the bundled Mozilla roots: no system OpenSSL, works the same
//!   in the AppImage and the Windows exe.
//! * `HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY` / `NO_PROXY` are honoured (handled by ureq).
//!   Custom CA bundles: `REQUESTS_CA_BUNDLE`, `CURL_CA_BUNDLE`, `SSL_CERT_FILE` (first readable
//!   one wins, same precedence idea as Python `requests`); when one is set it *replaces* the
//!   bundled roots, exactly like `requests` does.
//! * Redirects are followed (max 30, like `requests`), error statuses are reported with the
//!   `requests` wording (`404 Client Error: Not Found for url: ...`).
//! * [`Net`] bundles the [`Endpoints`] (hard-coded legacy URLs) with a [`Client`]; tests build it
//!   with [`Net::local`] and point every endpoint at a local `TcpListener` server.
//!
//! Downloads stream to disk in 64 KiB chunks, report [`Transfer`] progress and can verify a
//! SHA-256 on the fly.

use std::fmt;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use serde_json::Value;
use ureq::ResponseExt;

use crate::hash::Sha256;

/// Progress of one transfer, delivered to the GUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transfer {
    /// Bytes written so far.
    pub done: u64,
    /// `Content-Length` when the server sent one.
    pub total: Option<u64>,
}

impl Transfer {
    /// Whole percent (0..=100) when the total is known.
    pub fn percent(&self) -> Option<u8> {
        self.total.filter(|t| *t > 0).map(|t| (self.done.saturating_mul(100) / t).min(100) as u8)
    }
}

/// Progress callback of the typed install functions.
pub type ProgressFn<'a> = &'a mut dyn FnMut(&Transfer);

/// A no-op progress sink for callers that do not report progress.
pub fn no_progress(_: &Transfer) {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetError {
    /// Non-2xx answer; `Display` is Python `requests`' `raise_for_status` text.
    Status { code: u16, url: String },
    /// DNS / connect / TLS / timeout / protocol failure (ureq's text).
    Transport(String),
    /// Local file problem while saving a download.
    Io(String),
    /// Body was not valid JSON.
    Json(String),
    /// Server answered with an HTML page where a binary was expected.
    Html { url: String },
    /// Download larger than the caller's limit.
    TooLarge { limit: u64 },
    /// SHA-256 of the download is not in the allowed set.
    HashMismatch { file: String },
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetError::Status { code, url } => {
                let kind = if *code >= 500 { "Server" } else { "Client" };
                write!(f, "{code} {kind} Error: {} for url: {url}", reason_phrase(*code))
            }
            NetError::Transport(m) | NetError::Io(m) | NetError::Json(m) => f.write_str(m),
            NetError::Html { url } => write!(f, "Received HTML content from {url}; expected binary download."),
            NetError::TooLarge { limit } => write!(f, "Download exceeds the allowed size of {limit} bytes."),
            NetError::HashMismatch { file } => {
                write!(f, "Downloaded file failed integrity verification (SHA-256 mismatch): {file}")
            }
        }
    }
}

impl std::error::Error for NetError {}

fn reason_phrase(code: u16) -> &'static str {
    match code {
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        410 => "Gone",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Unknown",
    }
}

/// Base URLs of every remote the app talks to (legacy constants; injectable for tests).
#[derive(Debug, Clone)]
pub struct Endpoints {
    /// `https://api.github.com`
    pub github_api: String,
    /// `https://github.com`
    pub github_web: String,
    /// `https://gitlab.com`
    pub gitlab_web: String,
}

impl Endpoints {
    pub fn production() -> Endpoints {
        Endpoints {
            github_api: "https://api.github.com".into(),
            github_web: "https://github.com".into(),
            gitlab_web: "https://gitlab.com".into(),
        }
    }

    /// Every endpoint under one base URL (`http://127.0.0.1:PORT`); the path layout stays the same.
    pub fn local(base: &str) -> Endpoints {
        let base = base.trim_end_matches('/');
        Endpoints { github_api: base.into(), github_web: base.into(), gitlab_web: base.into() }
    }
}

/// HTTP client: one pooled agent, per-request timeouts.
pub struct Client {
    agent: ureq::Agent,
}

/// Python `requests`' default redirect limit.
const MAX_REDIRECTS: u32 = 30;
const CHUNK: usize = 64 * 1024;
/// Largest JSON document accepted (release lists with notes are well below this).
const MAX_JSON_BYTES: u64 = 32 * 1024 * 1024;

impl Client {
    /// Client for the real environment (system proxy variables, CA bundle variables).
    pub fn new() -> Client {
        Client::build(true)
    }

    /// Client that never uses a proxy (loopback test servers).
    pub fn direct() -> Client {
        Client::build(false)
    }

    fn build(use_env_proxy: bool) -> Client {
        let mut tls = ureq::tls::TlsConfig::builder();
        if let Some(certs) = custom_ca_certs() {
            tls = tls.root_certs(ureq::tls::RootCerts::new_with_certs(&certs));
        }
        let proxy = if use_env_proxy { ureq::Proxy::try_from_env() } else { None };
        let config = ureq::Agent::config_builder()
            .tls_config(tls.build())
            .proxy(proxy)
            .max_redirects(MAX_REDIRECTS)
            .http_status_as_error(false)
            .user_agent(USER_AGENT)
            .build();
        Client { agent: ureq::Agent::new_with_config(config) }
    }

    fn get(&self, url: &str, timeout: Duration) -> Result<ureq::http::Response<ureq::Body>, NetError> {
        let response = self
            .agent
            .get(url)
            .config()
            .timeout_connect(Some(timeout))
            .timeout_per_read(Some(timeout))
            .build()
            .call()
            .map_err(|e| NetError::Transport(e.to_string()))?;
        let code = response.status().as_u16();
        if !(200..300).contains(&code) {
            return Err(NetError::Status { code, url: response.get_uri().to_string() });
        }
        Ok(response)
    }

    /// GET a JSON document (`requests.get(url, timeout=..).json()`).
    pub fn get_json(&self, url: &str, timeout: Duration) -> Result<Value, NetError> {
        let mut response = self.get(url, timeout)?;
        let text = response
            .body_mut()
            .with_config()
            .limit(MAX_JSON_BYTES)
            .lossy_utf8(true)
            .read_to_string()
            .map_err(|e| NetError::Transport(e.to_string()))?;
        serde_json::from_str(&text).map_err(|e| NetError::Json(format!("Invalid JSON from {url}: {e}")))
    }

    /// Stream `url` into `dest` (created/truncated). On any failure the partial file is removed.
    /// Returns the SHA-256 of the bytes written.
    pub fn download(
        &self,
        url: &str,
        dest: &Path,
        opts: &DownloadOptions,
        progress: ProgressFn<'_>,
    ) -> Result<Downloaded, NetError> {
        let result = self.download_inner(url, dest, opts, progress);
        if result.is_err() {
            let _ = std::fs::remove_file(dest);
        }
        result
    }

    fn download_inner(
        &self,
        url: &str,
        dest: &Path,
        opts: &DownloadOptions,
        progress: ProgressFn<'_>,
    ) -> Result<Downloaded, NetError> {
        let response = self.get(url, opts.timeout)?;
        if opts.reject_html {
            let content_type = response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_ascii_lowercase();
            if content_type.contains("text/html") {
                return Err(NetError::Html { url: url.to_string() });
            }
        }
        let total = response.body().content_length();
        if let (Some(t), Some(limit)) = (total, opts.max_bytes) {
            if t > limit {
                return Err(NetError::TooLarge { limit });
            }
        }
        let (_, body) = response.into_parts();
        let mut reader = body.into_reader();

        let io_err = |e: std::io::Error| NetError::Io(crate::pyfmt::io_error_text(&e, Some(dest)));
        if let Some(parent) = dest.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(io_err)?;
        }
        let mut file = File::create(dest).map_err(io_err)?;
        let mut sha = Sha256::new();
        let mut buf = vec![0u8; CHUNK];
        let mut done = 0u64;
        progress(&Transfer { done, total });
        loop {
            let n = reader.read(&mut buf).map_err(|e| NetError::Transport(e.to_string()))?;
            if n == 0 {
                break;
            }
            done += n as u64;
            if let Some(limit) = opts.max_bytes {
                if done > limit {
                    return Err(NetError::TooLarge { limit });
                }
            }
            file.write_all(&buf[..n]).map_err(io_err)?;
            sha.update(&buf[..n]);
            progress(&Transfer { done, total });
        }
        file.flush().map_err(io_err)?;
        drop(file);

        let sha256 = sha.finish_hex();
        if !opts.expected_sha256.is_empty() && !opts.expected_sha256.iter().any(|h| h.eq_ignore_ascii_case(&sha256)) {
            let file = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            return Err(NetError::HashMismatch { file });
        }
        Ok(Downloaded { bytes: done, sha256 })
    }
}

impl Default for Client {
    fn default() -> Self {
        Client::new()
    }
}

/// GitHub release-asset `digest` field (`sha256:<64 hex>`, prefix and hex case-insensitive) as a
/// lower-case hex string; `None` for any other form.
pub fn parse_sha256_digest(raw: &str) -> Option<String> {
    let d = raw.trim();
    if !d.to_ascii_lowercase().starts_with("sha256:") {
        return None;
    }
    let c = d[7..].trim().to_ascii_lowercase();
    (c.len() == 64 && c.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))).then_some(c)
}

/// What to enforce while downloading.
#[derive(Debug, Clone)]
pub struct DownloadOptions {
    /// Connect and per-read timeout (the legacy `timeout=` values: 30 s, T7 60 s, Proton 120 s).
    pub timeout: Duration,
    /// Allowed lower-case hex SHA-256 digests; empty = no verification (legacy DXVK / Proton).
    pub expected_sha256: Vec<String>,
    /// Fail on `Content-Type: text/html` (legacy BO3 Enhanced download).
    pub reject_html: bool,
    /// Upper bound for the body; `None` = unlimited.
    pub max_bytes: Option<u64>,
}

impl DownloadOptions {
    pub fn new(timeout_secs: u64) -> DownloadOptions {
        DownloadOptions {
            timeout: Duration::from_secs(timeout_secs),
            expected_sha256: Vec::new(),
            reject_html: false,
            max_bytes: Some(DEFAULT_MAX_DOWNLOAD),
        }
    }
    pub fn verify(mut self, digests: impl IntoIterator<Item = String>) -> Self {
        self.expected_sha256 = digests.into_iter().map(|d| d.to_ascii_lowercase()).collect();
        self
    }
    pub fn reject_html(mut self) -> Self {
        self.reject_html = true;
        self
    }
}

/// No installer archive comes anywhere near this (largest: GDK-Proton, a few hundred MB).
pub const DEFAULT_MAX_DOWNLOAD: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Downloaded {
    pub bytes: u64,
    pub sha256: String,
}

/// Endpoints + client; what every network op receives.
pub struct Net {
    pub endpoints: Endpoints,
    pub client: Client,
}

impl Net {
    pub fn production() -> Net {
        Net { endpoints: Endpoints::production(), client: Client::new() }
    }

    /// Test seam: all endpoints at `base` (e.g. `http://127.0.0.1:34567`), no proxy.
    pub fn local(base: &str) -> Net {
        Net { endpoints: Endpoints::local(base), client: Client::direct() }
    }
}

impl Default for Net {
    fn default() -> Self {
        Net::production()
    }
}

/// `User-Agent` sent with every request (GitHub rejects requests without one).
pub const USER_AGENT: &str = concat!("PatchOpsIII/", env!("CARGO_PKG_VERSION"));

/// Certificates of the first readable CA bundle variable, if any.
fn custom_ca_certs() -> Option<Vec<ureq::tls::Certificate<'static>>> {
    for var in ["REQUESTS_CA_BUNDLE", "CURL_CA_BUNDLE", "SSL_CERT_FILE"] {
        let Some(path) = std::env::var_os(var).filter(|v| !v.is_empty()) else { continue };
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let certs: Vec<_> = ureq::tls::parse_pem(&bytes)
            .filter_map(|item| match item {
                Ok(ureq::tls::PemItem::Certificate(c)) => Some(c),
                _ => None,
            })
            .collect();
        if !certs.is_empty() {
            return Some(certs);
        }
    }
    None
}

#[cfg(test)]
pub(crate) mod testserver {
    //! Tiny canned-response HTTP server on a loopback `TcpListener` (no real network in tests).

    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    pub struct Route {
        pub status: u16,
        pub content_type: &'static str,
        pub body: Vec<u8>,
        /// Extra headers, e.g. `Location`.
        pub headers: Vec<(&'static str, String)>,
    }

    impl Route {
        pub fn ok(content_type: &'static str, body: impl Into<Vec<u8>>) -> Route {
            Route { status: 200, content_type, body: body.into(), headers: Vec::new() }
        }
        pub fn json(v: &serde_json::Value) -> Route {
            Route::ok("application/json", v.to_string())
        }
        pub fn bytes(body: impl Into<Vec<u8>>) -> Route {
            Route::ok("application/octet-stream", body)
        }
        pub fn status(code: u16) -> Route {
            Route { status: code, content_type: "text/plain", body: b"nope".to_vec(), headers: Vec::new() }
        }
        pub fn redirect(to: &str) -> Route {
            Route { status: 302, content_type: "text/plain", body: Vec::new(), headers: vec![("Location", to.to_string())] }
        }
    }

    pub struct Server {
        pub base: String,
        /// Request paths in arrival order.
        pub hits: Arc<Mutex<Vec<String>>>,
        /// `User-Agent` of every request.
        pub agents: Arc<Mutex<Vec<String>>>,
    }

    /// Serve `routes` (exact path -> response) until the process ends; unknown paths are 404.
    pub fn serve(routes: HashMap<String, Route>) -> Server {
        serve_listener(TcpListener::bind("127.0.0.1:0").unwrap(), routes)
    }

    /// Like [`serve`] on a specific address (lets a test learn the address before building routes).
    pub fn serve_on(addr: std::net::SocketAddr, routes: HashMap<String, Route>) -> Server {
        serve_listener(TcpListener::bind(addr).unwrap(), routes)
    }

    fn serve_listener(listener: TcpListener, routes: HashMap<String, Route>) -> Server {
        let base = format!("http://{}", listener.local_addr().unwrap());
        let hits = Arc::new(Mutex::new(Vec::new()));
        let agents = Arc::new(Mutex::new(Vec::new()));
        let (h2, a2) = (hits.clone(), agents.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).is_err() {
                    continue;
                }
                let path = request_line.split_whitespace().nth(1).unwrap_or("/").to_string();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.strip_prefix("User-Agent: ").or_else(|| line.strip_prefix("user-agent: ")) {
                        a2.lock().unwrap().push(v.trim().to_string());
                    }
                }
                h2.lock().unwrap().push(path.clone());
                let default = Route::status(404);
                let route = routes.get(&path).unwrap_or(&default);
                let mut head = format!(
                    "HTTP/1.1 {} X\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
                    route.status,
                    route.content_type,
                    route.body.len()
                );
                for (k, v) in &route.headers {
                    head.push_str(&format!("{k}: {v}\r\n"));
                }
                head.push_str("\r\n");
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&route.body);
            }
        });
        Server { base, hits, agents }
    }
}

#[cfg(test)]
mod tests {
    use super::testserver::*;
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn digest_parsing() {
        let h = "ab".repeat(32);
        assert_eq!(parse_sha256_digest(&format!("sha256:{h}")), Some(h.clone()));
        assert_eq!(parse_sha256_digest(&format!("  SHA256: {} ", h.to_uppercase())), Some(h.clone()));
        assert_eq!(parse_sha256_digest(&format!("sha1:{h}")), None);
        assert_eq!(parse_sha256_digest("sha256:abc"), None);
        assert_eq!(parse_sha256_digest(&format!("sha256:{}g", "a".repeat(63))), None);
        assert_eq!(parse_sha256_digest(""), None);
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("patchops-net-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn get_json_and_status_text() {
        let mut routes = HashMap::new();
        routes.insert("/ok".to_string(), Route::json(&serde_json::json!({"tag_name": "v1"})));
        routes.insert("/moved".to_string(), Route::redirect("/ok"));
        routes.insert("/boom".to_string(), Route::status(503));
        routes.insert("/bad".to_string(), Route::ok("application/json", "{nope"));
        let srv = serve(routes);
        let net = Net::local(&srv.base);
        let t = Duration::from_secs(5);
        assert_eq!(net.client.get_json(&format!("{}/ok", srv.base), t).unwrap()["tag_name"], "v1");
        assert_eq!(net.client.get_json(&format!("{}/moved", srv.base), t).unwrap()["tag_name"], "v1");
        let e = net.client.get_json(&format!("{}/missing", srv.base), t).unwrap_err();
        assert_eq!(e.to_string(), format!("404 Client Error: Not Found for url: {}/missing", srv.base));
        let e = net.client.get_json(&format!("{}/boom", srv.base), t).unwrap_err();
        assert!(e.to_string().starts_with("503 Server Error: Service Unavailable for url: "), "{e}");
        assert!(matches!(net.client.get_json(&format!("{}/bad", srv.base), t), Err(NetError::Json(_))));
        assert!(srv.agents.lock().unwrap().iter().all(|a| a.starts_with("PatchOpsIII/")));
    }

    #[test]
    fn download_streams_verifies_and_reports_progress() {
        let payload: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let digest = crate::hash::sha256_hex(&payload);
        let mut routes = HashMap::new();
        routes.insert("/f.bin".to_string(), Route::bytes(payload.clone()));
        routes.insert("/page".to_string(), Route::ok("text/html; charset=utf-8", "<html></html>"));
        let srv = serve(routes);
        let net = Net::local(&srv.base);
        let dir = tmp("download");
        let dest = dir.join("out").join("f.bin");

        let mut seen = Vec::new();
        let opts = DownloadOptions::new(5).verify([digest.to_uppercase()]);
        let got = net.client.download(&format!("{}/f.bin", srv.base), &dest, &opts, &mut |t| seen.push(*t)).unwrap();
        assert_eq!(got, Downloaded { bytes: payload.len() as u64, sha256: digest.clone() });
        assert_eq!(std::fs::read(&dest).unwrap(), payload);
        assert_eq!(seen.first().unwrap().done, 0);
        assert_eq!(*seen.last().unwrap(), Transfer { done: payload.len() as u64, total: Some(payload.len() as u64) });
        assert_eq!(seen.last().unwrap().percent(), Some(100));
        assert!(seen.windows(2).all(|w| w[0].done <= w[1].done) && seen.len() > 2);

        // Wrong digest: error and no file left behind.
        let bad = DownloadOptions::new(5).verify(["0".repeat(64)]);
        let e = net.client.download(&format!("{}/f.bin", srv.base), &dest, &bad, &mut |_| {}).unwrap_err();
        assert_eq!(e.to_string(), "Downloaded file failed integrity verification (SHA-256 mismatch): f.bin");
        assert!(!dest.exists());

        // HTML rejected only when asked.
        let html = dir.join("page");
        let e = net.client.download(&format!("{}/page", srv.base), &html, &DownloadOptions::new(5).reject_html(), &mut |_| {}).unwrap_err();
        assert!(e.to_string().starts_with("Received HTML content from "), "{e}");
        assert!(!html.exists());
        net.client.download(&format!("{}/page", srv.base), &html, &DownloadOptions::new(5), &mut |_| {}).unwrap();

        // Size cap.
        let mut small = DownloadOptions::new(5);
        small.max_bytes = Some(1000);
        let e = net.client.download(&format!("{}/f.bin", srv.base), &dest, &small, &mut |_| {}).unwrap_err();
        assert_eq!(e, NetError::TooLarge { limit: 1000 });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn connection_refused_is_transport_error() {
        let srv = serve(HashMap::new());
        let base = srv.base.clone();
        // A port nothing listens on: bind and drop.
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let _ = base;
        let net = Net::local(&format!("http://127.0.0.1:{port}"));
        let e = net.client.get_json(&format!("http://127.0.0.1:{port}/x"), Duration::from_secs(2)).unwrap_err();
        assert!(matches!(e, NetError::Transport(_)), "{e:?}");
    }
}
