use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use tar::Archive;
use walkdir::WalkDir;

use crate::logging::{LogCategory, log};

const DXVK_ASYNC_FILES: [&str; 2] = ["dxgi.dll", "d3d11.dll"];
const DXVK_RELEASE_API: &str = "https://gitlab.com/api/v4/projects/Ph42oN%2Fdxvk-gplasync/releases";

pub fn is_installed(game_dir: &Path) -> bool {
    DXVK_ASYNC_FILES
        .iter()
        .all(|file| game_dir.join(file).exists())
}

pub fn uninstall(game_dir: &Path) -> Result<()> {
    if !is_installed(game_dir) {
        log(LogCategory::Info, "DXVK-GPLAsync is not installed");
        return Ok(());
    }

    for file in DXVK_ASYNC_FILES {
        let path = game_dir.join(file);
        if path.exists() {
            fs::remove_file(&path).ok();
            log(LogCategory::Success, format!("Removed {}", file));
        }
    }
    let conf_path = game_dir.join("dxvk.conf");
    if conf_path.exists() {
        fs::remove_file(&conf_path).ok();
        log(LogCategory::Success, "Removed dxvk.conf");
    }
    log(LogCategory::Success, "DXVK-GPLAsync has been uninstalled");
    Ok(())
}

#[derive(Debug, serde::Deserialize)]
struct ReleaseAssetLink {
    url: String,
    name: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct ReleaseAssets {
    links: Option<Vec<ReleaseAssetLink>>,
    sources: Option<Vec<ReleaseAssetLink>>,
}

#[derive(Debug, serde::Deserialize)]
struct Release {
    assets: Option<ReleaseAssets>,
    name: Option<String>,
    tag_name: Option<String>,
}

fn preferred_asset_url(release: &Release) -> Result<String> {
    let mut candidates: Vec<&ReleaseAssetLink> = Vec::new();
    if let Some(ref assets) = release.assets {
        if let Some(ref links) = assets.links {
            for link in links {
                candidates.push(link);
            }
        }
        if candidates.is_empty() {
            if let Some(ref sources) = assets.sources {
                for source in sources {
                    candidates.push(source);
                }
            }
        }
    }
    if candidates.is_empty() {
        anyhow::bail!("No downloadable asset found in DXVK release metadata");
    }

    let preferred_suffixes = [
        ".zip", ".tar.xz", ".tar.gz", ".tar.bz2", ".tar.zst", ".tzst",
    ];
    for suffix in preferred_suffixes {
        if let Some(link) = candidates
            .iter()
            .find(|link| link.url.to_lowercase().ends_with(suffix))
        {
            return Ok(link.url.clone());
        }
    }
    Ok(candidates[0].url.clone())
}

pub fn install(game_dir: &Path, mod_dir: &Path) -> Result<()> {
    if is_installed(game_dir) {
        log(LogCategory::Info, "DXVK-GPLAsync is already installed");
        return Ok(());
    }

    log(LogCategory::Info, "Querying DXVK-GPLAsync releases...");
    let releases: Vec<Release> = reqwest::blocking::get(DXVK_RELEASE_API)?.json()?;
    let release = releases
        .get(0)
        .ok_or_else(|| anyhow::anyhow!("No releases returned from DXVK-GPLAsync API"))?;
    let url = preferred_asset_url(release)?;
    log(
        LogCategory::Info,
        format!(
            "Latest DXVK-GPLAsync release: {}",
            release
                .name
                .as_ref()
                .or(release.tag_name.as_ref())
                .map(String::as_str)
                .unwrap_or("Unknown")
        ),
    );

    fs::create_dir_all(mod_dir)?;
    let archive_path = download_file(mod_dir, &url)?;
    log(
        LogCategory::Success,
        format!("Downloaded DXVK archive from {}", url),
    );

    let extract_dir = mod_dir.join("dxvk_extracted");
    if extract_dir.exists() {
        fs::remove_dir_all(&extract_dir).ok();
    }
    fs::create_dir_all(&extract_dir)?;

    extract_archive(&archive_path, &extract_dir)?;
    log(LogCategory::Success, "Extracted DXVK archive");

    let mut x64_dir: Option<PathBuf> = None;
    for entry in WalkDir::new(&extract_dir)
        .into_iter()
        .filter_map(Result::ok)
    {
        if entry.file_type().is_file() {
            let file_name = entry.file_name().to_string_lossy();
            if DXVK_ASYNC_FILES.iter().all(|f| {
                entry
                    .path()
                    .parent()
                    .map(|p| p.join(f).exists())
                    .unwrap_or(false)
            }) {
                x64_dir = entry.path().parent().map(|p| p.to_path_buf());
                break;
            }
        }
    }

    if x64_dir.is_none() {
        anyhow::bail!("Required DXVK files (dxgi.dll, d3d11.dll) not found in extracted archive");
    }
    let x64_dir = x64_dir.unwrap();

    for file in DXVK_ASYNC_FILES {
        let src = x64_dir.join(file);
        let dst = game_dir.join(file);
        if src.exists() {
            fs::copy(&src, &dst)?;
            log(LogCategory::Success, format!("Installed {}", file));
        } else {
            anyhow::bail!("DXVK archive missing required file: {}", file);
        }
    }

    let conf_path = game_dir.join("dxvk.conf");
    let mut file = File::create(&conf_path)?;
    writeln!(file, "dxvk.enableAsync=true")?;
    writeln!(file, "dxvk.gplAsyncCache=true")?;
    writeln!(file, "dxvk.useRawSsbo=true")?;
    log(LogCategory::Success, "Wrote dxvk.conf");

    Ok(())
}

fn download_file(dir: &Path, url: &str) -> Result<PathBuf> {
    let mut response =
        reqwest::blocking::get(url).with_context(|| format!("Failed to download {}", url))?;
    let filename = response
        .url()
        .path_segments()
        .and_then(|segments| segments.last())
        .filter(|name| !name.is_empty())
        .map(String::from)
        .unwrap_or_else(|| "dxvk-gplasync".to_string());
    let path = dir.join(filename);
    let mut file = File::create(&path)?;
    std::io::copy(&mut response, &mut file)?;
    Ok(path)
}

fn extract_archive(archive_path: &Path, extract_dir: &Path) -> Result<()> {
    let lower = archive_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    if archive_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_ascii_lowercase().ends_with(".zip"))
        .unwrap_or(false)
    {
        return extract_zip(archive_path, extract_dir);
    }

    if lower == "gz"
        || lower == "tgz"
        || archive_path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_ascii_lowercase().ends_with(".tar.gz"))
            .unwrap_or(false)
    {
        return extract_tar_gz(archive_path, extract_dir);
    }

    if archive_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_ascii_lowercase().ends_with(".tar.xz"))
        .unwrap_or(false)
    {
        return extract_tar_xz(archive_path, extract_dir);
    }

    if archive_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| {
            name.to_ascii_lowercase().ends_with(".tar.zst")
                || name.to_ascii_lowercase().ends_with(".tzst")
        })
        .unwrap_or(false)
    {
        return extract_tar_zst(archive_path, extract_dir);
    }

    anyhow::bail!("Unsupported archive format: {}", archive_path.display())
}

fn extract_zip(archive_path: &Path, extract_dir: &Path) -> Result<()> {
    let file = File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = extract_dir.join(file.sanitized_name());
        if file.name().ends_with('/') {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut outfile = File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }
    Ok(())
}

fn extract_tar_gz(archive_path: &Path, extract_dir: &Path) -> Result<()> {
    let tar_gz = File::open(archive_path)?;
    let tar = GzDecoder::new(tar_gz);
    let mut archive = Archive::new(tar);
    archive.unpack(extract_dir)?;
    Ok(())
}

fn extract_tar_xz(archive_path: &Path, extract_dir: &Path) -> Result<()> {
    let file = File::open(archive_path)?;
    let mut decompressor = xz2::read::XzDecoder::new(file);
    let mut archive = Archive::new(&mut decompressor);
    archive.unpack(extract_dir)?;
    Ok(())
}

fn extract_tar_zst(archive_path: &Path, extract_dir: &Path) -> Result<()> {
    let file = File::open(archive_path)?;
    let decoder = zstd::stream::read::Decoder::new(file)?;
    let mut archive = Archive::new(decoder);
    archive.unpack(extract_dir)?;
    Ok(())
}
