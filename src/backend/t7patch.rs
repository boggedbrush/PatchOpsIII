use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use walkdir::WalkDir;

use crate::logging::{LogCategory, log};

const T7PATCH_URL: &str = "https://github.com/shiversoftdev/t7patch/releases/download/Current/Linux.Steamdeck.and.Manual.Windows.Install.zip";
const LPC_URL: &str =
    "https://github.com/shiversoftdev/t7patch/releases/download/Current/LPC.1.zip";

pub fn update_t7patch_conf(
    game_dir: &Path,
    new_name: Option<&str>,
    new_password: Option<&str>,
    friends_only: Option<bool>,
) -> Result<()> {
    let conf_path = game_dir.join("t7patch.conf");
    if !conf_path.exists() {
        log(
            LogCategory::Warning,
            format!("t7patch.conf not found in {}", game_dir.display()),
        );
        return Ok(());
    }

    let file = File::open(&conf_path)
        .with_context(|| format!("Failed to open {}", conf_path.display()))?;
    let reader = BufReader::new(file);
    let mut lines: Vec<String> = Vec::new();
    let mut name_found = false;
    let mut password_found = false;
    let mut friends_found = false;

    for line in reader.lines() {
        let line = line?;
        if let Some(name) = new_name {
            if line.starts_with("playername=") {
                lines.push(format!("playername={}", name));
                name_found = true;
                continue;
            }
        }
        if let Some(password) = new_password {
            if line.starts_with("networkpassword=") {
                lines.push(format!("networkpassword={}", password));
                password_found = true;
                continue;
            }
        }
        if let Some(flag) = friends_only {
            if line.starts_with("isfriendsonly=") {
                lines.push(format!("isfriendsonly={}", if flag { "1" } else { "0" }));
                friends_found = true;
                continue;
            }
        }
        lines.push(line);
    }

    if let Some(name) = new_name {
        if !name_found {
            lines.push(format!("playername={}", name));
        }
        log(
            LogCategory::Success,
            format!("Updated 'playername' to {}", name),
        );
    }
    if let Some(password) = new_password {
        if !password_found {
            lines.push(format!("networkpassword={}", password));
        }
        if password.is_empty() {
            log(LogCategory::Success, "Cleared network password");
        } else {
            log(LogCategory::Success, "Updated network password");
        }
    }
    if let Some(flag) = friends_only {
        if !friends_found {
            lines.push(format!("isfriendsonly={}", if flag { "1" } else { "0" }));
        }
        log(
            LogCategory::Success,
            format!("Set 'isfriendsonly' to {}", if flag { "On" } else { "Off" }),
        );
    }

    let mut file = File::create(&conf_path)
        .with_context(|| format!("Failed to open {} for writing", conf_path.display()))?;
    for line in lines {
        writeln!(file, "{}", line)?;
    }
    Ok(())
}

pub fn check_t7_patch_status(game_dir: &Path) -> Result<T7PatchStatus> {
    let conf_path = game_dir.join("t7patch.conf");
    let mut status = T7PatchStatus::default();
    if !conf_path.exists() {
        return Ok(status);
    }

    for line in BufReader::new(File::open(&conf_path)?).lines() {
        let line = line?;
        if line.starts_with("playername=") {
            status.gamertag = Some(
                line.split_once('=')
                    .map(|(_, v)| v.to_string())
                    .unwrap_or_default(),
            );
        } else if line.starts_with("networkpassword=") {
            status.password = Some(
                line.split_once('=')
                    .map(|(_, v)| v.to_string())
                    .unwrap_or_default(),
            );
        } else if line.starts_with("isfriendsonly=") {
            let value = line.split_once('=').map(|(_, v)| v == "1").unwrap_or(false);
            status.friends_only = Some(value);
        }
    }

    if let Some(ref tag) = status.gamertag {
        if tag.starts_with('^') && tag.len() >= 2 {
            status.color_code = Some(tag[..2].to_string());
            status.plain_name = Some(tag[2..].to_string());
        } else {
            status.plain_name = Some(tag.clone());
        }
    }

    Ok(status)
}

#[derive(Debug, Default, Clone)]
pub struct T7PatchStatus {
    pub gamertag: Option<String>,
    pub plain_name: Option<String>,
    pub color_code: Option<String>,
    pub password: Option<String>,
    pub friends_only: Option<bool>,
}

pub fn install_t7_patch(game_dir: &Path, mod_dir: &Path) -> Result<()> {
    log(LogCategory::Info, "Downloading T7 Patch...");
    let archive = download_to(mod_dir, T7PATCH_URL, "T7Patch.zip")?;
    let extract_dir = mod_dir.join("linux");
    if extract_dir.exists() {
        fs::remove_dir_all(&extract_dir).ok();
    }
    unzip(&archive, mod_dir)?;
    log(LogCategory::Success, "Extracted T7 Patch archive");

    if extract_dir.exists() {
        for entry in WalkDir::new(&extract_dir)
            .into_iter()
            .filter_map(Result::ok)
        {
            if entry.file_type().is_file() {
                let relative = entry
                    .path()
                    .strip_prefix(&extract_dir)
                    .unwrap_or(entry.path())
                    .to_path_buf();
                let destination = game_dir.join(&relative);
                if destination
                    .file_name()
                    .map(|f| f == "t7patch.conf")
                    .unwrap_or(false)
                    && destination.exists()
                {
                    continue;
                }
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(entry.path(), &destination)?;
            }
        }
    } else {
        anyhow::bail!("Extracted archive did not contain linux/ directory");
    }

    install_lpc_files(game_dir, mod_dir)?;
    log(LogCategory::Success, "T7 Patch installation complete");
    Ok(())
}

pub fn uninstall_t7_patch(game_dir: &Path, mod_dir: &Path) -> Result<()> {
    let files = [
        "t7patch.dll",
        "t7patch.conf",
        "discord_game_sdk.dll",
        "dsound.dll",
        "t7patchloader.dll",
        "zbr2.dll",
    ];

    for file in files {
        let target = game_dir.join(file);
        if target.exists() {
            fs::remove_file(&target).ok();
        }
    }
    let linux_dir = mod_dir.join("linux");
    if linux_dir.exists() {
        for entry in WalkDir::new(&linux_dir).into_iter().filter_map(Result::ok) {
            if entry.file_type().is_file() {
                let rel = entry
                    .path()
                    .strip_prefix(&linux_dir)
                    .unwrap_or(entry.path());
                let dest = mod_dir.join("linux").join(rel);
                let _ = fs::remove_file(dest);
            }
        }
        fs::remove_dir_all(&linux_dir).ok();
    }
    let _ = fs::remove_file(mod_dir.join("T7Patch.zip"));
    restore_lpc_backups(game_dir).ok();
    log(
        LogCategory::Success,
        "T7 Patch has been completely uninstalled",
    );
    Ok(())
}

pub fn install_lpc_files(game_dir: &Path, mod_dir: &Path) -> Result<()> {
    let archive = download_to(mod_dir, LPC_URL, "LPC.zip")?;
    let temp_dir = mod_dir.join("LPC_temp");
    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir).ok();
    }
    unzip(&archive, &temp_dir)?;

    let lpc_dir = game_dir.join("LPC");
    fs::create_dir_all(&lpc_dir)?;

    backup_lpc_files(game_dir)?;

    let src_lpc = if temp_dir.join("LPC").exists() {
        temp_dir.join("LPC")
    } else {
        temp_dir.clone()
    };

    for entry in WalkDir::new(&src_lpc).into_iter().filter_map(Result::ok) {
        if entry.file_type().is_file()
            && entry.path().extension().and_then(|s| s.to_str()) == Some("ff")
        {
            let dest = lpc_dir.join(entry.file_name());
            fs::copy(entry.path(), dest)?;
        }
    }

    fs::remove_file(&archive).ok();
    fs::remove_dir_all(&temp_dir).ok();
    log(LogCategory::Success, "Installed LPC files successfully");
    Ok(())
}

pub fn backup_lpc_files(game_dir: &Path) -> Result<()> {
    let lpc_dir = game_dir.join("LPC");
    fs::create_dir_all(&lpc_dir)?;
    let mut backed_up = 0u32;
    for entry in fs::read_dir(&lpc_dir)? {
        let entry = entry?;
        if entry.path().extension().and_then(|s| s.to_str()) == Some("ff") {
            let backup = entry.path().with_extension("ff.bak");
            if !backup.exists() {
                fs::rename(entry.path(), &backup)?;
                backed_up += 1;
            }
        }
    }
    if backed_up > 0 {
        log(
            LogCategory::Success,
            format!("Created backups for {} LPC files", backed_up),
        );
    }
    Ok(())
}

pub fn restore_lpc_backups(game_dir: &Path) -> Result<()> {
    let lpc_dir = game_dir.join("LPC");
    if !lpc_dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(&lpc_dir)? {
        let entry = entry?;
        let path = entry.path();
        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            if ext == "bak" {
                let dest = path.with_extension("ff");
                if dest.exists() {
                    fs::remove_file(&dest)?;
                }
                fs::rename(&path, &dest)?;
            }
        }
    }
    log(LogCategory::Success, "Restored LPC backups");
    Ok(())
}

fn download_to(dir: &Path, url: &str, filename: &str) -> Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let path = dir.join(filename);
    let mut response =
        reqwest::blocking::get(url).with_context(|| format!("Failed to download {}", url))?;
    if !response.status().is_success() {
        anyhow::bail!("Failed to download {}: {}", url, response.status());
    }
    let mut file = File::create(&path)?;
    while let Some(chunk) = response.chunk().transpose()? {
        file.write_all(&chunk)?;
    }
    Ok(path)
}

fn unzip(archive: &Path, destination: &Path) -> Result<()> {
    let file = File::open(archive)?;
    let mut archive = zip::ZipArchive::new(file)?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let outpath = destination.join(file.sanitized_name());

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
