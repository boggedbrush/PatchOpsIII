//! Archive extraction for the installers: `.zip` (T7 Patch, LPC, BO3 Enhanced) and `.tar.gz`
//! (DXVK-GPLAsync, GDK-Proton). Nothing else is downloaded, so nothing else is supported.
//!
//! Security (all members are checked *before* anything is written for them):
//! * zip-slip: absolute paths, drive prefixes and any `..` component are rejected;
//! * nothing is ever written through a symlink that an earlier member created;
//! * tar symlinks must stay inside the destination (lexically), hardlinks must point at an
//!   already extracted regular file inside it, device/fifo members are refused;
//! * limits on the number of members and the total unpacked size (zip-bomb guard), enforced on
//!   the bytes actually written, not on the sizes the headers claim.
//!
//! Zip reading reuses the [`crate::enhanced::zip`] reader (stored + deflate, CRC
//! checked). Inflate/CRC use `flate2` + `crc32fast`; gzip uses `flate2` (pure-Rust miniz_oxide backend); the tar parser is local
//! (ustar, GNU long names, pax `path`/`linkpath`/`size`).
//!
//! Errors are plain `String`s (the legacy code surfaced Python exception text).

use std::fs::{self, File};
use std::io::{self, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};

use crate::enhanced::zip::ZipArchive;

/// Caps applied to one extraction.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_entries: usize,
    pub max_total_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        // GDK-Proton unpacks to roughly 1.5 GB; the DXVK and T7 archives are a few MB.
        Limits { max_entries: 200_000, max_total_bytes: 6 * 1024 * 1024 * 1024 }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Extracted {
    pub files: usize,
    pub bytes: u64,
}

/// Extract by file name: `.zip`, `.tar.gz`, `.tgz` (case-insensitive).
pub fn extract_archive(archive: &Path, dest: &Path, limits: Limits) -> Result<Extracted, String> {
    let name = archive.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    if name.ends_with(".zip") {
        extract_zip(archive, dest, limits)
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        extract_tar_gz(archive, dest, limits)
    } else {
        Err(format!("Unsupported archive format: {}", archive.display()))
    }
}

// ---- path safety ----------------------------------------------------------------------------

/// Relative, normalised member path: strips leading separators, drops `.` and empty components,
/// rejects `..` and Windows drive/UNC prefixes. `Ok(None)` for names that normalise to nothing.
pub fn safe_relative_path(name: &str) -> Result<Option<PathBuf>, String> {
    let unsafe_name = || format!("Unsafe archive member path: {name}");
    let mut out = PathBuf::new();
    for part in name.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return Err(unsafe_name()),
            p if p.len() >= 2 && p.as_bytes()[1] == b':' && p.as_bytes()[0].is_ascii_alphabetic() => return Err(unsafe_name()),
            p if p.contains('\0') => return Err(unsafe_name()),
            p => out.push(p),
        }
    }
    if out.as_os_str().is_empty() {
        return Ok(None);
    }
    // Belt and braces: after normalisation only plain components may remain.
    if out.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(unsafe_name());
    }
    Ok(Some(out))
}

/// Create the parent directories of `dest_root/rel`, refusing to pass through a symlink, and return
/// the full target path.
fn prepare_target(dest_root: &Path, rel: &Path) -> Result<PathBuf, String> {
    let mut cur = dest_root.to_path_buf();
    let comps: Vec<_> = rel.components().collect();
    for (i, c) in comps.iter().enumerate() {
        cur.push(c);
        if i + 1 == comps.len() {
            break;
        }
        match fs::symlink_metadata(&cur) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(format!("Unsafe archive member path: {}", rel.display()));
            }
            Ok(m) if m.is_dir() => {}
            Ok(_) => return Err(format!("Archive member parent is not a directory: {}", rel.display())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => fs::create_dir(&cur).map_err(|e| io_text(&e, &cur))?,
            Err(e) => return Err(io_text(&e, &cur)),
        }
    }
    Ok(cur)
}

fn io_text(e: &io::Error, path: &Path) -> String {
    crate::pyfmt::io_error_text(e, Some(path))
}

/// Remove whatever sits at `path` (file or symlink; a real directory only if it is empty is not
/// our business: members replacing directories are refused).
fn clear_existing_file(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => Err(format!("Archive member collides with a directory: {}", path.display())),
        Ok(_) => fs::remove_file(path).map_err(|e| io_text(&e, path)),
        Err(_) => Ok(()),
    }
}

/// Write adapter that stops at `allowed` bytes and counts toward the total budget.
struct Budget<'a, W: Write> {
    inner: W,
    member_left: u64,
    total_left: &'a mut u64,
}

impl<W: Write> Write for Budget<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = buf.len() as u64;
        if n > self.member_left {
            return Err(io::Error::other("Archive member is larger than its header claims"));
        }
        if n > *self.total_left {
            return Err(io::Error::other("Archive exceeds the allowed unpacked size"));
        }
        self.member_left -= n;
        *self.total_left -= n;
        self.inner.write_all(buf)?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

// ---- zip ------------------------------------------------------------------------------------

pub fn extract_zip(archive: &Path, dest: &Path, limits: Limits) -> Result<Extracted, String> {
    let mut zip = ZipArchive::open(archive).map_err(|e| e.to_string())?;
    let entries = zip.entries().to_vec();
    if entries.len() > limits.max_entries {
        return Err(format!("Archive has too many members ({})", entries.len()));
    }
    // Declared total first (cheap early refusal), then enforced per byte while writing.
    let declared: u64 = entries.iter().map(|e| e.size).fold(0, u64::saturating_add);
    if declared > limits.max_total_bytes {
        return Err("Archive exceeds the allowed unpacked size".into());
    }
    fs::create_dir_all(dest).map_err(|e| io_text(&e, dest))?;
    let mut total_left = limits.max_total_bytes;
    let mut stats = Extracted::default();
    for entry in &entries {
        let Some(rel) = safe_relative_path(&entry.name)? else { continue };
        let target = prepare_target(dest, &rel)?;
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|e| io_text(&e, &target))?;
            continue;
        }
        clear_existing_file(&target)?;
        let file = File::create(&target).map_err(|e| io_text(&e, &target))?;
        let mut sink = Budget { inner: io::BufWriter::new(file), member_left: entry.size, total_left: &mut total_left };
        zip.extract(entry, &mut sink).map_err(|e| e.to_string())?;
        sink.flush().map_err(|e| io_text(&e, &target))?;
        stats.files += 1;
        stats.bytes += entry.size;
    }
    Ok(stats)
}

// ---- tar.gz ---------------------------------------------------------------------------------

pub fn extract_tar_gz(archive: &Path, dest: &Path, limits: Limits) -> Result<Extracted, String> {
    let file = File::open(archive).map_err(|e| io_text(&e, archive))?;
    let gz = flate2::read::MultiGzDecoder::new(BufReader::with_capacity(256 * 1024, file));
    extract_tar(gz, dest, limits)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    File,
    Dir,
    Symlink,
    Hardlink,
    Other(u8),
}

#[derive(Debug)]
struct Member {
    name: String,
    link: String,
    kind: Kind,
    size: u64,
    mode: u32,
}

fn str_field(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

fn octal(b: &[u8]) -> Result<u64, String> {
    // GNU base-256 for sizes that do not fit the octal field.
    if b.first().is_some_and(|c| c & 0x80 != 0) {
        let mut v = u64::from(b[0] & 0x7f);
        for &c in &b[1..] {
            v = v.checked_mul(256).ok_or("Invalid tar header (number too large)")? | u64::from(c);
        }
        return Ok(v);
    }
    let s = str_field(b);
    let s = s.trim_matches(|c: char| c == ' ' || c == '\0');
    if s.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(s, 8).map_err(|_| "Invalid tar header (bad number)".to_string())
}

fn read_full(r: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

fn skip(r: &mut impl Read, mut n: u64) -> io::Result<()> {
    let mut buf = [0u8; 8192];
    while n > 0 {
        let k = r.read(&mut buf[..(n.min(8192) as usize)])?;
        if k == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        n -= k as u64;
    }
    Ok(())
}

fn read_small(r: &mut impl Read, size: u64) -> Result<Vec<u8>, String> {
    if size > 1 << 20 {
        return Err("Invalid tar header (oversized metadata record)".into());
    }
    let mut data = vec![0u8; size as usize];
    r.read_exact(&mut data).map_err(|e| format!("Truncated tar archive: {e}"))?;
    skip(r, pad(size)).map_err(|e| format!("Truncated tar archive: {e}"))?;
    Ok(data)
}

fn pad(size: u64) -> u64 {
    (512 - size % 512) % 512
}

/// Size of a member's data including block padding (saturating: a hostile size then simply runs
/// into end-of-archive instead of overflowing).
fn padded(size: u64) -> u64 {
    size.saturating_add(pad(size))
}

/// pax extended header records: `"<len> <key>=<value>\n"`.
fn parse_pax(data: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut p = 0;
    while p < data.len() {
        let Some(sp) = data[p..].iter().position(|&c| c == b' ') else { break };
        let Ok(len) = String::from_utf8_lossy(&data[p..p + sp]).parse::<usize>() else { break };
        if len == 0 || len > data.len() - p || sp + 1 >= len {
            break;
        }
        let rec = &data[p + sp + 1..p + len];
        let rec = rec.strip_suffix(b"\n").unwrap_or(rec);
        if let Some(eq) = rec.iter().position(|&c| c == b'=') {
            out.push((String::from_utf8_lossy(&rec[..eq]).into_owned(), String::from_utf8_lossy(&rec[eq + 1..]).into_owned()));
        }
        p += len;
    }
    out
}

fn next_member(r: &mut impl Read) -> Result<Option<Member>, String> {
    let mut long_name: Option<String> = None;
    let mut long_link: Option<String> = None;
    let mut pax: Vec<(String, String)> = Vec::new();
    loop {
        let mut h = [0u8; 512];
        let n = read_full(r, &mut h).map_err(|e| format!("Truncated tar archive: {e}"))?;
        if n == 0 {
            return Ok(None);
        }
        if n < 512 {
            return Err("Truncated tar archive".into());
        }
        if h.iter().all(|&c| c == 0) {
            return Ok(None);
        }
        let stored = octal(&h[148..156])?;
        let sum: u64 = h.iter().enumerate().map(|(i, &c)| if (148..156).contains(&i) { 32 } else { u64::from(c) }).sum();
        if stored != sum {
            return Err("Invalid tar header (bad checksum)".into());
        }
        let typeflag = h[156];
        let mut size = octal(&h[124..136])?;
        match typeflag {
            b'L' => {
                long_name = Some(str_field(&read_small(r, size)?));
                continue;
            }
            b'K' => {
                long_link = Some(str_field(&read_small(r, size)?));
                continue;
            }
            b'x' => {
                pax = parse_pax(&read_small(r, size)?);
                continue;
            }
            b'g' => {
                read_small(r, size)?;
                continue;
            }
            _ => {}
        }
        let mut name = str_field(&h[0..100]);
        if &h[257..262] == b"ustar" {
            let prefix = str_field(&h[345..500]);
            if !prefix.is_empty() {
                name = format!("{prefix}/{name}");
            }
        }
        let mut link = str_field(&h[157..257]);
        if let Some(n) = long_name.take() {
            name = n;
        }
        if let Some(l) = long_link.take() {
            link = l;
        }
        for (k, v) in &pax {
            match k.as_str() {
                "path" => name = v.clone(),
                "linkpath" => link = v.clone(),
                "size" => size = v.parse().map_err(|_| "Invalid tar header (bad pax size)".to_string())?,
                _ => {}
            }
        }
        let kind = match typeflag {
            0 | b'0' | b'7' => Kind::File,
            b'5' => Kind::Dir,
            b'2' => Kind::Symlink,
            b'1' => Kind::Hardlink,
            other => Kind::Other(other),
        };
        let mode = octal(&h[100..108]).unwrap_or(0o644) as u32;
        return Ok(Some(Member { name, link, kind, size, mode }));
    }
}

/// Lexically resolve a symlink target against the link's parent directory (relative to the
/// extraction root); error if it is absolute or leaves the root.
fn check_link_target(member: &str, parent_rel: &Path, target: &str) -> Result<(), String> {
    let kind = |what: &str| format!("Symlink member {what}: {member}");
    if target.is_empty() {
        return Err(kind("missing link target"));
    }
    if target.starts_with('/') || target.starts_with('\\') || target.as_bytes().get(1) == Some(&b':') {
        return Err(kind("uses absolute link target"));
    }
    let mut depth = parent_rel.components().count() as i64;
    for part in target.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                depth -= 1;
                if depth < 0 {
                    return Err(kind("escapes extraction root"));
                }
            }
            _ => depth += 1,
        }
    }
    Ok(())
}

pub fn extract_tar(mut reader: impl Read, dest: &Path, limits: Limits) -> Result<Extracted, String> {
    fs::create_dir_all(dest).map_err(|e| io_text(&e, dest))?;
    let mut total_left = limits.max_total_bytes;
    let mut stats = Extracted::default();
    let mut members = 0usize;
    while let Some(m) = next_member(&mut reader)? {
        members += 1;
        if members > limits.max_entries {
            return Err("Archive has too many members".into());
        }
        let rel = match safe_relative_path(&m.name)? {
            Some(r) => r,
            None => {
                skip(&mut reader, padded(m.size)).map_err(|e| format!("Truncated tar archive: {e}"))?;
                continue;
            }
        };
        match m.kind {
            Kind::Other(_) => return Err(format!("Archive member uses forbidden device type or is unsupported: {}", m.name)),
            Kind::Dir => {
                let target = prepare_target(dest, &rel)?;
                fs::create_dir_all(&target).map_err(|e| io_text(&e, &target))?;
                skip(&mut reader, padded(m.size)).map_err(|e| format!("Truncated tar archive: {e}"))?;
            }
            Kind::Symlink => {
                let parent = rel.parent().unwrap_or(Path::new(""));
                check_link_target(&m.name, parent, &m.link)?;
                let target = prepare_target(dest, &rel)?;
                clear_existing_file(&target)?;
                make_symlink(&m.link, &target)?;
                stats.files += 1;
                skip(&mut reader, padded(m.size)).map_err(|e| format!("Truncated tar archive: {e}"))?;
            }
            Kind::Hardlink => {
                let src_rel = safe_relative_path(&m.link)
                    .map_err(|_| format!("Hardlink member escapes extraction root: {}", m.name))?
                    .ok_or_else(|| format!("Hardlink member missing link target: {}", m.name))?;
                let src = prepare_target(dest, &src_rel)?;
                let meta = fs::symlink_metadata(&src)
                    .map_err(|_| format!("Hardlink source is missing during extraction: {}", m.name))?;
                if !meta.is_file() {
                    return Err(format!("Hardlink source is not a regular file: {}", m.name));
                }
                let target = prepare_target(dest, &rel)?;
                clear_existing_file(&target)?;
                fs::hard_link(&src, &target).map_err(|e| io_text(&e, &target))?;
                stats.files += 1;
                skip(&mut reader, padded(m.size)).map_err(|e| format!("Truncated tar archive: {e}"))?;
            }
            Kind::File => {
                if m.size > total_left {
                    return Err("Archive exceeds the allowed unpacked size".into());
                }
                let target = prepare_target(dest, &rel)?;
                clear_existing_file(&target)?;
                let file = File::create(&target).map_err(|e| io_text(&e, &target))?;
                {
                    let mut sink = Budget { inner: io::BufWriter::new(file), member_left: m.size, total_left: &mut total_left };
                    let copied = io::copy(&mut (&mut reader).take(m.size), &mut sink).map_err(|e| e.to_string())?;
                    if copied != m.size {
                        return Err(format!("Truncated tar archive while reading {}", m.name));
                    }
                    sink.flush().map_err(|e| io_text(&e, &target))?;
                }
                set_mode(&target, m.mode);
                skip(&mut reader, pad(m.size)).map_err(|e| format!("Truncated tar archive: {e}"))?;
                stats.files += 1;
                stats.bytes += m.size;
            }
        }
    }
    Ok(stats)
}

#[cfg(unix)]
fn make_symlink(link_target: &str, at: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(link_target, at).map_err(|e| io_text(&e, at))
}

#[cfg(not(unix))]
fn make_symlink(_link_target: &str, at: &Path) -> Result<(), String> {
    Err(format!("Symlinks in archives are not supported on this platform: {}", at.display()))
}

/// Keep the permission bits (the Proton tool needs its executables); never setuid/setgid/sticky.
#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode((mode & 0o777) | 0o600));
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) {}

// ---- tests ----------------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod testutil {
    //! Builders for tiny archives used by tests in several modules.

    use std::io::Write;

    fn octal_field(buf: &mut [u8], v: u64) {
        let s = format!("{:0width$o}\0", v, width = buf.len() - 1);
        buf.copy_from_slice(s.as_bytes());
    }

    pub struct TarMember<'a> {
        pub name: &'a str,
        pub typeflag: u8,
        pub link: &'a str,
        pub data: &'a [u8],
        pub mode: u64,
    }

    impl<'a> TarMember<'a> {
        pub fn file(name: &'a str, data: &'a [u8]) -> Self {
            TarMember { name, typeflag: b'0', link: "", data, mode: 0o644 }
        }
        pub fn dir(name: &'a str) -> Self {
            TarMember { name, typeflag: b'5', link: "", data: b"", mode: 0o755 }
        }
        pub fn symlink(name: &'a str, link: &'a str) -> Self {
            TarMember { name, typeflag: b'2', link, data: b"", mode: 0o777 }
        }
    }

    pub fn tar(members: &[TarMember]) -> Vec<u8> {
        let mut out = Vec::new();
        for m in members {
            let mut h = [0u8; 512];
            h[..m.name.len()].copy_from_slice(m.name.as_bytes());
            octal_field(&mut h[100..108], m.mode);
            octal_field(&mut h[108..116], 0);
            octal_field(&mut h[116..124], 0);
            octal_field(&mut h[124..136], m.data.len() as u64);
            octal_field(&mut h[136..148], 0);
            h[148..156].fill(b' ');
            h[156] = m.typeflag;
            h[157..157 + m.link.len()].copy_from_slice(m.link.as_bytes());
            h[257..263].copy_from_slice(b"ustar\0");
            h[263..265].copy_from_slice(b"00");
            let sum: u64 = h.iter().map(|&c| u64::from(c)).sum();
            let s = format!("{sum:06o}\0 ");
            h[148..156].copy_from_slice(s.as_bytes());
            out.extend_from_slice(&h);
            out.extend_from_slice(m.data);
            out.extend(std::iter::repeat_n(0u8, (512 - m.data.len() % 512) % 512));
        }
        out.extend(std::iter::repeat_n(0u8, 1024));
        out
    }

    pub fn gzip(data: &[u8]) -> Vec<u8> {
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(data).unwrap();
        enc.finish().unwrap()
    }

    pub fn tar_gz(members: &[TarMember]) -> Vec<u8> {
        gzip(&tar(members))
    }

    /// A zip with *stored* members (uses the enhanced agent's test builder).
    pub fn zip(members: &[(&str, &[u8])]) -> Vec<u8> {
        crate::enhanced::zip::testutil::stored_zip(members)
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("patchops-archive-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn path_normalisation() {
        assert_eq!(safe_relative_path("a/b.txt").unwrap(), Some(PathBuf::from("a/b.txt")));
        assert_eq!(safe_relative_path("/abs/x").unwrap(), Some(PathBuf::from("abs/x")));
        assert_eq!(safe_relative_path("./a//b/").unwrap(), Some(PathBuf::from("a/b")));
        assert_eq!(safe_relative_path("./").unwrap(), None);
        assert_eq!(safe_relative_path("a\\b").unwrap(), Some(PathBuf::from("a/b")));
        for bad in ["../x", "a/../../x", "a/..", "C:/x", "c:\\x", "..\\x"] {
            assert!(safe_relative_path(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn tar_gz_extracts_files_dirs_symlinks_and_modes() {
        let dir = tmp("tar");
        let archive = dir.join("t.tar.gz");
        fs::write(
            &archive,
            tar_gz(&[
                TarMember::dir("pkg/"),
                TarMember::file("pkg/x64/dxgi.dll", b"dxgi"),
                TarMember { mode: 0o755, ..TarMember::file("pkg/proton", b"#!/bin/sh\n") },
                TarMember::symlink("pkg/link", "x64/dxgi.dll"),
                TarMember::symlink("pkg/x64/up", "../proton"),
            ]),
        )
        .unwrap();
        let out = dir.join("out");
        let stats = extract_archive(&archive, &out, Limits::default()).unwrap();
        assert_eq!(stats.files, 4);
        assert_eq!(fs::read(out.join("pkg/x64/dxgi.dll")).unwrap(), b"dxgi");
        assert_eq!(fs::read(out.join("pkg/link")).unwrap(), b"dxgi");
        assert_eq!(fs::read(out.join("pkg/x64/up")).unwrap(), b"#!/bin/sh\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(out.join("pkg/proton")).unwrap().permissions().mode() & 0o777, 0o755);
            assert_eq!(fs::metadata(out.join("pkg/x64/dxgi.dll")).unwrap().permissions().mode() & 0o777, 0o644);
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn tar_rejects_traversal_unsafe_links_and_devices() {
        let dir = tmp("tar-bad");
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("dotdot", tar(&[TarMember::file("../evil", b"x")])),
            ("dotdot-mid", tar(&[TarMember::file("a/../../evil", b"x")])),
            ("abs-link", tar(&[TarMember::symlink("l", "/etc/passwd")])),
            ("escape-link", tar(&[TarMember::symlink("l", "../outside")])),
            ("deep-escape", tar(&[TarMember::dir("a/"), TarMember::symlink("a/l", "../../outside")])),
            ("device", tar(&[TarMember { typeflag: b'3', ..TarMember::file("dev", b"") }])),
            (
                "through-symlink",
                tar(&[TarMember::dir("d/"), TarMember::symlink("d/s", "."), TarMember::file("d/s/../x", b"x")]),
            ),
        ];
        for (name, data) in cases {
            let out = dir.join(name);
            let err = extract_tar(&data[..], &out, Limits::default());
            assert!(err.is_err(), "{name} must be rejected");
            assert!(!dir.join("evil").exists() && !dir.join("outside").exists());
        }
        // Writing *through* an extracted symlink is refused as well.
        let out = dir.join("sym");
        let data = tar(&[TarMember::symlink("s", "sub"), TarMember::file("s/x", b"x")]);
        assert!(extract_tar(&data[..], &out, Limits::default()).unwrap_err().starts_with("Unsafe archive member path"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn tar_limits_and_corruption() {
        let dir = tmp("tar-limits");
        let data = tar(&[TarMember::file("a", &[1u8; 5000])]);
        let small = Limits { max_entries: 10, max_total_bytes: 1000 };
        assert!(extract_tar(&data[..], &dir.join("a"), small).unwrap_err().contains("allowed unpacked size"));
        let many = tar(&[TarMember::file("a", b"1"), TarMember::file("b", b"2"), TarMember::file("c", b"3")]);
        let few = Limits { max_entries: 2, max_total_bytes: 1 << 20 };
        assert!(extract_tar(&many[..], &dir.join("b"), few).unwrap_err().contains("too many members"));
        let mut bad = tar(&[TarMember::file("a", b"1")]);
        bad[10] ^= 0xff;
        assert!(extract_tar(&bad[..], &dir.join("c"), Limits::default()).unwrap_err().contains("bad checksum"));
        let truncated = &tar(&[TarMember::file("a", &[7u8; 2000])])[..700];
        assert!(extract_tar(truncated, &dir.join("d"), Limits::default()).is_err());
        assert!(extract_archive(&dir.join("x.rar"), &dir, Limits::default()).unwrap_err().starts_with("Unsupported archive format"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn zip_extracts_and_blocks_slip() {
        let dir = tmp("zip");
        let good = dir.join("g.zip");
        fs::write(&good, zip(&[("LPC/", b""), ("LPC/a.ff", b"AAA"), ("t7patch.dll", b"dll")])).unwrap();
        let out = dir.join("out");
        let stats = extract_archive(&good, &out, Limits::default()).unwrap();
        assert_eq!(stats.files, 2);
        assert_eq!(fs::read(out.join("LPC/a.ff")).unwrap(), b"AAA");

        for (i, evil) in ["../evil.txt", "a/../../evil.txt", "/etc/evil.txt", "C:\\evil.txt", "..\\evil.txt"].iter().enumerate() {
            let bad = dir.join(format!("b{i}.zip"));
            fs::write(&bad, zip(&[("ok.txt", b"1"), (evil, b"x")])).unwrap();
            let out = dir.join(format!("o{i}"));
            let r = extract_zip(&bad, &out, Limits::default());
            // "/etc/evil.txt" normalises to a path *inside* the root (leading slash stripped).
            if *evil == "/etc/evil.txt" {
                assert!(r.is_ok() && out.join("etc/evil.txt").exists());
            } else {
                assert!(r.is_err(), "{evil}");
            }
            assert!(!dir.join("evil.txt").exists() && !std::path::Path::new("/etc/evil.txt").exists());
        }
        let limited = Limits { max_entries: 1, max_total_bytes: 1 << 20 };
        assert!(extract_zip(&good, &dir.join("l"), limited).unwrap_err().contains("too many members"));
        let tiny = Limits { max_entries: 10, max_total_bytes: 4 };
        assert!(extract_zip(&good, &dir.join("l2"), tiny).unwrap_err().contains("allowed unpacked size"));
        fs::write(dir.join("notzip.zip"), b"hello").unwrap();
        assert!(extract_zip(&dir.join("notzip.zip"), &dir.join("l3"), Limits::default()).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    fn set_size_base256(tar: &mut [u8], size: u64) {
        tar[124..136].fill(0);
        tar[124] = 0x80;
        tar[128..136].copy_from_slice(&size.to_be_bytes());
        tar[148..156].fill(b' ');
        let sum: u64 = tar[..512].iter().map(|&c| u64::from(c)).sum();
        tar[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    }

    #[test]
    fn tar_hostile_pax_and_sizes_error_instead_of_panicking() {
        let dir = tmp("tar-hostile");
        // pax record whose declared length wraps `p + len` (regression: slice panic).
        for rec in ["18446744073709551615 path=x\n", "18446744073709551610 a=b\n", "99999999999999999999 a=b\n", "2 \n"] {
            let pax = TarMember { typeflag: b'x', ..TarMember::file("pax", rec.as_bytes()) };
            let data = tar(&[pax, TarMember::file("a", b"1")]);
            let _ = extract_tar(&data[..], &dir.join("p"), Limits::default()); // must not panic
        }
        // Sizes near u64::MAX: `size + padding` overflowed for dir/symlink/hardlink/skipped members.
        for member in [TarMember::dir("d/"), TarMember::symlink("l", "x"), TarMember::dir("./")] {
            for size in [u64::MAX, u64::MAX - 100, u64::MAX - 511] {
                let mut data = tar(&[member_clone(&member)]);
                set_size_base256(&mut data, size);
                assert!(extract_tar(&data[..], &dir.join("s"), Limits::default()).is_err(), "{size}");
            }
        }
        let _ = fs::remove_dir_all(dir);
    }

    fn member_clone<'a>(m: &TarMember<'a>) -> TarMember<'a> {
        TarMember { name: m.name, typeflag: m.typeflag, link: m.link, data: m.data, mode: m.mode }
    }
}
