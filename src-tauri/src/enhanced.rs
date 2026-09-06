use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs::{self, File},
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use crate::{app::AppState, exe, fs_ops, models::EnhancedState, steam};

pub const GITHUB_LATEST_ENHANCED_API: &str =
    "https://api.github.com/repos/shiversoftdev/BO3Enhanced/releases/latest";
pub const STATE_FILENAME: &str = "bo3_enhanced_state.json";
pub const ENHANCED_ARCHIVE_NAME: &str = "BO3Enhanced_latest.zip";
pub const CHECKSUMS_FILENAME: &str = "bo3_enhanced_checksums.json";

const EXPECTED_ENHANCED_FILES: [&str; 4] = [
    "T7WSBootstrapper.dll",
    "T7InternalWS.dll",
    "steam_api65.dll",
    "WindowsCodecs.dll",
];
const EXPECTED_DUMP_FILES: [&str; 2] = ["BlackOps3.exe", "MicrosoftGame.config"];
const UWP_DUMP_WHITELIST: [&str; 8] = [
    "BlackOps3.exe",
    "MicrosoftGame.config",
    "GameChat2.dll",
    "Party.dll",
    "PartyXboxLive.dll",
    "PlayFabMultiplayerGDK.dll",
    "libScePad.dll",
    "XCurl.dll",
];
const MAX_ARCHIVE_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 100_000;
const MAX_DOWNLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_RELEASE_METADATA_BYTES: u64 = 4 * 1024 * 1024;
const ENHANCED_RELEASE_PATH_PREFIX: &str = "/shiversoftdev/BO3Enhanced/releases/download/";

#[derive(Clone, Default, Serialize, Deserialize)]
struct PersistedState {
    #[serde(default)]
    game_directory: Option<String>,
    #[serde(default)]
    installed: bool,
    #[serde(default)]
    detected_at: Option<String>,
    #[serde(default)]
    acknowledged_at: Option<String>,
    #[serde(default)]
    installed_files: Vec<String>,
    #[serde(default)]
    dump_only_files: Vec<String>,
    #[serde(default)]
    created_files: Vec<String>,
    #[serde(default)]
    owned_backups: BTreeMap<String, String>,
    #[serde(default)]
    installed_hashes: BTreeMap<String, String>,
    #[serde(default)]
    original_backup_hashes: BTreeMap<String, String>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

enum MemberSource {
    File(PathBuf),
    Zip(usize),
}

struct PayloadMember {
    relative: PathBuf,
    source: MemberSource,
}

struct AppliedFile {
    target: PathBuf,
    rollback_copy: Option<PathBuf>,
    new_permanent_backup: Option<PathBuf>,
}

#[derive(Clone)]
enum UninstallAction {
    Restore(PathBuf),
    Remove,
}

#[derive(Clone)]
struct PlannedUninstall {
    target: PathBuf,
    action: UninstallAction,
}

struct AppliedUninstall {
    plan: PlannedUninstall,
    displaced: Option<PathBuf>,
}

struct EnhancedUninstallTransaction {
    applied: Vec<AppliedUninstall>,
    transaction: Option<PathBuf>,
    storage_dir: PathBuf,
    original_state: PersistedState,
}

#[derive(Debug)]
struct EnhancedRelease {
    version: String,
    asset_url: String,
    asset_sha256: String,
}

fn state_path(storage_dir: &Path) -> PathBuf {
    storage_dir.join(STATE_FILENAME)
}

fn checksums_path(storage_dir: &Path) -> PathBuf {
    storage_dir.join(CHECKSUMS_FILENAME)
}

fn load_state(storage_dir: &Path) -> PersistedState {
    fs::read(state_path(storage_dir))
        .ok()
        .and_then(|body| serde_json::from_slice(&body).ok())
        .unwrap_or_default()
}

fn save_state(storage_dir: &Path, state: &PersistedState) -> Result<(), String> {
    let body = serde_json::to_vec_pretty(state).map_err(|error| error.to_string())?;
    fs_ops::atomic_write(&state_path(storage_dir), &body)
}

fn canonical_game_key(game_dir: &Path) -> Result<String, String> {
    let path = fs::canonicalize(game_dir).map_err(|error| error.to_string())?;
    let key = path.to_string_lossy().into_owned();
    #[cfg(windows)]
    return Ok(key.to_ascii_lowercase());
    #[cfg(not(windows))]
    Ok(key)
}

fn state_belongs_to_game(state: &PersistedState, game_dir: &Path) -> bool {
    canonical_game_key(game_dir)
        .ok()
        .is_some_and(|key| state.game_directory.as_deref() == Some(key.as_str()))
}

fn state_has_ownership(state: &PersistedState) -> bool {
    state.installed
        || !state.installed_files.is_empty()
        || !state.dump_only_files.is_empty()
        || !state.created_files.is_empty()
        || !state.owned_backups.is_empty()
        || !state.installed_hashes.is_empty()
        || !state.original_backup_hashes.is_empty()
}

fn state_has_owned_install(state: &PersistedState, game_dir: &Path) -> bool {
    state_belongs_to_game(state, game_dir) && state_has_ownership(state)
}

pub fn has_owned_install(state: &AppState, game_dir: &Path) -> bool {
    state_has_owned_install(&load_state(state.data_dir()), game_dir)
}

fn load_checksums(storage_dir: &Path) -> BTreeMap<String, String> {
    fs::read(checksums_path(storage_dir))
        .ok()
        .and_then(|body| serde_json::from_slice(&body).ok())
        .unwrap_or_default()
}

fn save_checksums(storage_dir: &Path, checksums: &BTreeMap<String, String>) -> Result<(), String> {
    let body = serde_json::to_vec_pretty(checksums).map_err(|error| error.to_string())?;
    fs_ops::atomic_write(&checksums_path(storage_dir), &body)
}

fn utc_timestamp() -> String {
    let now = OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        now.year(),
        now.month() as u8,
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

pub fn detect_install(game_dir: &Path) -> bool {
    EXPECTED_ENHANCED_FILES
        .iter()
        .all(|name| game_dir.join(name).is_file())
}

fn status_at(
    storage_dir: &Path,
    game_dir: Option<&Path>,
    current_launch_options: Option<&str>,
    dump_source: String,
) -> (EnhancedState, Option<String>) {
    let stored = load_state(storage_dir);
    let mut persisted = match game_dir.filter(|game_dir| state_belongs_to_game(&stored, game_dir)) {
        Some(_) => stored,
        None => PersistedState::default(),
    };
    let installed_files = persisted.installed_files.len();
    let detected = game_dir.is_some_and(detect_install);
    let stale_state_error =
        if !detected && persisted.installed_files.is_empty() && persisted.installed {
            persisted.installed = false;
            save_state(storage_dir, &persisted).err()
        } else {
            None
        };
    let installed = game_dir.is_some() && (detected || persisted.installed);
    (
        EnhancedState {
            installed,
            detected_at: persisted.detected_at,
            acknowledged_at: persisted.acknowledged_at,
            launch_options_active: current_launch_options
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains("windowscodecs=n,b"),
            dump_source,
            files_installed: installed_files,
            backup_status: if installed && installed_files > 0 {
                "Created".into()
            } else {
                "Not created".into()
            },
        },
        stale_state_error,
    )
}

pub fn status(
    state: &AppState,
    game_dir: Option<&Path>,
    current_launch_options: Option<&str>,
    dump_source: String,
) -> EnhancedState {
    let (result, stale_state_error) = status_at(
        state.data_dir(),
        game_dir,
        current_launch_options,
        dump_source,
    );
    if let Some(error) = stale_state_error {
        state.log(
            "Warning",
            format!("Failed to clear stale BO3 Enhanced state: {error}"),
        );
    }
    result
}

fn safe_relative(path: &Path) -> bool {
    path.components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

fn contained_target(root: &Path, relative: &Path) -> Result<PathBuf, String> {
    if !safe_relative(relative) {
        return Err(format!(
            "Refusing unsafe payload path: {}",
            relative.display()
        ));
    }
    let canonical_root = fs::canonicalize(root).map_err(|error| error.to_string())?;
    let target = root.join(relative);
    let mut existing_parent = target.parent().unwrap_or(root);
    while !existing_parent.exists() {
        existing_parent = existing_parent
            .parent()
            .ok_or_else(|| "Payload target has no contained parent.".to_string())?;
    }
    let canonical_parent = fs::canonicalize(existing_parent).map_err(|error| error.to_string())?;
    if !canonical_parent.starts_with(&canonical_root) {
        return Err(format!(
            "Payload target escapes the game directory: {}",
            target.display()
        ));
    }
    Ok(target)
}

fn archive_relative(name: &str) -> Result<PathBuf, String> {
    if name.contains('\0') {
        return Err("Archive contains an unsafe path.".into());
    }
    let normalized = name.replace('\\', "/");
    let relative = PathBuf::from(normalized);
    if !safe_relative(&relative) {
        return Err(format!("Archive contains an unsafe path: {name}"));
    }
    Ok(relative)
}

fn strip_dump_prefix(path: &Path) -> PathBuf {
    let mut components = path.components();
    if components
        .next()
        .is_some_and(|component| component.as_os_str() == "DUMP")
    {
        components.as_path().to_owned()
    } else {
        path.to_owned()
    }
}

fn should_copy_dump_member(relative: &Path) -> bool {
    relative
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| UWP_DUMP_WHITELIST.contains(&name))
}

fn validate_required_dump_files(members: &[PayloadMember]) -> Result<(), String> {
    let found: HashSet<_> = members
        .iter()
        .map(|member| member.relative.to_string_lossy().replace('\\', "/"))
        .collect();
    let missing: Vec<_> = EXPECTED_DUMP_FILES
        .iter()
        .filter(|name| !found.contains(**name))
        .copied()
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Dump source missing required files: {}",
            missing.join(", ")
        ))
    }
}

fn collect_directory_members(
    root: &Path,
    directory: &Path,
    members: &mut Vec<PayloadMember>,
    seen: &mut HashSet<String>,
    total: &mut u64,
    count: &mut usize,
    depth: usize,
) -> Result<(), String> {
    if depth > 64 {
        return Err("Dump source directory nesting is too deep.".into());
    }
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        if file_type.is_symlink() {
            return Err(format!(
                "Dump source contains a symbolic link: {}",
                entry.path().display()
            ));
        }
        if file_type.is_dir() {
            collect_directory_members(root, &entry.path(), members, seen, total, count, depth + 1)?;
            continue;
        }
        if !file_type.is_file() {
            return Err(format!(
                "Dump source contains an unsupported file type: {}",
                entry.path().display()
            ));
        }
        *count += 1;
        if *count > MAX_ARCHIVE_ENTRIES {
            return Err("Dump source contains too many files.".into());
        }
        *total = total.saturating_add(entry.metadata().map_err(|error| error.to_string())?.len());
        if *total > MAX_ARCHIVE_BYTES {
            return Err("Dump source is larger than the allowed size.".into());
        }
        let relative = entry
            .path()
            .strip_prefix(root)
            .map_err(|error| error.to_string())?
            .to_owned();
        let relative = strip_dump_prefix(&relative);
        if relative.as_os_str().is_empty() || !should_copy_dump_member(&relative) {
            continue;
        }
        let key = relative_string(&relative).to_ascii_lowercase();
        if !safe_relative(&relative) || !seen.insert(key) {
            return Err(format!(
                "Dump source contains an unsafe or duplicate path: {}",
                relative.display()
            ));
        }
        members.push(PayloadMember {
            relative,
            source: MemberSource::File(entry.path()),
        });
    }
    Ok(())
}

fn zip_entry_is_link(entry: &zip::read::ZipFile<'_>) -> bool {
    entry
        .unix_mode()
        .is_some_and(|mode| mode & 0o170000 == 0o120000)
}

fn dump_members(source: &Path) -> Result<Vec<PayloadMember>, String> {
    let mut members = Vec::new();
    let mut seen = HashSet::new();
    let mut total = 0_u64;
    if source.is_dir() {
        let mut count = 0;
        collect_directory_members(
            source,
            source,
            &mut members,
            &mut seen,
            &mut total,
            &mut count,
            0,
        )?;
    } else {
        let mut archive = zip::ZipArchive::new(
            File::open(source).map_err(|error| format!("{}: {error}", source.display()))?,
        )
        .map_err(|_| "Dump source is not a valid zip file or directory.".to_string())?;
        if archive.len() > MAX_ARCHIVE_ENTRIES {
            return Err("Dump archive contains too many entries.".into());
        }
        for index in 0..archive.len() {
            let entry = archive.by_index(index).map_err(|error| error.to_string())?;
            let relative = archive_relative(entry.name())?;
            if zip_entry_is_link(&entry) {
                return Err(format!("Dump archive contains a link: {}", entry.name()));
            }
            total = total.saturating_add(entry.size());
            if total > MAX_ARCHIVE_BYTES {
                return Err("Dump archive expands beyond the allowed size.".into());
            }
            if entry.is_dir() {
                continue;
            }
            let relative = strip_dump_prefix(&relative);
            if relative.as_os_str().is_empty() || !should_copy_dump_member(&relative) {
                continue;
            }
            let key = relative_string(&relative).to_ascii_lowercase();
            if !safe_relative(&relative) || !seen.insert(key) {
                return Err(format!(
                    "Dump archive contains an unsafe or duplicate path: {}",
                    relative.display()
                ));
            }
            members.push(PayloadMember {
                relative,
                source: MemberSource::Zip(index),
            });
        }
    }
    validate_required_dump_files(&members)?;
    Ok(members)
}

fn enhanced_members(archive_path: &Path) -> Result<Vec<PayloadMember>, String> {
    let mut archive = zip::ZipArchive::new(
        File::open(archive_path).map_err(|error| format!("{}: {error}", archive_path.display()))?,
    )
    .map_err(|_| "Enhanced archive is not a valid zip.".to_string())?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err("Enhanced archive contains too many entries.".into());
    }

    let mut members = Vec::new();
    let mut found = HashSet::new();
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let relative = archive_relative(entry.name())?;
        if zip_entry_is_link(&entry) {
            return Err(format!(
                "Enhanced archive contains a link: {}",
                entry.name()
            ));
        }
        total = total.saturating_add(entry.size());
        if total > MAX_ARCHIVE_BYTES {
            return Err("Enhanced archive expands beyond the allowed size.".into());
        }
        if entry.is_dir() {
            continue;
        }
        let Some(name) = relative.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !EXPECTED_ENHANCED_FILES.contains(&name) {
            continue;
        }
        if !found.insert(name.to_owned()) {
            return Err(format!(
                "Enhanced archive contains duplicate expected file: {name}"
            ));
        }
        members.push(PayloadMember {
            relative: PathBuf::from(name),
            source: MemberSource::Zip(index),
        });
    }
    let missing: Vec<_> = EXPECTED_ENHANCED_FILES
        .iter()
        .filter(|name| !found.contains(**name))
        .copied()
        .collect();
    if missing.is_empty() {
        Ok(members)
    } else {
        Err(format!(
            "Enhanced archive missing expected files: {}",
            missing.join(", ")
        ))
    }
}

pub fn validate_dump_source(source: &Path) -> Result<bool, String> {
    if source.as_os_str().is_empty() || !source.exists() {
        return Ok(false);
    }
    Ok(dump_members(source).is_ok())
}

pub fn validate_enhanced_archive(archive: &Path) -> Result<bool, String> {
    if !archive.is_file() {
        return Ok(false);
    }
    Ok(enhanced_members(archive).is_ok())
}

pub fn validate_and_remember_dump_source(state: &AppState, source: &Path) -> Result<bool, String> {
    let mut settings = state.load_settings();
    settings.enhanced_dump_source = Some(source.display().to_string());
    state.save_settings(&settings)?;
    validate_dump_source(source)
}

fn valid_enhanced_asset_url(value: &str) -> bool {
    reqwest::Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("github.com")
            && url.port().is_none()
            && url.username().is_empty()
            && url.password().is_none()
            && url.path().starts_with(ENHANCED_RELEASE_PATH_PREFIX)
            && url.path().to_ascii_lowercase().ends_with(".zip")
            && url.query().is_none()
            && url.fragment().is_none()
    })
}

fn fetch_latest_release() -> Result<EnhancedRelease, String> {
    let response = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| error.to_string())?
        .get(GITHUB_LATEST_ENHANCED_API)
        .header(reqwest::header::USER_AGENT, "PatchOpsIII")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| format!("Failed to fetch BO3 Enhanced release metadata: {error}"))?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RELEASE_METADATA_BYTES)
    {
        return Err("BO3 Enhanced release metadata was unexpectedly large.".into());
    }
    let mut body = Vec::new();
    response
        .take(MAX_RELEASE_METADATA_BYTES + 1)
        .read_to_end(&mut body)
        .map_err(|error| error.to_string())?;
    if body.len() as u64 > MAX_RELEASE_METADATA_BYTES {
        return Err("BO3 Enhanced release metadata was unexpectedly large.".into());
    }
    let data: Value = serde_json::from_slice(&body)
        .map_err(|error| format!("Invalid BO3 Enhanced release metadata: {error}"))?;

    let assets = data
        .get("assets")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            "No downloadable asset found for BO3 Enhanced latest release.".to_string()
        })?;
    let selected = assets
        .iter()
        .find(|asset| {
            asset
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| name.to_ascii_lowercase().ends_with(".zip"))
        })
        .ok_or_else(|| {
            "No downloadable asset found for BO3 Enhanced latest release.".to_string()
        })?;
    let digest = selected
        .get("digest")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let checksum = digest.strip_prefix("sha256:").unwrap_or_default().trim();
    if checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Latest BO3 Enhanced release metadata did not include a SHA-256 digest; refusing unverified download.".into());
    }
    let asset_url = selected
        .get("browser_download_url")
        .and_then(Value::as_str)
        .filter(|url| valid_enhanced_asset_url(url))
        .ok_or_else(|| "BO3 Enhanced release asset has an invalid download URL.".to_string())?;
    Ok(EnhancedRelease {
        version: data
            .get("tag_name")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .or_else(|| data.get("name").and_then(Value::as_str))
            .unwrap_or("0.0.0")
            .to_owned(),
        asset_url: asset_url.to_owned(),
        asset_sha256: checksum.to_owned(),
    })
}

pub fn download_latest(state: &AppState) -> Result<PathBuf, String> {
    let release = fetch_latest_release()?;
    fs::create_dir_all(state.mod_files_dir()).map_err(|error| error.to_string())?;
    let destination = state.mod_files_dir().join(ENHANCED_ARCHIVE_NAME);
    let checksum = fs_ops::download(
        &release.asset_url,
        &destination,
        Some(&release.asset_sha256),
        MAX_DOWNLOAD_BYTES,
    )?;
    if !validate_enhanced_archive(&destination)? {
        let _ = fs::remove_file(&destination);
        return Err("Enhanced archive failed validation.".into());
    }
    let mut checksums = load_checksums(state.data_dir());
    checksums.insert(ENHANCED_ARCHIVE_NAME.into(), checksum);
    save_checksums(state.data_dir(), &checksums)?;
    state.log(
        "Success",
        format!(
            "Downloaded BO3 Enhanced {} to {}",
            release.version,
            destination.display()
        ),
    );
    Ok(destination)
}

fn verify_cached_checksum(storage_dir: &Path, archive: &Path) -> Result<(), String> {
    let Some(filename) = archive.file_name().and_then(|name| name.to_str()) else {
        return Err("Invalid Enhanced archive filename.".into());
    };
    let checksums = load_checksums(storage_dir);
    let Some(expected) = checksums.get(filename) else {
        return Ok(());
    };
    let actual = fs_ops::sha256_file(archive)?;
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err("Enhanced archive failed cached SHA-256 verification.".into())
    }
}

fn sha256_reader(reader: &mut impl Read) -> Result<String, String> {
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn member_hashes(
    source_archive: Option<&Path>,
    members: &[PayloadMember],
) -> Result<BTreeMap<String, String>, String> {
    let mut archive = source_archive
        .map(|path| {
            zip::ZipArchive::new(
                File::open(path).map_err(|error| format!("{}: {error}", path.display()))?,
            )
            .map_err(|error| error.to_string())
        })
        .transpose()?;
    let mut hashes = BTreeMap::new();
    for member in members {
        if !safe_relative(&member.relative) || member.relative.as_os_str().is_empty() {
            return Err(format!(
                "Refusing unsafe legacy provenance path: {}",
                member.relative.display()
            ));
        }
        let hash = match &member.source {
            MemberSource::File(source) => {
                if !regular_file_without_links(source) {
                    return Err(format!(
                        "Legacy provenance source is missing or unsafe: {}",
                        source.display()
                    ));
                }
                let hash = fs_ops::sha256_file(source)?;
                if !regular_file_without_links(source) {
                    return Err(format!(
                        "Legacy provenance source changed while it was read: {}",
                        source.display()
                    ));
                }
                hash
            }
            MemberSource::Zip(index) => {
                let mut entry = archive
                    .as_mut()
                    .ok_or_else(|| "Missing legacy provenance archive.".to_string())?
                    .by_index(*index)
                    .map_err(|error| error.to_string())?;
                if entry.is_dir() || zip_entry_is_link(&entry) {
                    return Err(format!(
                        "Legacy provenance archive member is unsafe: {}",
                        entry.name()
                    ));
                }
                sha256_reader(&mut entry)?
            }
        };
        let relative = relative_string(&member.relative);
        if hashes.insert(relative.clone(), hash).is_some() {
            return Err(format!(
                "Legacy provenance contains a duplicate path: {relative}"
            ));
        }
    }
    Ok(hashes)
}

fn verified_cached_enhanced_hashes(
    storage_dir: &Path,
    archive: &Path,
) -> Result<BTreeMap<String, String>, String> {
    if !regular_file_without_links(archive) {
        return Err(
            "Legacy Enhanced ownership cannot be verified because the cached archive is missing or unsafe."
                .into(),
        );
    }
    let filename = archive
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Invalid cached Enhanced archive filename.".to_string())?;
    let checksums = load_checksums(storage_dir);
    let expected = checksums.get(filename).ok_or_else(|| {
        "Legacy Enhanced ownership cannot be verified because the cached archive has no recorded SHA-256 checksum."
            .to_string()
    })?;
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(
            "Legacy Enhanced ownership cannot be verified because the cached archive checksum is invalid."
                .into(),
        );
    }
    let before = fs_ops::sha256_file(archive)?;
    if !before.eq_ignore_ascii_case(expected) {
        return Err(
            "Legacy Enhanced ownership cannot be verified because the cached archive failed SHA-256 verification."
                .into(),
        );
    }
    let members = enhanced_members(archive)?;
    let hashes = member_hashes(Some(archive), &members)?;
    let after = fs_ops::sha256_file(archive)?;
    if !after.eq_ignore_ascii_case(expected) || !after.eq_ignore_ascii_case(&before) {
        return Err(
            "Legacy Enhanced ownership cannot be verified because the cached archive changed while it was inspected."
                .into(),
        );
    }
    Ok(hashes)
}

fn verified_dump_member_hashes(source: &Path) -> Result<BTreeMap<String, String>, String> {
    let metadata = fs::symlink_metadata(source).map_err(|_| {
        "Legacy Enhanced ownership cannot be verified because the original dump source is unavailable."
            .to_string()
    })?;
    if metadata.file_type().is_symlink() || (!metadata.is_file() && !metadata.is_dir()) {
        return Err(
            "Legacy Enhanced ownership cannot be verified because the original dump source is unsafe."
                .into(),
        );
    }
    let archive_hash = metadata
        .is_file()
        .then(|| fs_ops::sha256_file(source))
        .transpose()?;
    let members = dump_members(source)?;
    let hashes = member_hashes(metadata.is_file().then_some(source), &members)?;
    if let Some(before) = archive_hash {
        let after = fs_ops::sha256_file(source)?;
        if !after.eq_ignore_ascii_case(&before) {
            return Err(
                "Legacy Enhanced ownership cannot be verified because the dump archive changed while it was inspected."
                    .into(),
            );
        }
    }
    Ok(hashes)
}

fn normalized_legacy_path(value: &str) -> Result<(PathBuf, String), String> {
    let normalized = value.replace('\\', "/");
    let relative = PathBuf::from(&normalized);
    if normalized.is_empty() || !safe_relative(&relative) {
        return Err(format!(
            "Legacy Enhanced state contains an unsafe path: {value}."
        ));
    }
    Ok((relative, normalized))
}

fn legacy_backup_for_target(
    game_dir: &Path,
    target: &Path,
    relative_text: &str,
) -> Result<Option<(String, String)>, String> {
    for candidate in [
        fs_ops::backup_path(target),
        fs_ops::legacy_backup_path(target),
    ] {
        let relative = candidate.strip_prefix(game_dir).map_err(|_| {
            format!("Legacy Enhanced backup escaped the game directory for {relative_text}.")
        })?;
        if !safe_relative(relative) || relative.as_os_str().is_empty() {
            return Err(format!(
                "Legacy Enhanced backup path is unsafe for {relative_text}."
            ));
        }
        let candidate = contained_target(game_dir, relative)?;
        match fs::symlink_metadata(&candidate) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                let hash = fs_ops::sha256_file(&candidate)?;
                return Ok(Some((relative_string(relative), hash)));
            }
            Ok(_) => {
                return Err(format!(
                    "Legacy Enhanced backup is not a regular file for {relative_text}: {}.",
                    candidate.display()
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(None)
}

fn adopt_legacy_state(
    game_dir: &Path,
    enhanced_archive: &Path,
    dump_source: Option<&Path>,
    storage_dir: &Path,
) -> Result<bool, String> {
    let mut persisted = load_state(storage_dir);
    if persisted.game_directory.is_some() || !state_has_ownership(&persisted) {
        return Ok(false);
    }
    if !game_dir.is_dir() {
        return Err("Invalid game directory for legacy Enhanced ownership adoption.".into());
    }
    if !persisted.created_files.is_empty()
        || !persisted.owned_backups.is_empty()
        || !persisted.installed_hashes.is_empty()
        || !persisted.original_backup_hashes.is_empty()
    {
        return Err(
            "Enhanced ownership state is unbound but already contains partial ownership metadata; refusing unsafe adoption."
                .into(),
        );
    }

    let mut tracked = BTreeSet::new();
    for value in persisted
        .installed_files
        .iter()
        .chain(persisted.dump_only_files.iter())
    {
        let (_, normalized) = normalized_legacy_path(value)?;
        tracked.insert(normalized);
    }
    if tracked.is_empty() {
        return Err(
            "Legacy Enhanced ownership cannot be verified because no managed files were recorded."
                .into(),
        );
    }

    let mut needs_enhanced = false;
    let mut needs_dump = false;
    for relative_text in &tracked {
        let (relative, _) = normalized_legacy_path(relative_text)?;
        if EXPECTED_ENHANCED_FILES.contains(&relative_text.as_str()) {
            needs_enhanced = true;
        } else if relative
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| UWP_DUMP_WHITELIST.contains(&name))
        {
            needs_dump = true;
        } else {
            return Err(format!(
                "Legacy Enhanced state contains a path outside the managed payload: {relative_text}."
            ));
        }
    }

    let enhanced_hashes = needs_enhanced
        .then(|| verified_cached_enhanced_hashes(storage_dir, enhanced_archive))
        .transpose()?
        .unwrap_or_default();
    let dump_hashes = if needs_dump {
        let source = dump_source.ok_or_else(|| {
            "Legacy Enhanced ownership cannot be verified because the original dump source is unavailable."
                .to_string()
        })?;
        verified_dump_member_hashes(source)?
    } else {
        BTreeMap::new()
    };

    let mut created = BTreeSet::new();
    let mut owned_backups = BTreeMap::new();
    let mut installed_hashes = BTreeMap::new();
    let mut original_backup_hashes = BTreeMap::new();
    let mut canonical_targets = HashSet::new();
    for relative_text in &tracked {
        let (relative, _) = normalized_legacy_path(relative_text)?;
        let expected = enhanced_hashes
            .get(relative_text)
            .or_else(|| dump_hashes.get(relative_text))
            .ok_or_else(|| {
                format!(
                    "Legacy Enhanced ownership cannot be verified because {relative_text} is absent from its provenance source."
                )
            })?;
        let target = contained_target(game_dir, &relative)?;
        let metadata = fs::symlink_metadata(&target).map_err(|_| {
            format!(
                "Legacy Enhanced ownership cannot be verified because the managed target is missing: {}.",
                target.display()
            )
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!(
                "Legacy Enhanced ownership cannot be verified because the managed target is unsafe: {}.",
                target.display()
            ));
        }
        let canonical_target = canonical_game_key(&target)?;
        if !canonical_targets.insert(canonical_target) {
            return Err(format!(
                "Legacy Enhanced state refers to the same target more than once: {relative_text}."
            ));
        }
        let actual = fs_ops::sha256_file(&target)?;
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(format!(
                "Legacy Enhanced target does not match its verified payload source: {}.",
                target.display()
            ));
        }
        installed_hashes.insert(relative_text.clone(), actual);

        match legacy_backup_for_target(game_dir, &target, relative_text)? {
            Some((backup, hash)) => {
                owned_backups.insert(relative_text.clone(), backup);
                original_backup_hashes.insert(relative_text.clone(), hash);
            }
            None if relative
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("BlackOps3.exe")) =>
            {
                return Err(
                    "Legacy Enhanced ownership cannot be adopted because BlackOps3.exe has no original backup."
                        .into(),
                );
            }
            None => {
                created.insert(relative_text.clone());
            }
        }
    }

    let mut dump_only = BTreeSet::new();
    for value in &persisted.dump_only_files {
        let (_, normalized) = normalized_legacy_path(value)?;
        dump_only.insert(normalized);
    }
    persisted.game_directory = Some(canonical_game_key(game_dir)?);
    persisted.installed_files = tracked.into_iter().collect();
    persisted.dump_only_files = dump_only.into_iter().collect();
    persisted.created_files = created.into_iter().collect();
    persisted.owned_backups = owned_backups;
    persisted.installed_hashes = installed_hashes;
    persisted.original_backup_hashes = original_backup_hashes;
    save_state(storage_dir, &persisted)?;
    Ok(true)
}

fn private_dir(parent: &Path, label: &str) -> Result<PathBuf, String> {
    for index in 1..1000 {
        let candidate = parent.join(format!(".patchops-{label}-{}-{index}", std::process::id()));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
    Err(format!("Could not allocate a private {label} directory."))
}

fn regular_file_without_links(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
}

fn allocate_owned_backup(game_dir: &Path, target: &Path) -> Result<(PathBuf, String), String> {
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Invalid Enhanced backup filename.".to_string())?;
    for index in 1..1000 {
        let backup = target.with_file_name(format!(
            "{name}.patchops.original-{}-{index}.bak",
            std::process::id()
        ));
        match fs::symlink_metadata(&backup) {
            Ok(_) => continue,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        let relative = backup
            .strip_prefix(game_dir)
            .map_err(|_| "Enhanced backup escaped the game directory.".to_string())?
            .to_path_buf();
        if !safe_relative(&relative) {
            return Err("Enhanced backup path was unsafe.".into());
        }
        return Ok((backup, relative_string(&relative)));
    }
    Err(format!(
        "Could not allocate a unique backup for {}.",
        target.display()
    ))
}

fn exact_owned_backup(
    game_dir: &Path,
    state: &PersistedState,
    relative_text: &str,
) -> Result<Option<PathBuf>, String> {
    let Some(stored) = state.owned_backups.get(relative_text) else {
        return Ok(None);
    };
    let relative = PathBuf::from(stored);
    if relative.as_os_str().is_empty() || !safe_relative(&relative) || stored == relative_text {
        return Err(format!(
            "Enhanced backup manifest is invalid for {relative_text}."
        ));
    }
    let backup = contained_target(game_dir, &relative)?;
    if !regular_file_without_links(&backup) {
        return Err(format!(
            "The exact PatchOpsIII backup for {relative_text} is missing or unsafe."
        ));
    }
    let expected = state
        .original_backup_hashes
        .get(relative_text)
        .ok_or_else(|| {
            format!(
                "No original-backup checksum is recorded for {relative_text}; refusing to restore it."
            )
        })?;
    let actual = fs_ops::sha256_file(&backup)?;
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(format!(
            "The exact PatchOpsIII backup for {relative_text} was modified or corrupted; refusing to restore it."
        ));
    }
    Ok(Some(backup))
}

fn stage_members(
    source_archive: Option<&Path>,
    members: &[PayloadMember],
    destination: &Path,
) -> Result<(), String> {
    let mut archive = source_archive
        .map(|path| {
            zip::ZipArchive::new(File::open(path).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())
        })
        .transpose()?;
    for member in members {
        if !safe_relative(&member.relative) {
            return Err(format!(
                "Refusing unsafe payload path: {}",
                member.relative.display()
            ));
        }
        let output = destination.join(&member.relative);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        match &member.source {
            MemberSource::File(source) => {
                fs::copy(source, &output).map_err(|error| error.to_string())?;
            }
            MemberSource::Zip(index) => {
                let mut entry = archive
                    .as_mut()
                    .ok_or_else(|| "Missing source archive.".to_string())?
                    .by_index(*index)
                    .map_err(|error| error.to_string())?;
                let mut output = File::create(&output).map_err(|error| error.to_string())?;
                io::copy(&mut entry, &mut output).map_err(|error| error.to_string())?;
            }
        }
    }
    Ok(())
}

fn relative_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn rollback_install(applied: &mut Vec<AppliedFile>) -> Result<(), String> {
    let mut errors = Vec::new();
    for file in applied.drain(..).rev() {
        if file.target.exists() {
            if let Err(error) = fs::remove_file(&file.target) {
                errors.push(format!("{}: {error}", file.target.display()));
                continue;
            }
        }
        let restore = file
            .rollback_copy
            .as_ref()
            .filter(|path| path.exists())
            .or_else(|| {
                file.new_permanent_backup
                    .as_ref()
                    .filter(|path| path.exists())
            });
        if let Some(restore) = restore {
            if let Err(error) = fs::rename(restore, &file.target) {
                errors.push(format!("{}: {error}", file.target.display()));
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn install_files(
    game_dir: &Path,
    enhanced_archive: &Path,
    dump_source: &Path,
    storage_dir: &Path,
) -> Result<(), String> {
    if !game_dir.is_dir() {
        return Err("Invalid game directory for Enhanced install.".into());
    }
    let game_key = canonical_game_key(game_dir)?;
    verify_cached_checksum(storage_dir, enhanced_archive)?;
    let enhanced_archive_hash = fs_ops::sha256_file(enhanced_archive)?;
    let dump = dump_members(dump_source)?;
    let enhanced = enhanced_members(enhanced_archive)?;
    let stage = private_dir(game_dir, "enhanced-stage")?;
    let rollback = match private_dir(game_dir, "enhanced-rollback") {
        Ok(path) => path,
        Err(error) => {
            let _ = fs::remove_dir_all(&stage);
            return Err(error);
        }
    };

    let mut applied = Vec::new();
    let result = (|| {
        stage_members(
            (!dump_source.is_dir()).then_some(dump_source),
            &dump,
            &stage,
        )?;
        stage_members(Some(enhanced_archive), &enhanced, &stage)?;
        if !fs_ops::sha256_file(enhanced_archive)?.eq_ignore_ascii_case(&enhanced_archive_hash) {
            return Err("Enhanced archive changed while it was being staged.".into());
        }

        let loaded = load_state(storage_dir);
        let mut persisted = if loaded.game_directory.as_deref() == Some(game_key.as_str()) {
            loaded
        } else if state_has_ownership(&loaded) {
            return Err(
                "Enhanced ownership state belongs to a different or unbound game directory; refusing to reuse it."
                    .into(),
            );
        } else {
            PersistedState::default()
        };
        persisted.game_directory = Some(game_key.clone());
        let prior_tracked: HashSet<String> = persisted.installed_files.iter().cloned().collect();
        let mut installed: BTreeSet<String> = persisted.installed_files.drain(..).collect();
        let mut dump_files: BTreeSet<String> = persisted.dump_only_files.drain(..).collect();
        let mut created: BTreeSet<String> = persisted.created_files.drain(..).collect();
        let mut payload = Vec::new();
        for member in &dump {
            let path = relative_string(&member.relative);
            dump_files.insert(path.clone());
            payload.push((member.relative.clone(), path));
        }
        for member in &enhanced {
            let path = relative_string(&member.relative);
            payload.push((member.relative.clone(), path));
        }

        for (relative, relative_text) in &payload {
            let target = contained_target(game_dir, relative)?;
            if prior_tracked.contains(relative_text)
                && persisted.owned_backups.contains_key(relative_text)
            {
                exact_owned_backup(game_dir, &persisted, relative_text)?;
            }
            if prior_tracked.contains(relative_text)
                && !persisted.owned_backups.contains_key(relative_text)
                && !created.contains(relative_text)
            {
                return Err(format!(
                    "Enhanced ownership data is incomplete for {}; refusing to overwrite it.",
                    target.display()
                ));
            }
            match fs::symlink_metadata(&target) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                    return Err(format!(
                        "Refusing to replace an unsafe Enhanced target: {}",
                        target.display()
                    ));
                }
                Ok(_) if prior_tracked.contains(relative_text) => {
                    if let Some(expected) = persisted.installed_hashes.get(relative_text) {
                        let actual = fs_ops::sha256_file(&target)?;
                        if !actual.eq_ignore_ascii_case(expected) {
                            return Err(format!(
                                "Enhanced target was modified since installation; refusing to overwrite {}.",
                                target.display()
                            ));
                        }
                    } else {
                        return Err(format!(
                            "Enhanced ownership data is incomplete for {}; refusing to overwrite it.",
                            target.display()
                        ));
                    }
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
            }
        }

        for (relative, relative_text) in payload {
            if !safe_relative(&relative) {
                return Err(format!("Refusing unsafe payload path: {relative_text}"));
            }
            let target = contained_target(game_dir, &relative)?;
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            let existed_before = regular_file_without_links(&target);
            let mut new_permanent_backup = None;
            if existed_before
                && !persisted.owned_backups.contains_key(&relative_text)
                && !created.contains(&relative_text)
            {
                let (backup, backup_relative) = allocate_owned_backup(game_dir, &target)?;
                fs::rename(&target, &backup).map_err(|error| error.to_string())?;
                persisted
                    .owned_backups
                    .insert(relative_text.clone(), backup_relative);
                new_permanent_backup = Some(backup);
            } else if !prior_tracked.contains(&relative_text) && !existed_before {
                created.insert(relative_text.clone());
            }

            applied.push(AppliedFile {
                target: target.clone(),
                rollback_copy: None,
                new_permanent_backup,
            });
            if let Some(backup) = applied
                .last()
                .and_then(|file| file.new_permanent_backup.as_ref())
            {
                persisted
                    .original_backup_hashes
                    .insert(relative_text.clone(), fs_ops::sha256_file(backup)?);
            }
            if target.is_file() {
                let path = rollback.join(&relative);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
                }
                fs::rename(&target, &path).map_err(|error| error.to_string())?;
                applied.last_mut().expect("applied entry").rollback_copy = Some(path);
            }
            fs::rename(stage.join(&relative), &target).map_err(|error| error.to_string())?;
            persisted
                .installed_hashes
                .insert(relative_text.clone(), fs_ops::sha256_file(&target)?);
            installed.insert(relative_text);
        }

        persisted.installed = !installed.is_empty();
        persisted.detected_at.get_or_insert_with(utc_timestamp);
        persisted.installed_files = installed.into_iter().collect();
        persisted.dump_only_files = dump_files.into_iter().collect();
        persisted.created_files = created.into_iter().collect();
        save_state(storage_dir, &persisted)?;
        Ok(())
    })();

    let mut remove_rollback = true;
    let result = match result {
        Ok(()) => Ok(()),
        Err(error) => match rollback_install(&mut applied) {
            Ok(()) => Err(error),
            Err(rollback_error) => {
                remove_rollback = false;
                Err(format!(
                    "{error} Rollback also failed: {rollback_error}. Recovery files were preserved at {}",
                    rollback.display()
                ))
            }
        },
    };
    let _ = fs::remove_dir_all(&stage);
    if remove_rollback {
        let _ = fs::remove_dir_all(&rollback);
    }
    result
}

fn apply_uninstall_plan(
    plan: PlannedUninstall,
    transaction: &Path,
    index: usize,
) -> Result<AppliedUninstall, String> {
    let displaced = if plan.target.exists() {
        let path = transaction.join(index.to_string());
        fs::rename(&plan.target, &path).map_err(|error| error.to_string())?;
        Some(path)
    } else {
        None
    };

    if let UninstallAction::Restore(backup) = &plan.action {
        if let Err(error) = fs::rename(backup, &plan.target) {
            return if let Some(displaced) = displaced {
                match fs::rename(&displaced, &plan.target) {
                    Ok(()) => Err(error.to_string()),
                    Err(rollback) => Err(format!(
                        "{error}; restoring the managed file also failed: {rollback}"
                    )),
                }
            } else {
                Err(error.to_string())
            };
        }
    }

    Ok(AppliedUninstall { plan, displaced })
}

fn rollback_uninstall(applied: &mut Vec<AppliedUninstall>) -> Result<(), String> {
    let mut errors = Vec::new();
    for item in applied.drain(..).rev() {
        if let UninstallAction::Restore(backup) = &item.plan.action {
            if item.plan.target.exists() {
                if let Err(error) = fs::rename(&item.plan.target, backup) {
                    errors.push(format!("{}: {error}", backup.display()));
                    continue;
                }
            }
        }
        if let Some(displaced) = item.displaced {
            if let Err(error) = fs::rename(displaced, &item.plan.target) {
                errors.push(format!("{}: {error}", item.plan.target.display()));
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

impl EnhancedUninstallTransaction {
    fn rollback(mut self) -> Result<(), String> {
        let mut errors = Vec::new();
        if let Err(error) = rollback_uninstall(&mut self.applied) {
            errors.push(error);
        }
        if let Err(error) = save_state(&self.storage_dir, &self.original_state) {
            errors.push(format!("ownership-state rollback failed: {error}"));
        }
        if errors.is_empty() {
            if let Some(transaction) = self.transaction {
                let _ = fs::remove_dir_all(transaction);
            }
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    fn commit(self) -> Result<(), String> {
        match self.transaction {
            Some(transaction) => fs::remove_dir_all(&transaction)
                .map_err(|error| format!("{}: {error}", transaction.display())),
            None => Ok(()),
        }
    }
}

fn begin_uninstall_files(
    game_dir: &Path,
    storage_dir: &Path,
) -> Result<EnhancedUninstallTransaction, String> {
    if !game_dir.is_dir() {
        return Err("Invalid game directory for Enhanced uninstall.".into());
    }
    let game_key = canonical_game_key(game_dir)?;
    let original_state = load_state(storage_dir);
    match original_state.game_directory.as_deref() {
        Some(bound) if bound == game_key => {}
        Some(_) => {
            return Err(
                "Enhanced ownership state belongs to a different game directory; refusing cleanup."
                    .into(),
            );
        }
        None if state_has_ownership(&original_state) => {
            return Err(
                "Enhanced ownership state is not bound to a game directory; refusing cleanup."
                    .into(),
            );
        }
        None => {}
    }
    let mut persisted = original_state.clone();
    let tracked: BTreeSet<String> = persisted.installed_files.iter().cloned().collect();
    if tracked.is_empty() {
        if persisted.installed || detect_install(game_dir) {
            return Err(
                "Enhanced files were detected without an ownership manifest; refusing unsafe cleanup."
                    .into(),
            );
        }
        return Ok(EnhancedUninstallTransaction {
            applied: Vec::new(),
            transaction: None,
            storage_dir: storage_dir.to_path_buf(),
            original_state,
        });
    }
    let created: HashSet<String> = persisted.created_files.iter().cloned().collect();
    let mut plans = Vec::new();

    for relative_text in tracked {
        let relative = PathBuf::from(&relative_text);
        if relative.as_os_str().is_empty() || !safe_relative(&relative) {
            return Err(format!(
                "Enhanced ownership manifest contains an unsafe path: {relative_text}."
            ));
        }
        let target = contained_target(game_dir, &relative)?;
        let backup = exact_owned_backup(game_dir, &persisted, &relative_text)?;
        let target_exists = match fs::symlink_metadata(&target) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => true,
            Ok(_) => {
                return Err(format!(
                    "Enhanced target is no longer a regular file; refusing cleanup: {}.",
                    target.display()
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.to_string()),
        };
        if target_exists {
            let expected = persisted
                .installed_hashes
                .get(&relative_text)
                .ok_or_else(|| {
                    format!(
                    "No managed checksum is recorded for {}; refusing to overwrite or remove it.",
                    target.display()
                )
                })?;
            let actual = fs_ops::sha256_file(&target)?;
            if !actual.eq_ignore_ascii_case(expected) {
                return Err(format!(
                    "Enhanced target was modified after installation; leaving it and its backup untouched: {}.",
                    target.display()
                ));
            }
        }

        match backup {
            Some(backup) => plans.push(PlannedUninstall {
                target,
                action: UninstallAction::Restore(backup),
            }),
            None if created.contains(&relative_text) => {
                if target_exists {
                    plans.push(PlannedUninstall {
                        target,
                        action: UninstallAction::Remove,
                    });
                }
            }
            None if !target_exists => {
                return Err(format!(
                    "The original file and exact PatchOpsIII backup are both missing for {relative_text}."
                ));
            }
            None => {
                return Err(format!(
                    "No exact PatchOpsIII-owned backup is recorded for {}; refusing unsafe cleanup.",
                    target.display()
                ));
            }
        }
    }

    let transaction = private_dir(game_dir, "enhanced-uninstall")?;
    let mut applied = Vec::new();
    let result = (|| {
        for (index, plan) in plans.into_iter().enumerate() {
            applied.push(apply_uninstall_plan(plan, &transaction, index)?);
        }
        persisted.installed = false;
        persisted.installed_files.clear();
        persisted.dump_only_files.clear();
        persisted.created_files.clear();
        persisted.owned_backups.clear();
        persisted.installed_hashes.clear();
        persisted.original_backup_hashes.clear();
        save_state(storage_dir, &persisted)
    })();

    match result {
        Ok(()) => Ok(EnhancedUninstallTransaction {
            applied,
            transaction: Some(transaction),
            storage_dir: storage_dir.to_path_buf(),
            original_state,
        }),
        Err(error) => match rollback_uninstall(&mut applied) {
            Ok(()) => {
                let state_error = save_state(storage_dir, &original_state).err();
                let _ = fs::remove_dir_all(&transaction);
                Err(match state_error {
                    Some(rollback) => {
                        format!("{error} Ownership-state rollback also failed: {rollback}")
                    }
                    None => error,
                })
            }
            Err(rollback) => Err(format!(
                "{error} Rollback also failed: {rollback}. Recovery files were preserved at {}",
                transaction.display()
            )),
        },
    }
}

fn uninstall_files(game_dir: &Path, storage_dir: &Path) -> Result<(), String> {
    begin_uninstall_files(game_dir, storage_dir)?.commit()
}

#[cfg(test)]
pub(crate) fn benchmark_record_cached_archive(
    storage_dir: &Path,
    archive: &Path,
) -> Result<(), String> {
    let filename = archive
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "Invalid Enhanced benchmark archive filename.".to_string())?;
    let mut checksums = BTreeMap::new();
    checksums.insert(filename.to_owned(), fs_ops::sha256_file(archive)?);
    save_checksums(storage_dir, &checksums)
}

#[cfg(test)]
pub(crate) fn benchmark_install_transaction(
    game_dir: &Path,
    enhanced_archive: &Path,
    dump_source: &Path,
    storage_dir: &Path,
) -> Result<(), String> {
    install_files(game_dir, enhanced_archive, dump_source, storage_dir)
}

#[cfg(test)]
pub(crate) fn benchmark_status_transaction(
    storage_dir: &Path,
    game_dir: &Path,
    current_launch_options: &str,
    dump_source: &Path,
) -> Result<EnhancedState, String> {
    let (state, stale_state_error) = status_at(
        storage_dir,
        Some(game_dir),
        Some(current_launch_options),
        dump_source.to_string_lossy().into_owned(),
    );
    match stale_state_error {
        Some(error) => Err(error),
        None => Ok(state),
    }
}

#[cfg(test)]
pub(crate) fn benchmark_uninstall_transaction(
    game_dir: &Path,
    storage_dir: &Path,
) -> Result<(), String> {
    uninstall_files(game_dir, storage_dir)
}

fn expand_home(path: &Path) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path.to_owned();
    };
    if text == "~" || text.starts_with("~/") || text.starts_with("~\\") {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            return PathBuf::from(home)
                .join(text.trim_start_matches('~').trim_start_matches(['/', '\\']));
        }
    }
    path.to_owned()
}

#[cfg(target_os = "linux")]
fn configure_linux(state: &AppState) -> Result<(), String> {
    steam::configure_bo3_enhanced_linux(state, None)
}

#[cfg(not(target_os = "linux"))]
fn configure_linux(_: &AppState) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "linux")]
fn cleanup_linux(state: &AppState) -> Result<(), String> {
    steam::cleanup_bo3_enhanced_linux(state)
}

#[cfg(not(target_os = "linux"))]
fn cleanup_linux(_: &AppState) -> Result<(), String> {
    Ok(())
}

pub fn install(state: &AppState, game_dir: &Path, dump_source: &Path) -> Result<(), String> {
    let dump_source = expand_home(dump_source);
    if dump_source.as_os_str().is_empty() || !dump_source.exists() {
        return Err("The selected dump source does not exist.".into());
    }
    if !validate_dump_source(&dump_source)? {
        return Err("The dump source is missing required files.".into());
    }
    let mut settings = state.load_settings();
    settings.enhanced_dump_source = Some(dump_source.display().to_string());
    state.save_settings(&settings)?;

    let cached_archive = state.mod_files_dir().join(ENHANCED_ARCHIVE_NAME);
    if adopt_legacy_state(
        game_dir,
        &cached_archive,
        Some(&dump_source),
        state.data_dir(),
    )? {
        state.log(
            "Info",
            "Adopted verified legacy BO3 Enhanced ownership before updating the cached release.",
        );
    }

    state.log("Info", "Fetching latest BO3 Enhanced release...");
    let archive = download_latest(state)?;
    state.log("Info", "Installing BO3 Enhanced files...");
    install_files(game_dir, &archive, &dump_source, state.data_dir())?;
    if let Err(error) = configure_linux(state) {
        let rollback = uninstall_files(game_dir, state.data_dir()).err();
        return Err(match rollback {
            Some(rollback) => format!(
                "Linux compatibility setup failed: {error} File rollback also failed: {rollback}"
            ),
            None => format!("Linux compatibility setup failed: {error}"),
        });
    }

    if let Some(executable) = exe::find_executable(game_dir) {
        let hash = fs_ops::sha256_file(&executable)?;
        exe::record_enhanced_hash(state, game_dir, &hash)?;
    }
    if let Err(error) = exe::write_variant(game_dir, exe::ENHANCED_EXE_ID) {
        state.log(
            "Warning",
            format!(
                "Enhanced was installed, but its EXE variant marker could not be saved: {error}"
            ),
        );
    }
    state.log("Success", "Installed BO3 Enhanced successfully.");
    Ok(())
}

pub fn uninstall(state: &AppState, game_dir: &Path) -> Result<(), String> {
    let dump_source = state
        .load_settings()
        .enhanced_dump_source
        .map(|source| expand_home(Path::new(&source)));
    let cached_archive = state.mod_files_dir().join(ENHANCED_ARCHIVE_NAME);
    if adopt_legacy_state(
        game_dir,
        &cached_archive,
        dump_source.as_deref(),
        state.data_dir(),
    )? {
        state.log("Info", "Adopted verified legacy BO3 Enhanced ownership.");
    }
    let transaction = begin_uninstall_files(game_dir, state.data_dir())?;
    if let Err(error) = cleanup_linux(state) {
        return Err(match transaction.rollback() {
            Ok(()) => error,
            Err(rollback) => format!(
                "Linux compatibility cleanup failed: {error} Enhanced file rollback also failed: {rollback}"
            ),
        });
    }
    transaction.commit()?;
    if let Err(error) = exe::write_variant(game_dir, "default") {
        state.log(
            "Warning",
            format!("Enhanced was removed, but its EXE variant marker could not be saved: {error}"),
        );
    }
    state.log("Success", "Uninstalled BO3 Enhanced successfully.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Write,
        sync::atomic::{AtomicUsize, Ordering},
    };
    use zip::write::SimpleFileOptions;

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "patchops-enhanced-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn write_zip(path: &Path, files: &[(&str, &[u8])]) {
        let file = File::create(path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        for (name, body) in files {
            archive
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            archive.write_all(body).unwrap();
        }
        archive.finish().unwrap();
    }

    fn enhanced_zip(path: &Path) {
        write_zip(
            path,
            &EXPECTED_ENHANCED_FILES
                .iter()
                .map(|name| (*name, b"enhanced".as_slice()))
                .collect::<Vec<_>>(),
        );
    }

    fn enhanced_zip_with_body(path: &Path, body: &'static [u8]) {
        write_zip(
            path,
            &EXPECTED_ENHANCED_FILES
                .iter()
                .map(|name| (*name, body))
                .collect::<Vec<_>>(),
        );
    }

    fn record_cached_checksum(storage: &Path, archive: &Path) {
        let mut checksums = BTreeMap::new();
        checksums.insert(
            archive.file_name().unwrap().to_string_lossy().into_owned(),
            fs_ops::sha256_file(archive).unwrap(),
        );
        save_checksums(storage, &checksums).unwrap();
    }

    #[test]
    fn adopts_verified_legacy_state_before_upgrade_and_restores_originals() {
        let root = temp_dir("legacy-adoption-roundtrip");
        let game = root.join("game");
        let storage = root.join("storage");
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(&storage).unwrap();

        let enhanced = root.join(ENHANCED_ARCHIVE_NAME);
        enhanced_zip_with_body(&enhanced, b"legacy enhanced");
        record_cached_checksum(&storage, &enhanced);
        let dump = root.join("dump.zip");
        write_zip(
            &dump,
            &[
                ("DUMP/BlackOps3.exe", b"legacy dump exe"),
                ("DUMP/MicrosoftGame.config", b"legacy dump config"),
                ("DUMP/GameChat2.dll", b"legacy dump dll"),
            ],
        );

        for name in EXPECTED_ENHANCED_FILES {
            fs::write(game.join(name), b"legacy enhanced").unwrap();
        }
        fs::write(game.join("BlackOps3.exe"), b"legacy dump exe").unwrap();
        fs::write(game.join("BlackOps3.exe.patchops.bak"), b"original exe").unwrap();
        fs::write(game.join("MicrosoftGame.config"), b"legacy dump config").unwrap();
        fs::write(game.join("MicrosoftGame.config.bak"), b"original config").unwrap();
        fs::write(game.join("GameChat2.dll"), b"legacy dump dll").unwrap();

        let mut legacy_files = EXPECTED_ENHANCED_FILES
            .iter()
            .map(|name| Value::String((*name).to_owned()))
            .collect::<Vec<_>>();
        legacy_files.extend([
            Value::String("BlackOps3.exe".into()),
            Value::String("MicrosoftGame.config".into()),
            Value::String("GameChat2.dll".into()),
        ]);
        let legacy = serde_json::json!({
            "installed": true,
            "detected_at": "2025-01-02T03:04:05Z",
            "installed_files": legacy_files,
            "dump_only_files": ["GameChat2.dll"],
            "legacy_custom": {"keep": true}
        });
        fs::write(
            state_path(&storage),
            serde_json::to_vec_pretty(&legacy).unwrap(),
        )
        .unwrap();

        assert!(adopt_legacy_state(&game, &enhanced, Some(&dump), &storage).unwrap());
        let adopted = load_state(&storage);
        assert!(state_belongs_to_game(&adopted, &game));
        assert_eq!(
            adopted.extra.get("legacy_custom"),
            Some(&serde_json::json!({"keep": true}))
        );
        assert!(adopted
            .created_files
            .contains(&"T7WSBootstrapper.dll".into()));
        assert!(adopted.created_files.contains(&"GameChat2.dll".into()));
        assert!(!adopted.created_files.contains(&"BlackOps3.exe".into()));
        assert_eq!(
            adopted.owned_backups.get("BlackOps3.exe"),
            Some(&"BlackOps3.exe.patchops.bak".into())
        );
        assert_eq!(
            adopted.owned_backups.get("MicrosoftGame.config"),
            Some(&"MicrosoftGame.config.bak".into())
        );
        assert_eq!(adopted.installed_hashes.len(), 7);
        assert_eq!(adopted.original_backup_hashes.len(), 2);

        enhanced_zip_with_body(&enhanced, b"current enhanced");
        record_cached_checksum(&storage, &enhanced);
        install_files(&game, &enhanced, &dump, &storage).unwrap();
        let upgraded = load_state(&storage);
        for name in EXPECTED_ENHANCED_FILES {
            assert!(upgraded.created_files.contains(&name.to_owned()));
            assert!(!upgraded.owned_backups.contains_key(name));
            assert_eq!(fs::read(game.join(name)).unwrap(), b"current enhanced");
        }

        uninstall_files(&game, &storage).unwrap();
        assert_eq!(
            fs::read(game.join("BlackOps3.exe")).unwrap(),
            b"original exe"
        );
        assert_eq!(
            fs::read(game.join("MicrosoftGame.config")).unwrap(),
            b"original config"
        );
        assert!(!game.join("GameChat2.dll").exists());
        assert!(EXPECTED_ENHANCED_FILES
            .iter()
            .all(|name| !game.join(name).exists()));
        assert_eq!(
            load_state(&storage).extra.get("legacy_custom"),
            Some(&serde_json::json!({"keep": true}))
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_adoption_requires_a_verified_cached_archive_without_mutating_state() {
        let root = temp_dir("legacy-cache-provenance");
        let game = root.join("game");
        let storage = root.join("storage");
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(&storage).unwrap();
        let enhanced = root.join(ENHANCED_ARCHIVE_NAME);
        enhanced_zip(&enhanced);
        for name in EXPECTED_ENHANCED_FILES {
            fs::write(game.join(name), b"enhanced").unwrap();
        }
        let legacy = serde_json::json!({
            "installed": true,
            "installed_files": EXPECTED_ENHANCED_FILES,
            "legacy_custom": "unchanged"
        });
        let original = serde_json::to_vec_pretty(&legacy).unwrap();
        fs::write(state_path(&storage), &original).unwrap();

        let error = adopt_legacy_state(&game, &enhanced, None, &storage).unwrap_err();
        assert!(error.contains("no recorded SHA-256 checksum"));
        assert_eq!(fs::read(state_path(&storage)).unwrap(), original);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_marker_without_tracked_files_remains_unowned() {
        let root = temp_dir("legacy-marker-only");
        let game = root.join("game");
        let storage = root.join("storage");
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(&storage).unwrap();
        for name in EXPECTED_ENHANCED_FILES {
            fs::write(game.join(name), b"enhanced").unwrap();
        }
        let legacy = serde_json::json!({
            "installed": true,
            "legacy_custom": "unchanged"
        });
        let original = serde_json::to_vec_pretty(&legacy).unwrap();
        fs::write(state_path(&storage), &original).unwrap();

        let error = adopt_legacy_state(&game, &root.join(ENHANCED_ARCHIVE_NAME), None, &storage)
            .unwrap_err();
        assert!(error.contains("no managed files were recorded"));
        assert_eq!(fs::read(state_path(&storage)).unwrap(), original);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_adoption_rejects_dump_mismatch_without_mutating_state() {
        let root = temp_dir("legacy-dump-mismatch");
        let game = root.join("game");
        let storage = root.join("storage");
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(&storage).unwrap();
        fs::write(game.join("BlackOps3.exe"), b"modified dump exe").unwrap();
        fs::write(game.join("BlackOps3.exe.patchops.bak"), b"original exe").unwrap();
        let dump = root.join("dump.zip");
        write_zip(
            &dump,
            &[
                ("DUMP/BlackOps3.exe", b"verified dump exe"),
                ("DUMP/MicrosoftGame.config", b"verified config"),
            ],
        );
        let legacy = serde_json::json!({
            "installed": false,
            "dump_only_files": ["BlackOps3.exe"],
            "legacy_custom": 42
        });
        let original = serde_json::to_vec_pretty(&legacy).unwrap();
        fs::write(state_path(&storage), &original).unwrap();

        let error = adopt_legacy_state(
            &game,
            &root.join(ENHANCED_ARCHIVE_NAME),
            Some(&dump),
            &storage,
        )
        .unwrap_err();
        assert!(error.contains("does not match its verified payload source"));
        assert_eq!(fs::read(state_path(&storage)).unwrap(), original);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_adoption_rejects_missing_executable_backup_and_unsafe_backup() {
        let root = temp_dir("legacy-backup-preflight");
        let game = root.join("game");
        let storage = root.join("storage");
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(&storage).unwrap();
        fs::write(game.join("BlackOps3.exe"), b"dump exe").unwrap();
        let dump = root.join("dump.zip");
        write_zip(
            &dump,
            &[
                ("DUMP/BlackOps3.exe", b"dump exe"),
                ("DUMP/MicrosoftGame.config", b"config"),
            ],
        );
        let legacy = serde_json::json!({
            "installed": false,
            "dump_only_files": ["BlackOps3.exe"]
        });
        let original = serde_json::to_vec_pretty(&legacy).unwrap();
        fs::write(state_path(&storage), &original).unwrap();

        let error = adopt_legacy_state(
            &game,
            &root.join(ENHANCED_ARCHIVE_NAME),
            Some(&dump),
            &storage,
        )
        .unwrap_err();
        assert!(error.contains("BlackOps3.exe has no original backup"));
        assert_eq!(fs::read(state_path(&storage)).unwrap(), original);

        fs::create_dir(game.join("BlackOps3.exe.patchops.bak")).unwrap();
        let error = adopt_legacy_state(
            &game,
            &root.join(ENHANCED_ARCHIVE_NAME),
            Some(&dump),
            &storage,
        )
        .unwrap_err();
        assert!(error.contains("backup is not a regular file"));
        assert_eq!(fs::read(state_path(&storage)).unwrap(), original);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn accepts_only_expected_enhanced_release_urls() {
        assert!(valid_enhanced_asset_url(
            "https://github.com/shiversoftdev/BO3Enhanced/releases/download/v1.15/BO3Enhanced.zip"
        ));
        assert!(!valid_enhanced_asset_url(
            "http://github.com/shiversoftdev/BO3Enhanced/releases/download/v1.15/BO3Enhanced.zip"
        ));
        assert!(!valid_enhanced_asset_url(
            "https://github.com.evil.invalid/shiversoftdev/BO3Enhanced/releases/download/v1.15/BO3Enhanced.zip"
        ));
        assert!(!valid_enhanced_asset_url(
            "https://github.com/other/BO3Enhanced/releases/download/v1.15/BO3Enhanced.zip"
        ));
        assert!(!valid_enhanced_asset_url(
            "https://github.com/shiversoftdev/BO3Enhanced/releases/download/v1.15/BO3Enhanced.7z"
        ));
    }

    #[test]
    fn rejects_archive_path_traversal() {
        let root = temp_dir("traversal");
        let archive = root.join("dump.zip");
        write_zip(
            &archive,
            &[
                ("DUMP/BlackOps3.exe", b"exe"),
                ("DUMP/MicrosoftGame.config", b"config"),
                ("../GameChat2.dll", b"escape"),
            ],
        );
        assert!(dump_members(&archive).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn combined_uninstall_restores_every_dump_file() {
        let root = temp_dir("roundtrip");
        let game = root.join("game");
        let storage = root.join("storage");
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(&storage).unwrap();
        fs::write(game.join("BlackOps3.exe"), b"original exe").unwrap();
        fs::write(game.join("MicrosoftGame.config"), b"original config").unwrap();

        let enhanced = root.join(ENHANCED_ARCHIVE_NAME);
        enhanced_zip(&enhanced);
        let dump = root.join("dump.zip");
        write_zip(
            &dump,
            &[
                ("DUMP/BlackOps3.exe", b"dump exe"),
                ("DUMP/MicrosoftGame.config", b"dump config"),
                ("DUMP/GameChat2.dll", b"dump dll"),
            ],
        );

        install_files(&game, &enhanced, &dump, &storage).unwrap();
        assert_eq!(fs::read(game.join("BlackOps3.exe")).unwrap(), b"dump exe");
        assert!(game.join("GameChat2.dll").is_file());
        uninstall_files(&game, &storage).unwrap();
        assert_eq!(
            fs::read(game.join("BlackOps3.exe")).unwrap(),
            b"original exe"
        );
        assert_eq!(
            fs::read(game.join("MicrosoftGame.config")).unwrap(),
            b"original config"
        );
        assert!(!game.join("GameChat2.dll").exists());
        assert!(EXPECTED_ENHANCED_FILES
            .iter()
            .all(|name| !game.join(name).exists()));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn uninstall_restores_exact_owned_backup_and_leaves_stale_backup_untouched() {
        let root = temp_dir("owned-backup");
        let game = root.join("game");
        let storage = root.join("storage");
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(&storage).unwrap();
        fs::write(game.join("BlackOps3.exe"), b"exact original").unwrap();
        fs::write(game.join("MicrosoftGame.config"), b"original config").unwrap();
        let stale = game.join("BlackOps3.exe.patchops.bak");
        fs::write(&stale, b"stale backup").unwrap();

        let enhanced = root.join(ENHANCED_ARCHIVE_NAME);
        enhanced_zip(&enhanced);
        let dump = root.join("dump.zip");
        write_zip(
            &dump,
            &[
                ("DUMP/BlackOps3.exe", b"dump exe"),
                ("DUMP/MicrosoftGame.config", b"dump config"),
            ],
        );

        install_files(&game, &enhanced, &dump, &storage).unwrap();
        let state = load_state(&storage);
        let owned = state.owned_backups.get("BlackOps3.exe").unwrap();
        assert_ne!(owned, "BlackOps3.exe.patchops.bak");
        assert_eq!(fs::read(&stale).unwrap(), b"stale backup");

        uninstall_files(&game, &storage).unwrap();
        assert_eq!(
            fs::read(game.join("BlackOps3.exe")).unwrap(),
            b"exact original"
        );
        assert_eq!(fs::read(&stale).unwrap(), b"stale backup");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn uninstall_refuses_modified_managed_file_without_consuming_backup() {
        let root = temp_dir("modified-conflict");
        let game = root.join("game");
        let storage = root.join("storage");
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(&storage).unwrap();
        fs::write(game.join("BlackOps3.exe"), b"exact original").unwrap();
        fs::write(game.join("MicrosoftGame.config"), b"original config").unwrap();

        let enhanced = root.join(ENHANCED_ARCHIVE_NAME);
        enhanced_zip(&enhanced);
        let dump = root.join("dump.zip");
        write_zip(
            &dump,
            &[
                ("DUMP/BlackOps3.exe", b"dump exe"),
                ("DUMP/MicrosoftGame.config", b"dump config"),
            ],
        );

        install_files(&game, &enhanced, &dump, &storage).unwrap();
        let state = load_state(&storage);
        let backup = game.join(state.owned_backups.get("BlackOps3.exe").unwrap());
        fs::write(game.join("BlackOps3.exe"), b"user modified").unwrap();

        let error = uninstall_files(&game, &storage).unwrap_err();
        assert!(error.contains("modified after installation"));
        assert_eq!(
            fs::read(game.join("BlackOps3.exe")).unwrap(),
            b"user modified"
        );
        assert_eq!(fs::read(&backup).unwrap(), b"exact original");
        assert!(load_state(&storage).installed);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deferred_uninstall_can_restore_files_and_ownership_state() {
        let root = temp_dir("deferred-uninstall");
        let game = root.join("game");
        let storage = root.join("storage");
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(&storage).unwrap();
        fs::write(game.join("BlackOps3.exe"), b"original exe").unwrap();
        fs::write(game.join("MicrosoftGame.config"), b"original config").unwrap();
        let enhanced = root.join(ENHANCED_ARCHIVE_NAME);
        enhanced_zip(&enhanced);
        let dump = root.join("dump.zip");
        write_zip(
            &dump,
            &[
                ("DUMP/BlackOps3.exe", b"dump exe"),
                ("DUMP/MicrosoftGame.config", b"dump config"),
            ],
        );

        install_files(&game, &enhanced, &dump, &storage).unwrap();
        let transaction = begin_uninstall_files(&game, &storage).unwrap();
        assert_eq!(
            fs::read(game.join("BlackOps3.exe")).unwrap(),
            b"original exe"
        );
        assert!(!load_state(&storage).installed);

        transaction.rollback().unwrap();
        assert_eq!(fs::read(game.join("BlackOps3.exe")).unwrap(), b"dump exe");
        let state = load_state(&storage);
        assert!(state.installed);
        assert!(state.owned_backups.contains_key("BlackOps3.exe"));
        uninstall_files(&game, &storage).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ownership_state_is_bound_to_one_canonical_game_directory() {
        let root = temp_dir("game-binding");
        let game_a = root.join("game-a");
        let game_b = root.join("game-b");
        let storage = root.join("storage");
        for game in [&game_a, &game_b] {
            fs::create_dir_all(game).unwrap();
            fs::write(game.join("BlackOps3.exe"), b"original exe").unwrap();
            fs::write(game.join("MicrosoftGame.config"), b"original config").unwrap();
        }
        fs::create_dir_all(&storage).unwrap();
        let enhanced = root.join(ENHANCED_ARCHIVE_NAME);
        enhanced_zip(&enhanced);
        let dump = root.join("dump.zip");
        write_zip(
            &dump,
            &[
                ("DUMP/BlackOps3.exe", b"dump exe"),
                ("DUMP/MicrosoftGame.config", b"dump config"),
            ],
        );

        install_files(&game_a, &enhanced, &dump, &storage).unwrap();
        let state = load_state(&storage);
        assert!(state_belongs_to_game(&state, &game_a));
        assert!(!state_belongs_to_game(&state, &game_b));

        assert!(install_files(&game_b, &enhanced, &dump, &storage)
            .unwrap_err()
            .contains("different or unbound game directory"));
        let error = match begin_uninstall_files(&game_b, &storage) {
            Err(error) => error,
            Ok(_) => panic!("uninstall unexpectedly accepted another game directory"),
        };
        assert!(error.contains("different game directory"));
        assert_eq!(fs::read(game_a.join("BlackOps3.exe")).unwrap(), b"dump exe");
        assert_eq!(
            fs::read(game_b.join("BlackOps3.exe")).unwrap(),
            b"original exe"
        );
        uninstall_files(&game_a, &storage).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn partial_owned_install_remains_eligible_for_cleanup() {
        let root = temp_dir("partial-owned-install");
        let game_a = root.join("game-a");
        let game_b = root.join("game-b");
        fs::create_dir_all(&game_a).unwrap();
        fs::create_dir_all(&game_b).unwrap();

        let state = PersistedState {
            game_directory: Some(canonical_game_key(&game_a).unwrap()),
            installed: false,
            installed_files: vec!["T7WSBootstrapper.dll".into()],
            ..PersistedState::default()
        };
        assert!(state_has_owned_install(&state, &game_a));
        assert!(!state_has_owned_install(&state, &game_b));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn uninstall_refuses_a_changed_exact_original_backup() {
        let root = temp_dir("changed-original-backup");
        let game = root.join("game");
        let storage = root.join("storage");
        fs::create_dir_all(&game).unwrap();
        fs::create_dir_all(&storage).unwrap();
        fs::write(game.join("BlackOps3.exe"), b"original exe").unwrap();
        fs::write(game.join("MicrosoftGame.config"), b"original config").unwrap();
        let enhanced = root.join(ENHANCED_ARCHIVE_NAME);
        enhanced_zip(&enhanced);
        let dump = root.join("dump.zip");
        write_zip(
            &dump,
            &[
                ("DUMP/BlackOps3.exe", b"dump exe"),
                ("DUMP/MicrosoftGame.config", b"dump config"),
            ],
        );

        install_files(&game, &enhanced, &dump, &storage).unwrap();
        let state = load_state(&storage);
        assert_eq!(
            state.original_backup_hashes.len(),
            state.owned_backups.len()
        );
        let backup = game.join(state.owned_backups.get("BlackOps3.exe").unwrap());
        fs::write(&backup, b"corrupt backup").unwrap();

        let error = uninstall_files(&game, &storage).unwrap_err();
        assert!(error.contains("modified or corrupted"));
        assert_eq!(fs::read(game.join("BlackOps3.exe")).unwrap(), b"dump exe");
        assert_eq!(fs::read(backup).unwrap(), b"corrupt backup");
        assert!(load_state(&storage).installed);
        fs::remove_dir_all(root).unwrap();
    }
}
