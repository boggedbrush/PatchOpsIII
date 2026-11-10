//! Minimal ZIP reader (no dependency): central-directory listing (incl. ZIP64 and prepended data,
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
    f.read_exact(&mut tail)?;
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
        f.read_exact(&mut loc)?;
        if u32le(&loc, 0) == SIG_LOC64 {
            let off64 = u64le(&loc, 8);
            f.seek(SeekFrom::Start(off64))?;
            let mut r64 = [0u8; 56];
            f.read_exact(&mut r64)?;
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
        let mut concat = 0u64;
        let expected_end = er.cd_offset + er.cd_size;
        if er.pos >= expected_end && er.pos - expected_end > 0 && er.pos - expected_end < er.pos {
            concat = er.pos - expected_end;
        }
        let cd_start = er.cd_offset + concat;
        if cd_start > er.pos {
            return Err(ZipError::Bad("Bad offset for central directory".into()));
        }
        file.seek(SeekFrom::Start(cd_start))?;
        let mut cd = vec![0u8; er.cd_size as usize];
        file.read_exact(&mut cd).map_err(|_| ZipError::Bad("Truncated central directory".into()))?;
        let mut entries = Vec::with_capacity(er.entries.min(1 << 20) as usize);
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
            entries.push(ZipEntry { name, method, flags, crc32, compressed_size: comp, size, local_offset: off + concat });
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
        self.file.seek(SeekFrom::Start(entry.local_offset))?;
        let mut h = [0u8; 30];
        self.file.read_exact(&mut h).map_err(|_| ZipError::Bad("Truncated file header".into()))?;
        if u32le(&h, 0) != SIG_LOCAL {
            return Err(ZipError::Bad("Bad magic number for file header".into()));
        }
        let skip = u16le(&h, 26) as i64 + u16le(&h, 28) as i64;
        self.file.seek(SeekFrom::Current(skip))?;
        let src = BufReader::with_capacity(256 * 1024, (&mut self.file).take(entry.compressed_size));
        let mut sink = CrcWriter { inner: out, crc: !0, written: 0 };
        match entry.method {
            0 => {
                let mut src = src;
                io::copy(&mut src, &mut sink)?;
            }
            8 => inflate(src, &mut sink)?,
            m => return Err(ZipError::Bad(format!("That compression method is not supported ({m})"))),
        }
        if sink.written != entry.size {
            return Err(ZipError::Bad(format!("Bad size for '{}'", entry.name)));
        }
        if !sink.crc != entry.crc32 {
            return Err(ZipError::Bad(format!("Bad CRC-32 for file '{}'", entry.name)));
        }
        Ok(())
    }
}

// ---- CRC-32 ---------------------------------------------------------------------------------

static CRC_TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();

fn crc_table() -> &'static [u32; 256] {
    CRC_TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        t
    })
}

/// Running CRC-32 state (inverted); finish with `!`.
pub fn crc32_update(mut crc: u32, data: &[u8]) -> u32 {
    let t = crc_table();
    for &b in data {
        crc = t[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc
}

pub fn crc32(data: &[u8]) -> u32 {
    !crc32_update(!0, data)
}

struct CrcWriter<'a, W: Write> {
    inner: &'a mut W,
    crc: u32,
    written: u64,
}

impl<W: Write> Write for CrcWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.crc = crc32_update(self.crc, &buf[..n]);
        self.written += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

// ---- inflate (RFC 1951) ---------------------------------------------------------------------

fn bad(msg: &str) -> ZipError {
    ZipError::Bad(msg.to_string())
}

struct Huffman {
    count: [u16; 16],
    symbol: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Huffman {
        let mut count = [0u16; 16];
        for &l in lengths {
            count[l as usize] += 1;
        }
        let mut offs = [0u16; 16];
        for i in 1..15 {
            offs[i + 1] = offs[i] + count[i];
        }
        let mut symbol = vec![0u16; lengths.len()];
        for (s, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbol[offs[l as usize] as usize] = s as u16;
                offs[l as usize] += 1;
            }
        }
        Huffman { count, symbol }
    }
}

struct Bits<R: Read> {
    r: R,
    buf: u32,
    cnt: u32,
}

impl<R: Read> Bits<R> {
    fn bits(&mut self, need: u32) -> Result<u32, ZipError> {
        while self.cnt < need {
            let mut b = [0u8; 1];
            self.r.read_exact(&mut b).map_err(|_| bad("Unexpected end of deflate stream"))?;
            self.buf |= (b[0] as u32) << self.cnt;
            self.cnt += 8;
        }
        let v = self.buf & ((1u32 << need) - 1);
        self.buf >>= need;
        self.cnt -= need;
        Ok(v)
    }

    fn decode(&mut self, h: &Huffman) -> Result<u16, ZipError> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= self.bits(1)? as i32;
            let count = h.count[len] as i32;
            if code - count < first {
                return Ok(h.symbol[(index + (code - first)) as usize]);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err(bad("Invalid Huffman code in deflate stream"))
    }
}

const LEN_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] =
    [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

/// Sliding window + pending output, flushed to `W` in chunks.
struct Window<'a, W: Write> {
    buf: Vec<u8>,
    flushed: usize,
    out: &'a mut W,
}

impl<W: Write> Window<'_, W> {
    fn push(&mut self, b: u8) -> io::Result<()> {
        self.buf.push(b);
        if self.buf.len() - self.flushed >= 1 << 16 {
            self.flush(false)?;
        }
        Ok(())
    }

    fn flush(&mut self, all: bool) -> io::Result<()> {
        self.out.write_all(&self.buf[self.flushed..])?;
        self.flushed = self.buf.len();
        if self.buf.len() > (1 << 17) || all {
            let keep = self.buf.len().saturating_sub(32768);
            self.buf.drain(..keep);
            self.flushed -= keep;
        }
        Ok(())
    }
}

fn inflate<R: Read, W: Write>(src: R, out: &mut W) -> Result<(), ZipError> {
    let mut br = Bits { r: src, buf: 0, cnt: 0 };
    let mut win = Window { buf: Vec::with_capacity(1 << 18), flushed: 0, out };
    loop {
        let last = br.bits(1)?;
        match br.bits(2)? {
            0 => {
                br.buf = 0;
                br.cnt = 0;
                let mut h = [0u8; 4];
                br.r.read_exact(&mut h).map_err(|_| bad("Unexpected end of deflate stream"))?;
                let len = u16::from_le_bytes([h[0], h[1]]);
                if len != !u16::from_le_bytes([h[2], h[3]]) {
                    return Err(bad("Corrupt stored block in deflate stream"));
                }
                for _ in 0..len {
                    let mut b = [0u8; 1];
                    br.r.read_exact(&mut b).map_err(|_| bad("Unexpected end of deflate stream"))?;
                    win.push(b[0])?;
                }
            }
            1 => {
                let mut l = [0u8; 288];
                for (i, v) in l.iter_mut().enumerate() {
                    *v = match i {
                        0..=143 => 8,
                        144..=255 => 9,
                        256..=279 => 7,
                        _ => 8,
                    };
                }
                codes(&mut br, &mut win, &Huffman::new(&l), &Huffman::new(&[5u8; 30]))?;
            }
            2 => {
                let nlen = br.bits(5)? as usize + 257;
                let ndist = br.bits(5)? as usize + 1;
                let ncode = br.bits(4)? as usize + 4;
                if nlen > 286 || ndist > 30 {
                    return Err(bad("Bad dynamic block counts in deflate stream"));
                }
                const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
                let mut lengths = [0u8; 320];
                for &o in ORDER.iter().take(ncode) {
                    lengths[o] = br.bits(3)? as u8;
                }
                let lencode = Huffman::new(&lengths[..19]);
                let mut idx = 0;
                let mut ll = [0u8; 320];
                while idx < nlen + ndist {
                    let sym = br.decode(&lencode)?;
                    if sym < 16 {
                        ll[idx] = sym as u8;
                        idx += 1;
                    } else {
                        let (prev, rep) = match sym {
                            16 => {
                                if idx == 0 {
                                    return Err(bad("Bad repeat in deflate stream"));
                                }
                                (ll[idx - 1], 3 + br.bits(2)? as usize)
                            }
                            17 => (0, 3 + br.bits(3)? as usize),
                            _ => (0, 11 + br.bits(7)? as usize),
                        };
                        if idx + rep > nlen + ndist {
                            return Err(bad("Bad repeat in deflate stream"));
                        }
                        for _ in 0..rep {
                            ll[idx] = prev;
                            idx += 1;
                        }
                    }
                }
                codes(&mut br, &mut win, &Huffman::new(&ll[..nlen]), &Huffman::new(&ll[nlen..nlen + ndist]))?;
            }
            _ => return Err(bad("Invalid block type in deflate stream")),
        }
        if last == 1 {
            break;
        }
    }
    win.flush(true)?;
    Ok(())
}

fn codes<R: Read, W: Write>(br: &mut Bits<R>, win: &mut Window<'_, W>, lit: &Huffman, dist: &Huffman) -> Result<(), ZipError> {
    loop {
        let sym = br.decode(lit)? as usize;
        match sym {
            0..=255 => win.push(sym as u8)?,
            256 => return Ok(()),
            257..=285 => {
                let s = sym - 257;
                let len = LEN_BASE[s] as usize + br.bits(LEN_EXTRA[s] as u32)? as usize;
                let ds = br.decode(dist)? as usize;
                if ds >= 30 {
                    return Err(bad("Invalid distance symbol in deflate stream"));
                }
                let d = DIST_BASE[ds] as usize + br.bits(DIST_EXTRA[ds] as u32)? as usize;
                if d > win.buf.len() {
                    return Err(bad("Invalid distance in deflate stream"));
                }
                for _ in 0..len {
                    let b = win.buf[win.buf.len() - d];
                    win.push(b)?;
                }
            }
            _ => return Err(bad("Invalid literal/length symbol in deflate stream")),
        }
    }
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

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn inflates_fixed_and_dynamic() {
        // "hello hello hello hello" deflated with fixed Huffman (zlib level 9, raw).
        let fixed = [0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0xc8, 0x40, 0x27, 0x01];
        let mut out = Vec::new();
        inflate(&fixed[..], &mut out).unwrap();
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
        inflate(&dynamic[..], &mut out).unwrap();
        assert_eq!(out, expect);
        // stored block
        let stored = [0x01, 0x03, 0x00, 0xfc, 0xff, b'a', b'b', b'c'];
        let mut out = Vec::new();
        inflate(&stored[..], &mut out).unwrap();
        assert_eq!(out, b"abc");
    }
}
