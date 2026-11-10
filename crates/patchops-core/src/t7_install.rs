//! `t7-install` machinery (legacy `backend/api.py::_install_t7_patch` and `t7_patch.py`
//! release-asset resolution, download, LPC install, Defender exclusions).
//!
//! Integrity: archives are only installed when their SHA-256 is known, either from the GitHub
//! release metadata (`digest: "sha256:..."`) or from a pinned trusted hash (compatible archive,
//! LPC). The current-build archive has no pin: without a GitHub digest the install is refused.
//! Archives are unpacked through [`crate::archive`] (zip-slip / size guards).

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::archive::{extract_archive, Limits};
use crate::backup;
use crate::context::Core;
use crate::net::{DownloadOptions, Net, ProgressFn};
use crate::ops::enhanced::exe_swap_state;
use crate::ops::t7::T7_PATCH_COMPATIBLE_ONLY_FILES;
use crate::pyfmt::io_error_text;
use crate::state::StateCx;

const CORE_REPO: &str = "Scroptss/T7Patch";
const CORE_TAG: &str = "latest";
const LEGACY_REPO: &str = "shiversoftdev/t7patch";
const LEGACY_TAG: &str = "Current";
const ARCHIVE_NAME: &str = "Linux.Steamdeck.and.Manual.Windows.Install.zip";
const LPC_NAME: &str = "LPC.1.zip";
const COMPATIBLE_TRUSTED: &str = "388491c01643b0abd51f13290d0c36dec9737fcfbb0ed5e2f5ef6804e1b73dcb";
const LPC_TRUSTED: &str = "c94855841a233c9dcdea2799c12693fed8554d0e59fe68257ae66ffbdf2fa58b";
const REQUIRED_FILES: [&str; 2] = ["t7patch.dll", "t7patchloader.dll"];

/// Legacy `T7PATCH_ASSETS` keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKey {
    Current,
    Compatible,
    Lpc,
}

struct AssetMeta {
    asset_name: &'static str,
    repo: &'static str,
    tag: &'static str,
    trusted: &'static [&'static str],
}

fn meta(key: AssetKey) -> AssetMeta {
    match key {
        AssetKey::Current => AssetMeta { asset_name: ARCHIVE_NAME, repo: CORE_REPO, tag: CORE_TAG, trusted: &[] },
        AssetKey::Compatible => AssetMeta { asset_name: ARCHIVE_NAME, repo: LEGACY_REPO, tag: LEGACY_TAG, trusted: &[COMPATIBLE_TRUSTED] },
        AssetKey::Lpc => AssetMeta { asset_name: LPC_NAME, repo: LEGACY_REPO, tag: LEGACY_TAG, trusted: &[LPC_TRUSTED] },
    }
}

fn release_api(net: &Net, repo: &str, tag: &str) -> String {
    if tag == "latest" {
        format!("{}/repos/{repo}/releases/latest", net.endpoints.github_api)
    } else {
        format!("{}/repos/{repo}/releases/tags/{tag}", net.endpoints.github_api)
    }
}

fn asset_url(net: &Net, repo: &str, tag: &str, asset: &str) -> String {
    if tag == "latest" {
        format!("{}/{repo}/releases/latest/download/{asset}", net.endpoints.github_web)
    } else {
        format!("{}/{repo}/releases/download/{tag}/{asset}", net.endpoints.github_web)
    }
}

#[derive(Debug, Clone, Default)]
struct ReleaseAsset {
    download_url: String,
    sha256: String,
}

/// Release metadata fetched at most once per install (legacy: process-wide cache).
#[derive(Default)]
pub struct ReleaseCache(BTreeMap<String, BTreeMap<String, ReleaseAsset>>);

/// Legacy `_fetch_t7patch_release_assets`: any failure yields an empty map (pinned hashes can
/// still carry the install).
fn fetch_release_assets<'c>(net: &Net, cache: &'c mut ReleaseCache, api: &str) -> &'c BTreeMap<String, ReleaseAsset> {
    cache.0.entry(api.to_string()).or_insert_with(|| {
        let mut out = BTreeMap::new();
        if let Ok(data) = net.client.get_json(api, Duration::from_secs(30)) {
            for asset in data.get("assets").and_then(Value::as_array).into_iter().flatten() {
                let s = |k: &str| asset.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
                let name = s("name");
                if name.is_empty() {
                    continue;
                }
                let mut info = ReleaseAsset { download_url: s("browser_download_url"), sha256: String::new() };
                if let Some(d) = crate::net::parse_sha256_digest(&s("digest")) {
                    info.sha256 = d;
                }
                out.insert(name, info);
            }
        }
        out
    })
}

/// Legacy `_resolve_t7patch_asset`: `(download_url, allowed sha256 set)`.
pub fn resolve_asset(core: &Core, net: &Net, cache: &mut ReleaseCache, key: AssetKey) -> (String, HashSet<String>) {
    let m = meta(key);
    let api = release_api(net, m.repo, m.tag);
    let assets = fetch_release_assets(net, cache, &api).clone();
    let mut asset_name = m.asset_name.to_string();
    let mut release_asset = assets.get(&asset_name).cloned().unwrap_or_default();
    let found = |a: &ReleaseAsset| !a.download_url.is_empty() || !a.sha256.is_empty();

    // Versioned names appended by upstream: `<stem>.<anything>.zip`.
    if !assets.contains_key(&asset_name) && asset_name.to_lowercase().ends_with(".zip") {
        let stem = asset_name[..asset_name.len() - 4].to_lowercase();
        let versioned: Vec<&String> = assets
            .keys()
            .filter(|n| {
                let l = n.to_lowercase();
                l.len() > stem.len() + 5
                    && l.starts_with(&format!("{stem}."))
                    && l.ends_with(".zip")
                    && !l[stem.len() + 1..l.len() - 4].contains('/')
            })
            .collect();
        if versioned.len() == 1 {
            asset_name = versioned[0].clone();
            release_asset = assets[&asset_name].clone();
        }
    }

    // Renamed packages in the latest Scroptss release: platform match, digest required.
    if !assets.contains_key(&asset_name) && !found(&release_asset) && key == AssetKey::Current {
        let zips: Vec<&String> = assets.iter().filter(|(n, a)| n.to_lowercase().ends_with(".zip") && !a.sha256.is_empty()).map(|(n, _)| n).collect();
        let has = |n: &str, t: &str| n.to_lowercase().contains(t);
        let mut candidates: Vec<&String> = if cfg!(windows) {
            zips.iter().copied().filter(|n| has(n, "windows")).collect()
        } else {
            zips.iter().copied().filter(|n| has(n, "linux") || has(n, "steamdeck")).collect()
        };
        if candidates.is_empty() {
            candidates = zips.iter().copied().filter(|n| !["windows", "linux", "steamdeck"].iter().any(|t| has(n, t))).collect();
        }
        let universal: Vec<&String> = candidates.iter().copied().filter(|n| has(n, "linux") && has(n, "windows")).collect();
        if !universal.is_empty() {
            candidates = universal;
        }
        if candidates.len() == 1 {
            asset_name = candidates[0].clone();
            release_asset = assets[&asset_name].clone();
            core.log.info(format!("Using latest T7 Patch release asset {asset_name}."));
        }
    }

    let download_url = if release_asset.download_url.is_empty() { asset_url(net, m.repo, m.tag, m.asset_name) } else { release_asset.download_url.clone() };
    if !release_asset.sha256.is_empty() {
        return (download_url, HashSet::from([release_asset.sha256.to_lowercase()]));
    }
    let trusted: HashSet<String> = m.trusted.iter().map(|s| s.to_lowercase()).collect();
    if !trusted.is_empty() {
        core.log.warning(format!("Release metadata digest unavailable for {asset_name}; using pinned trusted hash."));
    }
    (download_url, trusted)
}

/// Legacy `t7_patch.download_file` (with SHA-256 verification and log lines).
fn download_file(core: &Core, net: &Net, url: &str, dest: &Path, expected: &HashSet<String>, progress: ProgressFn<'_>) -> Result<(), String> {
    core.log.info(format!("Downloading from {url}"));
    let opts = DownloadOptions::new(60).verify(expected.iter().cloned());
    net.client.download(url, dest, &opts, progress).map_err(|e| e.to_string())?;
    core.log.success(format!("Downloaded file saved as: {}", dest.display()));
    Ok(())
}

// ---- LPC ------------------------------------------------------------------------------------

/// Legacy `backup_lpc_files`: rename every `LPC/*.ff` to `.patchops.bak` unless a backup exists.
pub fn backup_lpc_files(core: &Core, game: &Path) -> bool {
    let lpc = game.join("LPC");
    if !lpc.exists() {
        if let Err(e) = std::fs::create_dir_all(&lpc) {
            core.log.error(format!("Error during LPC backup process: {}", io_error_text(&e, Some(&lpc))));
            return false;
        }
        core.log.info("Created LPC directory.");
        return true;
    }
    let result = (|| -> Result<usize, String> {
        let mut n = 0;
        let rd = std::fs::read_dir(&lpc).map_err(|e| io_error_text(&e, Some(&lpc)))?;
        for e in rd {
            let e = e.map_err(|e| io_error_text(&e, Some(&lpc)))?;
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".ff") {
                continue;
            }
            let src = e.path();
            if backup::existing(&src).is_none() {
                let dst = backup::patchops_path(&src);
                if let Err(err) = std::fs::rename(&src, &dst) {
                    core.log.error(format!("Failed to backup {name}: {}", io_error_text(&err, None)));
                    return Err(String::new());
                }
                n += 1;
            }
        }
        Ok(n)
    })();
    match result {
        Ok(n) => {
            if n > 0 {
                core.log.success(format!("Created backups for {n} LPC files"));
            }
            true
        }
        Err(m) if m.is_empty() => false,
        Err(m) => {
            core.log.error(format!("Error during LPC backup process: {m}"));
            false
        }
    }
}

/// Legacy `install_lpc_files`.
fn install_lpc_files(core: &Core, net: &Net, cache: &mut ReleaseCache, game: &Path, progress: ProgressFn<'_>) -> bool {
    let (url, expected) = resolve_asset(core, net, cache, AssetKey::Lpc);
    let mod_files = &core.paths.mod_files;
    let zip_dest = mod_files.join("LPC.zip");
    let temp = mod_files.join("LPC_temp");
    let lpc = game.join("LPC");
    let _ = std::fs::create_dir_all(&lpc);
    let _ = std::fs::remove_dir_all(&temp);

    let ok = (|| -> Result<bool, String> {
        if expected.is_empty() {
            core.log.error("No trusted SHA-256 available for LPC.1.zip; aborting download.");
            return Ok(false);
        }
        download_file(core, net, &url, &zip_dest, &expected, progress)?;
        if !backup_lpc_files(core, game) {
            return Ok(false);
        }
        std::fs::create_dir_all(&temp).map_err(|e| io_error_text(&e, Some(&temp)))?;
        extract_archive(&zip_dest, &temp, Limits::default())?;
        let mut src = temp.join("LPC");
        if !src.exists() {
            src = temp.clone();
        }
        let rd = std::fs::read_dir(&src).map_err(|e| io_error_text(&e, Some(&src)))?;
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.ends_with(".ff") && e.path().is_file() {
                let dst = lpc.join(&name);
                crate::enhanced::files::copy2(&e.path(), &dst).map_err(|e| io_error_text(&e, Some(&dst)))?;
            }
        }
        core.log.success("Installed LPC files successfully.");
        Ok(true)
    })();
    let ok = match ok {
        Ok(v) => v,
        Err(e) => {
            core.log.error(format!("Error installing LPC files: {e}"));
            false
        }
    };
    let _ = std::fs::remove_file(&zip_dest);
    let _ = std::fs::remove_dir_all(&temp);
    ok
}

// ---- Windows Defender -----------------------------------------------------------------------

static DEFENDER_WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn powershell(script: &str) -> Option<String> {
    let out = crate::proc::command("powershell").args(["-Command", script]).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Legacy `add_defender_exclusion` (Windows only; elsewhere never called).
pub fn add_defender_exclusion(core: &Core, path: &Path) -> bool {
    let active = powershell("Get-MpComputerStatus").is_some_and(|o| o.contains("AntivirusEnabled") && o.contains("True"));
    if !active {
        if !DEFENDER_WARNED.swap(true, std::sync::atomic::Ordering::SeqCst) {
            core.log.warning("Windows Defender is not active or accessible.");
            core.log.warning("If you are using another anti-virus, then you will have to exclude the game folder manually.");
        }
        return false;
    }
    let p = path.display();
    if powershell(&format!("Add-MpPreference -ExclusionPath '{p}'")).is_some() {
        core.log.success(format!("Added Windows Defender exclusion to {p}."));
        true
    } else {
        core.log.warning(format!("Could not add Windows Defender exclusion for {p}. This is normal if using a different antivirus."));
        false
    }
}

// ---- install --------------------------------------------------------------------------------

fn find_source_dir(root: &Path) -> Option<PathBuf> {
    fn dirs(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                out.push(e.path());
                dirs(&e.path(), out);
            }
        }
    }
    let is_source = |p: &Path| -> bool {
        let Ok(rd) = std::fs::read_dir(p) else { return false };
        let names: HashSet<String> = rd.flatten().filter(|e| e.path().is_file()).map(|e| e.file_name().to_string_lossy().to_lowercase()).collect();
        REQUIRED_FILES.iter().all(|f| names.contains(*f))
    };
    let mut candidates = vec![root.to_path_buf()];
    dirs(root, &mut candidates);
    let depth = |p: &PathBuf| p.components().count();
    candidates.sort_by_key(|p| {
        let is_linux = p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("linux"));
        (u8::from(!is_linux), depth(p), p.to_string_lossy().to_lowercase())
    });
    candidates.into_iter().find(|c| is_source(c))
}

fn copy_tree(src: &Path, dst_root: &Path) -> Result<(), String> {
    fn walk(src: &Path, rel: &Path, dst_root: &Path) -> Result<(), String> {
        let dir = src.join(rel);
        let dest_dir = dst_root.join(rel);
        std::fs::create_dir_all(&dest_dir).map_err(|e| io_error_text(&e, Some(&dest_dir)))?;
        let mut entries: Vec<_> = std::fs::read_dir(&dir).map_err(|e| io_error_text(&e, Some(&dir)))?.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        let mut subdirs = Vec::new();
        for e in entries {
            let name = e.file_name();
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                subdirs.push(rel.join(&name));
                continue;
            }
            let dst = dest_dir.join(&name);
            if name.to_string_lossy().eq_ignore_ascii_case("t7patch.conf") && dst.exists() {
                continue;
            }
            crate::enhanced::files::copy2(&e.path(), &dst).map_err(|e| io_error_text(&e, Some(&dst)))?;
        }
        for s in subdirs {
            walk(src, &s, dst_root)?;
        }
        Ok(())
    }
    walk(src, Path::new(""), dst_root)
}

/// Legacy `_install_t7_patch`. `Err` is the legacy exception text.
pub fn install_t7_patch(core: &Core, net: &Net, game: &Path, progress: ProgressFn<'_>) -> Result<(), String> {
    if cfg!(windows) && !crate::elevate::is_admin() {
        return Err("T7 Patch install needs Windows administrator approval.".into());
    }
    let mod_files = &core.paths.mod_files;
    std::fs::create_dir_all(mod_files).map_err(|e| io_error_text(&e, Some(mod_files)))?;
    if cfg!(windows) {
        add_defender_exclusion(core, mod_files);
        add_defender_exclusion(core, game);
    }

    let cx = StateCx { core, game_dir: Some(game.to_string_lossy().into_owned()), config: String::new(), launch_options: None };
    let status = exe_swap_state(&cx);
    if !status.trusted_executable {
        return Err(format!("Cannot install T7 Patch for an unverified executable. {}", status.integrity_message));
    }
    let compatible = status.profile == "compatible";
    let key = if compatible { AssetKey::Compatible } else { AssetKey::Current };
    let patch_label = if compatible { "T7 Patch 2.04" } else { crate::ops::enhanced::T7_PATCH_LABEL_CURRENT };
    core.log.info(format!("Detected {}. Installing {patch_label}...", status.mode_label));

    let mut cache = ReleaseCache::default();
    let (url, expected) = resolve_asset(core, net, &mut cache, key);
    let zip_dest = mod_files.join("T7Patch.zip");
    let extract_dir = mod_files.join("T7Patch_extracted");
    if zip_dest.exists() {
        std::fs::remove_file(&zip_dest).map_err(|e| io_error_text(&e, Some(&zip_dest)))?;
    }
    for stale in [mod_files.join("linux"), mod_files.join("Linux.Steamdeck.and.Manual.Windows.Install"), extract_dir.clone()] {
        if stale.exists() {
            std::fs::remove_dir_all(&stale).map_err(|e| io_error_text(&e, Some(&stale)))?;
        }
    }
    if expected.is_empty() {
        return Err("No GitHub SHA-256 digest available for T7 Patch archive.".into());
    }
    download_file(core, net, &url, &zip_dest, &expected, progress)?;
    core.log.success("Downloaded T7 Patch successfully.");

    extract_archive(&zip_dest, &extract_dir, Limits::default())?;
    core.log.success("Extracted T7 Patch successfully.");

    let source = find_source_dir(&extract_dir).ok_or("T7 Patch archive did not contain the expected T7 Patch files.")?;
    if !compatible {
        for name in T7_PATCH_COMPATIBLE_ONLY_FILES {
            let stale = game.join(name);
            if stale.exists() {
                std::fs::remove_file(&stale).map_err(|e| io_error_text(&e, Some(&stale)))?;
            }
        }
    }
    copy_tree(&source, game)?;

    if !backup_lpc_files(core, game) {
        return Err("Failed to back up LPC files.".into());
    }
    if !install_lpc_files(core, net, &mut cache, game, progress) {
        return Err("Failed to install LPC files.".into());
    }
    core.log.success(format!("Installed {patch_label} successfully."));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::testserver::{serve, Route};
    use crate::net::no_progress;
    use crate::paths::Paths;
    use serde_json::json;
    use std::collections::HashMap;

    fn setup(name: &str, trusted: bool) -> (Core, PathBuf) {
        let root = std::env::temp_dir().join(format!("patchops-t7i-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let game = root.join("game");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::create_dir_all(root.join("home")).unwrap();
        std::fs::write(game.join("BlackOps3.exe"), b"not a known build").unwrap();
        if trusted {
            // Enhanced DLLs present => executable is trusted ("enhanced" profile) => current T7 patch.
            for f in crate::ops::enhanced::EXPECTED_ENHANCED_FILES {
                std::fs::write(game.join(f), b"x").unwrap();
            }
        }
        let core = Core::with_paths(Paths::with_app_data(root.join("home"), root.join("appdata")), "test".into());
        let g = game.to_string_lossy().into_owned();
        core.update_settings(|s| s.set_str("game_dir", g)).unwrap();
        (core, game)
    }

    fn release(base: &str, assets: &[(&str, &str, Option<String>)]) -> serde_json::Value {
        let list: Vec<_> = assets
            .iter()
            .map(|(name, path, digest)| {
                let mut a = json!({"name": name, "browser_download_url": format!("{base}{path}")});
                if let Some(d) = digest {
                    a["digest"] = json!(format!("sha256:{d}"));
                }
                a
            })
            .collect();
        json!({"tag_name": "x", "assets": list})
    }

    /// Server with both releases; `t7_digest` overrides the digest announced for the T7 zip.
    fn server(t7_zip: Vec<u8>, t7_digest: Option<String>, lpc_zip: Vec<u8>) -> crate::net::testserver::Server {
        let mut routes = HashMap::new();
        // The JSON needs the server's own address: reserve a port, then bind it.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let base = format!("http://{addr}");
        let t7_sha = crate::hash::sha256_hex(&t7_zip);
        let lpc_sha = crate::hash::sha256_hex(&lpc_zip);
        routes.insert(
            "/repos/Scroptss/T7Patch/releases/latest".to_string(),
            Route::json(&release(&base, &[(ARCHIVE_NAME, "/dl/t7.zip", t7_digest.or(Some(t7_sha)))])),
        );
        routes.insert(
            "/repos/shiversoftdev/t7patch/releases/tags/Current".to_string(),
            Route::json(&release(&base, &[(LPC_NAME, "/dl/lpc.zip", Some(lpc_sha))])),
        );
        routes.insert("/dl/t7.zip".to_string(), Route::bytes(t7_zip));
        routes.insert("/dl/lpc.zip".to_string(), Route::bytes(lpc_zip));
        crate::net::testserver::serve_on(addr, routes)
    }

    fn zips() -> (Vec<u8>, Vec<u8>) {
        let z = crate::enhanced::zip::testutil::stored_zip;
        (
            z(&[
                ("Linux.Steamdeck.and.Manual.Windows.Install/linux/t7patch.dll", b"patch"),
                ("Linux.Steamdeck.and.Manual.Windows.Install/linux/t7patchloader.dll", b"loader"),
                ("Linux.Steamdeck.and.Manual.Windows.Install/linux/t7patch.conf", b"playername=fresh\n"),
                ("Linux.Steamdeck.and.Manual.Windows.Install/linux/sub/extra.txt", b"extra"),
                ("Linux.Steamdeck.and.Manual.Windows.Install/readme.txt", b"readme"),
            ]),
            z(&[("LPC/new.ff", b"NEW"), ("LPC/ignored.txt", b"no")]),
        )
    }

    #[test]
    fn full_install_flow() {
        let (core, game) = setup("flow", true);
        std::fs::write(game.join("zbr2.dll"), b"old compat only").unwrap();
        std::fs::write(game.join("t7patch.conf"), b"playername=mine\n").unwrap();
        std::fs::create_dir_all(game.join("LPC")).unwrap();
        std::fs::write(game.join("LPC/old.ff"), b"OLD").unwrap();
        let (t7, lpc) = zips();
        let srv = server(t7, None, lpc);
        let net = Net::local(&srv.base);
        let mut p = 0u64;
        install_t7_patch(&core, &net, &game, &mut |t| p = p.max(t.done)).unwrap();
        assert!(p > 0);
        assert_eq!(std::fs::read(game.join("t7patch.dll")).unwrap(), b"patch");
        assert_eq!(std::fs::read(game.join("sub/extra.txt")).unwrap(), b"extra");
        assert!(!game.join("readme.txt").exists());
        assert!(!game.join("zbr2.dll").exists());
        assert_eq!(std::fs::read(game.join("t7patch.conf")).unwrap(), b"playername=mine\n");
        assert_eq!(std::fs::read(game.join("LPC/new.ff")).unwrap(), b"NEW");
        assert_eq!(std::fs::read(game.join("LPC/old.ff.patchops.bak")).unwrap(), b"OLD");
        assert!(!game.join("LPC/old.ff").exists() && !game.join("LPC/ignored.txt").exists());
        let msgs: Vec<String> = core.log.recent(40).into_iter().map(|e| e.message).collect();
        assert!(msgs[0].starts_with("Detected Enhanced. Installing T7 Patch Scroptss/T7Patch..."), "{msgs:?}");
        assert!(msgs.contains(&"Downloaded T7 Patch successfully.".to_string()));
        assert!(msgs.contains(&"Extracted T7 Patch successfully.".to_string()));
        assert!(msgs.contains(&"Created backups for 1 LPC files".to_string()));
        assert!(msgs.contains(&"Installed LPC files successfully.".to_string()));
        assert_eq!(msgs.last().unwrap(), "Installed T7 Patch Scroptss/T7Patch successfully.");
        // Cache files are cleaned up (LPC zip/temp) and the T7 zip is kept like the legacy code does.
        assert!(!core.paths.mod_files.join("LPC.zip").exists() && !core.paths.mod_files.join("LPC_temp").exists());
        assert!(srv.hits.lock().unwrap().iter().any(|h| h == "/dl/lpc.zip"));
        let _ = std::fs::remove_dir_all(game.parent().unwrap());
    }

    #[test]
    fn refuses_unverified_executable_missing_digest_and_bad_digest() {
        let (core, game) = setup("refuse", false);
        let (t7, lpc) = zips();
        let srv = server(t7.clone(), None, lpc.clone());
        let net = Net::local(&srv.base);
        let e = install_t7_patch(&core, &net, &game, &mut no_progress).unwrap_err();
        assert!(e.starts_with("Cannot install T7 Patch for an unverified executable. Active executable hash "), "{e}");

        let (core, game) = setup("nodigest", true);
        let srv = serve(HashMap::new()); // every API call 404s -> empty release map -> no digest
        let e = install_t7_patch(&core, &Net::local(&srv.base), &game, &mut no_progress).unwrap_err();
        assert_eq!(e, "No GitHub SHA-256 digest available for T7 Patch archive.");

        let srv = server(t7, Some("0".repeat(64)), lpc);
        let e = install_t7_patch(&core, &Net::local(&srv.base), &game, &mut no_progress).unwrap_err();
        assert_eq!(e, "Downloaded file failed integrity verification (SHA-256 mismatch): T7Patch.zip");
        assert!(!game.join("t7patch.dll").exists() && !core.paths.mod_files.join("T7Patch.zip").exists());
    }

    #[test]
    fn archive_without_patch_files_is_rejected_and_traversal_blocked() {
        let (core, game) = setup("badzip", true);
        let z = crate::enhanced::zip::testutil::stored_zip;
        let (_, lpc) = zips();
        let srv = server(z(&[("readme.txt", b"x")]), None, lpc.clone());
        let e = install_t7_patch(&core, &Net::local(&srv.base), &game, &mut no_progress).unwrap_err();
        assert_eq!(e, "T7 Patch archive did not contain the expected T7 Patch files.");
        let srv = server(z(&[("../../evil.dll", b"x"), ("t7patch.dll", b"a"), ("t7patchloader.dll", b"b")]), None, lpc);
        let e = install_t7_patch(&core, &Net::local(&srv.base), &game, &mut no_progress).unwrap_err();
        assert!(e.starts_with("Unsafe archive member path"), "{e}");
        assert!(!game.join("t7patch.dll").exists());
    }

    #[test]
    fn asset_resolution_prefers_digest_then_pins_and_discovers_renamed() {
        let (core, _) = setup("resolve", true);
        let mut routes = HashMap::new();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let base = format!("http://{addr}");
        let d = "a".repeat(64);
        routes.insert(
            "/repos/Scroptss/T7Patch/releases/latest".to_string(),
            Route::json(&release(
                &base,
                &[
                    ("T7Patch-Windows-1.2.zip", "/w.zip", Some("b".repeat(64))),
                    ("T7Patch-Linux-Steamdeck-1.2.zip", "/l.zip", Some(d.clone())),
                    ("T7Patch-Linux-nodigest.zip", "/n.zip", None),
                ],
            )),
        );
        routes.insert(
            "/repos/shiversoftdev/t7patch/releases/tags/Current".to_string(),
            Route::json(&release(&base, &[("LPC.1.v2.zip", "/lpc2.zip", None)])),
        );
        let _srv = crate::net::testserver::serve_on(addr, routes);
        let net = Net::local(&base);
        let mut cache = ReleaseCache::default();
        let (url, hashes) = resolve_asset(&core, &net, &mut cache, AssetKey::Current);
        if cfg!(windows) {
            assert_eq!(url, format!("{base}/w.zip"));
        } else {
            assert_eq!(url, format!("{base}/l.zip"));
            assert_eq!(hashes, HashSet::from([d]));
        }
        // LPC: versioned name, no digest => pinned hash + warning.
        let (url, hashes) = resolve_asset(&core, &net, &mut cache, AssetKey::Lpc);
        assert_eq!(url, format!("{base}/lpc2.zip"));
        assert_eq!(hashes, HashSet::from([LPC_TRUSTED.to_string()]));
        assert_eq!(core.log.recent(1)[0].message, "Release metadata digest unavailable for LPC.1.v2.zip; using pinned trusted hash.");
        // Unreachable API: fall back to the direct download URL; current has no pin.
        let dead = Net::local("http://127.0.0.1:9");
        let (url, hashes) = resolve_asset(&core, &dead, &mut ReleaseCache::default(), AssetKey::Current);
        assert_eq!(url, "http://127.0.0.1:9/Scroptss/T7Patch/releases/latest/download/Linux.Steamdeck.and.Manual.Windows.Install.zip");
        assert!(hashes.is_empty());
        let (url, hashes) = resolve_asset(&core, &dead, &mut ReleaseCache::default(), AssetKey::Compatible);
        assert!(url.ends_with("/shiversoftdev/t7patch/releases/download/Current/Linux.Steamdeck.and.Manual.Windows.Install.zip"));
        assert_eq!(hashes, HashSet::from([COMPATIBLE_TRUSTED.to_string()]));
    }
}
