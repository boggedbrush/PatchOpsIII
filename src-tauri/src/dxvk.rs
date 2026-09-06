use std::{
    collections::HashSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use serde::{Deserialize, Serialize};

use crate::{
    app::AppState,
    fs_ops::{self, remove_file_if_present, Snapshot},
    models::{DxvkSettings, DxvkState},
};

const RELEASE_API: &str =
    "https://gitlab.com/api/v4/projects/Ph42oN%2Fdxvk-gplasync/releases/v3.0-1";
const DXVK_FILES: [&str; 2] = ["dxgi.dll", "d3d11.dll"];
const MANAGED_FILES: [&str; 3] = ["dxgi.dll", "d3d11.dll", "dxvk.conf"];
const MANAGED_STATE_DIRECTORY: &str = "DXVK Managed";
const MANAGED_MANIFEST_FILENAME: &str = "manifest.json";
const MANAGED_BACKUP_DIRECTORY: &str = "originals";
const MANAGED_STATE_VERSION: u32 = 1;
const MAX_RELEASE_JSON_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ARCHIVE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const UNMANAGED_CONFIG_MESSAGE: &str = "DXVK is not managed by PatchOpsIII; install it before applying settings so existing files can be backed up safely.";
// Exact x64 member pairs from every official DXVK-GPLAsync release that the
// legacy application's dynamic "latest" installer could have installed since
// PatchOpsIII's first public beta. Pairing prevents mixed-release adoption.
const KNOWN_LEGACY_DXVK_PAIRS: [[&str; 2]; 8] = [
    // v2.5.2-1
    [
        "e534c7b9aadd90bf5744be7637abcccbeb66dd09ccf6b998e11b303bb14d1294",
        "a3bd2c6fdec65f3984550cd8dc852e1ac2bff9a30430bcba897cac78a2d9bcb3",
    ],
    // v2.5.3-1 (latest when PatchOpsIII's first public beta shipped)
    [
        "a5c3fa8b51559c77e9046a7c530b734eaf4a4d8a6f3d66edaa6375264bd3f4e9",
        "c373241f40f465f762e15a3bf53c236585bfae755e6d8eaadad091c8bcd15c55",
    ],
    // v2.6-1
    [
        "d159fc62a0f8f47e8cfafd8b87a479c71ab907ba91fb597e640b997b46a0b237",
        "e628c11af143ee75870c7758425dd2fe0039f898362176bef8cc23d6fee25be9",
    ],
    // v2.6.1-1
    [
        "7db4863ee96ee5358a64b70358c124c3c5228fa25f619c121dd9a38b48a2db71",
        "8cd8cda2d5923c91c9cb095e22490f01cfd098254b1b261c1f8c5a916985dfd8",
    ],
    // v2.6.2-1
    [
        "d4811fca51d8e2517f63c680b724a9d566d163fad94c858d2029c6ab466d4163",
        "225b611cd14b6291ddd46d598b05f0362263869b1ef2252c1b637d43565c3b9d",
    ],
    // v2.7-1
    [
        "5e291e1b41a2a5ec0727a629fab7b43b8ebc9a269f58439ea8eed937925084f9",
        "e9b3a452bc623c39b025e06e248276a9b8bcd87eb7eee7de55e8d5c5bf822522",
    ],
    // v2.7.1-1
    [
        "8d5ae3c40962a4846e4dae9d373a8660227e85f051d12e5bb882ef92d136dfca",
        "7f20637b7a9527fbed53985fa55c5fc56c7d8bd132589219e92bd49f488c2754",
    ],
    // v3.0-1
    [
        "177cea0f3d64ac7a2834e24637aecb4ab133e036c50c2568489079cefb8fd7ec",
        "c9e9d1a7844077df38cd0e540be1b435db7c9fed878a60fc32d22c778345fc09",
    ],
];
static STAGE_COUNTER: AtomicU64 = AtomicU64::new(0);

// App-owned trust pin for the public GitLab artifact produced by release v3.0-1.
// Provenance: GitLab job 15069375988, 18,365,240 bytes; the downloaded artifact's
// MD5 also matched GitLab's x-goog-hash/ETag value ae79003c73e0e36c7bd1ea985d77478f.
const RELEASE_PINS: [ReleasePin; 1] = [ReleasePin {
    tag_name: "v3.0-1",
    asset_name: "dxvk-gplasync-v3.0-1.zip",
    url: "https://gitlab.com/Ph42oN/dxvk-gplasync/-/jobs/15069375988/artifacts/download?file_type=archive",
    sha256: "83af6b77a080373ae00718239491e815f2bdc3145e074e8198ed7200cb296f7b",
}];

#[derive(Debug, Deserialize)]
struct Release {
    #[serde(default)]
    tag_name: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    assets: ReleaseAssets,
}

#[derive(Debug, Default, Deserialize)]
struct ReleaseAssets {
    #[serde(default)]
    links: Vec<ReleaseLink>,
}

#[derive(Debug, Deserialize)]
struct ReleaseLink {
    #[serde(default)]
    name: String,
    #[serde(default)]
    url: String,
}

#[derive(Clone, Copy, Debug)]
struct ReleasePin {
    tag_name: &'static str,
    asset_name: &'static str,
    url: &'static str,
    sha256: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArchiveKind {
    Zip,
    TarGz,
}

#[derive(Debug, PartialEq, Eq)]
struct ReleaseAsset {
    url: String,
    filename: String,
    kind: ArchiveKind,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct ManagedManifest {
    version: u32,
    game_dir: PathBuf,
    files: Vec<ManagedFile>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct ManagedFile {
    name: String,
    installed_sha256: String,
    original_sha256: Option<String>,
}

/// Read DXVK installation and configuration state. Invalid or stale game paths
/// are reported as an uninstalled default state.
pub fn status(game_dir: Option<&Path>) -> DxvkState {
    let Some(game_dir) = game_dir.filter(|path| !path.as_os_str().is_empty() && path.is_dir())
    else {
        return DxvkState::default();
    };

    let conf = game_dir.join("dxvk.conf");
    let conf_exists = regular_file(&conf);
    let settings = if conf_exists {
        fs::read(&conf)
            .map(|body| parse_conf(&String::from_utf8_lossy(&body)))
            .unwrap_or_default()
    } else {
        DxvkSettings::default()
    };

    DxvkState {
        installed: DXVK_FILES
            .iter()
            .all(|filename| regular_file(&game_dir.join(filename))),
        conf_exists,
        settings,
    }
}

/// Validate and atomically replace `dxvk.conf` using the renderer's existing
/// settings model.
pub fn configure(state: &AppState, game_dir: &Path, settings: &DxvkSettings) -> Result<(), String> {
    validate_settings(settings)?;
    let game_dir = validate_game_dir(game_dir)?;
    configure_managed(
        state.data_dir(),
        &game_dir,
        settings,
        &state.mod_files_dir(),
    )?;
    state.log("Success", "Updated dxvk.conf from DXVK settings.");
    Ok(())
}

fn configure_managed(
    storage_dir: &Path,
    game_dir: &Path,
    settings: &DxvkSettings,
    staging_root: &Path,
) -> Result<(), String> {
    let conf = flat_target(game_dir, "dxvk.conf")?;
    let mut manifest = managed_manifest_for_configure(storage_dir, game_dir)?;
    verify_managed_targets(&manifest, game_dir, storage_dir)?;
    let stage = create_private_dir(staging_root, "dxvk-configure")?;
    let snapshot = Snapshot::capture(std::slice::from_ref(&conf), &stage.join("rollback"))?;
    let result = (|| {
        fs_ops::atomic_write(&conf, build_conf(settings, true).as_bytes())?;
        update_installed_hashes(&mut manifest, game_dir)?;
        save_managed_manifest(storage_dir, &manifest)
    })();
    if let Err(error) = result {
        return match snapshot.restore() {
            Ok(()) => {
                let _ = fs::remove_dir_all(&stage);
                Err(format!("{error}; previous DXVK configuration was restored"))
            }
            Err(rollback) => Err(format!(
                "{error}; rollback failed: {rollback}. Recovery files remain in {}",
                stage.display()
            )),
        };
    }
    let _ = fs::remove_dir_all(&stage);
    Ok(())
}

/// Download the pinned DXVK-GPLAsync release, validate it in an isolated
/// staging directory, and commit the two required DLLs plus `dxvk.conf` with
/// rollback on failure.
pub fn install(state: &AppState, game_dir: &Path, settings: &DxvkSettings) -> Result<(), String> {
    validate_settings(settings)?;
    let game_dir = validate_game_dir(game_dir)?;
    let stage = create_stage(state, "dxvk")?;

    let result = (|| {
        state.log("Info", "Downloading DXVK-GPLAsync...");
        let release = fetch_pinned_release()?;
        let asset = select_release_asset(&release)?;
        let archive = stage.join(&asset.filename);
        fs_ops::download(&asset.url, &archive, Some(&asset.sha256), MAX_ARCHIVE_BYTES)?;
        state.log("Success", "Downloaded DXVK-GPLAsync successfully.");

        let extracted = stage.join("extracted");
        match asset.kind {
            ArchiveKind::Zip => fs_ops::extract_zip(&archive, &extracted)?,
            ArchiveKind::TarGz => fs_ops::extract_tar_gz(&archive, &extracted)?,
        }
        let sources = find_dxvk_files(&extracted)?;
        state.log(
            "Success",
            "Extracted and validated DXVK-GPLAsync successfully.",
        );

        apply_managed_install(
            state.data_dir(),
            &game_dir,
            &sources,
            settings,
            supports_gpl_async_cache(&release),
            &stage.join("rollback"),
        )?;

        state.log("Success", "DXVK-GPLAsync installed successfully.");
        Ok(())
    })();

    if result.is_ok() {
        let _ = fs::remove_dir_all(&stage);
    } else {
        state.log(
            "Warning",
            format!("DXVK staging files were retained at {}.", stage.display()),
        );
    }
    result
}

/// Remove PatchOpsIII-managed DXVK files with rollback if any removal fails.
pub fn uninstall(state: &AppState, game_dir: &Path) -> Result<(), String> {
    let game_dir = validate_game_dir(game_dir)?;
    let known = known_legacy_pairs();
    adopt_legacy_install(state.data_dir(), &game_dir, &known)?;
    match uninstall_managed_files(state.data_dir(), &game_dir) {
        Ok(true) => {
            state.log("Success", "DXVK-GPLAsync has been uninstalled.");
            Ok(())
        }
        Ok(false) => {
            state.log("Info", "DXVK-GPLAsync was not installed.");
            Ok(())
        }
        Err(error) => {
            state.log(
                "Warning",
                format!(
                    "DXVK recovery state was retained at {}.",
                    managed_state_dir(state.data_dir()).display()
                ),
            );
            Err(error)
        }
    }
}

fn fetch_pinned_release() -> Result<Release, String> {
    let response = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| error.to_string())?
        .get(RELEASE_API)
        .header(reqwest::header::USER_AGENT, "PatchOpsIII")
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| format!("failed to query DXVK releases: {error}"))?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RELEASE_JSON_BYTES)
    {
        return Err("DXVK release metadata is unexpectedly large".into());
    }
    let mut body = Vec::new();
    response
        .take(MAX_RELEASE_JSON_BYTES + 1)
        .read_to_end(&mut body)
        .map_err(|error| error.to_string())?;
    if body.len() as u64 > MAX_RELEASE_JSON_BYTES {
        return Err("DXVK release metadata is unexpectedly large".into());
    }
    serde_json::from_slice::<Release>(&body)
        .map_err(|error| format!("invalid DXVK release metadata: {error}"))
}

fn select_release_asset(release: &Release) -> Result<ReleaseAsset, String> {
    select_release_asset_with_pins(release, &RELEASE_PINS)
}

fn select_release_asset_with_pins(
    release: &Release,
    pins: &[ReleasePin],
) -> Result<ReleaseAsset, String> {
    let pin = pins
        .iter()
        .find(|pin| release.tag_name.trim() == pin.tag_name)
        .ok_or_else(|| {
            format!(
                "DXVK-GPLAsync release {} is not supported by this PatchOpsIII build",
                release.tag_name.trim()
            )
        })?;
    let sha256 = normalize_sha256(pin.sha256).ok_or_else(|| {
        format!(
            "DXVK-GPLAsync release {} does not have a valid app-owned SHA-256 pin",
            pin.tag_name
        )
    })?;
    validate_download_url(pin.url)?;
    let kind = archive_kind(pin.asset_name).ok_or_else(|| {
        format!(
            "DXVK-GPLAsync release {} has an unsupported pinned archive type",
            pin.tag_name
        )
    })?;
    let link = release
        .assets
        .links
        .iter()
        .find(|link| link.name.trim() == pin.asset_name && link.url.trim() == pin.url)
        .ok_or_else(|| {
            format!(
                "DXVK-GPLAsync release {} did not contain its exact pinned asset",
                pin.tag_name
            )
        })?;
    Ok(ReleaseAsset {
        url: link.url.trim().to_owned(),
        filename: archive_filename(&link.name, &link.url, kind),
        kind,
        sha256,
    })
}

fn archive_kind(value: &str) -> Option<ArchiveKind> {
    let value = value
        .split(['?', '#'])
        .next()
        .unwrap_or(value)
        .to_ascii_lowercase();
    if value.ends_with(".zip") {
        Some(ArchiveKind::Zip)
    } else if value.ends_with(".tar.gz") || value.ends_with(".tgz") {
        Some(ArchiveKind::TarGz)
    } else {
        None
    }
}

fn archive_filename(name: &str, url: &str, kind: ArchiveKind) -> String {
    let from_url = reqwest::Url::parse(url)
        .ok()
        .and_then(|url| url.path_segments()?.next_back().map(str::to_string));
    [Some(name.trim().to_string()), from_url]
        .into_iter()
        .flatten()
        .find(|value| {
            !value.is_empty()
                && value.len() <= 200
                && !value.contains(['/', '\\'])
                && archive_kind(value) == Some(kind)
        })
        .unwrap_or_else(|| match kind {
            ArchiveKind::Zip => "dxvk-gplasync.zip".into(),
            ArchiveKind::TarGz => "dxvk-gplasync.tar.gz".into(),
        })
}

fn validate_download_url(value: &str) -> Result<(), String> {
    let url = reqwest::Url::parse(value).map_err(|_| "invalid DXVK download URL".to_string())?;
    if url.scheme() != "https"
        || url.host_str() != Some("gitlab.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || url.fragment().is_some()
        || url.path() != "/Ph42oN/dxvk-gplasync/-/jobs/15069375988/artifacts/download"
        || url.query() != Some("file_type=archive")
    {
        return Err("DXVK release metadata returned an unexpected download URL".into());
    }
    Ok(())
}

fn normalize_sha256(value: &str) -> Option<String> {
    let value = value.trim();
    let value = value
        .split_once(':')
        .filter(|(prefix, _)| prefix.eq_ignore_ascii_case("sha256"))
        .map_or(value, |(_, digest)| digest)
        .trim()
        .to_ascii_lowercase();
    (value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())).then_some(value)
}

fn managed_state_dir(storage_dir: &Path) -> PathBuf {
    storage_dir.join(MANAGED_STATE_DIRECTORY)
}

fn managed_manifest_path(storage_dir: &Path) -> PathBuf {
    managed_state_dir(storage_dir).join(MANAGED_MANIFEST_FILENAME)
}

fn managed_backup_path(state_dir: &Path, name: &str) -> Result<PathBuf, String> {
    if !MANAGED_FILES.contains(&name) {
        return Err("DXVK managed state contains an invalid filename".into());
    }
    Ok(state_dir.join(MANAGED_BACKUP_DIRECTORY).join(name))
}

fn managed_targets(game_dir: &Path) -> Result<Vec<PathBuf>, String> {
    MANAGED_FILES
        .iter()
        .map(|name| flat_target(game_dir, name))
        .collect()
}

fn load_managed_manifest(
    storage_dir: &Path,
    game_dir: &Path,
) -> Result<Option<ManagedManifest>, String> {
    let state_dir = managed_state_dir(storage_dir);
    match fs::symlink_metadata(&state_dir) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(format!(
                "refusing unsafe DXVK managed state at {}",
                state_dir.display()
            ));
        }
        Ok(_) => {}
    }
    let path = managed_manifest_path(storage_dir);
    if !regular_file(&path) {
        return Err(format!(
            "DXVK managed state is incomplete; recovery files were retained at {}",
            state_dir.display()
        ));
    }
    let body = fs::read(&path).map_err(|error| error.to_string())?;
    if body.len() > 64 * 1024 {
        return Err("DXVK managed manifest is unexpectedly large".into());
    }
    let manifest: ManagedManifest = serde_json::from_slice(&body)
        .map_err(|error| format!("invalid DXVK managed manifest: {error}"))?;
    validate_managed_manifest(&manifest, game_dir, &state_dir)?;
    Ok(Some(manifest))
}

fn managed_manifest_for_configure(
    storage_dir: &Path,
    game_dir: &Path,
) -> Result<ManagedManifest, String> {
    load_managed_manifest(storage_dir, game_dir)?
        .ok_or_else(|| UNMANAGED_CONFIG_MESSAGE.to_string())
}

fn validate_managed_manifest(
    manifest: &ManagedManifest,
    game_dir: &Path,
    state_dir: &Path,
) -> Result<(), String> {
    if manifest.version != MANAGED_STATE_VERSION {
        return Err("unsupported DXVK managed manifest version".into());
    }
    if manifest.game_dir != game_dir {
        return Err(format!(
            "DXVK is managed for {}, not {}",
            manifest.game_dir.display(),
            game_dir.display()
        ));
    }
    let mut names = HashSet::new();
    for file in &manifest.files {
        if !MANAGED_FILES.contains(&file.name.as_str()) || !names.insert(file.name.as_str()) {
            return Err("DXVK managed manifest contains invalid or duplicate files".into());
        }
        if normalize_sha256(&file.installed_sha256).is_none() {
            return Err("DXVK managed manifest contains an invalid installed checksum".into());
        }
        let backup = managed_backup_path(state_dir, &file.name)?;
        match &file.original_sha256 {
            Some(expected) => {
                if normalize_sha256(expected).is_none()
                    || !regular_file(&backup)
                    || !fs_ops::sha256_file(&backup)?.eq_ignore_ascii_case(expected)
                {
                    return Err(format!(
                        "DXVK original backup {} failed validation",
                        backup.display()
                    ));
                }
            }
            None if backup.exists() => {
                return Err(format!(
                    "DXVK managed state contains an unexpected backup at {}",
                    backup.display()
                ));
            }
            None => {}
        }
    }
    if names.len() != MANAGED_FILES.len() {
        return Err("DXVK managed manifest is incomplete".into());
    }
    Ok(())
}

fn verify_managed_targets(
    manifest: &ManagedManifest,
    game_dir: &Path,
    storage_dir: &Path,
) -> Result<(), String> {
    validate_managed_manifest(manifest, game_dir, &managed_state_dir(storage_dir))?;
    let mut changed = Vec::new();
    for file in &manifest.files {
        let target = flat_target(game_dir, &file.name)?;
        let matches = regular_file(&target)
            && fs_ops::sha256_file(&target)
                .map(|actual| actual.eq_ignore_ascii_case(&file.installed_sha256))
                .unwrap_or(false);
        if !matches {
            changed.push(file.name.clone());
        }
    }
    if changed.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "DXVK files changed after PatchOpsIII installed them ({}); refusing to overwrite them. Recovery state remains at {}",
            changed.join(", "),
            managed_state_dir(storage_dir).display()
        ))
    }
}

fn known_legacy_pairs() -> Vec<[String; 2]> {
    KNOWN_LEGACY_DXVK_PAIRS
        .iter()
        .map(|pair| [pair[0].to_owned(), pair[1].to_owned()])
        .collect()
}

fn source_pair(sources: &[PathBuf; 2]) -> Result<[String; 2], String> {
    Ok([
        fs_ops::sha256_file(&sources[0])?,
        fs_ops::sha256_file(&sources[1])?,
    ])
}

/// Adopt only an exact origin/main DXVK payload. The DLL pair proves the
/// legacy release; the user-editable config is backed up as an original so an
/// uninstall restores its current bytes instead of deleting them.
fn adopt_legacy_install(
    storage_dir: &Path,
    game_dir: &Path,
    expected_pairs: &[[String; 2]],
) -> Result<bool, String> {
    if load_managed_manifest(storage_dir, game_dir)?.is_some() {
        return Ok(false);
    }
    let targets = managed_targets(game_dir)?;
    let dlls = &targets[..DXVK_FILES.len()];
    if dlls.iter().all(|target| !target.exists()) {
        return Ok(false);
    }
    if dlls.iter().any(|target| !regular_file(target)) {
        return Err(
            "The unmanaged DXVK DLL pair is incomplete or unsafe; no files were changed".into(),
        );
    }
    let actual = [
        fs_ops::sha256_file(&dlls[0])?,
        fs_ops::sha256_file(&dlls[1])?,
    ];
    if !expected_pairs.iter().any(|pair| pair == &actual) {
        return Err("The unmanaged DXVK DLLs do not match the same verified release known to this build; no files were changed. Remove or restore the unknown installation manually before installing PatchOpsIII-managed DXVK.".into());
    }
    let config = &targets[2];
    if !regular_file(config) {
        return Err("The unmanaged DXVK config is missing or unsafe; no files were changed".into());
    }

    let state_dir = managed_state_dir(storage_dir);
    fs::create_dir(&state_dir).map_err(|error| error.to_string())?;
    let result = (|| {
        let backup_dir = state_dir.join(MANAGED_BACKUP_DIRECTORY);
        fs::create_dir(&backup_dir).map_err(|error| error.to_string())?;
        let config_hash = fs_ops::sha256_file(config)?;
        let config_backup = managed_backup_path(&state_dir, "dxvk.conf")?;
        fs::copy(config, &config_backup).map_err(|error| error.to_string())?;
        if !fs_ops::sha256_file(&config_backup)?.eq_ignore_ascii_case(&config_hash) {
            return Err("DXVK legacy config backup failed verification".into());
        }
        let manifest = ManagedManifest {
            version: MANAGED_STATE_VERSION,
            game_dir: game_dir.to_path_buf(),
            files: vec![
                ManagedFile {
                    name: "dxgi.dll".into(),
                    installed_sha256: actual[0].clone(),
                    original_sha256: None,
                },
                ManagedFile {
                    name: "d3d11.dll".into(),
                    installed_sha256: actual[1].clone(),
                    original_sha256: None,
                },
                ManagedFile {
                    name: "dxvk.conf".into(),
                    installed_sha256: config_hash.clone(),
                    original_sha256: Some(config_hash),
                },
            ],
        };
        save_managed_manifest(storage_dir, &manifest)
    })();
    if let Err(error) = result {
        let _ = remove_new_managed_state(storage_dir);
        return Err(error);
    }
    Ok(true)
}

fn prepare_managed_install(
    storage_dir: &Path,
    game_dir: &Path,
    targets: &[PathBuf],
) -> Result<(ManagedManifest, bool), String> {
    if let Some(manifest) = load_managed_manifest(storage_dir, game_dir)? {
        verify_managed_targets(&manifest, game_dir, storage_dir)?;
        return Ok((manifest, false));
    }
    if targets.len() != MANAGED_FILES.len() {
        return Err("DXVK managed target list is incomplete".into());
    }

    let state_dir = managed_state_dir(storage_dir);
    fs::create_dir(&state_dir).map_err(|error| error.to_string())?;
    let result = (|| {
        let backup_dir = state_dir.join(MANAGED_BACKUP_DIRECTORY);
        fs::create_dir(&backup_dir).map_err(|error| error.to_string())?;
        let mut files = Vec::with_capacity(MANAGED_FILES.len());
        for (name, target) in MANAGED_FILES.iter().zip(targets) {
            let original_sha256 = if target.exists() {
                if !regular_file(target) {
                    return Err(format!("refusing to back up {}", target.display()));
                }
                let expected = fs_ops::sha256_file(target)?;
                let backup = managed_backup_path(&state_dir, name)?;
                fs::copy(target, &backup).map_err(|error| error.to_string())?;
                if fs_ops::sha256_file(&backup)? != expected {
                    return Err(format!(
                        "DXVK original backup {} failed verification",
                        backup.display()
                    ));
                }
                Some(expected)
            } else {
                None
            };
            files.push(ManagedFile {
                name: (*name).to_owned(),
                installed_sha256: String::new(),
                original_sha256,
            });
        }
        Ok(ManagedManifest {
            version: MANAGED_STATE_VERSION,
            game_dir: game_dir.to_path_buf(),
            files,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&state_dir);
    }
    result.map(|manifest| (manifest, true))
}

fn apply_managed_install(
    storage_dir: &Path,
    game_dir: &Path,
    sources: &[PathBuf; 2],
    settings: &DxvkSettings,
    include_gpl_async_cache: bool,
    rollback_directory: &Path,
) -> Result<(), String> {
    let targets = managed_targets(game_dir)?;
    let snapshot = Snapshot::capture(&targets, rollback_directory)?;
    let mut expected_pairs = vec![source_pair(sources)?];
    expected_pairs.extend(known_legacy_pairs());
    let adopted = adopt_legacy_install(storage_dir, game_dir, &expected_pairs)?;
    let (mut manifest, new_managed_state) =
        match prepare_managed_install(storage_dir, game_dir, &targets) {
            Ok(prepared) => prepared,
            Err(error) => {
                if adopted {
                    remove_new_managed_state(storage_dir).map_err(|cleanup| {
                        format!("{error}; legacy ownership migration cleanup failed: {cleanup}")
                    })?;
                }
                return Err(error);
            }
        };
    let commit = (|| {
        for (source, target) in sources.iter().zip(targets.iter()) {
            fs::copy(source, target).map_err(|error| {
                format!(
                    "failed to install {} to {}: {error}",
                    source.display(),
                    target.display()
                )
            })?;
        }
        fs_ops::atomic_write(
            &targets[2],
            build_conf(settings, include_gpl_async_cache).as_bytes(),
        )?;
        if !targets.iter().all(|path| regular_file(path)) {
            return Err("DXVK install verification failed".to_string());
        }
        update_installed_hashes(&mut manifest, game_dir)?;
        save_managed_manifest(storage_dir, &manifest)?;
        Ok(())
    })();

    if let Err(error) = commit {
        return match snapshot.restore() {
            Ok(()) => {
                if new_managed_state || adopted {
                    remove_new_managed_state(storage_dir).map_err(|cleanup| {
                        format!(
                            "{error}; previous DXVK files were restored, but recovery-state cleanup failed: {cleanup}"
                        )
                    })?;
                }
                Err(format!("{error}; previous DXVK files were restored"))
            }
            Err(rollback) => Err(format!(
                "{error}; rollback failed: {rollback}. Recovery files remain in {}",
                rollback_directory.display()
            )),
        };
    }
    Ok(())
}

fn update_installed_hashes(manifest: &mut ManagedManifest, game_dir: &Path) -> Result<(), String> {
    for file in &mut manifest.files {
        let target = flat_target(game_dir, &file.name)?;
        if !regular_file(&target) {
            return Err(format!(
                "DXVK install verification failed for {}",
                target.display()
            ));
        }
        file.installed_sha256 = fs_ops::sha256_file(&target)?;
    }
    Ok(())
}

fn save_managed_manifest(storage_dir: &Path, manifest: &ManagedManifest) -> Result<(), String> {
    validate_managed_manifest(
        manifest,
        &manifest.game_dir,
        &managed_state_dir(storage_dir),
    )?;
    let body = serde_json::to_vec_pretty(manifest).map_err(|error| error.to_string())?;
    fs_ops::atomic_write(&managed_manifest_path(storage_dir), &body)
}

fn remove_new_managed_state(storage_dir: &Path) -> Result<(), String> {
    let state_dir = managed_state_dir(storage_dir);
    let metadata = fs::symlink_metadata(&state_dir).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "refusing unsafe DXVK managed state at {}",
            state_dir.display()
        ));
    }
    fs::remove_dir_all(state_dir).map_err(|error| error.to_string())
}

fn uninstall_managed_files(storage_dir: &Path, game_dir: &Path) -> Result<bool, String> {
    let targets = managed_targets(game_dir)?;
    let Some(manifest) = load_managed_manifest(storage_dir, game_dir)? else {
        if targets.iter().any(|target| target.exists()) {
            return Err(
                "No PatchOpsIII-managed DXVK record exists; refusing to remove unowned files"
                    .into(),
            );
        }
        return Ok(false);
    };
    verify_managed_targets(&manifest, game_dir, storage_dir)?;

    let transaction = create_private_dir(storage_dir, "dxvk-uninstall")?;
    let snapshot = match Snapshot::capture(&targets, &transaction.join("rollback")) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            let _ = fs::remove_dir_all(&transaction);
            return Err(error);
        }
    };
    let state_dir = managed_state_dir(storage_dir);
    let moved_state = transaction.join("managed-state");
    if let Err(error) = fs::rename(&state_dir, &moved_state) {
        let _ = fs::remove_dir_all(&transaction);
        return Err(format!("failed to secure DXVK recovery state: {error}"));
    }

    let restore = (|| {
        for file in &manifest.files {
            let target = flat_target(game_dir, &file.name)?;
            if let Some(expected) = &file.original_sha256 {
                let backup = managed_backup_path(&moved_state, &file.name)?;
                fs::copy(&backup, &target)
                    .map_err(|error| format!("failed to restore {}: {error}", target.display()))?;
                if fs_ops::sha256_file(&target)? != *expected {
                    return Err(format!(
                        "restored DXVK file {} failed verification",
                        target.display()
                    ));
                }
            } else {
                remove_file_if_present(&target)?;
                if target.exists() {
                    return Err(format!("failed to remove {}", target.display()));
                }
            }
        }
        Ok(())
    })();

    if let Err(error) = restore {
        let rollback = snapshot.restore();
        let state_restore = fs::rename(&moved_state, &state_dir);
        if rollback.is_ok() && state_restore.is_ok() {
            let _ = fs::remove_dir_all(&transaction);
            return Err(format!(
                "{error}; the managed DXVK installation was restored"
            ));
        }
        return Err(format!(
            "{error}; recovery was incomplete. Files were retained at {}",
            transaction.display()
        ));
    }

    fs::remove_dir_all(&transaction).map_err(|error| {
        format!(
            "DXVK files were restored, but recovery-state cleanup failed at {}: {error}",
            transaction.display()
        )
    })?;
    Ok(true)
}

#[cfg(test)]
pub(crate) fn benchmark_install_transaction(
    storage_dir: &Path,
    game_dir: &Path,
    sources: &[PathBuf; 2],
    settings: &DxvkSettings,
    rollback_directory: &Path,
) -> Result<(), String> {
    validate_settings(settings)?;
    apply_managed_install(
        storage_dir,
        game_dir,
        sources,
        settings,
        false,
        rollback_directory,
    )
}

#[cfg(test)]
pub(crate) fn benchmark_configure_transaction(
    storage_dir: &Path,
    game_dir: &Path,
    settings: &DxvkSettings,
    staging_root: &Path,
) -> Result<(), String> {
    validate_settings(settings)?;
    configure_managed(storage_dir, game_dir, settings, staging_root)
}

#[cfg(test)]
pub(crate) fn benchmark_uninstall_transaction(
    storage_dir: &Path,
    game_dir: &Path,
) -> Result<bool, String> {
    uninstall_managed_files(storage_dir, game_dir)
}

fn supports_gpl_async_cache(release: &Release) -> bool {
    let label = if release.tag_name.trim().is_empty() {
        release.name.as_str()
    } else {
        release.tag_name.as_str()
    };
    let Some(start) = label.find(|character: char| character.is_ascii_digit()) else {
        return true;
    };
    let version = &label[start..];
    let major_end = version
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(version.len());
    let Ok(major) = version[..major_end].parse::<u32>() else {
        return true;
    };
    let Some(minor_text) = version
        .get(major_end..)
        .and_then(|rest| rest.strip_prefix('.'))
    else {
        return true;
    };
    let minor_end = minor_text
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(minor_text.len());
    let Ok(minor) = minor_text[..minor_end].parse::<u32>() else {
        return true;
    };
    (major, minor) < (2, 7)
}

fn validate_settings(settings: &DxvkSettings) -> Result<(), String> {
    if !(0..=64).contains(&settings.num_compiler_threads) {
        return Err("DXVK compiler threads must be between 0 and 64".into());
    }
    if !(0..=360).contains(&settings.max_frame_rate) {
        return Err("DXVK frame rate must be between 0 and 360".into());
    }
    if !(0..=16).contains(&settings.max_frame_latency) {
        return Err("DXVK frame latency must be between 0 and 16".into());
    }
    if !matches!(settings.tear_free.as_str(), "Auto" | "True" | "False") {
        return Err("DXVK tear-free mode must be Auto, True, or False".into());
    }
    Ok(())
}

fn build_conf(settings: &DxvkSettings, include_gpl_async_cache: bool) -> String {
    let mut lines = vec![format!(
        "dxvk.enableAsync={}",
        if settings.enable_async {
            "true"
        } else {
            "false"
        }
    )];
    if include_gpl_async_cache && settings.gpl_async_cache {
        lines.push("dxvk.gplAsyncCache=true".into());
    }
    lines.extend([
        format!("dxvk.numCompilerThreads={}", settings.num_compiler_threads),
        format!("dxgi.maxFrameRate={}", settings.max_frame_rate),
        format!("dxgi.maxFrameLatency={}", settings.max_frame_latency),
        format!("dxvk.tearFree={}", settings.tear_free),
    ]);
    if settings.hud_enabled {
        lines.push("dxvk.hud=fps,frametimes,gpuload".into());
    }
    lines.join("\n") + "\n"
}

fn parse_conf(contents: &str) -> DxvkSettings {
    let mut settings = DxvkSettings::default();
    for line in contents.lines() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "dxvk.enableAsync" => settings.enable_async = value.eq_ignore_ascii_case("true"),
            "dxvk.gplAsyncCache" => settings.gpl_async_cache = value.eq_ignore_ascii_case("true"),
            "dxvk.numCompilerThreads" => {
                if let Ok(value) = value.parse() {
                    settings.num_compiler_threads = value;
                }
            }
            "dxgi.maxFrameRate" => {
                if let Ok(value) = value.parse() {
                    settings.max_frame_rate = value;
                }
            }
            "dxgi.maxFrameLatency" => {
                if let Ok(value) = value.parse() {
                    settings.max_frame_latency = value;
                }
            }
            "dxvk.tearFree" => settings.tear_free = value.to_string(),
            "dxvk.hud" => settings.hud_enabled = !value.is_empty(),
            _ => {}
        }
    }
    settings
}

fn find_dxvk_files(root: &Path) -> Result<[PathBuf; 2], String> {
    let mut directories = vec![root.to_path_buf()];
    let mut index = 0;
    while index < directories.len() {
        let directory = directories[index].clone();
        index += 1;
        for entry in fs::read_dir(&directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let metadata = fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
            if metadata.file_type().is_symlink() {
                return Err("DXVK archive contains a link".into());
            }
            if metadata.is_dir() {
                directories.push(entry.path());
            }
        }
    }
    directories.sort_by_key(|path| {
        let components = path
            .components()
            .filter_map(|component| component.as_os_str().to_str())
            .map(str::to_ascii_lowercase)
            .collect::<Vec<_>>();
        let architecture = if components.iter().any(|part| part == "x64") {
            0
        } else if components.iter().any(|part| part == "x32") {
            2
        } else {
            1
        };
        (
            architecture,
            components.len(),
            path.to_string_lossy().to_lowercase(),
        )
    });

    for directory in directories {
        let components = directory
            .components()
            .filter_map(|component| component.as_os_str().to_str())
            .collect::<Vec<_>>();
        if components
            .iter()
            .any(|part| part.eq_ignore_ascii_case("x32"))
            && !components
                .iter()
                .any(|part| part.eq_ignore_ascii_case("x64"))
        {
            continue;
        }
        let files = fs::read_dir(&directory)
            .map_err(|error| error.to_string())?
            .filter_map(Result::ok)
            .filter(|entry| regular_file(&entry.path()))
            .map(|entry| {
                (
                    entry.file_name().to_string_lossy().to_ascii_lowercase(),
                    entry.path(),
                )
            })
            .collect::<Vec<_>>();
        let dxgi = files.iter().find(|(name, _)| name == "dxgi.dll");
        let d3d11 = files.iter().find(|(name, _)| name == "d3d11.dll");
        if let (Some((_, dxgi)), Some((_, d3d11))) = (dxgi, d3d11) {
            return Ok([dxgi.clone(), d3d11.clone()]);
        }
    }
    Err("Required 64-bit DXVK files were not found in the archive".into())
}

fn validate_game_dir(path: &Path) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return Err("Game directory is not set".into());
    }
    let path = path
        .canonicalize()
        .map_err(|error| format!("invalid game directory: {error}"))?;
    if !path.is_dir() {
        return Err("Game directory is not a directory".into());
    }
    if !["BlackOpsIII.exe", "BlackOps3.exe"]
        .iter()
        .any(|name| regular_file(&path.join(name)))
    {
        return Err("Game directory does not contain a Black Ops III executable".into());
    }
    Ok(path)
}

fn flat_target(root: &Path, filename: &str) -> Result<PathBuf, String> {
    if filename.is_empty() || filename.contains(['/', '\\']) || filename == "." || filename == ".."
    {
        return Err("invalid game filename".into());
    }
    let target = root.join(filename);
    if let Ok(metadata) = fs::symlink_metadata(&target) {
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "refusing to replace linked path {}",
                target.display()
            ));
        }
        if !metadata.is_file() {
            return Err(format!("expected a file at {}", target.display()));
        }
    }
    Ok(target)
}

fn regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn create_stage(state: &AppState, label: &str) -> Result<PathBuf, String> {
    let root = state.mod_files_dir();
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    create_private_dir(&root, label)
}

fn create_private_dir(root: &Path, label: &str) -> Result<PathBuf, String> {
    fs::create_dir_all(root).map_err(|error| error.to_string())?;
    for _ in 0..10 {
        let id = STAGE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let stage = root.join(format!(".{label}-stage-{}-{id}", std::process::id()));
        match fs::create_dir(&stage) {
            Ok(()) => return Ok(stage),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("could not create a unique staging directory".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "patchops-dxvk-{label}-{}-{}",
            std::process::id(),
            TEST_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn writes_and_parses_the_existing_dxvk_format() {
        let settings = DxvkSettings {
            hud_enabled: true,
            ..DxvkSettings::default()
        };
        let conf = build_conf(&settings, true);
        assert_eq!(
            conf,
            "dxvk.enableAsync=true\n\
dxvk.gplAsyncCache=true\n\
dxvk.numCompilerThreads=0\n\
dxgi.maxFrameRate=0\n\
dxgi.maxFrameLatency=1\n\
dxvk.tearFree=True\n\
dxvk.hud=fps,frametimes,gpuload\n"
        );
        assert_eq!(
            parse_conf(&format!("# preserved comment\n{conf}")),
            settings
        );
        assert!(!build_conf(&settings, false).contains("gplAsyncCache"));
    }

    #[test]
    fn release_selection_requires_the_exact_app_owned_pin() {
        let release: Release = serde_json::from_str(
            r#"{
                "tag_name":"v3.0-1",
                "assets":{"links":[
                    {"name":"dxvk-gplasync-v3.0-1.tar.gz","url":"https://gitlab.com/Ph42oN/dxvk-gplasync/-/raw/main/releases/dxvk-gplasync-v3.0-1.tar.gz"},
                    {"name":"dxvk-gplasync-v3.0-1.zip","url":"https://gitlab.com/Ph42oN/dxvk-gplasync/-/jobs/15069375988/artifacts/download?file_type=archive"}
                ]}
            }"#,
        )
        .unwrap();
        let selected = select_release_asset(&release).unwrap();
        assert_eq!(selected.kind, ArchiveKind::Zip);
        assert_eq!(selected.sha256, RELEASE_PINS[0].sha256);
        assert!(!supports_gpl_async_cache(&release));

        let raw_main_only: Release = serde_json::from_str(
            r#"{
                "tag_name":"v3.0-1",
                "assets":{"links":[
                    {"name":"dxvk-gplasync-v3.0-1.zip","url":"https://gitlab.com/Ph42oN/dxvk-gplasync/-/raw/main/releases/dxvk-gplasync-v3.0-1.zip"}
                ]}
            }"#,
        )
        .unwrap();
        assert!(select_release_asset(&raw_main_only).is_err());
    }

    #[test]
    fn release_selection_rejects_missing_or_invalid_pinned_digests() {
        let release: Release = serde_json::from_str(
            r#"{
                "tag_name":"v3.0-1",
                "assets":{"links":[
                    {"name":"dxvk-gplasync-v3.0-1.zip","url":"https://gitlab.com/Ph42oN/dxvk-gplasync/-/jobs/15069375988/artifacts/download?file_type=archive"}
                ]}
            }"#,
        )
        .unwrap();
        for sha256 in ["", "sha256:not-a-digest"] {
            let pins = [ReleasePin {
                sha256,
                ..RELEASE_PINS[0]
            }];
            let error = select_release_asset_with_pins(&release, &pins).unwrap_err();
            assert!(error.contains("valid app-owned SHA-256"));
        }
    }

    #[test]
    fn accepts_only_the_pinned_gitlab_release_url() {
        assert!(validate_download_url(RELEASE_PINS[0].url).is_ok());
        for url in [
            "http://gitlab.com/Ph42oN/dxvk-gplasync/-/jobs/15069375988/artifacts/download?file_type=archive",
            "https://gitlab.com.evil.invalid/Ph42oN/dxvk-gplasync/-/jobs/15069375988/artifacts/download?file_type=archive",
            "https://gitlab.com/Ph42oN/dxvk-gplasync/-/raw/main/releases/dxvk-gplasync-v3.0-1.zip",
            "https://gitlab.com/Ph42oN/dxvk-gplasync/-/jobs/999/artifacts/download?file_type=archive",
            "https://gitlab.com:444/Ph42oN/dxvk-gplasync/-/jobs/15069375988/artifacts/download?file_type=archive",
        ] {
            assert!(validate_download_url(url).is_err(), "accepted {url}");
        }
    }

    #[test]
    fn chooses_x64_payload_and_snapshot_rolls_back() {
        let root = temp_dir("source");
        for architecture in ["x32", "x64"] {
            let dir = root.join(architecture);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("dxgi.dll"), architecture).unwrap();
            fs::write(dir.join("d3d11.dll"), architecture).unwrap();
        }
        let selected = find_dxvk_files(&root).unwrap();
        assert!(selected
            .iter()
            .all(|path| path.starts_with(root.join("x64"))));

        let target = root.join("installed.dll");
        fs::write(&target, b"original").unwrap();
        let snapshot =
            Snapshot::capture(std::slice::from_ref(&target), &root.join("rollback")).unwrap();
        fs::write(&target, b"partial").unwrap();
        snapshot.restore().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"original");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_out_of_range_or_injected_settings() {
        let settings = DxvkSettings {
            max_frame_rate: 361,
            ..DxvkSettings::default()
        };
        assert!(validate_settings(&settings).is_err());
        let settings = DxvkSettings {
            tear_free: "True\ndxvk.hud=full".into(),
            ..DxvkSettings::default()
        };
        assert!(validate_settings(&settings).is_err());
    }

    #[test]
    fn managed_uninstall_restores_exact_originals_and_removes_created_files() {
        let root = temp_dir("managed-restore");
        let storage = root.join("data");
        let game = root.join("game");
        fs::create_dir_all(&storage).unwrap();
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("dxgi.dll"), b"original-dxgi").unwrap();
        fs::write(game.join("dxvk.conf"), b"original-conf").unwrap();

        let targets = managed_targets(&game).unwrap();
        let (mut manifest, new_state) = prepare_managed_install(&storage, &game, &targets).unwrap();
        assert!(new_state);
        fs::write(game.join("dxgi.dll"), b"managed-dxgi").unwrap();
        fs::write(game.join("d3d11.dll"), b"managed-d3d11").unwrap();
        fs::write(game.join("dxvk.conf"), b"managed-conf").unwrap();
        update_installed_hashes(&mut manifest, &game).unwrap();
        save_managed_manifest(&storage, &manifest).unwrap();

        assert!(uninstall_managed_files(&storage, &game).unwrap());
        assert_eq!(fs::read(game.join("dxgi.dll")).unwrap(), b"original-dxgi");
        assert!(!game.join("d3d11.dll").exists());
        assert_eq!(fs::read(game.join("dxvk.conf")).unwrap(), b"original-conf");
        assert!(!managed_state_dir(&storage).exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn managed_uninstall_refuses_changed_or_unowned_files() {
        let root = temp_dir("managed-conflict");
        let storage = root.join("data");
        let game = root.join("game");
        fs::create_dir_all(&storage).unwrap();
        fs::create_dir_all(&game).unwrap();
        let targets = managed_targets(&game).unwrap();
        let (mut manifest, _) = prepare_managed_install(&storage, &game, &targets).unwrap();
        for (name, body) in [
            ("dxgi.dll", b"managed-dxgi".as_slice()),
            ("d3d11.dll", b"managed-d3d11".as_slice()),
            ("dxvk.conf", b"managed-conf".as_slice()),
        ] {
            fs::write(game.join(name), body).unwrap();
        }
        update_installed_hashes(&mut manifest, &game).unwrap();
        save_managed_manifest(&storage, &manifest).unwrap();
        fs::write(game.join("dxgi.dll"), b"user-modified").unwrap();

        let error = uninstall_managed_files(&storage, &game).unwrap_err();
        assert!(error.contains("changed after PatchOpsIII installed"));
        assert_eq!(fs::read(game.join("dxgi.dll")).unwrap(), b"user-modified");
        assert_eq!(fs::read(game.join("d3d11.dll")).unwrap(), b"managed-d3d11");
        assert!(managed_state_dir(&storage).is_dir());

        let unmanaged_storage = root.join("unmanaged-data");
        fs::create_dir_all(&unmanaged_storage).unwrap();
        let error = uninstall_managed_files(&unmanaged_storage, &game).unwrap_err();
        assert!(error.contains("refusing to remove unowned files"));
        assert_eq!(fs::read(game.join("dxgi.dll")).unwrap(), b"user-modified");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn managed_reinstall_and_reconfigure_preserve_the_first_originals() {
        let root = temp_dir("managed-update");
        let storage = root.join("data");
        let game = root.join("game");
        fs::create_dir_all(&storage).unwrap();
        fs::create_dir_all(&game).unwrap();
        for (name, body) in [
            ("dxgi.dll", b"original-dxgi".as_slice()),
            ("d3d11.dll", b"original-d3d11".as_slice()),
            ("dxvk.conf", b"original-conf".as_slice()),
        ] {
            fs::write(game.join(name), body).unwrap();
        }

        let targets = managed_targets(&game).unwrap();
        let (mut first, _) = prepare_managed_install(&storage, &game, &targets).unwrap();
        for target in &targets {
            fs::write(target, b"managed-v1").unwrap();
        }
        update_installed_hashes(&mut first, &game).unwrap();
        save_managed_manifest(&storage, &first).unwrap();

        let (mut second, new_state) = prepare_managed_install(&storage, &game, &targets).unwrap();
        assert!(!new_state);
        for target in &targets {
            fs::write(target, b"managed-v2").unwrap();
        }
        update_installed_hashes(&mut second, &game).unwrap();
        save_managed_manifest(&storage, &second).unwrap();
        fs_ops::atomic_write(&game.join("dxvk.conf"), b"reconfigured").unwrap();
        update_installed_hashes(&mut second, &game).unwrap();
        save_managed_manifest(&storage, &second).unwrap();

        assert!(uninstall_managed_files(&storage, &game).unwrap());
        assert_eq!(fs::read(game.join("dxgi.dll")).unwrap(), b"original-dxgi");
        assert_eq!(fs::read(game.join("d3d11.dll")).unwrap(), b"original-d3d11");
        assert_eq!(fs::read(game.join("dxvk.conf")).unwrap(), b"original-conf");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exact_legacy_pair_is_adopted_and_custom_config_is_restored() {
        let root = temp_dir("legacy-adopt");
        let storage = root.join("data");
        let game = root.join("game");
        fs::create_dir_all(&storage).unwrap();
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("dxgi.dll"), b"legacy-dxgi").unwrap();
        fs::write(game.join("d3d11.dll"), b"legacy-d3d11").unwrap();
        fs::write(game.join("dxvk.conf"), b"custom legacy config\n").unwrap();
        let pair = [[
            fs_ops::sha256_file(&game.join("dxgi.dll")).unwrap(),
            fs_ops::sha256_file(&game.join("d3d11.dll")).unwrap(),
        ]];

        assert!(adopt_legacy_install(&storage, &game, &pair).unwrap());
        let mut manifest = load_managed_manifest(&storage, &game).unwrap().unwrap();
        assert!(manifest.files[..2]
            .iter()
            .all(|file| file.original_sha256.is_none()));
        assert!(manifest.files[2].original_sha256.is_some());

        for target in managed_targets(&game).unwrap() {
            fs::write(target, b"updated managed bytes").unwrap();
        }
        update_installed_hashes(&mut manifest, &game).unwrap();
        save_managed_manifest(&storage, &manifest).unwrap();
        assert!(uninstall_managed_files(&storage, &game).unwrap());
        assert!(!game.join("dxgi.dll").exists());
        assert!(!game.join("d3d11.dll").exists());
        assert_eq!(
            fs::read(game.join("dxvk.conf")).unwrap(),
            b"custom legacy config\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn historical_release_pairs_remain_exact_and_pair_bound() {
        let pairs = known_legacy_pairs();
        assert_eq!(pairs.len(), KNOWN_LEGACY_DXVK_PAIRS.len());
        assert!(pairs.contains(&[
            "8d5ae3c40962a4846e4dae9d373a8660227e85f051d12e5bb882ef92d136dfca".into(),
            "7f20637b7a9527fbed53985fa55c5fc56c7d8bd132589219e92bd49f488c2754".into(),
        ]));
        assert!(!pairs.contains(&[
            KNOWN_LEGACY_DXVK_PAIRS[6][0].into(),
            KNOWN_LEGACY_DXVK_PAIRS[7][1].into(),
        ]));
    }

    #[test]
    fn legacy_adoption_rejects_incomplete_or_mixed_dll_pairs_atomically() {
        let root = temp_dir("legacy-reject");
        let storage = root.join("data");
        let game = root.join("game");
        fs::create_dir_all(&storage).unwrap();
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("dxgi.dll"), b"known-dxgi").unwrap();
        fs::write(game.join("dxvk.conf"), b"custom config").unwrap();
        let pair = [[
            fs_ops::sha256_file(&game.join("dxgi.dll")).unwrap(),
            fs_ops::sha256_file(&game.join("dxvk.conf")).unwrap(),
        ]];
        assert!(adopt_legacy_install(&storage, &game, &pair).is_err());
        assert!(!managed_state_dir(&storage).exists());
        assert_eq!(fs::read(game.join("dxgi.dll")).unwrap(), b"known-dxgi");

        fs::write(game.join("d3d11.dll"), b"different-release").unwrap();
        assert!(adopt_legacy_install(&storage, &game, &pair).is_err());
        assert!(!managed_state_dir(&storage).exists());
        assert_eq!(
            fs::read(game.join("d3d11.dll")).unwrap(),
            b"different-release"
        );
        assert_eq!(fs::read(game.join("dxvk.conf")).unwrap(), b"custom config");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn configure_without_managed_state_cannot_overwrite_an_existing_config() {
        let root = temp_dir("configure-unmanaged");
        let storage = root.join("data");
        let game = root.join("game");
        fs::create_dir_all(&storage).unwrap();
        fs::create_dir_all(&game).unwrap();
        let config = game.join("dxvk.conf");
        fs::write(&config, b"unmanaged sentinel").unwrap();

        let staging = root.join("staging");
        let error =
            configure_managed(&storage, &game, &DxvkSettings::default(), &staging).unwrap_err();
        assert_eq!(error, UNMANAGED_CONFIG_MESSAGE);
        assert_eq!(fs::read(&config).unwrap(), b"unmanaged sentinel");
        assert!(!managed_state_dir(&storage).exists());
        assert!(!staging.exists());

        fs::remove_dir_all(root).unwrap();
    }
}
