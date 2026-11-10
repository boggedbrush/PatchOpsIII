//! Network half of `enhanced-install` (legacy `bo3_enhanced.py::fetch_latest_release` /
//! `download_latest_enhanced` and `utils.py::_prepare_enhanced_tool_cache` for the Linux GDK-Proton
//! tool). The local half (dump validation, file placement, Steam wiring) is in [`crate::enhanced`].
//!
//! Integrity: the BO3 Enhanced zip is only used when the GitHub release metadata carries a SHA-256
//! digest and the download matches it (the legacy behaviour); the GDK-Proton tarball is unpinned in
//! the legacy code and stays so (extraction is hardened instead, see [`crate::archive`]).

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use crate::archive::{extract_tar_gz, Limits};
use crate::context::Core;
use crate::enhanced::files::{validate_enhanced_archive, ENHANCED_ARCHIVE_NAME};
use crate::enhanced::linux::{write_compat_manifest, ENHANCED_TOOL_NAME};
use crate::log::Category;
use crate::net::{DownloadOptions, NetError, Net, ProgressFn};
use crate::pyfmt::io_error_text;

const CHECKSUMS_FILENAME: &str = "bo3_enhanced_checksums.json";
const ENHANCED_REPO_API: &str = "/repos/shiversoftdev/BO3Enhanced/releases/latest";
pub const PROTON_TAG: &str = "release10-32";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnhancedRelease {
    pub version: String,
    pub asset_url: String,
    pub asset_name: String,
    pub asset_sha256: Option<String>,
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).filter(|x| !x.is_empty()).map(str::to_string)
}

/// Legacy `fetch_latest_release` (failures are logged to the file only and give `None`).
pub fn fetch_latest_release(core: &Core, net: &Net) -> Option<EnhancedRelease> {
    let url = format!("{}{ENHANCED_REPO_API}", net.endpoints.github_api);
    let data = match net.client.get_json(&url, Duration::from_secs(30)) {
        Ok(d) => d,
        Err(e) => {
            core.log.log_file_only(Category::Warning, format!("Failed to fetch BO3 Enhanced release metadata: {e}"));
            return None;
        }
    };
    let assets = data.get("assets").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut selected: Option<&Value> = None;
    for asset in &assets {
        let name = asset.get("name").and_then(Value::as_str).unwrap_or("").to_lowercase();
        if name.ends_with(".zip") {
            selected = Some(asset);
            break;
        }
        if name.ends_with(".7z") && selected.is_none() {
            selected = Some(asset);
        }
    }
    let Some(asset) = selected else {
        core.log.log_file_only(Category::Warning, "No downloadable asset found for BO3 Enhanced latest release");
        return None;
    };
    let digest = asset.get("digest").and_then(Value::as_str).unwrap_or("").trim().to_string();
    let asset_sha256 = if digest.to_ascii_lowercase().starts_with("sha256:") {
        let c = digest[7..].trim().to_ascii_lowercase();
        (c.len() == 64 && c.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())).then_some(c)
    } else {
        None
    };
    Some(EnhancedRelease {
        version: s(&data, "tag_name").or_else(|| s(&data, "name")).unwrap_or_else(|| "0.0.0".into()),
        asset_url: s(asset, "browser_download_url").unwrap_or_default(),
        asset_name: s(asset, "name").unwrap_or_else(|| ENHANCED_ARCHIVE_NAME.into()),
        asset_sha256,
    })
}

fn save_checksum(core: &Core, storage_dir: &Path, file_name: &str, sha: &str) {
    let path = storage_dir.join(CHECKSUMS_FILENAME);
    let mut map: serde_json::Map<String, Value> =
        std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    map.insert(file_name.to_string(), json!(sha));
    let text = crate::pyjson::dumps_indent2(&Value::Object(map));
    let res = std::fs::create_dir_all(storage_dir).and_then(|_| std::fs::write(&path, text));
    if let Err(e) = res {
        core.log.log_file_only(Category::Warning, format!("Failed to save checksum cache: {e}"));
    }
}

/// Legacy `download_latest_enhanced`: `<mod_files>/BO3Enhanced_latest.zip` on success. Everything
/// that goes wrong is logged (file only, like the legacy `write_log(..., None)`) and gives `None`.
pub fn download_latest_enhanced(core: &Core, net: &Net, mod_files: &Path, storage_dir: &Path, progress: ProgressFn<'_>) -> Option<PathBuf> {
    let release = fetch_latest_release(core, net)?;
    if release.asset_url.is_empty() {
        return None;
    }
    let Some(expected) = release.asset_sha256.clone() else {
        core.log.log_file_only(
            Category::Error,
            "Latest BO3 Enhanced release metadata did not include a SHA-256 digest; refusing unverified download.",
        );
        return None;
    };
    let dest = mod_files.join(ENHANCED_ARCHIVE_NAME);
    let opts = DownloadOptions::new(30).verify([expected.clone()]).reject_html();
    match net.client.download(&release.asset_url, &dest, &opts, progress) {
        Ok(_) => {}
        Err(NetError::HashMismatch { .. }) => {
            core.log.log_file_only(Category::Error, "BO3 Enhanced download failed integrity verification (SHA-256 mismatch).");
            return None;
        }
        Err(e @ NetError::Html { .. }) => {
            core.log.log_file_only(Category::Error, e.to_string());
            return None;
        }
        Err(e @ (NetError::Status { .. } | NetError::Transport(_))) => {
            core.log.log_file_only(Category::Warning, format!("Download failed for {}: {e}", release.asset_url));
            return None;
        }
        Err(e) => {
            core.log.log_file_only(Category::Error, format!("Unexpected error downloading {}: {e}", release.asset_url));
            return None;
        }
    }
    if !validate_enhanced_archive(core, &dest) {
        return None;
    }
    save_checksum(core, storage_dir, ENHANCED_ARCHIVE_NAME, &expected);
    core.log.log_file_only(Category::Success, format!("Downloaded BO3 Enhanced {} to {}", release.version, dest.display()));
    Some(dest)
}

// ---- GDK-Proton tool (Linux) -------------------------------------------------------------------

pub fn proton_archive_url(net: &Net) -> String {
    format!("{}/Weather-OS/GDK-Proton/releases/download/{PROTON_TAG}/GDK-Proton10-32.tar.gz", net.endpoints.github_web)
}

/// Where a previously prepared / bundled tool lives: bundled next to the executable, else the cache.
pub fn existing_tool_source(core: &Core) -> Option<PathBuf> {
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.join("bo3-enhanced-proton").join(ENHANCED_TOOL_NAME)));
    let cached = core.paths.app_data.join("bo3-enhanced-proton-cache").join(ENHANCED_TOOL_NAME);
    [bundled, Some(cached)].into_iter().flatten().find(|p| p.is_dir())
}

/// Legacy `resolve_bo3_enhanced_tool_source`: a bundled/cached tool, else download + prepare the
/// GDK-Proton cache. Failures are logged (Error) and give `None`.
pub fn resolve_tool_source(core: &Core, net: &Net, progress: ProgressFn<'_>) -> Option<PathBuf> {
    if let Some(p) = existing_tool_source(core) {
        return Some(p);
    }
    prepare_tool_cache(core, net, progress)
}

/// Legacy `_prepare_enhanced_tool_cache`.
pub fn prepare_tool_cache(core: &Core, net: &Net, progress: ProgressFn<'_>) -> Option<PathBuf> {
    let cache_root = core.paths.app_data.join("bo3-enhanced-proton-cache");
    if let Err(e) = std::fs::create_dir_all(&cache_root) {
        core.log.error(format!("Failed to prepare BO3 Enhanced Proton cache: {}", io_error_text(&e, Some(&cache_root))));
        return None;
    }
    let target = cache_root.join(ENHANCED_TOOL_NAME);
    if target.is_dir() && target.join("compatibilitytool.vdf").is_file() {
        return Some(target);
    }
    let archive = cache_root.join(format!("GDK-Proton-{PROTON_TAG}.tar.gz"));
    if !archive.exists() {
        core.log.info(format!("Downloading BO3 Enhanced Proton ({PROTON_TAG})..."));
        let opts = DownloadOptions::new(120);
        if let Err(e) = net.client.download(&proton_archive_url(net), &archive, &opts, progress) {
            core.log.error(format!("Failed to download BO3 Enhanced Proton archive: {e}"));
            return None;
        }
    }
    let extract_root = cache_root.join(format!("extract-{PROTON_TAG}"));
    let result = (|| -> Result<PathBuf, String> {
        let _ = std::fs::remove_dir_all(&extract_root);
        std::fs::create_dir_all(&extract_root).map_err(|e| io_error_text(&e, Some(&extract_root)))?;
        extract_tar_gz(&archive, &extract_root, Limits::default())?;
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(&extract_root)
            .map_err(|e| io_error_text(&e, Some(&extract_root)))?
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| e.path())
            .collect();
        dirs.sort();
        let Some(source) = dirs.into_iter().next() else {
            core.log.error("Downloaded Proton archive did not contain a tool directory.");
            return Err(String::new());
        };
        let temp = cache_root.join(format!("{ENHANCED_TOOL_NAME}.tmp"));
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::rename(&source, &temp).map_err(|e| io_error_text(&e, Some(&source)))?;
        write_compat_manifest(&temp.join("compatibilitytool.vdf"), ENHANCED_TOOL_NAME)?;
        let _ = std::fs::remove_dir_all(&target);
        std::fs::rename(&temp, &target).map_err(|e| io_error_text(&e, Some(&target)))?;
        core.log.success(format!("Prepared cached BO3 Enhanced Proton at {}", target.display()));
        Ok(target.clone())
    })();
    let _ = std::fs::remove_dir_all(&extract_root);
    match result {
        Ok(p) => Some(p),
        Err(m) if m.is_empty() => None,
        Err(m) => {
            core.log.error(format!("Failed to prepare BO3 Enhanced Proton cache: {m}"));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::testutil::{tar_gz, TarMember};
    use crate::net::no_progress;
    use crate::net::testserver::{serve_on, Route};
    use crate::paths::Paths;
    use std::collections::HashMap;

    fn core(name: &str) -> (Core, PathBuf) {
        let root = std::env::temp_dir().join(format!("patchops-enet-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("home")).unwrap();
        let c = Core::with_paths(Paths::with_app_data(root.join("home"), root.join("appdata")), "test".into());
        std::fs::create_dir_all(&c.paths.mod_files).unwrap();
        (c, root)
    }

    fn addr() -> std::net::SocketAddr {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap()
    }

    fn enhanced_zip() -> Vec<u8> {
        crate::enhanced::zip::testutil::stored_zip(&[
            ("T7WSBootstrapper.dll", b"a"),
            ("T7InternalWS.dll", b"b"),
            ("steam_api65.dll", b"c"),
            ("WindowsCodecs.dll", b"d"),
        ])
    }

    fn release(base: &str, digest: Option<String>) -> Value {
        let mut a = json!({"name": "BO3Enhanced.zip", "browser_download_url": format!("{base}/dl/e.zip")});
        if let Some(d) = digest {
            a["digest"] = json!(format!("sha256:{d}"));
        }
        json!({"tag_name": "v9", "assets": [{"name": "notes.txt", "browser_download_url": "x"}, a]})
    }

    #[test]
    fn downloads_verifies_and_records_checksum() {
        let (core, root) = core("ok");
        let zip = enhanced_zip();
        let sha = crate::hash::sha256_hex(&zip);
        let a = addr();
        let base = format!("http://{a}");
        let mut routes = HashMap::new();
        routes.insert(ENHANCED_REPO_API.to_string(), Route::json(&release(&base, Some(sha.clone()))));
        routes.insert("/dl/e.zip".to_string(), Route::bytes(zip));
        let _srv = serve_on(a, routes);
        let net = Net::local(&base);
        let mut done = 0;
        let got = download_latest_enhanced(&core, &net, &core.paths.mod_files, &core.paths.app_data, &mut |t| done = t.done).unwrap();
        assert_eq!(got, core.paths.mod_files.join("BO3Enhanced_latest.zip"));
        assert!(done > 0);
        let sums: Value = serde_json::from_str(&std::fs::read_to_string(core.paths.app_data.join(CHECKSUMS_FILENAME)).unwrap()).unwrap();
        assert_eq!(sums["BO3Enhanced_latest.zip"], sha);
        assert!(core.log.read_file().contains("Downloaded BO3 Enhanced v9 to "));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn refuses_without_digest_with_bad_digest_html_or_bad_zip() {
        let (core, root) = core("refuse");
        let a = addr();
        let base = format!("http://{a}");
        let net = Net::local(&base);
        let mut routes = HashMap::new();
        routes.insert(ENHANCED_REPO_API.to_string(), Route::json(&release(&base, None)));
        routes.insert("/dl/e.zip".to_string(), Route::bytes(enhanced_zip()));
        let _srv = serve_on(a, routes);
        assert!(download_latest_enhanced(&core, &net, &core.paths.mod_files, &core.paths.app_data, &mut no_progress).is_none());
        assert!(core.log.read_file().contains("refusing unverified download"));
        assert!(!core.paths.mod_files.join("BO3Enhanced_latest.zip").exists());

        // wrong digest
        let a = addr();
        let base = format!("http://{a}");
        let mut routes = HashMap::new();
        routes.insert(ENHANCED_REPO_API.to_string(), Route::json(&release(&base, Some("1".repeat(64)))));
        routes.insert("/dl/e.zip".to_string(), Route::bytes(enhanced_zip()));
        let _s2 = serve_on(a, routes);
        assert!(download_latest_enhanced(&core, &Net::local(&base), &core.paths.mod_files, &core.paths.app_data, &mut no_progress).is_none());
        assert!(core.log.read_file().contains("BO3 Enhanced download failed integrity verification (SHA-256 mismatch)."));
        assert!(!core.paths.mod_files.join("BO3Enhanced_latest.zip").exists());

        // html page
        let a = addr();
        let base = format!("http://{a}");
        let mut routes = HashMap::new();
        routes.insert(ENHANCED_REPO_API.to_string(), Route::json(&release(&base, Some("1".repeat(64)))));
        routes.insert("/dl/e.zip".to_string(), Route::ok("text/html", "<html>"));
        let _s3 = serve_on(a, routes);
        assert!(download_latest_enhanced(&core, &Net::local(&base), &core.paths.mod_files, &core.paths.app_data, &mut no_progress).is_none());
        assert!(core.log.read_file().contains("expected binary download."));

        // valid digest but not an Enhanced archive
        let junk = crate::enhanced::zip::testutil::stored_zip(&[("a.txt", b"x")]);
        let a = addr();
        let base = format!("http://{a}");
        let mut routes = HashMap::new();
        routes.insert(ENHANCED_REPO_API.to_string(), Route::json(&release(&base, Some(crate::hash::sha256_hex(&junk)))));
        routes.insert("/dl/e.zip".to_string(), Route::bytes(junk));
        let _s4 = serve_on(a, routes);
        assert!(download_latest_enhanced(&core, &Net::local(&base), &core.paths.mod_files, &core.paths.app_data, &mut no_progress).is_none());
        assert!(core.log.read_file().contains("Enhanced archive missing expected files"));

        // API failure
        let dead = Net::local("http://127.0.0.1:9");
        assert!(fetch_latest_release(&core, &dead).is_none());
        assert!(core.log.read_file().contains("Failed to fetch BO3 Enhanced release metadata"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn prepares_proton_cache_from_tarball() {
        let (core, root) = core("proton");
        let tarball = tar_gz(&[
            TarMember::dir("GDK-Proton10-32/"),
            TarMember { mode: 0o755, ..TarMember::file("GDK-Proton10-32/proton", b"#!/usr/bin/env python3\n") },
            TarMember::file("GDK-Proton10-32/compatibilitytool.vdf", b"\"compatibilitytools\"\n{\n}\n"),
            TarMember::symlink("GDK-Proton10-32/current", "proton"),
        ]);
        let a = addr();
        let base = format!("http://{a}");
        let mut routes = HashMap::new();
        routes.insert("/Weather-OS/GDK-Proton/releases/download/release10-32/GDK-Proton10-32.tar.gz".to_string(), Route::bytes(tarball));
        let _srv = serve_on(a, routes);
        let net = Net::local(&base);
        let dir = resolve_tool_source(&core, &net, &mut no_progress).unwrap();
        assert!(dir.ends_with("bo3-enhanced-proton-cache/BO3 Enhanced") || dir.ends_with("bo3-enhanced-proton/BO3 Enhanced"));
        assert!(dir.join("proton").is_file() && dir.join("current").exists());
        let manifest = std::fs::read_to_string(dir.join("compatibilitytool.vdf")).unwrap();
        assert!(manifest.contains("BO3 Enhanced") && manifest.contains("install_path"), "{manifest}");
        let msgs: Vec<String> = core.log.recent(10).into_iter().map(|e| e.message).collect();
        assert_eq!(msgs[0], "Downloading BO3 Enhanced Proton (release10-32)...");
        assert!(msgs[1].starts_with("Prepared cached BO3 Enhanced Proton at "));
        // Second resolution hits the cache: no further download message.
        let before = core.log.recent(50).len();
        assert_eq!(resolve_tool_source(&core, &net, &mut no_progress).unwrap(), dir);
        assert_eq!(core.log.recent(50).len(), before);
        // Temp dirs gone.
        let names: Vec<_> = std::fs::read_dir(core.paths.app_data.join("bo3-enhanced-proton-cache")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert!(!names.iter().any(|n| n.starts_with("extract-") || n.ends_with(".tmp")), "{names:?}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn proton_download_failure_is_logged() {
        let (core, root) = core("proton-fail");
        let net = Net::local("http://127.0.0.1:9");
        assert!(resolve_tool_source(&core, &net, &mut no_progress).is_none());
        assert!(core.log.recent(5).iter().any(|e| e.message.starts_with("Failed to download BO3 Enhanced Proton archive: ")));
        let _ = std::fs::remove_dir_all(root);
    }
}
