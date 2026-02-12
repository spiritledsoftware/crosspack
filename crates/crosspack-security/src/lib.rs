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

pub fn verify_sha256_file(path: &Path, expected_hex: &str) -> Result<bool> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read file for checksum: {}", path.display()))?;
    Ok(verify_sha256(&bytes, expected_hex))
}

#[cfg(test)]
mod tests {
    use super::sha256_hex;

    #[test]
    fn stable_sha256_hash() {
        let value = sha256_hex(b"crosspack");
        assert_eq!(
            value,
            "4ff4df7f8cd2ca95c37ac3f71463fab340f7f7d0c9586bcd6c9db9eb0e07bb95"
        );
    }
}
