use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex::encode(digest)
}

pub fn verify_sha256(bytes: &[u8], expected_hex: &str) -> bool {
    let actual = sha256_hex(bytes);
    actual.eq_ignore_ascii_case(expected_hex)
}

pub fn verify_sha256_reader<R: Read>(reader: &mut R, expected_hex: &str) -> Result<bool> {
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 16 * 1024];
    loop {
        let read = reader
            .read(&mut buf)
            .context("failed reading stream for checksum")?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }

    let actual = hex::encode(hasher.finalize());
    Ok(actual.eq_ignore_ascii_case(expected_hex))
}

pub fn verify_sha256_file(path: &Path, expected_hex: &str) -> Result<bool> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("failed to read file for checksum: {}", path.display()))?;
    verify_sha256_reader(&mut file, expected_hex)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{sha256_hex, verify_sha256_reader};

    #[test]
    fn stable_sha256_hash() {
        let value = sha256_hex(b"crosspack");
        assert_eq!(
            value,
            "650c2cb6e617c91277fa43239c46df0d39c198cd2b936b83dd9136da7cfe60ab"
        );
    }

    #[test]
    fn verify_reader() {
        let mut reader = Cursor::new(b"crosspack");
        let ok = verify_sha256_reader(
            &mut reader,
            "650c2cb6e617c91277fa43239c46df0d39c198cd2b936b83dd9136da7cfe60ab",
        )
        .expect("must read");
        assert!(ok);
    }
}
