use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};

pub const PATCHOPS_BACKUP_SUFFIX: &str = ".patchops.bak";
pub const LEGACY_BACKUP_SUFFIX: &str = ".bak";
const MAX_ARCHIVE_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 100_000;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) struct Snapshot {
    entries: Vec<(PathBuf, Option<PathBuf>)>,
}

pub(crate) fn remove_file_if_present(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(format!(
            "refusing to remove non-file path {}",
            path.display()
        )),
        Ok(_) => fs::remove_file(path).map_err(|error| error.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

impl Snapshot {
    pub(crate) fn capture(targets: &[PathBuf], directory: &Path) -> Result<Self, String> {
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
        let mut seen = HashSet::new();
        let mut entries = Vec::new();
        for target in targets {
            if !seen.insert(target.clone()) {
                continue;
            }
            let backup = match fs::symlink_metadata(target) {
                Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                    let backup = directory.join(entries.len().to_string());
                    fs::copy(target, &backup).map_err(|error| error.to_string())?;
                    Some(backup)
                }
                Ok(_) => return Err(format!("refusing to replace {}", target.display())),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.to_string()),
            };
            entries.push((target.clone(), backup));
        }
        Ok(Self { entries })
    }

    pub(crate) fn restore(&self) -> Result<(), String> {
        let mut errors = Vec::new();
        for (target, backup) in self.entries.iter().rev() {
            if let Err(error) = remove_file_if_present(target) {
                errors.push(error);
                continue;
            }
            if let Some(backup) = backup {
                if let Some(parent) = target.parent() {
                    if let Err(error) = fs::create_dir_all(parent) {
                        errors.push(format!("failed to recreate {}: {error}", parent.display()));
                        continue;
                    }
                }
                if let Err(error) = fs::copy(backup, target) {
                    errors.push(format!("failed to restore {}: {error}", target.display()));
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "invalid output filename".to_string())?;
    let (temporary, mut file) = (0..100)
        .find_map(|_| {
            let id = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let temporary =
                parent.join(format!(".{name}.patchops-{}-{id}.tmp", std::process::id()));
            match OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)
            {
                Ok(file) => Some(Ok((temporary, file))),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(error) => Some(Err(error.to_string())),
            }
        })
        .unwrap_or_else(|| Err("could not allocate a unique temporary file".into()))?;
    let write_result = file
        .write_all(contents)
        .and_then(|_| file.sync_all())
        .map_err(|error| error.to_string());
    drop(file);
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    if let Err(error) = replace_path(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_path(source: &Path, destination: &Path) -> Result<(), String> {
    fs::rename(source, destination).map_err(|error| error.to_string())
}

#[cfg(windows)]
fn replace_path(source: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let ok = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}

pub fn backup_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}{}", path.display(), PATCHOPS_BACKUP_SUFFIX))
}

pub fn legacy_backup_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}{}", path.display(), LEGACY_BACKUP_SUFFIX))
}

pub fn existing_backup(path: &Path) -> Option<PathBuf> {
    [backup_path(path), legacy_backup_path(path)]
        .into_iter()
        .find(|candidate| candidate.is_file())
}

pub fn download(
    url: &str,
    destination: &Path,
    expected_sha256: Option<&str>,
    max_bytes: u64,
) -> Result<String, String> {
    let response = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|error| error.to_string())?
        .get(url)
        .header(reqwest::header::USER_AGENT, "PatchOpsIII")
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| error.to_string())?;
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes)
    {
        return Err("download is larger than the allowed size".into());
    }

    let parent = destination
        .parent()
        .ok_or_else(|| "download destination has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let part = PathBuf::from(format!("{}.part", destination.display()));
    let mut output = File::create(&part).map_err(|error| error.to_string())?;
    let mut input = response;
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 128 * 1024];
    let result = (|| {
        loop {
            let read = input.read(&mut buffer).map_err(|error| error.to_string())?;
            if read == 0 {
                break;
            }
            total += read as u64;
            if total > max_bytes {
                return Err("download is larger than the allowed size".into());
            }
            digest.update(&buffer[..read]);
            output
                .write_all(&buffer[..read])
                .map_err(|error| error.to_string())?;
        }
        output.sync_all().map_err(|error| error.to_string())?;
        let actual = format!("{:x}", digest.finalize());
        if expected_sha256.is_some_and(|expected| !actual.eq_ignore_ascii_case(expected)) {
            return Err("download failed SHA-256 verification".into());
        }
        replace_path(&part, destination)?;
        Ok(actual)
    })();
    if result.is_err() {
        let _ = fs::remove_file(part);
    }
    result
}

fn safe_relative(path: &Path) -> bool {
    path.components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

fn resolved_link_target(base: &Path, target: &Path) -> Option<PathBuf> {
    let mut parts = base
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_owned()),
            Component::CurDir => None,
            _ => None,
        })
        .collect::<Vec<_>>();
    for component in target.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => parts.push(part.to_owned()),
            Component::ParentDir => {
                parts.pop()?;
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(parts.into_iter().collect())
}

fn prepare_empty_directory(destination: &Path) -> Result<(), String> {
    match fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err("archive destination is not a regular directory".into());
        }
        Ok(_) => {
            if fs::read_dir(destination)
                .map_err(|error| error.to_string())?
                .next()
                .is_some()
            {
                return Err("archive destination must be empty".into());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(destination).map_err(|error| error.to_string())?;
        }
        Err(error) => return Err(error.to_string()),
    }
    Ok(())
}

pub fn extract_zip(archive: &Path, destination: &Path) -> Result<(), String> {
    prepare_empty_directory(destination)?;
    let file = File::open(archive).map_err(|error| error.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
    if zip.len() > MAX_ARCHIVE_ENTRIES {
        return Err("archive contains too many entries".into());
    }
    let mut total = 0_u64;
    let mut seen = HashSet::new();
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|error| error.to_string())?;
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| "archive contains an unsafe path".to_string())?
            .to_path_buf();
        if !safe_relative(&relative) || !seen.insert(relative.clone()) {
            return Err("archive contains an unsafe or duplicate path".into());
        }
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("archive links are not allowed".into());
        }
        total = total.saturating_add(entry.size());
        if total > MAX_ARCHIVE_BYTES {
            return Err("archive expands beyond the allowed size".into());
        }
        let output = destination.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&output).map_err(|error| error.to_string())?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut file = File::create(output).map_err(|error| error.to_string())?;
        std::io::copy(&mut entry, &mut file).map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub fn extract_tar_gz(archive: &Path, destination: &Path) -> Result<(), String> {
    prepare_empty_directory(destination)?;
    let decoder = GzDecoder::new(File::open(archive).map_err(|error| error.to_string())?);
    let mut tar = tar::Archive::new(decoder);
    let mut count = 0_usize;
    let mut total = 0_u64;
    let mut seen = HashSet::new();
    let mut hard_links = Vec::new();
    for item in tar.entries().map_err(|error| error.to_string())? {
        let mut entry = item.map_err(|error| error.to_string())?;
        count += 1;
        if count > MAX_ARCHIVE_ENTRIES {
            return Err("archive contains too many entries".into());
        }
        let relative = entry
            .path()
            .map_err(|error| error.to_string())?
            .into_owned();
        if !safe_relative(&relative) || !seen.insert(relative.clone()) {
            return Err("archive contains an unsafe or duplicate path".into());
        }
        let kind = entry.header().entry_type();
        total = total.saturating_add(entry.size());
        if total > MAX_ARCHIVE_BYTES {
            return Err("archive expands beyond the allowed size".into());
        }
        let output = destination.join(&relative);
        if kind.is_file() || kind.is_dir() {
            entry.unpack(&output).map_err(|error| error.to_string())?;
            continue;
        }
        if !kind.is_symlink() && !kind.is_hard_link() {
            return Err("archive contains a special file".into());
        }
        let target = entry
            .link_name()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "archive link has no target".to_string())?
            .into_owned();
        let base = if kind.is_symlink() {
            relative.parent().unwrap_or_else(|| Path::new(""))
        } else {
            Path::new("")
        };
        let resolved = resolved_link_target(base, &target)
            .ok_or_else(|| "archive link escapes the destination".to_string())?;
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        if kind.is_hard_link() {
            hard_links.push((destination.join(resolved), output));
            continue;
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &output).map_err(|error| error.to_string())?;
        #[cfg(not(unix))]
        return Err("archive symbolic links are not supported on this platform".into());
    }
    for (target, output) in hard_links {
        if !target.is_file() {
            return Err("archive hard link target is missing or is not a file".into());
        }
        fs::hard_link(target, output).map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "patchops-{label}-{}-{}",
                std::process::id(),
                TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_zip(path: &Path, method: CompressionMethod, entries: &[(&str, &[u8])]) {
        let mut writer = ZipWriter::new(File::create(path).unwrap());
        let options = SimpleFileOptions::default().compression_method(method);
        for (name, contents) in entries {
            writer.start_file(*name, options).unwrap();
            writer.write_all(contents).unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn snapshot_restores_deleted_parents_and_removes_new_files() {
        let root = TestDirectory::new("snapshot");
        let parent = root.path().join("game");
        fs::create_dir_all(&parent).unwrap();
        let original = parent.join("original.dll");
        let created = root.path().join("new.dll");
        fs::write(&original, b"original").unwrap();
        let snapshot = Snapshot::capture(
            &[original.clone(), created.clone(), original.clone()],
            &root.path().join("rollback"),
        )
        .unwrap();
        assert_eq!(snapshot.entries.len(), 2);
        fs::remove_dir_all(&parent).unwrap();
        fs::write(&created, b"new").unwrap();
        snapshot.restore().unwrap();
        assert_eq!(fs::read(&original).unwrap(), b"original");
        assert!(!created.exists());
    }

    #[test]
    fn hashes_known_bytes() {
        let path = std::env::temp_dir().join(format!("patchops-hash-{}", std::process::id()));
        fs::write(&path, b"patchops").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            "3e0cec93c1876296af254daff5dba161649c003683320e5fefbaf1ad21e67f98"
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_parent_paths() {
        assert!(!safe_relative(Path::new("../outside")));
        assert!(!safe_relative(Path::new("/outside")));
        assert!(safe_relative(Path::new("inside/file")));
    }

    #[test]
    fn link_targets_must_resolve_inside_the_archive_root() {
        assert_eq!(
            resolved_link_target(Path::new("bin"), Path::new("../lib/tool")),
            Some(PathBuf::from("lib/tool"))
        );
        assert_eq!(
            resolved_link_target(Path::new("bin"), Path::new("../../outside")),
            None
        );
        assert_eq!(
            resolved_link_target(Path::new(""), Path::new("/outside")),
            None
        );
    }

    #[test]
    fn extracts_each_enabled_zip_compression_method() {
        let root = TestDirectory::new("zip-compression-methods");
        let payload = b"PatchOpsIII archive fixture\n".repeat(128);
        for (label, method) in [
            ("stored", CompressionMethod::Stored),
            ("deflate", CompressionMethod::Deflated),
            ("bzip2", CompressionMethod::Bzip2),
            ("zstd", CompressionMethod::Zstd),
        ] {
            let archive = root.path().join(format!("{label}.zip"));
            let destination = root.path().join(format!("{label}-output"));
            write_zip(
                &archive,
                method,
                &[("nested/payload.txt", payload.as_slice())],
            );
            {
                let mut zip = zip::ZipArchive::new(File::open(&archive).unwrap()).unwrap();
                assert_eq!(zip.by_index(0).unwrap().compression(), method);
            }

            extract_zip(&archive, &destination).unwrap();

            assert_eq!(
                fs::read(destination.join("nested/payload.txt")).unwrap(),
                payload
            );
        }
    }

    #[test]
    fn rejects_malformed_zip_without_writing_files() {
        let root = TestDirectory::new("malformed-zip");
        let archive = root.path().join("malformed.zip");
        let destination = root.path().join("output");
        fs::write(&archive, b"this is not a ZIP archive").unwrap();

        assert!(extract_zip(&archive, &destination).is_err());
        assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
    }

    #[test]
    fn rejects_zip_path_traversal_and_allows_caller_cleanup() {
        let root = TestDirectory::new("zip-traversal");
        let archive = root.path().join("traversal.zip");
        let destination = root.path().join("output");
        let entries: &[(&str, &[u8])] = &[
            ("safe.txt", b"written before the invalid entry"),
            ("../outside.txt", b"must not escape"),
        ];
        write_zip(&archive, CompressionMethod::Stored, entries);

        let error = extract_zip(&archive, &destination).unwrap_err();

        assert!(error.contains("unsafe path"), "{error}");
        assert_eq!(
            fs::read(destination.join("safe.txt")).unwrap(),
            entries[0].1
        );
        assert!(!root.path().join("outside.txt").exists());
        fs::remove_dir_all(&destination).unwrap();
        assert!(!destination.exists());
    }

    #[test]
    fn rejects_zip_symlinks() {
        let root = TestDirectory::new("zip-symlink");
        let archive = root.path().join("symlink.zip");
        let destination = root.path().join("output");
        let mut writer = ZipWriter::new(File::create(&archive).unwrap());
        writer
            .add_symlink("link", "../outside", SimpleFileOptions::default())
            .unwrap();
        writer.finish().unwrap();

        let error = extract_zip(&archive, &destination).unwrap_err();

        assert!(error.contains("links are not allowed"), "{error}");
        assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
    }

    #[test]
    fn rejects_equivalent_duplicate_zip_paths_without_overwriting_partial_output() {
        let root = TestDirectory::new("zip-duplicate");
        let archive = root.path().join("duplicate.zip");
        let destination = root.path().join("output");
        let entries: &[(&str, &[u8])] = &[
            ("nested/duplicate.txt", b"first entry"),
            ("nested/./duplicate.txt", b"second entry"),
        ];
        write_zip(&archive, CompressionMethod::Stored, entries);

        let error = extract_zip(&archive, &destination).unwrap_err();

        assert!(error.contains("duplicate path"), "{error}");
        assert_eq!(
            fs::read(destination.join("nested/duplicate.txt")).unwrap(),
            b"first entry"
        );
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_does_not_follow_a_predictable_temporary_symlink() {
        let root = std::env::temp_dir().join(format!(
            "patchops-atomic-{}-{}",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let target = root.join("settings.json");
        let victim = root.join("victim");
        fs::write(&victim, b"untouched").unwrap();
        std::os::unix::fs::symlink(&victim, root.join(".settings.json.patchops.tmp")).unwrap();

        atomic_write(&target, b"new settings").unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"new settings");
        assert_eq!(fs::read(&victim).unwrap(), b"untouched");
        fs::remove_dir_all(root).unwrap();
    }
}
