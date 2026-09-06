use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    app::AppState,
    fs_ops::{self, remove_file_if_present, Snapshot},
    models::T7State,
};

const CURRENT_RELEASE_API: &str = "https://api.github.com/repos/Scroptss/T7Patch/releases/latest";
const CURRENT_ARCHIVE_URL: &str = "https://github.com/Scroptss/T7Patch/releases/latest/download/Linux.Steamdeck.and.Manual.Windows.Install.zip";
const LEGACY_RELEASE_API: &str =
    "https://api.github.com/repos/shiversoftdev/t7patch/releases/tags/Current";
const COMPATIBLE_ARCHIVE_URL: &str = "https://github.com/shiversoftdev/t7patch/releases/download/Current/Linux.Steamdeck.and.Manual.Windows.Install.zip";
const LPC_ARCHIVE_URL: &str =
    "https://github.com/shiversoftdev/t7patch/releases/download/Current/LPC.1.zip";
const PATCH_ARCHIVE_NAME: &str = "Linux.Steamdeck.and.Manual.Windows.Install.zip";
const LPC_ARCHIVE_NAME: &str = "LPC.1.zip";
const COMPATIBLE_ARCHIVE_SHA256: &str =
    "388491c01643b0abd51f13290d0c36dec9737fcfbb0ed5e2f5ef6804e1b73dcb";
const LPC_ARCHIVE_SHA256: &str = "c94855841a233c9dcdea2799c12693fed8554d0e59fe68257ae66ffbdf2fa58b";
const T7_MARKERS: [&str; 2] = ["t7patchloader.dll", "t7patch.dll"];
const T7_REQUIRED_FILES: [&str; 2] = ["t7patch.dll", "t7patchloader.dll"];
const T7_GAME_FILES: [&str; 6] = [
    "t7patch.dll",
    "t7patch.conf",
    "discord_game_sdk.dll",
    "dsound.dll",
    "t7patchloader.dll",
    "zbr2.dll",
];
const COMPATIBLE_ONLY_FILES: [&str; 2] = ["discord_game_sdk.dll", "zbr2.dll"];
const MAX_RELEASE_JSON_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const OWNERSHIP_DIRECTORY: &str = ".patchopsiii";
const OWNERSHIP_BACKUP_DIRECTORY: &str = "t7-backups";
const OWNERSHIP_MANIFEST: &str = "t7-ownership.json";
const OWNERSHIP_VERSION: u32 = 1;
const UNTRACKED_UNINSTALL_ERROR: &str = "T7 ownership manifest was not found; refusing to remove files from a legacy or untracked install. PatchOpsIII cannot safely infer ownership from .bak or .patchops.bak filenames";
// Exact member hashes from the verified release archives shipped by
// origin/main at migration time: current archive SHA-256
// 57e5cb5abba203382a0549e46c431ed34c4ce6c7aa3f4cdd8003ef0325d3dce5,
// plus the pinned compatible/LPC archives declared above. They are used only
// to recognize legacy installs; new downloads still use their archive digest.
const KNOWN_CURRENT_FILES: [(&str, &str); 3] = [
    (
        "dsound.dll",
        "fe05893af78173bdae36bd8202f8e69739d7c1d16c562941eef25a0b5e7c57d2",
    ),
    (
        "t7patch.dll",
        "bc4c9e4edb895f237477a9074552a8186e69b7d36db4886d983dc13c143645c6",
    ),
    (
        "t7patchloader.dll",
        "17614526688ee9d8c85fe89ade78700687ef19d04885dfc133e7ba587fdda811",
    ),
];
// Historical Scroptss archives that this application's legacy dynamic
// `releases/latest` installer could have installed before v3.04 became
// current. The release API supplied the archive digests and these member
// hashes were derived from those exact official archives.
// v3.03 archive SHA-256: 609546e72dd4f9c5649b8197cfdf71dcee47318b438754035e947b5586ee9493.
const KNOWN_SCROPTSS_V3_03_FILES: [(&str, &str); 3] = [
    (
        "dsound.dll",
        "fe05893af78173bdae36bd8202f8e69739d7c1d16c562941eef25a0b5e7c57d2",
    ),
    (
        "t7patch.dll",
        "935cb98a4cc7d33ad023444d403be25f6da3865f028975b65c69c7a8884d4078",
    ),
    (
        "t7patchloader.dll",
        "17614526688ee9d8c85fe89ade78700687ef19d04885dfc133e7ba587fdda811",
    ),
];
// v3.02 archive SHA-256: e34411e70d3c99773445ab758851304d7f6a80867a987ec7f4a1a1df72b11bb1.
const KNOWN_SCROPTSS_V3_02_FILES: [(&str, &str); 3] = [
    (
        "dsound.dll",
        "fe05893af78173bdae36bd8202f8e69739d7c1d16c562941eef25a0b5e7c57d2",
    ),
    (
        "t7patch.dll",
        "4c582171dfa4409440f31141b4b0fa48f297fcca98b3e36917960bb55d3a1542",
    ),
    (
        "t7patchloader.dll",
        "1ef71784bd654afb45ec58dde5f7e454ea91351faf8c4977d65affa236e1b7ba",
    ),
];
// v2.03 archive SHA-256: 5e6159c264a1d6f2c4425fafdaf2ca382ef49abf4b1915d7c7d183bcb5513440.
const KNOWN_SHIVERSOFT_V2_03_FILES: [(&str, &str); 5] = [
    (
        "discord_game_sdk.dll",
        "527768710ddb0953fce5eb1700c2566b6451135d76f1d0610b63907cd5ba94c5",
    ),
    (
        "dsound.dll",
        "fe05893af78173bdae36bd8202f8e69739d7c1d16c562941eef25a0b5e7c57d2",
    ),
    (
        "t7patch.dll",
        "21dd4bac65d467b3f890ec4f7d77d73944646803e0c95fca8c1ee5b6ba71f92d",
    ),
    (
        "t7patchloader.dll",
        "a140a38d7cb451c2ccf44e693921b5305cb0fbdccfde05913efcb40e5b83509c",
    ),
    (
        "zbr2.dll",
        "065fe09942b8005099319feee7734404872d5d0e9a2a1ee6de4877de4c030ede",
    ),
];
const KNOWN_COMPATIBLE_FILES: [(&str, &str); 5] = [
    (
        "discord_game_sdk.dll",
        "527768710ddb0953fce5eb1700c2566b6451135d76f1d0610b63907cd5ba94c5",
    ),
    (
        "dsound.dll",
        "fe05893af78173bdae36bd8202f8e69739d7c1d16c562941eef25a0b5e7c57d2",
    ),
    (
        "t7patch.dll",
        "aaaaaaea2e889db7c4453fa355c86f17bebf74b6822ea7a3701d420dae481d3f",
    ),
    (
        "t7patchloader.dll",
        "4df88503030a671d0804012ea585e6f6b01b70bec74cf3ddc42b9f4c73fe73cd",
    ),
    (
        "zbr2.dll",
        "065fe09942b8005099319feee7734404872d5d0e9a2a1ee6de4877de4c030ede",
    ),
];
const KNOWN_LPC_FILES: [(&str, &str); 13] = [
    (
        "bp_core_ffotd_tu32_593.ff",
        "729ff5a20e81085be6884a131cf68300b56f141dc0e10a1be56d1b27e96f2d25",
    ),
    (
        "core_ffotd_tu32_593.ff",
        "beba007a3474abc3d59bc9048da222bdd9757401f59657683e5f99cf05b6befb",
    ),
    (
        "ea_core_ffotd_tu32_593.ff",
        "b2e03032eefee5b20706009b0b76bc3dd187e200a65de37144746799621229bb",
    ),
    (
        "en_core_ffotd_tu32_593.ff",
        "218d1ae315a222ef13483d43b4f92f2ec1f974556d8483746f9c2046883f68b2",
    ),
    (
        "es_core_ffotd_tu32_593.ff",
        "ed6db64b5444fd4990d70a642d4b088ac3cee09d09c690c1a45d6258a035ba2f",
    ),
    (
        "fr_core_ffotd_tu32_593.ff",
        "36403f40c2e81eb4e98a9ff49591c9afecb8f7390e35d55d6779a213be1aba93",
    ),
    (
        "ge_core_ffotd_tu32_593.ff",
        "a132a000248646b50a8919a1f2f10455d51102efba3e1068d9f884a81d4af54a",
    ),
    (
        "it_core_ffotd_tu32_593.ff",
        "e3cf42fbdef69b2ebe996495bafecc367cc0c9b434b2b7e08cf473b337c60c80",
    ),
    (
        "ja_core_ffotd_tu32_593.ff",
        "f1d7acb16e7634bea8cac749c7f115b7605300d5c9d9fd63256a91ce719a963d",
    ),
    (
        "po_core_ffotd_tu32_593.ff",
        "cd189d4ad235aa75cc3f32228e2b7782f6ee2decd7efa37c41f5d580c708932d",
    ),
    (
        "ru_core_ffotd_tu32_593.ff",
        "ce96501ac3e5a5cbd50c493ecc3110ffad8a8208fe33c5c256bb56b30ed65fb7",
    ),
    (
        "sc_core_ffotd_tu32_593.ff",
        "f04645efe12219d6fef3f706e8cd01afcbf000de95bfb0107e385362a5dc5ae6",
    ),
    (
        "tc_core_ffotd_tu32_593.ff",
        "031066009975c5c6c1d1d68a54f18855077287eadee556482a8a45e060d08b2f",
    ),
];
static STAGE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PatchProfile {
    Current,
    Compatible,
}

#[derive(Clone, Copy)]
struct AssetSpec {
    name: &'static str,
    url: &'static str,
    pinned_sha256: Option<&'static str>,
}

const CURRENT_ARCHIVE: AssetSpec = AssetSpec {
    name: PATCH_ARCHIVE_NAME,
    url: CURRENT_ARCHIVE_URL,
    pinned_sha256: None,
};
const COMPATIBLE_ARCHIVE: AssetSpec = AssetSpec {
    name: PATCH_ARCHIVE_NAME,
    url: COMPATIBLE_ARCHIVE_URL,
    pinned_sha256: Some(COMPATIBLE_ARCHIVE_SHA256),
};
const LPC_ARCHIVE: AssetSpec = AssetSpec {
    name: LPC_ARCHIVE_NAME,
    url: LPC_ARCHIVE_URL,
    pinned_sha256: Some(LPC_ARCHIVE_SHA256),
};

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubAsset {
    #[serde(default)]
    name: String,
    #[serde(default)]
    digest: String,
}

#[derive(Clone, Debug)]
struct InstallOperation {
    source: PathBuf,
    relative: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct OwnershipManifest {
    version: u32,
    entries: Vec<ManagedFile>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ManagedFile {
    path: PathBuf,
    managed_sha256: String,
    original: OriginalFile,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum OriginalFile {
    Created,
    Overwritten {
        backup_path: PathBuf,
        sha256: String,
        #[serde(default)]
        active_was_missing: bool,
    },
}

/// Read T7 Patch markers and `t7patch.conf`. `executable_profile` is the
/// already-validated EXE profile (`current`, `compatible`, or `enhanced`) and
/// avoids hashing the large game executable again during ordinary state reads.
pub fn status(game_dir: Option<&Path>, executable_profile: Option<&str>) -> T7State {
    let Some(game_dir) = game_dir.filter(|path| !path.as_os_str().is_empty() && path.is_dir())
    else {
        return empty_state("Unknown");
    };

    let conf = game_dir.join("t7patch.conf");
    let conf_exists = regular_file(&conf);
    let mut result = if conf_exists {
        fs::read(&conf)
            .map(|body| parse_conf(&String::from_utf8_lossy(&body)))
            .unwrap_or_else(|_| empty_state("Unknown"))
    } else {
        empty_state("Unknown")
    };
    result.installed = T7_MARKERS
        .iter()
        .any(|name| regular_file(&game_dir.join(name)));
    result.conf_exists = conf_exists;
    result.mode = mode_for_profile(game_dir, executable_profile).into();
    result
}

/// Update one or more T7 config values while preserving all unrelated lines.
pub fn configure(
    state: &AppState,
    game_dir: &Path,
    gamertag: Option<&str>,
    color_code: &str,
    network_password: Option<&str>,
    friends_only: Option<bool>,
) -> Result<(), String> {
    let game_dir = validate_game_dir(game_dir)?;
    let conf = safe_target(&game_dir, Path::new("t7patch.conf"))?;
    if !regular_file(&conf) {
        return Err(
            "t7patch.conf was not found. Install T7 Patch before updating settings.".into(),
        );
    }

    let name = gamertag
        .map(|value| validate_gamertag(value, color_code))
        .transpose()?;
    let password = network_password.map(validate_config_value).transpose()?;
    let contents = fs::read_to_string(&conf).map_err(|error| error.to_string())?;
    let updated = update_conf(
        &contents,
        name.as_deref(),
        password.as_deref(),
        friends_only,
    );
    let stage = create_stage(state, "t7-config")?;
    let result =
        write_config_with_ownership(&game_dir, updated.as_bytes(), &stage.join("rollback"));
    if result.is_ok() {
        let _ = fs::remove_dir_all(&stage);
    } else {
        state.log(
            "Warning",
            format!(
                "T7 configuration recovery files were retained at {}.",
                stage.display()
            ),
        );
    }
    result?;

    if name.is_some() {
        state.log("Success", "Updated playername in t7patch.conf.");
    }
    if let Some(password) = password {
        state.log(
            "Success",
            if password.is_empty() {
                "Cleared network password in t7patch.conf."
            } else {
                "Updated network password in t7patch.conf."
            },
        );
    }
    if let Some(enabled) = friends_only {
        state.log(
            "Success",
            format!(
                "Updated isfriendsonly in t7patch.conf to {}.",
                if enabled { "On" } else { "Off" }
            ),
        );
    }
    Ok(())
}

/// Install the T7 archive matching the active executable profile together with
/// the pinned legacy LPC payload. Everything is staged and validated before
/// the game directory is changed, then restored from a snapshot on failure.
pub fn install(state: &AppState, game_dir: &Path, executable_profile: &str) -> Result<(), String> {
    let profile = patch_profile(executable_profile)?;
    let game_dir = validate_game_dir(game_dir)?;
    let stage = create_stage(state, "t7")?;

    let result = (|| {
        let legacy_release = fetch_release_digests(LEGACY_RELEASE_API);
        let current_release = if profile == PatchProfile::Current {
            Some(fetch_release_digests(CURRENT_RELEASE_API))
        } else {
            None
        };
        let patch_spec = match profile {
            PatchProfile::Current => CURRENT_ARCHIVE,
            PatchProfile::Compatible => COMPATIBLE_ARCHIVE,
        };
        let patch_release = current_release.as_ref().unwrap_or(&legacy_release);
        let patch_digest = trusted_digest(patch_spec, patch_release, state)?;
        let lpc_digest = trusted_digest(LPC_ARCHIVE, &legacy_release, state)?;

        state.log(
            "Info",
            format!(
                "Detected {}. Installing {}...",
                if profile == PatchProfile::Compatible {
                    "Compatible EXE"
                } else {
                    "Current EXE"
                },
                if profile == PatchProfile::Compatible {
                    "T7 Patch 2.04"
                } else {
                    "T7 Patch Scroptss/T7Patch"
                }
            ),
        );

        let patch_archive = stage.join("T7Patch.zip");
        fs_ops::download(
            patch_spec.url,
            &patch_archive,
            Some(&patch_digest),
            MAX_ARCHIVE_BYTES,
        )?;
        let patch_extract = stage.join("patch");
        fs_ops::extract_zip(&patch_archive, &patch_extract)?;
        let patch_source = find_patch_source(&patch_extract)?;
        let mut patch_files = collect_files(&patch_source)?;
        patch_files.retain(|(_, relative)| {
            relative.components().count() == 1
                && relative
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        T7_GAME_FILES
                            .iter()
                            .any(|allowed| name.eq_ignore_ascii_case(allowed))
                    })
        });
        if profile == PatchProfile::Current {
            patch_files.retain(|(_, relative)| {
                relative
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_none_or(|name| {
                        !COMPATIBLE_ONLY_FILES
                            .iter()
                            .any(|blocked| name.eq_ignore_ascii_case(blocked))
                    })
            });
        }

        let lpc_archive = stage.join("LPC.zip");
        fs_ops::download(
            LPC_ARCHIVE.url,
            &lpc_archive,
            Some(&lpc_digest),
            MAX_ARCHIVE_BYTES,
        )?;
        let lpc_extract = stage.join("lpc");
        fs_ops::extract_zip(&lpc_archive, &lpc_extract)?;
        let lpc_files = find_lpc_files(&lpc_extract)?;
        state.log(
            "Success",
            "Downloaded and validated T7 Patch and LPC archives.",
        );

        validate_lpc_dir(&game_dir)?;
        let mut operations = Vec::new();
        let mut retained = Vec::new();
        for (source, relative) in patch_files {
            let target = safe_target(&game_dir, &relative)?;
            if relative
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("t7patch.conf"))
                && regular_file(&target)
            {
                retained.push(relative);
                continue;
            }
            operations.push(InstallOperation { source, relative });
        }
        for source in lpc_files {
            let filename = source
                .file_name()
                .ok_or_else(|| "invalid LPC filename".to_string())?
                .to_owned();
            operations.push(InstallOperation {
                source,
                relative: Path::new("LPC").join(filename),
            });
        }

        let staged_hashes = expected_install_hashes(&operations)?;
        let legacy_hashes = match known_legacy_hashes(&game_dir) {
            Ok(Some(expected)) => expected,
            Ok(None) | Err(_) => staged_hashes,
        };
        let adopted = adopt_legacy_install(&game_dir, &legacy_hashes)?;
        let install_result = reject_unsafe_profile_transition(&game_dir, profile).and_then(|()| {
            apply_managed_install(&game_dir, &operations, &retained, &stage.join("rollback"))
        });
        if let Err(error) = install_result {
            if adopted {
                discard_adopted_manifest(&game_dir).map_err(|cleanup| {
                    format!("{error}; legacy ownership migration cleanup failed: {cleanup}")
                })?;
            }
            return Err(error);
        }

        state.log(
            "Success",
            if profile == PatchProfile::Compatible {
                "Installed T7 Patch 2.04 successfully."
            } else {
                "Installed T7 Patch Scroptss/T7Patch successfully."
            },
        );
        Ok(())
    })();

    if result.is_ok() {
        let _ = fs::remove_dir_all(&stage);
    } else {
        state.log(
            "Warning",
            format!("T7 staging files were retained at {}.", stage.display()),
        );
    }
    result
}

/// Remove only files recorded in the T7 ownership manifest. Managed bytes must
/// still match their recorded hashes, and overwritten files are restored only
/// from the exact PatchOps-owned backups recorded at install time.
pub fn uninstall(state: &AppState, game_dir: &Path) -> Result<(), String> {
    let game_dir = validate_game_dir(game_dir)?;
    if load_manifest(&game_dir)?.is_none() {
        let Some(expected) = known_legacy_hashes(&game_dir)? else {
            state.log("Info", "T7 Patch was not installed.");
            return Ok(());
        };
        adopt_legacy_install(&game_dir, &expected)?;
    }
    let stage = create_stage(state, "t7-uninstall")?;
    let result = apply_managed_uninstall(&game_dir, &stage.join("rollback"));

    if result.is_ok() {
        let _ = fs::remove_dir_all(&stage);
        clear_legacy_cache(state);
        state.log("Success", "T7 Patch has been completely uninstalled.");
    } else {
        state.log(
            "Warning",
            format!("T7 recovery files were retained at {}.", stage.display()),
        );
    }
    result
}

fn patch_profile(value: &str) -> Result<PatchProfile, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "current" | "default" | "enhanced" => Ok(PatchProfile::Current),
        "compatible" => Ok(PatchProfile::Compatible),
        _ => Err("Cannot install T7 Patch for an unverified executable profile".into()),
    }
}

fn mode_for_profile(game_dir: &Path, profile: Option<&str>) -> &'static str {
    match profile.map(str::trim) {
        Some(value) if value.eq_ignore_ascii_case("enhanced") => "Enhanced",
        Some(value)
            if value.eq_ignore_ascii_case("current") || value.eq_ignore_ascii_case("default") =>
        {
            "Default"
        }
        Some(value) if value.eq_ignore_ascii_case("compatible") => "Compatible",
        Some(_) => "Custom",
        None if ["BlackOpsIII.exe", "BlackOps3.exe"]
            .iter()
            .any(|name| regular_file(&game_dir.join(name))) =>
        {
            "Custom"
        }
        None => "Unknown",
    }
}

fn empty_state(mode: &str) -> T7State {
    T7State {
        installed: false,
        conf_exists: false,
        gamertag: String::new(),
        plain_name: String::new(),
        color_code: String::new(),
        network_password: String::new(),
        friends_only: false,
        mode: mode.into(),
    }
}

fn parse_conf(contents: &str) -> T7State {
    let mut state = empty_state("Unknown");
    for line in contents.lines() {
        if let Some(value) = line.strip_prefix("playername=") {
            state.gamertag = value.trim().into();
            if state.gamertag.starts_with('^') && state.gamertag.chars().count() > 2 {
                state.color_code = state.gamertag.chars().take(2).collect();
                state.plain_name = state.gamertag.chars().skip(2).collect();
            } else {
                state.plain_name = state.gamertag.clone();
            }
        } else if let Some(value) = line.strip_prefix("networkpassword=") {
            state.network_password = value.trim().into();
        } else if let Some(value) = line.strip_prefix("isfriendsonly=") {
            state.friends_only = value.trim() == "1";
        }
    }
    state
}

fn validate_gamertag(value: &str, color_code: &str) -> Result<String, String> {
    let value = value.trim();
    validate_config_value(value)?;
    if value.is_empty() {
        return Err("Gamertag cannot be empty".into());
    }
    if value.chars().count() > 20 {
        return Err("Gamertag cannot exceed 20 characters".into());
    }
    if !matches!(
        color_code,
        "" | "^0" | "^1" | "^2" | "^3" | "^4" | "^5" | "^6" | "^8" | "^9"
    ) {
        return Err("Invalid gamertag color code".into());
    }
    Ok(format!("{color_code}{value}"))
}

fn validate_config_value(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.contains(['\r', '\n', '\0']) {
        return Err("T7 Patch settings cannot contain line breaks or NUL bytes".into());
    }
    Ok(value.into())
}

fn update_conf(
    contents: &str,
    name: Option<&str>,
    password: Option<&str>,
    friends_only: Option<bool>,
) -> String {
    let mut name_found = false;
    let mut password_found = false;
    let mut friends_found = false;
    let mut lines = contents
        .split_inclusive('\n')
        .map(|line| {
            if let Some(value) = name.filter(|_| line.starts_with("playername=")) {
                name_found = true;
                format!("playername={value}\n")
            } else if let Some(value) = password.filter(|_| line.starts_with("networkpassword=")) {
                password_found = true;
                format!("networkpassword={value}\n")
            } else if let Some(value) = friends_only.filter(|_| line.starts_with("isfriendsonly="))
            {
                friends_found = true;
                format!("isfriendsonly={}\n", i32::from(value))
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>();
    if contents.is_empty() {
        lines.clear();
    }
    if let Some(value) = name.filter(|_| !name_found) {
        lines.insert(0, format!("playername={value}\n"));
    }
    if let Some(value) = password.filter(|_| !password_found) {
        lines.insert(0, format!("networkpassword={value}\n"));
    }
    if let Some(value) = friends_only.filter(|_| !friends_found) {
        lines.insert(0, format!("isfriendsonly={}\n", i32::from(value)));
    }
    lines.concat()
}

fn fetch_release_digests(api: &str) -> Result<HashMap<String, String>, String> {
    let response = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| error.to_string())?
        .get(api)
        .header(reqwest::header::USER_AGENT, "PatchOpsIII")
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| format!("failed to query T7 Patch release metadata: {error}"))?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RELEASE_JSON_BYTES)
    {
        return Err("T7 Patch release metadata is unexpectedly large".into());
    }
    let mut body = Vec::new();
    response
        .take(MAX_RELEASE_JSON_BYTES + 1)
        .read_to_end(&mut body)
        .map_err(|error| error.to_string())?;
    if body.len() as u64 > MAX_RELEASE_JSON_BYTES {
        return Err("T7 Patch release metadata is unexpectedly large".into());
    }
    release_digests_from_json(&body)
}

fn release_digests_from_json(body: &[u8]) -> Result<HashMap<String, String>, String> {
    let release: GitHubRelease = serde_json::from_slice(body)
        .map_err(|error| format!("invalid T7 Patch release metadata: {error}"))?;
    Ok(release
        .assets
        .into_iter()
        .filter_map(|asset| normalize_sha256(&asset.digest).map(|digest| (asset.name, digest)))
        .collect())
}

fn trusted_digest(
    spec: AssetSpec,
    release: &Result<HashMap<String, String>, String>,
    state: &AppState,
) -> Result<String, String> {
    if let Ok(digests) = release {
        if let Some(digest) = digests.get(spec.name) {
            return Ok(digest.clone());
        }
    }
    if let Some(digest) = spec.pinned_sha256 {
        state.log(
            "Warning",
            format!(
                "Release metadata digest unavailable for {}; using pinned trusted hash.",
                spec.name
            ),
        );
        return Ok(digest.into());
    }
    Err(match release {
        Err(error) => format!("{error}; no trusted SHA-256 is pinned for {}", spec.name),
        Ok(_) => format!("No GitHub SHA-256 digest available for {}", spec.name),
    })
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

fn find_patch_source(root: &Path) -> Result<PathBuf, String> {
    let mut directories = collect_directories(root)?;
    directories.sort_by_key(|path| {
        (
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("linux"))
            {
                0
            } else {
                1
            },
            path.components().count(),
            path.to_string_lossy().to_ascii_lowercase(),
        )
    });
    directories
        .into_iter()
        .find(|directory| {
            let names = fs::read_dir(directory)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .filter(|entry| regular_file(&entry.path()))
                .map(|entry| entry.file_name().to_string_lossy().to_ascii_lowercase())
                .collect::<HashSet<_>>();
            T7_REQUIRED_FILES.iter().all(|name| names.contains(*name))
        })
        .ok_or_else(|| "T7 Patch archive did not contain the expected files".into())
}

fn find_lpc_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut directories = collect_directories(root)?;
    directories.sort_by_key(|path| {
        (
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("lpc"))
            {
                0
            } else {
                1
            },
            path.components().count(),
            path.to_string_lossy().to_ascii_lowercase(),
        )
    });
    for directory in directories {
        let mut files = fs::read_dir(&directory)
            .map_err(|error| error.to_string())?
            .filter_map(Result::ok)
            .filter(|entry| {
                regular_file(&entry.path())
                    && entry
                        .file_name()
                        .to_str()
                        .is_some_and(|name| name.ends_with(".ff"))
            })
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        files.sort();
        if !files.is_empty() {
            return Ok(files);
        }
    }
    Err("LPC archive did not contain any .ff files".into())
}

fn collect_directories(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut directories = vec![root.to_path_buf()];
    let mut index = 0;
    while index < directories.len() {
        let directory = directories[index].clone();
        index += 1;
        for entry in fs::read_dir(&directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let metadata = fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
            if metadata.file_type().is_symlink() {
                return Err("archive contains a link".into());
            }
            if metadata.is_dir() {
                directories.push(entry.path());
            } else if !metadata.is_file() {
                return Err("archive contains a special file".into());
            }
        }
    }
    Ok(directories)
}

fn collect_files(root: &Path) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let mut files = Vec::new();
    let mut folded = HashSet::new();
    for directory in collect_directories(root)? {
        for entry in fs::read_dir(&directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            if !regular_file(&entry.path()) {
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|_| "archive path escaped extraction root".to_string())?
                .to_path_buf();
            if !safe_relative(&relative) {
                return Err("archive contains an unsafe path".into());
            }
            let key = relative
                .to_string_lossy()
                .replace('\\', "/")
                .to_ascii_lowercase();
            if !folded.insert(key) {
                return Err("archive contains case-colliding paths".into());
            }
            files.push((entry.path(), relative));
        }
    }
    files.sort_by(|left, right| left.1.cmp(&right.1));
    Ok(files)
}

fn safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
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

fn validate_lpc_dir(game_dir: &Path) -> Result<PathBuf, String> {
    let lpc = game_dir.join("LPC");
    match fs::symlink_metadata(&lpc) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(format!("refusing to use unsafe LPC path {}", lpc.display()))
        }
        Ok(_) => Ok(lpc),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(lpc),
        Err(error) => Err(error.to_string()),
    }
}

fn safe_target(root: &Path, relative: &Path) -> Result<PathBuf, String> {
    if !safe_relative(relative) {
        return Err("invalid game-relative path".into());
    }
    let components = relative.components().collect::<Vec<_>>();
    let mut target = root.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(component) = component else {
            return Err("invalid game-relative path".into());
        };
        target.push(component);
        if let Ok(metadata) = fs::symlink_metadata(&target) {
            if metadata.file_type().is_symlink() {
                return Err(format!(
                    "refusing to follow linked path {}",
                    target.display()
                ));
            }
            if index + 1 < components.len() && !metadata.is_dir() {
                return Err(format!("expected a directory at {}", target.display()));
            }
            if index + 1 == components.len() && !metadata.is_file() {
                return Err(format!("expected a file at {}", target.display()));
            }
        }
    }
    Ok(target)
}

fn regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn regular_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn copy_file(source: &Path, target: &Path) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::copy(source, target)
        .map(|_| ())
        .map_err(|error| format!("failed to install {}: {error}", target.display()))
}

fn create_backup_copy(source: &Path, target: &Path) -> Result<(), String> {
    let mut input = fs::File::open(source).map_err(|error| error.to_string())?;
    let mut output = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(target)
        .map_err(|error| format!("failed to reserve backup {}: {error}", target.display()))?;
    let result = std::io::copy(&mut input, &mut output)
        .and_then(|_| output.sync_all())
        .map_err(|error| format!("failed to write backup {}: {error}", target.display()));
    if result.is_err() {
        drop(output);
        let _ = fs::remove_file(target);
    }
    result
}

fn ownership_manifest_relative() -> PathBuf {
    Path::new(OWNERSHIP_DIRECTORY).join(OWNERSHIP_MANIFEST)
}

fn ownership_manifest_path(game_dir: &Path) -> Result<PathBuf, String> {
    safe_target(game_dir, &ownership_manifest_relative())
}

fn folded_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase()
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn managed_relative(path: &Path) -> bool {
    if !safe_relative(path) || path.starts_with(OWNERSHIP_DIRECTORY) {
        return false;
    }
    let components = path.components().collect::<Vec<_>>();
    match components.as_slice() {
        [Component::Normal(name)] => name.to_str().is_some_and(|name| {
            T7_GAME_FILES
                .iter()
                .any(|allowed| name.eq_ignore_ascii_case(allowed))
        }),
        [Component::Normal(directory), Component::Normal(name)] => {
            directory
                .to_str()
                .is_some_and(|directory| directory.eq_ignore_ascii_case("LPC"))
                && name.to_str().is_some_and(|name| {
                    name.to_ascii_lowercase().ends_with(".ff")
                        && !name.ends_with(fs_ops::PATCHOPS_BACKUP_SUFFIX)
                        && !name.ends_with(fs_ops::LEGACY_BACKUP_SUFFIX)
                })
        }
        _ => false,
    }
}

fn owned_backup_relative(path: &Path) -> bool {
    if !safe_relative(path) {
        return false;
    }
    let components = path.components().collect::<Vec<_>>();
    let [Component::Normal(owner), Component::Normal(directory), Component::Normal(generation), Component::Normal(filename)] =
        components.as_slice()
    else {
        return false;
    };
    owner == &std::ffi::OsStr::new(OWNERSHIP_DIRECTORY)
        && directory == &std::ffi::OsStr::new(OWNERSHIP_BACKUP_DIRECTORY)
        && generation.to_str().is_some_and(|value| {
            !value.is_empty()
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        && filename.to_str().is_some_and(|value| {
            let digits = value
                .strip_prefix("entry-")
                .and_then(|value| value.strip_suffix(".original"));
            digits.is_some_and(|digits| {
                !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
            })
        })
}

fn validate_manifest(manifest: &OwnershipManifest) -> Result<(), String> {
    if manifest.version != OWNERSHIP_VERSION {
        return Err(format!(
            "unsupported T7 ownership manifest version {}",
            manifest.version
        ));
    }
    let mut paths = HashSet::new();
    let mut backups = HashSet::new();
    for entry in &manifest.entries {
        if !managed_relative(&entry.path) {
            return Err(format!(
                "T7 ownership manifest contains an invalid managed path: {}",
                entry.path.display()
            ));
        }
        if !paths.insert(folded_path(&entry.path)) {
            return Err("T7 ownership manifest contains duplicate managed paths".into());
        }
        if !valid_sha256(&entry.managed_sha256) {
            return Err("T7 ownership manifest contains an invalid managed SHA-256".into());
        }
        if let OriginalFile::Overwritten {
            backup_path,
            sha256,
            ..
        } = &entry.original
        {
            if !owned_backup_relative(backup_path) {
                return Err(format!(
                    "T7 ownership manifest contains an invalid backup path: {}",
                    backup_path.display()
                ));
            }
            if !backups.insert(folded_path(backup_path)) {
                return Err("T7 ownership manifest reuses a backup path".into());
            }
            if !valid_sha256(sha256) {
                return Err("T7 ownership manifest contains an invalid backup SHA-256".into());
            }
        }
    }
    Ok(())
}

fn load_manifest(game_dir: &Path) -> Result<Option<OwnershipManifest>, String> {
    let path = ownership_manifest_path(game_dir)?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => metadata,
        Ok(_) => {
            return Err(format!(
                "refusing to use unsafe T7 ownership manifest {}",
                path.display()
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let state_dir = game_dir.join(OWNERSHIP_DIRECTORY);
            return match fs::symlink_metadata(&state_dir) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error.to_string()),
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => Err(
                    format!("refusing unsafe T7 ownership state {}", state_dir.display()),
                ),
                Ok(_) => {
                    let mut entries =
                        fs::read_dir(&state_dir).map_err(|error| error.to_string())?;
                    if entries.next().is_none() {
                        Ok(None)
                    } else {
                        Err(format!(
                            "T7 ownership state is incomplete; recovery files were retained at {}",
                            state_dir.display()
                        ))
                    }
                }
            };
        }
        Err(error) => return Err(error.to_string()),
    };
    if metadata.len() > MAX_MANIFEST_BYTES {
        return Err("T7 ownership manifest is unexpectedly large".into());
    }
    let mut body = Vec::new();
    fs::File::open(&path)
        .map_err(|error| error.to_string())?
        .take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut body)
        .map_err(|error| error.to_string())?;
    if body.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("T7 ownership manifest is unexpectedly large".into());
    }
    let manifest: OwnershipManifest = serde_json::from_slice(&body)
        .map_err(|error| format!("invalid T7 ownership manifest: {error}"))?;
    validate_manifest(&manifest)?;
    Ok(Some(manifest))
}

fn write_manifest(game_dir: &Path, manifest: &mut OwnershipManifest) -> Result<(), String> {
    manifest
        .entries
        .sort_by_key(|entry| folded_path(&entry.path));
    validate_manifest(manifest)?;
    let path = ownership_manifest_path(game_dir)?;
    let parent = path
        .parent()
        .ok_or_else(|| "T7 ownership manifest has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    ownership_manifest_path(game_dir)?;
    safe_target(
        game_dir,
        &Path::new(OWNERSHIP_DIRECTORY).join(format!(".{OWNERSHIP_MANIFEST}.patchops.tmp")),
    )?;
    let mut body = serde_json::to_vec_pretty(manifest).map_err(|error| error.to_string())?;
    body.push(b'\n');
    fs_ops::atomic_write(&path, &body)
}

fn verify_managed_entry(game_dir: &Path, entry: &ManagedFile) -> Result<(), String> {
    let active = safe_target(game_dir, &entry.path)?;
    let restore_only = matches!(
        &entry.original,
        OriginalFile::Overwritten {
            sha256,
            active_was_missing: true,
            ..
        } if sha256.eq_ignore_ascii_case(&entry.managed_sha256)
    );
    match fs::symlink_metadata(&active) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            let actual = fs_ops::sha256_file(&active)?;
            if !actual.eq_ignore_ascii_case(&entry.managed_sha256) {
                return Err(format!(
                    "T7 ownership conflict: {} changed outside PatchOpsIII; no files were removed",
                    active.display()
                ));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && restore_only => {}
        _ => {
            return Err(format!(
                "T7 ownership conflict: managed file is missing or unsafe: {}",
                active.display()
            ));
        }
    }
    if let OriginalFile::Overwritten {
        backup_path,
        sha256,
        ..
    } = &entry.original
    {
        let backup = safe_target(game_dir, backup_path)?;
        if !regular_file(&backup) {
            return Err(format!(
                "T7 recovery conflict: original backup is missing or unsafe: {}",
                backup.display()
            ));
        }
        let actual = fs_ops::sha256_file(&backup)?;
        if !actual.eq_ignore_ascii_case(sha256) {
            return Err(format!(
                "T7 recovery conflict: original backup changed: {}",
                backup.display()
            ));
        }
    }
    Ok(())
}

fn create_backup_generation(game_dir: &Path) -> Result<String, String> {
    let backup_root_relative = Path::new(OWNERSHIP_DIRECTORY).join(OWNERSHIP_BACKUP_DIRECTORY);
    let probe = backup_root_relative.join(".patchops-directory-probe");
    safe_target(game_dir, &probe)?;
    let backup_root = game_dir.join(&backup_root_relative);
    fs::create_dir_all(&backup_root).map_err(|error| error.to_string())?;
    safe_target(game_dir, &probe)?;
    for _ in 0..100 {
        let counter = STAGE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let generation = format!("{timestamp}-{}-{counter}", std::process::id());
        let path = backup_root.join(&generation);
        match fs::create_dir(&path) {
            Ok(()) => return Ok(generation),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("could not allocate a unique T7 backup directory".into())
}

fn backup_relative(generation: &str, index: usize) -> PathBuf {
    Path::new(OWNERSHIP_DIRECTORY)
        .join(OWNERSHIP_BACKUP_DIRECTORY)
        .join(generation)
        .join(format!("entry-{index:04}.original"))
}

fn expected_install_hashes(
    operations: &[InstallOperation],
) -> Result<HashMap<String, String>, String> {
    operations
        .iter()
        .map(|operation| {
            Ok((
                folded_path(&operation.relative),
                fs_ops::sha256_file(&operation.source)?,
            ))
        })
        .collect()
}

fn known_t7_profile(marker_hashes: [&str; 2]) -> Option<&'static [(&'static str, &'static str)]> {
    [
        KNOWN_CURRENT_FILES.as_slice(),
        KNOWN_SCROPTSS_V3_03_FILES.as_slice(),
        KNOWN_SCROPTSS_V3_02_FILES.as_slice(),
        KNOWN_COMPATIBLE_FILES.as_slice(),
        KNOWN_SHIVERSOFT_V2_03_FILES.as_slice(),
    ]
    .into_iter()
    .find(|profile| {
        T7_REQUIRED_FILES.iter().all(|name| {
            let expected = profile
                .iter()
                .find(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
                .map(|(_, hash)| *hash);
            let actual = T7_REQUIRED_FILES
                .iter()
                .position(|candidate| candidate == name)
                .map(|index| marker_hashes[index]);
            expected.is_some() && expected == actual
        })
    })
}

fn known_legacy_hashes(game_dir: &Path) -> Result<Option<HashMap<String, String>>, String> {
    let marker_hashes = T7_REQUIRED_FILES
        .iter()
        .map(|name| {
            let target = safe_target(game_dir, Path::new(name))?;
            if !target.exists() {
                return Ok(None);
            }
            if !regular_file(&target) {
                return Err(format!(
                    "refusing unsafe legacy T7 file {}",
                    target.display()
                ));
            }
            fs_ops::sha256_file(&target).map(Some)
        })
        .collect::<Result<Vec<_>, String>>()?;
    if marker_hashes.iter().all(Option::is_none) {
        return Ok(None);
    }
    if marker_hashes.iter().any(Option::is_none) {
        return Err(format!(
            "{UNTRACKED_UNINSTALL_ERROR}. The legacy T7 marker pair is incomplete"
        ));
    }

    let profile = known_t7_profile([
        marker_hashes[0]
            .as_deref()
            .expect("complete marker pair checked above"),
        marker_hashes[1]
            .as_deref()
            .expect("complete marker pair checked above"),
    ])
        .ok_or_else(|| {
            format!(
                "{UNTRACKED_UNINSTALL_ERROR}. The installed T7 payload does not match a release known to this build"
            )
        })?;

    let mut expected = profile
        .iter()
        .map(|(path, hash)| (folded_path(Path::new(path)), (*hash).to_owned()))
        .collect::<HashMap<_, _>>();
    expected.extend(KNOWN_LPC_FILES.iter().map(|(name, hash)| {
        (
            folded_path(&Path::new("LPC").join(name)),
            (*hash).to_owned(),
        )
    }));
    Ok(Some(expected))
}

fn suffixed_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    value.into()
}

/// Enumerate only direct legacy LPC backups that the Python implementation
/// could have created. Matching is case-insensitive for ownership purposes,
/// while `.patchops.bak` wins over the older `.bak` when both exist.
fn legacy_lpc_backups(game_dir: &Path) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let lpc = validate_lpc_dir(game_dir)?;
    match fs::symlink_metadata(&lpc) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => return Err(format!("refusing unsafe LPC directory {}", lpc.display())),
    }

    let mut selected = HashMap::<String, (PathBuf, PathBuf, u8)>::new();
    for item in fs::read_dir(&lpc).map_err(|error| error.to_string())? {
        let item = item.map_err(|error| error.to_string())?;
        let Some(filename) = item.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let (base, priority) =
            if let Some(base) = filename.strip_suffix(fs_ops::PATCHOPS_BACKUP_SUFFIX) {
                (base, 2)
            } else if let Some(base) = filename.strip_suffix(fs_ops::LEGACY_BACKUP_SUFFIX) {
                (base, 1)
            } else {
                continue;
            };
        if !base.to_ascii_lowercase().ends_with(".ff") {
            continue;
        }
        let relative = Path::new("LPC").join(base);
        if !managed_relative(&relative) {
            return Err(format!(
                "refusing unsafe legacy LPC backup {}",
                item.path().display()
            ));
        }
        let key = folded_path(&relative);
        if let Some((existing_relative, _, existing_priority)) = selected.get(&key) {
            if existing_relative != &relative {
                return Err(format!(
                    "legacy LPC backups contain ambiguous path casing for {}",
                    relative.display()
                ));
            }
            if priority <= *existing_priority {
                continue;
            }
        }
        selected.insert(key, (relative, item.path(), priority));
    }
    let mut selected = selected
        .into_values()
        .map(|(relative, backup, _)| (relative, backup))
        .collect::<Vec<_>>();
    selected.sort_by_key(|(relative, _)| folded_path(relative));
    Ok(selected)
}

/// Convert an origin/main T7 install into the ownership format only when every
/// fixed payload byte matches a verified archive member. User-editable config
/// remains unowned. LPC suffix backups are copied into app-owned recovery
/// storage; the legacy copies are kept until the follow-on operation succeeds.
fn adopt_legacy_install(
    game_dir: &Path,
    expected: &HashMap<String, String>,
) -> Result<bool, String> {
    if load_manifest(game_dir)?.is_some() {
        return Ok(false);
    }
    let marker_paths = T7_REQUIRED_FILES
        .iter()
        .map(|name| safe_target(game_dir, Path::new(name)))
        .collect::<Result<Vec<_>, _>>()?;
    if marker_paths.iter().all(|path| !path.exists()) {
        return Ok(false);
    }
    if marker_paths.iter().any(|path| !regular_file(path)) {
        return Err(format!(
            "{UNTRACKED_UNINSTALL_ERROR}. The legacy T7 marker pair is incomplete or unsafe"
        ));
    }
    for path in &marker_paths {
        let relative = path
            .strip_prefix(game_dir)
            .map_err(|_| "legacy T7 path escaped the game directory".to_string())?;
        let Some(expected_hash) = expected.get(&folded_path(relative)) else {
            return Err(format!(
                "{UNTRACKED_UNINSTALL_ERROR}. The installed T7 payload is not recognized by this build"
            ));
        };
        if !fs_ops::sha256_file(path)?.eq_ignore_ascii_case(expected_hash) {
            return Err(format!(
                "{UNTRACKED_UNINSTALL_ERROR}. {} does not match the verified legacy payload",
                path.display()
            ));
        }
    }

    let config = safe_target(game_dir, Path::new("t7patch.conf"))?;
    if config.exists() && !regular_file(&config) {
        return Err(format!(
            "refusing unsafe legacy T7 config {}",
            config.display()
        ));
    }

    let mut entries = Vec::new();
    for name in T7_GAME_FILES {
        if name.eq_ignore_ascii_case("t7patch.conf") {
            continue;
        }
        let relative = PathBuf::from(name);
        let target = safe_target(game_dir, &relative)?;
        if !target.exists() {
            continue;
        }
        if !regular_file(&target) {
            return Err(format!(
                "refusing unsafe legacy T7 file {}",
                target.display()
            ));
        }
        let Some(expected_hash) = expected.get(&folded_path(&relative)) else {
            return Err(format!(
                "legacy T7 file {} is not part of the recognized payload; no files were changed",
                target.display()
            ));
        };
        let actual = fs_ops::sha256_file(&target)?;
        if !actual.eq_ignore_ascii_case(expected_hash) {
            return Err(format!(
                "legacy T7 file {} changed after installation; no files were changed",
                target.display()
            ));
        }
        entries.push(ManagedFile {
            path: relative,
            managed_sha256: actual,
            original: OriginalFile::Created,
        });
    }

    let mut generation = None;
    let mut copied_backups = Vec::new();
    let result = (|| {
        for (name, _) in KNOWN_LPC_FILES {
            let relative = Path::new("LPC").join(name);
            let Some(expected_hash) = expected.get(&folded_path(&relative)) else {
                continue;
            };
            let target = safe_target(game_dir, &relative)?;
            if !target.exists() {
                continue;
            }
            if !regular_file(&target) {
                return Err(format!(
                    "refusing unsafe legacy LPC file {}",
                    target.display()
                ));
            }
            let actual = fs_ops::sha256_file(&target)?;
            if !actual.eq_ignore_ascii_case(expected_hash) {
                return Err(format!(
                    "legacy LPC file {} changed after installation; no files were changed",
                    target.display()
                ));
            }

            let patchops = suffixed_path(&target, fs_ops::PATCHOPS_BACKUP_SUFFIX);
            let legacy = suffixed_path(&target, fs_ops::LEGACY_BACKUP_SUFFIX);
            let selected = if patchops.exists() {
                Some(patchops)
            } else if legacy.exists() {
                Some(legacy)
            } else {
                None
            };
            let original = if let Some(selected) = selected {
                if !regular_file(&selected) {
                    return Err(format!(
                        "refusing unsafe legacy LPC backup {}",
                        selected.display()
                    ));
                }
                let original_sha256 = fs_ops::sha256_file(&selected)?;
                if generation.is_none() {
                    generation = Some(create_backup_generation(game_dir)?);
                }
                let owned_relative = backup_relative(
                    generation
                        .as_deref()
                        .ok_or_else(|| "failed to allocate T7 backup generation".to_string())?,
                    copied_backups.len(),
                );
                let owned = safe_target(game_dir, &owned_relative)?;
                create_backup_copy(&selected, &owned)?;
                if !fs_ops::sha256_file(&owned)?.eq_ignore_ascii_case(&original_sha256) {
                    return Err(format!(
                        "failed to verify adopted LPC backup {}",
                        owned.display()
                    ));
                }
                copied_backups.push(owned);
                OriginalFile::Overwritten {
                    backup_path: owned_relative,
                    sha256: original_sha256,
                    active_was_missing: false,
                }
            } else {
                OriginalFile::Created
            };
            entries.push(ManagedFile {
                path: relative,
                managed_sha256: actual,
                original,
            });
        }

        let mut adopted_paths = entries
            .iter()
            .map(|entry| folded_path(&entry.path))
            .collect::<HashSet<_>>();
        for (relative, selected) in legacy_lpc_backups(game_dir)? {
            let key = folded_path(&relative);
            if adopted_paths.contains(&key) {
                continue;
            }
            if !regular_file(&selected) {
                return Err(format!(
                    "refusing unsafe legacy LPC backup {}",
                    selected.display()
                ));
            }
            let target = safe_target(game_dir, &relative)?;
            match fs::symlink_metadata(&target) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.to_string()),
                Ok(_) => {
                    return Err(format!(
                        "legacy LPC target {} is not part of the recognized payload; no files were changed",
                        target.display()
                    ));
                }
            }
            let original_sha256 = fs_ops::sha256_file(&selected)?;
            if generation.is_none() {
                generation = Some(create_backup_generation(game_dir)?);
            }
            let owned_relative = backup_relative(
                generation
                    .as_deref()
                    .ok_or_else(|| "failed to allocate T7 backup generation".to_string())?,
                copied_backups.len(),
            );
            let owned = safe_target(game_dir, &owned_relative)?;
            create_backup_copy(&selected, &owned)?;
            if !fs_ops::sha256_file(&owned)?.eq_ignore_ascii_case(&original_sha256) {
                return Err(format!(
                    "failed to verify adopted LPC backup {}",
                    owned.display()
                ));
            }
            copied_backups.push(owned);
            entries.push(ManagedFile {
                path: relative,
                managed_sha256: original_sha256.clone(),
                original: OriginalFile::Overwritten {
                    backup_path: owned_relative,
                    sha256: original_sha256,
                    active_was_missing: true,
                },
            });
            adopted_paths.insert(key);
        }
        write_manifest(
            game_dir,
            &mut OwnershipManifest {
                version: OWNERSHIP_VERSION,
                entries,
            },
        )
    })();
    if let Err(error) = result {
        for backup in copied_backups {
            let _ = remove_file_if_present(&backup);
        }
        if let Some(generation) = generation {
            let directory = game_dir
                .join(OWNERSHIP_DIRECTORY)
                .join(OWNERSHIP_BACKUP_DIRECTORY)
                .join(generation);
            let _ = fs::remove_dir(directory);
            let _ = fs::remove_dir(
                game_dir
                    .join(OWNERSHIP_DIRECTORY)
                    .join(OWNERSHIP_BACKUP_DIRECTORY),
            );
            let _ = fs::remove_dir(game_dir.join(OWNERSHIP_DIRECTORY));
        }
        return Err(error);
    }
    Ok(true)
}

fn discard_adopted_manifest(game_dir: &Path) -> Result<(), String> {
    let Some(manifest) = load_manifest(game_dir)? else {
        return Ok(());
    };
    for entry in &manifest.entries {
        if let OriginalFile::Overwritten { backup_path, .. } = &entry.original {
            remove_file_if_present(&safe_target(game_dir, backup_path)?)?;
        }
    }
    remove_file_if_present(&ownership_manifest_path(game_dir)?)?;
    remove_empty_ownership_directories(game_dir);
    Ok(())
}

fn remove_empty_ownership_directories(game_dir: &Path) {
    let backup_root = game_dir
        .join(OWNERSHIP_DIRECTORY)
        .join(OWNERSHIP_BACKUP_DIRECTORY);
    if regular_directory(&backup_root) {
        if let Ok(entries) = fs::read_dir(&backup_root) {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if regular_directory(&path) {
                    let _ = fs::remove_dir(path);
                }
            }
        }
        let _ = fs::remove_dir(&backup_root);
    }
    let _ = fs::remove_dir(game_dir.join(OWNERSHIP_DIRECTORY));
}

fn reject_unsafe_profile_transition(game_dir: &Path, profile: PatchProfile) -> Result<(), String> {
    if profile != PatchProfile::Current {
        return Ok(());
    }
    let Some(manifest) = load_manifest(game_dir)? else {
        return Ok(());
    };
    let would_restore_compatible_file = manifest.entries.iter().any(|entry| {
        entry
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                COMPATIBLE_ONLY_FILES
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(name))
            })
            && matches!(entry.original, OriginalFile::Overwritten { .. })
    });
    if would_restore_compatible_file {
        Err("Cannot switch this managed T7 install to the Current profile because doing so would restore a pre-existing compatible-only DLL into the active game directory. Uninstall T7 Patch first and review the restored file before installing the Current profile.".into())
    } else {
        Ok(())
    }
}

fn rollback_transaction(
    snapshot: &Snapshot,
    error: String,
    rollback_directory: &Path,
) -> Result<(), String> {
    match snapshot.restore() {
        Ok(()) => Err(format!("{error}; previous T7 files were restored")),
        Err(rollback) => Err(format!(
            "{error}; rollback failed: {rollback}. Recovery files remain in {}",
            rollback_directory.display()
        )),
    }
}

fn apply_managed_install(
    game_dir: &Path,
    operations: &[InstallOperation],
    retained: &[PathBuf],
    rollback_directory: &Path,
) -> Result<(), String> {
    let previous = load_manifest(game_dir)?.unwrap_or(OwnershipManifest {
        version: OWNERSHIP_VERSION,
        entries: Vec::new(),
    });
    let mut previous_by_path = previous
        .entries
        .into_iter()
        .map(|entry| (folded_path(&entry.path), entry))
        .collect::<HashMap<_, _>>();
    let mut generation = None;
    let mut desired_paths = HashSet::new();
    let mut prepared = Vec::new();
    let mut new_backup_index = 0_usize;

    for operation in operations {
        if !regular_file(&operation.source) || !managed_relative(&operation.relative) {
            return Err(format!(
                "refusing unsafe T7 install operation for {}",
                operation.relative.display()
            ));
        }
        let key = folded_path(&operation.relative);
        if !desired_paths.insert(key.clone()) {
            return Err("T7 install contains duplicate target paths".into());
        }
        let active = safe_target(game_dir, &operation.relative)?;
        let managed_sha256 = fs_ops::sha256_file(&operation.source)?;
        let (original, new_backup) = if let Some(previous) = previous_by_path.remove(&key) {
            if previous.path.as_path() != operation.relative.as_path() {
                return Err(format!(
                    "T7 ownership path casing changed from {} to {}; refusing an ambiguous update",
                    previous.path.display(),
                    operation.relative.display()
                ));
            }
            verify_managed_entry(game_dir, &previous)?;
            (previous.original, None)
        } else if regular_file(&active) {
            let original_sha256 = fs_ops::sha256_file(&active)?;
            if generation.is_none() {
                generation = Some(create_backup_generation(game_dir)?);
            }
            let generation = generation
                .as_deref()
                .ok_or_else(|| "failed to allocate T7 backup generation".to_string())?;
            let relative = backup_relative(generation, new_backup_index);
            new_backup_index += 1;
            let backup = safe_target(game_dir, &relative)?;
            match fs::symlink_metadata(&backup) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Ok(_) => return Err(format!("refusing to reuse backup {}", backup.display())),
                Err(error) => return Err(error.to_string()),
            }
            (
                OriginalFile::Overwritten {
                    backup_path: relative,
                    sha256: original_sha256,
                    active_was_missing: false,
                },
                Some(backup),
            )
        } else {
            (OriginalFile::Created, None)
        };
        prepared.push((
            operation,
            active,
            ManagedFile {
                path: operation.relative.clone(),
                managed_sha256,
                original,
            },
            new_backup,
        ));
    }

    let mut entries = prepared
        .iter()
        .map(|(_, _, entry, _)| entry.clone())
        .collect::<Vec<_>>();
    for relative in retained {
        if !managed_relative(relative) {
            return Err(format!(
                "refusing unsafe retained T7 path {}",
                relative.display()
            ));
        }
        let key = folded_path(relative);
        if !desired_paths.insert(key.clone()) {
            return Err("T7 install contains duplicate retained paths".into());
        }
        if let Some(previous) = previous_by_path.remove(&key) {
            if previous.path.as_path() != relative.as_path() {
                return Err(format!(
                    "T7 ownership path casing changed from {} to {}; refusing an ambiguous update",
                    previous.path.display(),
                    relative.display()
                ));
            }
            verify_managed_entry(game_dir, &previous)?;
            entries.push(previous);
        }
    }

    let retired = previous_by_path.into_values().collect::<Vec<_>>();
    for entry in &retired {
        verify_managed_entry(game_dir, entry)?;
    }

    let manifest_path = ownership_manifest_path(game_dir)?;
    let mut targets = vec![manifest_path];
    for (_, active, _, new_backup) in &prepared {
        targets.push(active.clone());
        if let Some(backup) = new_backup {
            targets.push(backup.clone());
        }
    }
    for entry in &retired {
        targets.push(safe_target(game_dir, &entry.path)?);
    }
    let snapshot = Snapshot::capture(&targets, rollback_directory)?;

    let commit: Result<(), String> = (|| {
        for (_, active, entry, new_backup) in &prepared {
            if let Some(backup) = new_backup {
                create_backup_copy(active, backup)?;
                let expected = match &entry.original {
                    OriginalFile::Overwritten { sha256, .. } => sha256,
                    OriginalFile::Created => unreachable!(),
                };
                if !fs_ops::sha256_file(backup)?.eq_ignore_ascii_case(expected) {
                    return Err(format!(
                        "failed to verify original T7 backup {}",
                        backup.display()
                    ));
                }
            }
        }
        for (operation, active, _, _) in &prepared {
            copy_file(&operation.source, active)?;
        }
        for entry in &retired {
            let active = safe_target(game_dir, &entry.path)?;
            match &entry.original {
                OriginalFile::Created => {
                    remove_file_if_present(&active)?;
                    if active.exists() {
                        return Err(format!(
                            "failed to retire managed T7 file {}",
                            active.display()
                        ));
                    }
                }
                OriginalFile::Overwritten {
                    backup_path,
                    sha256,
                    ..
                } => {
                    let backup = safe_target(game_dir, backup_path)?;
                    copy_file(&backup, &active)?;
                    if !fs_ops::sha256_file(&active)?.eq_ignore_ascii_case(sha256) {
                        return Err(format!(
                            "failed to verify restored T7 file {}",
                            active.display()
                        ));
                    }
                }
            }
        }
        for entry in &entries {
            let active = safe_target(game_dir, &entry.path)?;
            if !regular_file(&active)
                || !fs_ops::sha256_file(&active)?.eq_ignore_ascii_case(&entry.managed_sha256)
            {
                return Err(format!(
                    "T7 Patch install verification failed for {}",
                    active.display()
                ));
            }
        }
        write_manifest(
            game_dir,
            &mut OwnershipManifest {
                version: OWNERSHIP_VERSION,
                entries,
            },
        )
    })();

    if let Err(error) = commit {
        return rollback_transaction(&snapshot, error, rollback_directory);
    }
    for entry in &retired {
        if let OriginalFile::Overwritten { backup_path, .. } = &entry.original {
            if let Ok(backup) = safe_target(game_dir, backup_path) {
                let _ = remove_file_if_present(&backup);
            }
        }
    }
    remove_empty_ownership_directories(game_dir);
    Ok(())
}

fn apply_managed_uninstall(game_dir: &Path, rollback_directory: &Path) -> Result<(), String> {
    let manifest = load_manifest(game_dir)?.ok_or_else(|| UNTRACKED_UNINSTALL_ERROR.to_string())?;
    for entry in &manifest.entries {
        verify_managed_entry(game_dir, entry)?;
    }

    let manifest_path = ownership_manifest_path(game_dir)?;
    let mut targets = vec![manifest_path.clone()];
    for entry in &manifest.entries {
        targets.push(safe_target(game_dir, &entry.path)?);
        if let OriginalFile::Overwritten { backup_path, .. } = &entry.original {
            targets.push(safe_target(game_dir, backup_path)?);
        }
    }
    let snapshot = Snapshot::capture(&targets, rollback_directory)?;
    let commit: Result<(), String> = (|| {
        for entry in &manifest.entries {
            let active = safe_target(game_dir, &entry.path)?;
            match &entry.original {
                OriginalFile::Created => {
                    remove_file_if_present(&active)?;
                    if active.exists() {
                        return Err(format!(
                            "failed to remove managed T7 file {}",
                            active.display()
                        ));
                    }
                }
                OriginalFile::Overwritten {
                    backup_path,
                    sha256,
                    ..
                } => {
                    let backup = safe_target(game_dir, backup_path)?;
                    copy_file(&backup, &active)?;
                    if !fs_ops::sha256_file(&active)?.eq_ignore_ascii_case(sha256) {
                        return Err(format!(
                            "failed to verify restored T7 file {}",
                            active.display()
                        ));
                    }
                    remove_file_if_present(&backup)?;
                }
            }
        }
        remove_file_if_present(&manifest_path)
    })();
    if let Err(error) = commit {
        return rollback_transaction(&snapshot, error, rollback_directory);
    }
    remove_empty_ownership_directories(game_dir);
    Ok(())
}

fn write_config_with_ownership(
    game_dir: &Path,
    contents: &[u8],
    rollback_directory: &Path,
) -> Result<(), String> {
    let relative = PathBuf::from("t7patch.conf");
    let path = safe_target(game_dir, &relative)?;
    let Some(mut manifest) = load_manifest(game_dir)? else {
        return fs_ops::atomic_write(&path, contents);
    };
    let Some(index) = manifest
        .entries
        .iter()
        .position(|entry| folded_path(&entry.path) == folded_path(&relative))
    else {
        return fs_ops::atomic_write(&path, contents);
    };
    verify_managed_entry(game_dir, &manifest.entries[index])?;
    let manifest_path = ownership_manifest_path(game_dir)?;
    let snapshot = Snapshot::capture(&[path.clone(), manifest_path], rollback_directory)?;
    let commit: Result<(), String> = (|| {
        fs_ops::atomic_write(&path, contents)?;
        manifest.entries[index].managed_sha256 = fs_ops::sha256_file(&path)?;
        write_manifest(game_dir, &mut manifest)
    })();
    match commit {
        Ok(()) => Ok(()),
        Err(error) => rollback_transaction(&snapshot, error, rollback_directory),
    }
}

#[cfg(test)]
pub(crate) fn benchmark_install_transaction(
    game_dir: &Path,
    sources: &[(PathBuf, PathBuf)],
    rollback_directory: &Path,
) -> Result<(), String> {
    let operations = sources
        .iter()
        .map(|(source, relative)| InstallOperation {
            source: source.clone(),
            relative: relative.clone(),
        })
        .collect::<Vec<_>>();
    apply_managed_install(game_dir, &operations, &[], rollback_directory)
}

#[cfg(test)]
pub(crate) fn benchmark_configure_transaction(
    game_dir: &Path,
    contents: &[u8],
    rollback_directory: &Path,
) -> Result<(), String> {
    write_config_with_ownership(game_dir, contents, rollback_directory)
}

#[cfg(test)]
pub(crate) fn benchmark_uninstall_transaction(
    game_dir: &Path,
    rollback_directory: &Path,
) -> Result<(), String> {
    apply_managed_uninstall(game_dir, rollback_directory)
}

fn create_stage(state: &AppState, label: &str) -> Result<PathBuf, String> {
    let root = state.mod_files_dir();
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
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

fn clear_legacy_cache(state: &AppState) {
    let root = state.mod_files_dir();
    for directory in [
        root.join("linux"),
        root.join("Linux.Steamdeck.and.Manual.Windows.Install"),
        root.join("T7Patch_extracted"),
    ] {
        if fs::symlink_metadata(&directory)
            .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
        {
            let _ = fs::remove_dir_all(directory);
        }
    }
    let archive = root.join("T7Patch.zip");
    if regular_file(&archive) {
        let _ = fs::remove_file(archive);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "patchops-t7-{label}-{}-{}",
            std::process::id(),
            TEST_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn operation(source_root: &Path, relative: &str, contents: &[u8]) -> InstallOperation {
        let source = source_root.join(relative);
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, contents).unwrap();
        InstallOperation {
            source,
            relative: PathBuf::from(relative),
        }
    }

    #[test]
    fn parses_and_updates_t7_config_without_losing_unknown_lines() {
        let original = "unknown=keep\nplayername=Old\nnetworkpassword=old\nisfriendsonly=0\n";
        let updated = update_conf(original, Some("^2New"), Some("secret"), Some(true));
        assert_eq!(
            updated,
            "unknown=keep\nplayername=^2New\nnetworkpassword=secret\nisfriendsonly=1\n"
        );
        let parsed = parse_conf(&updated);
        assert_eq!(parsed.gamertag, "^2New");
        assert_eq!(parsed.color_code, "^2");
        assert_eq!(parsed.plain_name, "New");
        assert_eq!(parsed.network_password, "secret");
        assert!(parsed.friends_only);

        assert!(validate_gamertag("bad\nname", "^1").is_err());
        assert!(validate_gamertag("name", "^7").is_err());
    }

    #[test]
    fn selects_patch_by_executable_profile_and_parses_github_digest() {
        assert_eq!(patch_profile("current").unwrap(), PatchProfile::Current);
        assert_eq!(patch_profile("enhanced").unwrap(), PatchProfile::Current);
        assert_eq!(
            patch_profile("compatible").unwrap(),
            PatchProfile::Compatible
        );
        assert!(patch_profile("unverified").is_err());

        let game = temp_dir("profile");
        assert_eq!(mode_for_profile(&game, Some("compatible")), "Compatible");
        fs::remove_dir_all(game).unwrap();

        let digests = release_digests_from_json(
            br#"{"assets":[{"name":"Linux.Steamdeck.and.Manual.Windows.Install.zip","digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}]}"#,
        )
        .unwrap();
        assert_eq!(
            digests.get(PATCH_ARCHIVE_NAME).map(String::as_str),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
    }

    #[test]
    fn historical_t7_marker_pairs_select_only_their_exact_release_profile() {
        assert_eq!(
            known_t7_profile([
                KNOWN_SCROPTSS_V3_03_FILES[1].1,
                KNOWN_SCROPTSS_V3_03_FILES[2].1,
            ]),
            Some(KNOWN_SCROPTSS_V3_03_FILES.as_slice())
        );
        assert_eq!(
            known_t7_profile([
                KNOWN_SHIVERSOFT_V2_03_FILES[2].1,
                KNOWN_SHIVERSOFT_V2_03_FILES[3].1,
            ]),
            Some(KNOWN_SHIVERSOFT_V2_03_FILES.as_slice())
        );
        assert!(known_t7_profile([
            KNOWN_SCROPTSS_V3_03_FILES[1].1,
            KNOWN_SHIVERSOFT_V2_03_FILES[3].1,
        ])
        .is_none());
    }

    #[test]
    fn ownership_manifest_preserves_originals_across_updates_and_covers_lpc() {
        let game = temp_dir("ownership");
        let sources = temp_dir("ownership-sources");
        let rollback = temp_dir("ownership-rollback");
        fs::create_dir_all(game.join("LPC")).unwrap();
        fs::write(game.join("t7patch.dll"), b"original-patch").unwrap();
        fs::write(game.join("discord_game_sdk.dll"), b"original-discord").unwrap();
        fs::write(game.join("LPC/english.ff"), b"original-lpc").unwrap();

        let unrelated_patchops = game.join("LPC/english.ff.patchops.bak");
        let unrelated_legacy = game.join("LPC/english.ff.bak");
        fs::write(&unrelated_patchops, b"unrelated-patchops-backup").unwrap();
        fs::write(&unrelated_legacy, b"unrelated-legacy-backup").unwrap();

        let first = [
            operation(&sources, "first/t7patch.dll", b"managed-patch-v1"),
            operation(&sources, "first/t7patchloader.dll", b"managed-loader-v1"),
            operation(
                &sources,
                "first/discord_game_sdk.dll",
                b"managed-discord-v1",
            ),
            operation(&sources, "first/english.ff", b"managed-lpc-v1"),
        ];
        let first = [
            InstallOperation {
                source: first[0].source.clone(),
                relative: PathBuf::from("t7patch.dll"),
            },
            InstallOperation {
                source: first[1].source.clone(),
                relative: PathBuf::from("t7patchloader.dll"),
            },
            InstallOperation {
                source: first[2].source.clone(),
                relative: PathBuf::from("discord_game_sdk.dll"),
            },
            InstallOperation {
                source: first[3].source.clone(),
                relative: PathBuf::from("LPC/english.ff"),
            },
        ];
        apply_managed_install(&game, &first, &[], &rollback.join("first")).unwrap();

        let first_manifest = load_manifest(&game).unwrap().unwrap();
        assert_eq!(first_manifest.entries.len(), 4);
        let patch_entry = first_manifest
            .entries
            .iter()
            .find(|entry| entry.path == Path::new("t7patch.dll"))
            .unwrap();
        let OriginalFile::Overwritten {
            backup_path: patch_backup,
            sha256: patch_original_hash,
            ..
        } = &patch_entry.original
        else {
            panic!("existing patch file was not recorded as overwritten");
        };
        assert!(owned_backup_relative(patch_backup));
        assert!(!patch_backup.to_string_lossy().ends_with(".bak"));
        assert_eq!(
            fs_ops::sha256_file(&game.join(patch_backup)).unwrap(),
            *patch_original_hash
        );
        let preserved_patch_backup = patch_backup.clone();
        let lpc_entry = first_manifest
            .entries
            .iter()
            .find(|entry| entry.path == Path::new("LPC/english.ff"))
            .unwrap();
        assert!(matches!(
            lpc_entry.original,
            OriginalFile::Overwritten { .. }
        ));
        let loader_entry = first_manifest
            .entries
            .iter()
            .find(|entry| entry.path == Path::new("t7patchloader.dll"))
            .unwrap();
        assert!(matches!(loader_entry.original, OriginalFile::Created));

        let second = [
            InstallOperation {
                source: operation(&sources, "second/t7patch.dll", b"managed-patch-v2").source,
                relative: PathBuf::from("t7patch.dll"),
            },
            InstallOperation {
                source: operation(&sources, "second/t7patchloader.dll", b"managed-loader-v2")
                    .source,
                relative: PathBuf::from("t7patchloader.dll"),
            },
            InstallOperation {
                source: operation(&sources, "second/english.ff", b"managed-lpc-v2").source,
                relative: PathBuf::from("LPC/english.ff"),
            },
        ];
        apply_managed_install(&game, &second, &[], &rollback.join("second")).unwrap();

        let second_manifest = load_manifest(&game).unwrap().unwrap();
        let updated_patch = second_manifest
            .entries
            .iter()
            .find(|entry| entry.path == Path::new("t7patch.dll"))
            .unwrap();
        let OriginalFile::Overwritten { backup_path, .. } = &updated_patch.original else {
            panic!("updated patch lost its original backup");
        };
        assert_eq!(backup_path, &preserved_patch_backup);
        assert_eq!(fs::read(game.join(backup_path)).unwrap(), b"original-patch");
        assert_eq!(
            fs::read(game.join("discord_game_sdk.dll")).unwrap(),
            b"original-discord"
        );
        assert!(second_manifest
            .entries
            .iter()
            .all(|entry| entry.path != Path::new("discord_game_sdk.dll")));
        assert_eq!(
            fs::read(&unrelated_patchops).unwrap(),
            b"unrelated-patchops-backup"
        );
        assert_eq!(
            fs::read(&unrelated_legacy).unwrap(),
            b"unrelated-legacy-backup"
        );

        apply_managed_uninstall(&game, &rollback.join("uninstall")).unwrap();
        assert_eq!(
            fs::read(game.join("t7patch.dll")).unwrap(),
            b"original-patch"
        );
        assert_eq!(
            fs::read(game.join("LPC/english.ff")).unwrap(),
            b"original-lpc"
        );
        assert!(!game.join("t7patchloader.dll").exists());
        assert_eq!(
            fs::read(&unrelated_patchops).unwrap(),
            b"unrelated-patchops-backup"
        );
        assert_eq!(
            fs::read(&unrelated_legacy).unwrap(),
            b"unrelated-legacy-backup"
        );
        assert!(load_manifest(&game).unwrap().is_none());

        fs::remove_dir_all(game).unwrap();
        fs::remove_dir_all(sources).unwrap();
        fs::remove_dir_all(rollback).unwrap();
    }

    #[test]
    fn exact_legacy_install_is_adopted_and_lpc_original_is_restored() {
        let game = temp_dir("legacy-adopt");
        let rollback = temp_dir("legacy-adopt-rollback");
        fs::create_dir_all(game.join("LPC")).unwrap();
        fs::write(game.join("t7patch.dll"), b"legacy-patch").unwrap();
        fs::write(game.join("t7patchloader.dll"), b"legacy-loader").unwrap();
        fs::write(game.join("t7patch.conf"), b"playername=custom\n").unwrap();
        fs::write(game.join("LPC/en_core_ffotd_tu32_593.ff"), b"legacy-lpc").unwrap();
        let suffix_backup = game.join("LPC/en_core_ffotd_tu32_593.ff.patchops.bak");
        fs::write(&suffix_backup, b"original-lpc").unwrap();
        let extra_backup = game.join("LPC/custom_language.ff.patchops.bak");
        fs::write(&extra_backup, b"custom-original-lpc").unwrap();
        let expected = [
            PathBuf::from("t7patch.dll"),
            PathBuf::from("t7patchloader.dll"),
            PathBuf::from("LPC/en_core_ffotd_tu32_593.ff"),
        ]
        .into_iter()
        .map(|relative| {
            let hash = fs_ops::sha256_file(&game.join(&relative)).unwrap();
            (folded_path(&relative), hash)
        })
        .collect::<HashMap<_, _>>();

        assert!(adopt_legacy_install(&game, &expected).unwrap());
        let manifest = load_manifest(&game).unwrap().unwrap();
        assert!(manifest
            .entries
            .iter()
            .all(|entry| entry.path != Path::new("t7patch.conf")));
        let lpc = manifest
            .entries
            .iter()
            .find(|entry| entry.path == Path::new("LPC/en_core_ffotd_tu32_593.ff"))
            .unwrap();
        let OriginalFile::Overwritten { backup_path, .. } = &lpc.original else {
            panic!("legacy LPC backup was not adopted");
        };
        assert_ne!(game.join(backup_path), suffix_backup);
        assert_eq!(fs::read(game.join(backup_path)).unwrap(), b"original-lpc");
        let extra = manifest
            .entries
            .iter()
            .find(|entry| entry.path == Path::new("LPC/custom_language.ff"))
            .expect("backup-only legacy LPC original should be adopted");
        assert!(matches!(
            extra.original,
            OriginalFile::Overwritten {
                active_was_missing: true,
                ..
            }
        ));
        assert!(!game.join("LPC/custom_language.ff").exists());

        apply_managed_uninstall(&game, &rollback).unwrap();
        assert!(!game.join("t7patch.dll").exists());
        assert!(!game.join("t7patchloader.dll").exists());
        assert_eq!(
            fs::read(game.join("t7patch.conf")).unwrap(),
            b"playername=custom\n"
        );
        assert_eq!(
            fs::read(game.join("LPC/en_core_ffotd_tu32_593.ff")).unwrap(),
            b"original-lpc"
        );
        assert_eq!(fs::read(&suffix_backup).unwrap(), b"original-lpc");
        assert_eq!(
            fs::read(game.join("LPC/custom_language.ff")).unwrap(),
            b"custom-original-lpc"
        );
        assert_eq!(fs::read(&extra_backup).unwrap(), b"custom-original-lpc");
        fs::remove_dir_all(game).unwrap();
        fs::remove_dir_all(rollback).unwrap();
    }

    #[test]
    fn legacy_adoption_rejects_unknown_or_incomplete_markers_without_state() {
        let game = temp_dir("legacy-reject");
        fs::write(game.join("t7patch.dll"), b"known-patch").unwrap();
        let expected = [(
            folded_path(Path::new("t7patch.dll")),
            fs_ops::sha256_file(&game.join("t7patch.dll")).unwrap(),
        )]
        .into_iter()
        .collect::<HashMap<_, _>>();
        assert!(adopt_legacy_install(&game, &expected).is_err());
        assert!(!game.join(OWNERSHIP_DIRECTORY).exists());
        assert_eq!(fs::read(game.join("t7patch.dll")).unwrap(), b"known-patch");

        fs::write(game.join("t7patchloader.dll"), b"unexpected-loader").unwrap();
        assert!(adopt_legacy_install(&game, &expected).is_err());
        assert!(!game.join(OWNERSHIP_DIRECTORY).exists());
        assert_eq!(
            fs::read(game.join("t7patchloader.dll")).unwrap(),
            b"unexpected-loader"
        );
        fs::remove_dir_all(game).unwrap();
    }

    #[test]
    fn clean_legacy_probe_is_an_idempotent_noop() {
        let game = temp_dir("legacy-clean");
        assert!(known_legacy_hashes(&game).unwrap().is_none());
        assert!(!adopt_legacy_install(&game, &HashMap::new()).unwrap());
        assert!(!game.join(OWNERSHIP_DIRECTORY).exists());
        fs::remove_dir_all(game).unwrap();
    }

    #[test]
    fn incomplete_ownership_state_is_not_treated_as_a_clean_install() {
        let game = temp_dir("incomplete-state");
        let recovery = game
            .join(OWNERSHIP_DIRECTORY)
            .join(OWNERSHIP_BACKUP_DIRECTORY)
            .join("retained");
        fs::create_dir_all(&recovery).unwrap();
        fs::write(recovery.join("entry-0000.original"), b"recovery").unwrap();
        let error = load_manifest(&game).unwrap_err();
        assert!(error.contains("ownership state is incomplete"));
        assert_eq!(
            fs::read(recovery.join("entry-0000.original")).unwrap(),
            b"recovery"
        );
        fs::remove_dir_all(game).unwrap();
    }

    #[test]
    fn current_profile_refuses_to_restore_compatible_only_originals_mid_install() {
        let game = temp_dir("profile-transition");
        let sources = temp_dir("profile-transition-sources");
        let rollback = temp_dir("profile-transition-rollback");
        fs::write(game.join("discord_game_sdk.dll"), b"pre-existing").unwrap();
        let operations = [InstallOperation {
            source: operation(&sources, "discord_game_sdk.dll", b"compatible-managed").source,
            relative: PathBuf::from("discord_game_sdk.dll"),
        }];
        apply_managed_install(&game, &operations, &[], &rollback).unwrap();
        let before = fs::read(game.join("discord_game_sdk.dll")).unwrap();
        let error = reject_unsafe_profile_transition(&game, PatchProfile::Current).unwrap_err();
        assert!(error.contains("Uninstall T7 Patch first"));
        assert_eq!(fs::read(game.join("discord_game_sdk.dll")).unwrap(), before);
        fs::remove_dir_all(game).unwrap();
        fs::remove_dir_all(sources).unwrap();
        fs::remove_dir_all(rollback).unwrap();
    }

    #[test]
    fn uninstall_and_update_refuse_modified_managed_bytes_without_losing_recovery_state() {
        let game = temp_dir("conflict");
        let sources = temp_dir("conflict-sources");
        let rollback = temp_dir("conflict-rollback");
        fs::write(game.join("t7patch.dll"), b"original").unwrap();
        let first = [InstallOperation {
            source: operation(&sources, "v1.dll", b"managed-v1").source,
            relative: PathBuf::from("t7patch.dll"),
        }];
        apply_managed_install(&game, &first, &[], &rollback.join("install")).unwrap();
        let manifest_path = ownership_manifest_path(&game).unwrap();
        let manifest_before = fs::read(&manifest_path).unwrap();
        let manifest = load_manifest(&game).unwrap().unwrap();
        let backup = match &manifest.entries[0].original {
            OriginalFile::Overwritten { backup_path, .. } => game.join(backup_path),
            OriginalFile::Created => panic!("expected an original backup"),
        };
        let backup_before = fs::read(&backup).unwrap();
        fs::write(game.join("t7patch.dll"), b"user-modified").unwrap();

        let update = [InstallOperation {
            source: operation(&sources, "v2.dll", b"managed-v2").source,
            relative: PathBuf::from("t7patch.dll"),
        }];
        let update_error =
            apply_managed_install(&game, &update, &[], &rollback.join("refused-update"))
                .unwrap_err();
        assert!(update_error.contains("changed outside PatchOpsIII"));
        let uninstall_error =
            apply_managed_uninstall(&game, &rollback.join("refused-uninstall")).unwrap_err();
        assert!(uninstall_error.contains("changed outside PatchOpsIII"));
        assert_eq!(
            fs::read(game.join("t7patch.dll")).unwrap(),
            b"user-modified"
        );
        assert_eq!(fs::read(&manifest_path).unwrap(), manifest_before);
        assert_eq!(fs::read(&backup).unwrap(), backup_before);

        fs::write(game.join("t7patch.dll"), b"managed-v1").unwrap();
        fs::write(&backup, b"tampered-backup").unwrap();
        let backup_error =
            apply_managed_uninstall(&game, &rollback.join("refused-backup")).unwrap_err();
        assert!(backup_error.contains("original backup changed"));
        assert_eq!(fs::read(game.join("t7patch.dll")).unwrap(), b"managed-v1");
        assert!(manifest_path.is_file());
        assert_eq!(fs::read(&backup).unwrap(), b"tampered-backup");

        fs::remove_dir_all(game).unwrap();
        fs::remove_dir_all(sources).unwrap();
        fs::remove_dir_all(rollback).unwrap();
    }

    #[test]
    fn configure_refreshes_the_owned_config_hash() {
        let game = temp_dir("config-owned");
        let sources = temp_dir("config-owned-sources");
        let rollback = temp_dir("config-owned-rollback");
        let install = [InstallOperation {
            source: operation(&sources, "t7patch.conf", b"playername=Old\nunknown=keep\n").source,
            relative: PathBuf::from("t7patch.conf"),
        }];
        apply_managed_install(&game, &install, &[], &rollback.join("install")).unwrap();
        let before = load_manifest(&game).unwrap().unwrap().entries[0]
            .managed_sha256
            .clone();
        let updated = b"playername=^2New\nunknown=keep\n";
        write_config_with_ownership(&game, updated, &rollback.join("configure")).unwrap();
        let after = load_manifest(&game).unwrap().unwrap().entries[0]
            .managed_sha256
            .clone();
        assert_ne!(before, after);
        assert_eq!(
            after,
            fs_ops::sha256_file(&game.join("t7patch.conf")).unwrap()
        );

        apply_managed_uninstall(&game, &rollback.join("uninstall")).unwrap();
        assert!(!game.join("t7patch.conf").exists());
        fs::remove_dir_all(game).unwrap();
        fs::remove_dir_all(sources).unwrap();
        fs::remove_dir_all(rollback).unwrap();
    }

    #[test]
    fn legacy_install_without_manifest_is_left_byte_for_byte_untouched() {
        let game = temp_dir("legacy-untracked");
        let rollback = temp_dir("legacy-untracked-rollback");
        fs::create_dir_all(game.join("LPC")).unwrap();
        let active_patch = game.join("t7patch.dll");
        let active_lpc = game.join("LPC/english.ff");
        let patchops_backup = game.join("LPC/english.ff.patchops.bak");
        let legacy_backup = game.join("LPC/english.ff.bak");
        fs::write(&active_patch, b"legacy-managed-or-user-patch").unwrap();
        fs::write(&active_lpc, b"legacy-managed-or-user-lpc").unwrap();
        fs::write(&patchops_backup, b"ambiguous-patchops-backup").unwrap();
        fs::write(&legacy_backup, b"ambiguous-legacy-backup").unwrap();

        let error = apply_managed_uninstall(&game, &rollback).unwrap_err();
        assert!(error.contains("legacy or untracked install"));
        assert_eq!(
            fs::read(&active_patch).unwrap(),
            b"legacy-managed-or-user-patch"
        );
        assert_eq!(
            fs::read(&active_lpc).unwrap(),
            b"legacy-managed-or-user-lpc"
        );
        assert_eq!(
            fs::read(&patchops_backup).unwrap(),
            b"ambiguous-patchops-backup"
        );
        assert_eq!(
            fs::read(&legacy_backup).unwrap(),
            b"ambiguous-legacy-backup"
        );
        assert!(load_manifest(&game).unwrap().is_none());

        fs::remove_dir_all(game).unwrap();
        fs::remove_dir_all(rollback).unwrap();
    }

    #[test]
    fn manifest_cannot_claim_legacy_or_suffix_based_backups() {
        let game = temp_dir("reject-suffix-backup");
        let rollback = temp_dir("reject-suffix-backup-rollback");
        let active = game.join("t7patch.dll");
        let unrelated = game.join("t7patch.dll.bak");
        fs::write(&active, b"managed").unwrap();
        fs::write(&unrelated, b"unrelated-original").unwrap();
        let manifest = OwnershipManifest {
            version: OWNERSHIP_VERSION,
            entries: vec![ManagedFile {
                path: PathBuf::from("t7patch.dll"),
                managed_sha256: fs_ops::sha256_file(&active).unwrap(),
                original: OriginalFile::Overwritten {
                    backup_path: PathBuf::from("t7patch.dll.bak"),
                    sha256: fs_ops::sha256_file(&unrelated).unwrap(),
                    active_was_missing: false,
                },
            }],
        };
        let manifest_path = game.join(ownership_manifest_relative());
        fs::create_dir_all(manifest_path.parent().unwrap()).unwrap();
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();

        let error = apply_managed_uninstall(&game, &rollback).unwrap_err();
        assert!(error.contains("invalid backup path"));
        assert_eq!(fs::read(&active).unwrap(), b"managed");
        assert_eq!(fs::read(&unrelated).unwrap(), b"unrelated-original");
        assert!(manifest_path.is_file());

        fs::remove_dir_all(game).unwrap();
        fs::remove_dir_all(rollback).unwrap();
    }

    #[test]
    fn snapshot_restores_partial_file_changes() {
        let root = temp_dir("rollback");
        let existing = root.join("existing.dll");
        let created = root.join("created.dll");
        fs::write(&existing, b"original").unwrap();
        let snapshot = Snapshot::capture(
            &[existing.clone(), created.clone()],
            &root.join("snapshots"),
        )
        .unwrap();
        fs::write(&existing, b"partial").unwrap();
        fs::write(&created, b"partial").unwrap();
        snapshot.restore().unwrap();
        assert_eq!(fs::read(existing).unwrap(), b"original");
        assert!(!created.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
