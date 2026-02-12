use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use crosspack_core::PackageManifest;
use crosspack_security::verify_ed25519_signature_hex;

#[derive(Debug, Clone)]
pub struct RegistryIndex {
    root: PathBuf,
}

impl RegistryIndex {
    pub fn open(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn search_names(&self, needle: &str) -> Result<Vec<String>> {
        let index_root = self.root.join("index");
        if !index_root.exists() {
            return Ok(Vec::new());
        }

        let mut names = Vec::new();
        for entry in fs::read_dir(index_root).context("failed to read registry index")? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.contains(needle) {
                    let manifests = self.package_versions(&name)?;
                    if !manifests.is_empty() {
                        names.push(name);
                    }
                }
            }
        }

        names.sort();
        Ok(names)
    }

    pub fn package_versions(&self, package: &str) -> Result<Vec<PackageManifest>> {
        let package_dir = self.root.join("index").join(package);
        if !package_dir.exists() {
            return Ok(Vec::new());
        }

        let trusted_key_path = self.root.join("registry.pub");
        let trusted_public_key_hex = fs::read_to_string(&trusted_key_path).with_context(|| {
            format!(
                "failed to read trusted registry key: {}",
                trusted_key_path.display()
            )
        })?;
        let trusted_public_key_hex = trusted_public_key_hex.trim();
        let key_identifier: String = trusted_public_key_hex.chars().take(16).collect();

        let mut manifests = Vec::new();
        for entry in fs::read_dir(&package_dir)
            .with_context(|| format!("failed to read package directory: {package}"))?
        {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }

            let path = entry.path();
            if path.extension().and_then(|v| v.to_str()) != Some("toml") {
                continue;
            }

            let manifest_bytes = fs::read(&path)
                .with_context(|| format!("failed reading manifest: {}", path.display()))?;

            let signature_path = path.with_extension("toml.sig");
            let signature_hex = fs::read_to_string(&signature_path).with_context(|| {
                format!(
                    "failed reading manifest signature for key {}: {}",
                    key_identifier,
                    signature_path.display()
                )
            })?;
            let signature_hex = signature_hex.trim();

            let signature_is_valid = verify_ed25519_signature_hex(
                &manifest_bytes,
                trusted_public_key_hex,
                signature_hex,
            )
            .with_context(|| {
                format!(
                    "failed verifying manifest signature for key {}: {}",
                    key_identifier,
                    signature_path.display()
                )
            })?;
            if !signature_is_valid {
                anyhow::bail!(
                    "invalid manifest signature for key {}: manifest {}, signature {}",
                    key_identifier,
                    path.display(),
                    signature_path.display()
                );
            }

            let content = String::from_utf8(manifest_bytes)
                .with_context(|| format!("manifest is not valid UTF-8: {}", path.display()))?;
            let manifest = PackageManifest::from_toml_str(&content)
                .with_context(|| format!("failed parsing manifest: {}", path.display()))?;
            manifests.push(manifest);
        }

        manifests.sort_by(|a, b| b.version.cmp(&a.version));
        Ok(manifests)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use ed25519_dalek::{Signer, SigningKey};

    use super::RegistryIndex;

    #[test]
    fn search_names_fails_when_registry_public_key_is_missing() {
        let root = test_registry_root();
        let package_dir = root.join("index").join("ripgrep");
        fs::create_dir_all(&package_dir).expect("must create package dir");
        fs::write(
            package_dir.join("14.1.0.toml"),
            manifest_toml("ripgrep", "14.1.0"),
        )
        .expect("must write manifest");

        let index = RegistryIndex::open(&root);
        let err = index
            .search_names("rip")
            .expect_err("must fail when registry.pub is missing");
        assert!(err.to_string().contains("registry.pub"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn search_names_returns_matching_package_with_valid_signed_manifests() {
        let root = test_registry_root();
        let package_dir = root.join("index").join("ripgrep");
        fs::create_dir_all(&package_dir).expect("must create package dir");

        let signing_key = signing_key();
        fs::write(root.join("registry.pub"), public_key_hex(&signing_key))
            .expect("must write registry public key");
        write_signed_manifest(&package_dir, &signing_key, "14.1.0");

        let index = RegistryIndex::open(&root);
        let names = index
            .search_names("rip")
            .expect("must load matching package names");
        assert_eq!(names, vec!["ripgrep"]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn package_versions_fails_when_registry_public_key_is_missing() {
        let root = test_registry_root();
        let package_dir = root.join("index").join("ripgrep");
        fs::create_dir_all(&package_dir).expect("must create package dir");
        let manifest_path = package_dir.join("14.1.0.toml");
        fs::write(&manifest_path, manifest_toml("ripgrep", "14.1.0")).expect("must write manifest");

        let index = RegistryIndex::open(&root);
        let err = index
            .package_versions("ripgrep")
            .expect_err("must fail when registry.pub is missing");
        assert!(err.to_string().contains("registry.pub"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn package_versions_fails_when_manifest_signature_is_missing() {
        let root = test_registry_root();
        let package_dir = root.join("index").join("ripgrep");
        fs::create_dir_all(&package_dir).expect("must create package dir");
        let manifest_path = package_dir.join("14.1.0.toml");
        fs::write(&manifest_path, manifest_toml("ripgrep", "14.1.0")).expect("must write manifest");

        let signing_key = signing_key();
        fs::write(root.join("registry.pub"), public_key_hex(&signing_key))
            .expect("must write registry public key");

        let index = RegistryIndex::open(&root);
        let err = index
            .package_versions("ripgrep")
            .expect_err("must fail when signature sidecar is missing");
        assert!(err.to_string().contains(".sig"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn package_versions_fails_when_manifest_signature_is_invalid() {
        let root = test_registry_root();
        let package_dir = root.join("index").join("ripgrep");
        fs::create_dir_all(&package_dir).expect("must create package dir");

        let manifest = manifest_toml("ripgrep", "14.1.0");
        let manifest_path = package_dir.join("14.1.0.toml");
        fs::write(&manifest_path, manifest.as_bytes()).expect("must write manifest");

        let signing_key = signing_key();
        fs::write(root.join("registry.pub"), public_key_hex(&signing_key))
            .expect("must write registry public key");

        fs::write(manifest_path.with_extension("toml.sig"), "00")
            .expect("must write invalid signature");

        let index = RegistryIndex::open(&root);
        let err = index
            .package_versions("ripgrep")
            .expect_err("must fail when signature is invalid");
        assert!(err.to_string().contains("signature"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn package_versions_succeeds_with_valid_signatures_and_descending_sort() {
        let root = test_registry_root();
        let package_dir = root.join("index").join("ripgrep");
        fs::create_dir_all(&package_dir).expect("must create package dir");

        let signing_key = signing_key();
        fs::write(root.join("registry.pub"), public_key_hex(&signing_key))
            .expect("must write registry public key");

        write_signed_manifest(&package_dir, &signing_key, "14.0.0");
        write_signed_manifest(&package_dir, &signing_key, "14.1.0");

        let index = RegistryIndex::open(&root);
        let manifests = index
            .package_versions("ripgrep")
            .expect("must load manifests");

        let versions: Vec<String> = manifests
            .iter()
            .map(|manifest| manifest.version.to_string())
            .collect();
        assert_eq!(versions, vec!["14.1.0", "14.0.0"]);

        let _ = fs::remove_dir_all(&root);
    }

    fn write_signed_manifest(
        package_dir: &std::path::Path,
        signing_key: &SigningKey,
        version: &str,
    ) {
        let manifest_path = package_dir.join(format!("{version}.toml"));
        let manifest = manifest_toml("ripgrep", version);
        fs::write(&manifest_path, manifest.as_bytes()).expect("must write manifest");

        let signature = signing_key.sign(manifest.as_bytes());
        fs::write(
            manifest_path.with_extension("toml.sig"),
            hex::encode(signature.to_bytes()),
        )
        .expect("must write signature sidecar");
    }

    fn manifest_toml(name: &str, version: &str) -> String {
        format!(
            r#"name = "{name}"
version = "{version}"
"#
        )
    }

    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn public_key_hex(signing_key: &SigningKey) -> String {
        hex::encode(signing_key.verifying_key().to_bytes())
    }

    fn test_registry_root() -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        path.push(format!(
            "crosspack-registry-tests-{}-{}",
            std::process::id(),
            nanos
        ));
        path
    }
}
