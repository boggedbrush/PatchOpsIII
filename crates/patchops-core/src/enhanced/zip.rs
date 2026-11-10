//! Minimal ZIP reader (container parsing is local; inflate/CRC via flate2 + crc32fast): central-directory listing (incl. ZIP64 and prepended data,
//! like Python's `zipfile`) plus extraction of *stored* and *deflated* members with CRC-32 checking.
//!
//! It exists because BO3 Enhanced needs (a) directory listings of multi-GB dump archives for
//! validation and (b) extraction of a few whitelisted members during install. Encryption and
//! exotic compression methods are rejected.

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

#[derive(Debug)]
pub enum ZipError {
    /// Not a (readable) zip: Python's `BadZipFile`.
    Bad(String),
    Io(io::Error),
}

impl std::fmt::Display for ZipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ZipError::Bad(m) => f.write_str(m),
            ZipError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl From<io::Error> for ZipError {
    fn from(e: io::Error) -> Self {
        ZipError::Io(e)
    }
}

#[derive(Debug, Clone)]
pub struct ZipEntry {
    pub name: String,
    pub method: u16,
    pub flags: u16,
    pub crc32: u32,
    pub compressed_size: u64,
    pub size: u64,
    local_offset: u64,
}

impl ZipEntry {
    pub fn is_dir(&self) -> bool {
        self.name.ends_with('/')
    }
}

pub struct ZipArchive {
    file: File,
    entries: Vec<ZipEntry>,
}

const SIG_EOCD: [u8; 4] = *b"PK\x05\x06";
const SIG_EOCD64: u32 = 0x0606_4b50;
const SIG_LOC64: u32 = 0x0706_4b50;
const SIG_CEN: u32 = 0x0201_4b50;
const SIG_LOCAL: u32 = 0x0403_4b50;

fn u16le(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn u64le(b: &[u8], o: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[o..o + 8]);
    u64::from_le_bytes(a)
}

struct EndRecord {
    /// Offset of the end-of-central-directory record in the file.
    pos: u64,
    entries: u64,
    cd_size: u64,
    cd_offset: u64,
}

fn find_end_record(f: &mut File) -> Result<EndRecord, ZipError> {
    let len = f.seek(SeekFrom::End(0))?;
    if len < 22 {
        return Err(ZipError::Bad("File is not a zip file".into()));
    }
    // Python: look at the very end first, then scan the last 64 KiB + 22 bytes for the record.
    let tail_len = len.min(65_535 + 22);
    f.seek(SeekFrom::Start(len - tail_len))?;
    let mut tail = vec![0u8; tail_len as usize];
    f.read_exact(&mut tail).map_err(|_| ZipError::Bad("File is not a zip file".into()))?;
    let idx = if tail.len() >= 22 && tail[tail.len() - 22..tail.len() - 18] == SIG_EOCD && u16le(&tail, tail.len() - 2) == 0 {
        tail.len() - 22
    } else {
        tail.windows(4).rposition(|w| w == SIG_EOCD).filter(|i| i + 22 <= tail.len()).ok_or_else(|| ZipError::Bad("File is not a zip file".into()))?
    };
    let rec = &tail[idx..idx + 22];
    let pos = len - tail_len + idx as u64;
    let mut er = EndRecord { pos, entries: u16le(rec, 10) as u64, cd_size: u32le(rec, 12) as u64, cd_offset: u32le(rec, 16) as u64 };
    // ZIP64 locator directly in front of the end record.
    if pos >= 20 {
        f.seek(SeekFrom::Start(pos - 20))?;
        let mut loc = [0u8; 20];
        f.read_exact(&mut loc).map_err(|_| ZipError::Bad("File is not a zip file".into()))?;
        if u32le(&loc, 0) == SIG_LOC64 {
            let off64 = u64le(&loc, 8);
            // The zip64 record must lie entirely before the locator.
            if off64.checked_add(56).is_none_or(|e| e > pos - 20) {
                return Err(ZipError::Bad("Corrupt zip64 end of central directory record".into()));
            }
            f.seek(SeekFrom::Start(off64))?;
            let mut r64 = [0u8; 56];
            f.read_exact(&mut r64).map_err(|_| ZipError::Bad("Corrupt zip64 end of central directory record".into()))?;
            if u32le(&r64, 0) != SIG_EOCD64 {
                return Err(ZipError::Bad("Corrupt zip64 end of central directory record".into()));
            }
            er.entries = u64le(&r64, 32);
            er.cd_size = u64le(&r64, 40);
            er.cd_offset = u64le(&r64, 48);
            // `pos` for the prepended-data computation is the ZIP64 record.
            er.pos = off64;
        }
    }
    Ok(er)
}

fn decode_name(raw: &[u8], flags: u16) -> String {
    // Bit 11 = UTF-8. Otherwise Python uses cp437; for ASCII (all real dump names) they agree and
    // anything else is shown lossily.
    let _ = flags;
    String::from_utf8_lossy(raw).into_owned()
}

impl ZipArchive {
    /// Python `zipfile.is_zipfile`.
    pub fn is_zipfile(path: &Path) -> bool {
        File::open(path).ok().and_then(|mut f| find_end_record(&mut f).ok()).is_some()
    }

    pub fn open(path: &Path) -> Result<ZipArchive, ZipError> {
        let mut file = File::open(path)?;
        let er = find_end_record(&mut file)?;
        // Prepended data (e.g. self-extractors): shift by the difference between where the
        // directory should end and where the end record is.
        let bad_cd = || ZipError::Bad("Bad offset for central directory".into());
        let mut concat = 0u64;
        let expected_end = er.cd_offset.checked_add(er.cd_size).ok_or_else(bad_cd)?;
        if er.pos > expected_end {
            concat = er.pos - expected_end;
        }
        let cd_start = er.cd_offset.checked_add(concat).ok_or_else(bad_cd)?;
        // The directory must lie before its end record; this also bounds the allocation below by
        // the file size.
        let cd_end = cd_start.checked_add(er.cd_size).ok_or_else(bad_cd)?;
        if cd_end > er.pos {
            return Err(bad_cd());
        }
        file.seek(SeekFrom::Start(cd_start))?;
        let mut cd = vec![0u8; er.cd_size as usize];
        file.read_exact(&mut cd).map_err(|_| ZipError::Bad("Truncated central directory".into()))?;
        let mut entries = Vec::with_capacity(er.entries.min(er.cd_size / 46) as usize);
        let mut p = 0usize;
        while p + 46 <= cd.len() {
            if u32le(&cd, p) != SIG_CEN {
                return Err(ZipError::Bad("Bad magic number for central directory".into()));
            }
            let flags = u16le(&cd, p + 8);
            let method = u16le(&cd, p + 10);
            let crc32 = u32le(&cd, p + 16);
            let mut comp = u32le(&cd, p + 20) as u64;
            let mut size = u32le(&cd, p + 24) as u64;
            let name_len = u16le(&cd, p + 28) as usize;
            let extra_len = u16le(&cd, p + 30) as usize;
            let comment_len = u16le(&cd, p + 32) as usize;
            let mut off = u32le(&cd, p + 42) as u64;
            let name_end = p + 46 + name_len;
            let extra_end = name_end + extra_len;
            if extra_end + comment_len > cd.len() {
                return Err(ZipError::Bad("Truncated central directory".into()));
            }
            let name = decode_name(&cd[p + 46..name_end], flags);
            // ZIP64 extra field (id 1): values only for the fields that overflowed.
            let mut x = name_end;
            while x + 4 <= extra_end {
                let id = u16le(&cd, x);
                let sz = u16le(&cd, x + 2) as usize;
                let body = x + 4;
                if body + sz > extra_end {
                    break;
                }
                if id == 1 {
                    let mut q = body;
                    let mut take = |target: &mut u64, overflow: bool| {
                        if overflow && q + 8 <= body + sz {
                            *target = u64le(&cd, q);
                            q += 8;
                        }
                    };
                    let (o_size, o_comp, o_off) = (size == 0xFFFF_FFFF, comp == 0xFFFF_FFFF, off == 0xFFFF_FFFF);
                    take(&mut size, o_size);
                    take(&mut comp, o_comp);
                    take(&mut off, o_off);
                }
                x = body + sz;
            }
            let local_offset = off.checked_add(concat).ok_or_else(|| ZipError::Bad("Bad offset for local header".into()))?;
            entries.push(ZipEntry { name, method, flags, crc32, compressed_size: comp, size, local_offset });
            p = extra_end + comment_len;
        }
        Ok(ZipArchive { file, entries })
    }

    pub fn entries(&self) -> &[ZipEntry] {
        &self.entries
    }

    /// Stream one member into `out` (decompressing; verifies size and CRC-32).
    pub fn extract<W: Write>(&mut self, entry: &ZipEntry, out: &mut W) -> Result<(), ZipError> {
        if entry.flags & 1 != 0 {
            return Err(ZipError::Bad(format!("File '{}' is encrypted, password required for extraction", entry.name)));
        }
        self.file.seek(SeekFrom::Start(entry.local_offset)).map_err(|_| bad("Bad offset for local header"))?;
        let mut h = [0u8; 30];
        self.file.read_exact(&mut h).map_err(|_| ZipError::Bad("Truncated file header".into()))?;
        if u32le(&h, 0) != SIG_LOCAL {
            return Err(ZipError::Bad("Bad magic number for file header".into()));
        }
        let data_start = entry
            .local_offset
            .checked_add(30 + u16le(&h, 26) as u64 + u16le(&h, 28) as u64)
            .ok_or_else(|| ZipError::Bad("Bad offset for local header".into()))?;
        self.file.seek(SeekFrom::Start(data_start))?;
        let src = BufReader::with_capacity(256 * 1024, (&mut self.file).take(entry.compressed_size));
        // Never produce more than the declared size (+1 so an oversize stream is detected).
        let cap = entry.size.saturating_add(1);
        let (written, crc) = match entry.method {
            0 => pump(src.take(cap), out)?,
            8 => pump(flate2::read::DeflateDecoder::new(src).take(cap), out)?,
            m => return Err(ZipError::Bad(format!("That compression method is not supported ({m})"))),
        };
        if written != entry.size {
            return Err(ZipError::Bad(format!("Bad size for '{}'", entry.name)));
        }
        if crc != entry.crc32 {
            return Err(ZipError::Bad(format!("Bad CRC-32 for file '{}'", entry.name)));
        }
        Ok(())
    }
}

// ---- CRC-32 / inflate (crc32fast + flate2) ---------------------------------------------------

/// CRC-32 of `data`.
pub fn crc32(data: &[u8]) -> u32 {
    crc32fast::hash(data)
}

fn bad(msg: &str) -> ZipError {
    ZipError::Bad(msg.to_string())
}

/// Map a read error from the compressed stream: corrupt/truncated data is a bad archive, anything
/// else (real I/O failure) stays an I/O error.
fn read_err(e: io::Error) -> ZipError {
    match e.kind() {
        io::ErrorKind::InvalidData | io::ErrorKind::UnexpectedEof => ZipError::Bad(format!("Bad compressed data: {e}")),
        _ => ZipError::Io(e),
    }
}

/// Copy `src` (a reader over archive data) into `sink`, tracking size and CRC-32.
fn pump<R: Read, W: Write>(mut src: R, out: &mut W) -> Result<(u64, u32), ZipError> {
    let mut crc = crc32fast::Hasher::new();
    let mut written = 0u64;
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = src.read(&mut buf).map_err(read_err)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        crc.update(&buf[..n]);
        written += n as u64;
    }
    Ok((written, crc.finalize()))
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::crc32;

    /// Build a zip with *stored* members (for tests).
    pub fn stored_zip(members: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data) in members {
            let off = out.len() as u32;
            let crc = crc32(data);
            out.extend_from_slice(b"PK\x03\x04");
            out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);
            central.extend_from_slice(b"PK\x01\x02");
            central.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&[0; 12]);
            central.extend_from_slice(&off.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let cd_off = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(b"PK\x05\x06\0\0\0\0");
        out.extend_from_slice(&(members.len() as u16).to_le_bytes());
        out.extend_from_slice(&(members.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_vector() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn lists_and_extracts_stored() {
        let dir = std::env::temp_dir().join(format!("patchops-zip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.zip");
        std::fs::write(&p, testutil::stored_zip(&[("DUMP/BlackOps3.exe", b"exe-bytes"), ("DUMP/", b""), ("x.txt", b"hi")])).unwrap();
        assert!(ZipArchive::is_zipfile(&p));
        let mut z = ZipArchive::open(&p).unwrap();
        let names: Vec<_> = z.entries().iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, ["DUMP/BlackOps3.exe", "DUMP/", "x.txt"]);
        let e = z.entries()[0].clone();
        let mut out = Vec::new();
        z.extract(&e, &mut out).unwrap();
        assert_eq!(out, b"exe-bytes");
        std::fs::write(dir.join("b.zip"), b"not a zip at all, definitely not").unwrap();
        assert!(!ZipArchive::is_zipfile(&dir.join("b.zip")));
        std::fs::remove_dir_all(&dir).ok();
    }

    fn inflate(src: &[u8], out: &mut Vec<u8>) {
        pump(flate2::read::DeflateDecoder::new(src), out).unwrap();
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn inflates_fixed_and_dynamic() {
        // "hello hello hello hello" deflated with fixed Huffman (zlib level 9, raw).
        let fixed = [0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0xc8, 0x40, 0x27, 0x01];
        let mut out = Vec::new();
        inflate(&fixed[..], &mut out);
        assert_eq!(out, b"hello hello hello hello");
        // dynamic-Huffman block (zlib level 9) of 400 LCG-generated bytes.
        let dynamic = hex("1590cb0d00410842ef54416b0e2af65fc1b29998cc4f79401e6f9fea09d9b0f5fc3665cd9b2ebf51b7a15e9d569dcbc515ab76d0b7c5391a39df33acccc02b2ed3ef4119c327fb309a01f05e4b14f986e3165cca50bd1a72ea456caaa2b779c9887374f77458a33557762f07da7be5769ac2f3109539203e748fab9f51ec665f6d2ce92e15b87cea6a86ffd251ab092e5691d91fdfafe23a180e0876cdcabe6ecad39c462ccd032e098d2e46d0e04f12874dd6549282f84ad5ebe15ddf24c067d33f5e270061b25ef4da1bf52e7a3e");
        let mut x: u32 = 12345;
        let expect: Vec<u8> = (0..400)
            .map(|_| {
                x = x.wrapping_mul(1103515245).wrapping_add(12345) & 0x7fff_ffff;
                b"abcdefgh \n"[((x >> 16) % 10) as usize]
            })
            .collect();
        let mut out = Vec::new();
        inflate(&dynamic[..], &mut out);
        assert_eq!(out, expect);
        // stored block
        let stored = [0x01, 0x03, 0x00, 0xfc, 0xff, b'a', b'b', b'c'];
        let mut out = Vec::new();
        inflate(&stored[..], &mut out);
        assert_eq!(out, b"abc");
    }

    fn open_bytes(tag: &str, data: &[u8]) -> Result<ZipArchive, ZipError> {
        let p = std::env::temp_dir().join(format!("patchops-zipx-{tag}-{}.zip", std::process::id()));
        std::fs::write(&p, data).unwrap();
        let r = ZipArchive::open(&p);
        std::fs::remove_file(&p).ok();
        r
    }

    /// stored_zip with a hand-written ZIP64 end record + locator inserted before the EOCD.
    fn zip64_with(entries: u64, cd_size: u64, cd_offset: u64) -> Vec<u8> {
        let base = testutil::stored_zip(&[("a", b"x")]);
        let eocd = base.len() - 22;
        let mut v = base[..eocd].to_vec();
        let off64 = v.len() as u64;
        v.extend_from_slice(b"PK\x06\x06");
        v.extend_from_slice(&44u64.to_le_bytes());
        v.extend_from_slice(&[45, 0, 45, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        v.extend_from_slice(&entries.to_le_bytes());
        v.extend_from_slice(&entries.to_le_bytes());
        v.extend_from_slice(&cd_size.to_le_bytes());
        v.extend_from_slice(&cd_offset.to_le_bytes());
        v.extend_from_slice(b"PK\x06\x07\0\0\0\0");
        v.extend_from_slice(&off64.to_le_bytes());
        v.extend_from_slice(&1u32.to_le_bytes());
        v.extend_from_slice(&base[eocd..]);
        v
    }

    #[test]
    fn rejects_hostile_zip64_end_record() {
        // 9 EB central directory: must be an error, not an allocation abort.
        assert!(matches!(open_bytes("huge", &zip64_with(1, 0x7fff_ffff_ffff_0000, 0)), Err(ZipError::Bad(_))));
        // cd_offset + cd_size overflows u64.
        assert!(matches!(open_bytes("ovf", &zip64_with(1, u64::MAX, u64::MAX)), Err(ZipError::Bad(_))));
        assert!(matches!(open_bytes("ovf2", &zip64_with(1, 100, u64::MAX - 10)), Err(ZipError::Bad(_))));
        // Sane zip64 record still opens.
        let base = testutil::stored_zip(&[("a", b"x")]);
        let cd_off = u32le(&base, base.len() - 6) as u64;
        let cd_size = u32le(&base, base.len() - 10) as u64;
        assert_eq!(open_bytes("ok", &zip64_with(1, cd_size, cd_off)).unwrap().entries().len(), 1);
    }

    #[test]
    fn rejects_overflowing_local_offset_and_zip64_extra() {
        // Central entry with zip64 extra giving local offset u64::MAX while concat > 0.
        let mut z = vec![0u8; 5];
        z.extend(testutil::stored_zip(&[("a", b"x")]));
        let cen = z.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
        // Rebuild central entry with extra field: header 46 + name 1, add extra (4+8).
        let eocd = z.len() - 22;
        let mut entry = z[cen..eocd].to_vec();
        entry[30..32].copy_from_slice(&12u16.to_le_bytes());
        entry[42..46].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        entry.extend_from_slice(&[1, 0, 8, 0]);
        entry.extend_from_slice(&u64::MAX.to_le_bytes());
        let mut v = z[..cen].to_vec();
        v.extend_from_slice(&entry);
        let mut tail = z[eocd..].to_vec();
        tail[12..16].copy_from_slice(&(entry.len() as u32).to_le_bytes());
        v.extend_from_slice(&tail);
        assert!(matches!(open_bytes("loc", &v), Err(ZipError::Bad(_))));
    }

    #[test]
    fn corrupt_deflate_and_oversize_streams_error() {
        let mut z = testutil::stored_zip(&[("a", b"abc")]);
        let lh = 0; // local header offset
        z[lh + 8] = 8; // method deflate in local (unused) ...
        let cen = z.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
        z[cen + 10] = 8; // ... and central
        let p = std::env::temp_dir().join(format!("patchops-zipd-{}.zip", std::process::id()));
        std::fs::write(&p, &z).unwrap();
        let mut a = ZipArchive::open(&p).unwrap();
        let e = a.entries()[0].clone();
        let mut out = Vec::new();
        // "abc" is not a valid deflate stream (block type 3).
        assert!(matches!(a.extract(&e, &mut out), Err(ZipError::Bad(_))));
        std::fs::remove_file(&p).ok();
        // Declared size smaller than the real stream: error, bounded output.
        let mut out = Vec::new();
        let err = pump(flate2::read::DeflateDecoder::new(&[0x01u8, 0x03, 0x00, 0xfc, 0xff, b'a', b'b', b'c'][..]).take(2), &mut out).unwrap();
        assert_eq!(err.0, 2);
    }
}
