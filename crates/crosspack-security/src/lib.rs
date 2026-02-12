use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
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

pub fn verify_ed25519_signature_hex(
    payload: &[u8],
    public_key_hex: &str,
    signature_hex: &str,
) -> Result<bool> {
    let public_key_bytes =
        hex::decode(public_key_hex).context("failed to decode Ed25519 public key hex")?;
    let signature_bytes =
        hex::decode(signature_hex).context("failed to decode Ed25519 signature hex")?;
    let public_key_len = public_key_bytes.len();
    let signature_len = signature_bytes.len();

    let public_key_array: [u8; 32] = public_key_bytes.try_into().map_err(|_| {
        anyhow::anyhow!(
            "invalid Ed25519 public key length: expected 32 bytes, got {}",
            public_key_len
        )
    })?;
    let signature_array: [u8; 64] = signature_bytes.try_into().map_err(|_| {
        anyhow::anyhow!(
            "invalid Ed25519 signature length: expected 64 bytes, got {}",
            signature_len
        )
    })?;

    let verifying_key =
        VerifyingKey::from_bytes(&public_key_array).context("invalid Ed25519 public key bytes")?;
    let signature = Signature::from_bytes(&signature_array);

    Ok(verifying_key.verify(payload, &signature).is_ok())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{sha256_hex, verify_ed25519_signature_hex, verify_sha256_reader};

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

    #[test]
    fn verify_ed25519_accepts_valid_signature() {
        let payload = b"";
        let public_key_hex = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
        let signature_hex = concat!(
            "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155",
            "5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
        );

        let verified = verify_ed25519_signature_hex(payload, public_key_hex, signature_hex)
            .expect("verification must complete");

        assert!(verified);
    }

    #[test]
    fn verify_ed25519_returns_false_for_tampered_payload() {
        let payload = b"tampered";
        let public_key_hex = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
        let signature_hex = concat!(
            "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155",
            "5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
        );

        let verified = verify_ed25519_signature_hex(payload, public_key_hex, signature_hex)
            .expect("verification must complete");

        assert!(!verified);
    }

    #[test]
    fn verify_ed25519_errors_for_invalid_signature_hex_or_length() {
        let payload = b"";
        let public_key_hex = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";

        let invalid_hex = verify_ed25519_signature_hex(payload, public_key_hex, "zz");
        assert!(invalid_hex.is_err());

        let invalid_length = verify_ed25519_signature_hex(payload, public_key_hex, "00");
        assert!(invalid_length.is_err());
    }

    #[test]
    fn verify_ed25519_errors_for_invalid_public_key_hex_or_length() {
        let payload = b"";
        let signature_hex = concat!(
            "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e06522490155",
            "5fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
        );

        let invalid_hex = verify_ed25519_signature_hex(payload, "zz", signature_hex);
        assert!(invalid_hex.is_err());

        let invalid_length = verify_ed25519_signature_hex(payload, "00", signature_hex);
        assert!(invalid_length.is_err());
    }
}
