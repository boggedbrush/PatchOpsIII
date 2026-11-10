//! SHA-256 (via `ring`, already linked through rustls) for executable integrity checks.

use ring::digest::{Context, SHA256};
use std::io::Read;
use std::path::Path;

/// Streaming SHA-256.
pub struct Sha256(Context);

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    pub fn new() -> Sha256 {
        Sha256(Context::new(&SHA256))
    }

    pub fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    /// Lower-case hex digest.
    pub fn finish_hex(self) -> String {
        let d = self.0.finish();
        let mut s = String::with_capacity(64);
        for b in d.as_ref() {
            s.push(char::from_digit((b >> 4) as u32, 16).unwrap());
            s.push(char::from_digit((b & 15) as u32, 16).unwrap());
        }
        s
    }
}

/// Hex SHA-256 of a byte slice.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    h.finish_hex()
}

/// Hex SHA-256 of a file (legacy `file_sha256`): `None` when missing or unreadable.
pub fn file_sha256(path: &Path) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Some(h.finish_hex())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vectors() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let long = vec![b'a'; 1000];
        assert_eq!(sha256_hex(&long), "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3");
        // padding edge: 55/56/64 bytes
        assert_eq!(sha256_hex(&[b'a'; 55]).len(), 64);
        assert_eq!(
            sha256_hex(&[b'a'; 56]),
            "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a"
        );
    }
}
