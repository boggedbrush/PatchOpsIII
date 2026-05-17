use std::{fs::File, io::Read, path::Path};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::Result;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HashOutput {
    pub ok: bool,
    pub path: String,
    pub sha256: String,
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

pub fn hash_path(path: &Path) -> Result<HashOutput> {
    Ok(HashOutput {
        ok: true,
        path: path.to_string_lossy().into_owned(),
        sha256: sha256_file(path)?,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::sha256_file;

    #[test]
    fn hashes_file_with_sha256() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("sample.bin");
        fs::write(&file, b"patchops").unwrap();

        assert_eq!(
            sha256_file(&file).unwrap(),
            "3e0cec93c1876296af254daff5dba161649c003683320e5fefbaf1ad21e67f98"
        );
    }
}
