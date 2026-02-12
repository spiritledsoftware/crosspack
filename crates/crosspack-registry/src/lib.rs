use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use crosspack_core::PackageManifest;
use crosspack_security::verify_ed25519_signature_hex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RegistrySourceKind {
    Git,
    Filesystem,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistrySourceRecord {
    pub name: String,
    pub kind: RegistrySourceKind,
    pub location: String,
    #[serde(alias = "fingerprint")]
    pub fingerprint_sha256: String,
    #[serde(default = "source_enabled_default")]
    pub enabled: bool,
    pub priority: u32,
}

#[derive(Debug, Clone)]
pub struct RegistrySourceStore {
    state_root: PathBuf,
}

impl RegistrySourceStore {
    pub fn new(state_root: impl Into<PathBuf>) -> Self {
        Self {
            state_root: state_root.into(),
        }
    }

    pub fn add_source(&self, source: RegistrySourceRecord) -> Result<()> {
        validate_source_name(&source.name)?;
        validate_source_fingerprint(&source.fingerprint_sha256)?;

        let mut state = self.load_state()?;
        if state
            .sources
            .iter()
            .any(|existing| existing.name == source.name)
        {
            anyhow::bail!("source '{}' already exists", source.name);
        }

        state.sources.push(source);
        sort_sources(&mut state.sources);
        self.save_state(&state)
    }

    pub fn list_sources(&self) -> Result<Vec<RegistrySourceRecord>> {
        let mut state = self.load_state()?;
        sort_sources(&mut state.sources);
        Ok(state.sources)
    }

    pub fn remove_source(&self, name: &str) -> Result<()> {
        let mut state = self.load_state()?;
        let before = state.sources.len();
        state.sources.retain(|source| source.name != name);
        if state.sources.len() == before {
            anyhow::bail!("source '{}' not found", name);
        }

        sort_sources(&mut state.sources);
        self.save_state(&state)
    }

    fn sources_file_path(&self) -> PathBuf {
        self.state_root.join("sources.toml")
    }

    fn load_state(&self) -> Result<RegistrySourceStateFile> {
        let path = self.sources_file_path();
        if !path.exists() {
            return Ok(RegistrySourceStateFile::default());
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed reading source state: {}", path.display()))?;
        let mut state = parse_source_state_file(&content)
            .with_context(|| format!("failed parsing source state: {}", path.display()))?;
        sort_sources(&mut state.sources);
        Ok(state)
    }

    fn save_state(&self, state: &RegistrySourceStateFile) -> Result<()> {
        fs::create_dir_all(&self.state_root).with_context(|| {
            format!(
                "failed creating source state root: {}",
                self.state_root.display()
            )
        })?;

        let path = self.sources_file_path();
        let mut state = state.clone();
        sort_sources(&mut state.sources);
        let content = toml::to_string(&state)
            .with_context(|| format!("failed serializing source state: {}", path.display()))?;
        fs::write(&path, content)
            .with_context(|| format!("failed writing source state: {}", path.display()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistrySourceStateFile {
    #[serde(default = "state_file_version")]
    version: u32,
    #[serde(default)]
    sources: Vec<RegistrySourceRecord>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RegistrySourceStateFileCompat {
    Versioned(RegistrySourceStateFile),
    Legacy { sources: Vec<RegistrySourceRecord> },
}

impl Default for RegistrySourceStateFile {
    fn default() -> Self {
        Self {
            version: state_file_version(),
            sources: Vec::new(),
        }
    }
}

fn parse_source_state_file(content: &str) -> Result<RegistrySourceStateFile> {
    let state = toml::from_str::<RegistrySourceStateFileCompat>(content)?;
    Ok(match state {
        RegistrySourceStateFileCompat::Versioned(mut state) => {
            state.version = state_file_version();
            state
        }
        RegistrySourceStateFileCompat::Legacy { sources } => RegistrySourceStateFile {
            version: state_file_version(),
            sources,
        },
    })
}

fn state_file_version() -> u32 {
    1
}

fn source_enabled_default() -> bool {
    true
}

fn sort_sources(sources: &mut [RegistrySourceRecord]) {
    sources.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.name.cmp(&right.name))
    });
}

fn validate_source_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 64 {
        anyhow::bail!("invalid source name: must not be empty");
    }

    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        anyhow::bail!("invalid source name: '{name}'");
    };

    let first_is_valid = first.is_ascii_lowercase() || first.is_ascii_digit();
    let rest_is_valid =
        chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_');
    if !first_is_valid || !rest_is_valid {
        anyhow::bail!("invalid source name: '{name}'");
    }

    Ok(())
}

fn validate_source_fingerprint(fingerprint: &str) -> Result<()> {
    if fingerprint.len() != 64 || !fingerprint.chars().all(|ch| ch.is_ascii_hexdigit()) {
        anyhow::bail!("invalid source fingerprint: '{fingerprint}'");
    }

    Ok(())
}

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

    use super::{RegistryIndex, RegistrySourceKind, RegistrySourceRecord, RegistrySourceStore};

    #[test]
    fn source_store_add_rejects_duplicate_name() {
        let root = test_registry_root();
        let store = RegistrySourceStore::new(&root);

        store
            .add_source(source_record("official", 10))
            .expect("must add source");
        let err = store
            .add_source(source_record("official", 5))
            .expect_err("must reject duplicate source name");
        assert!(err.to_string().contains("already exists"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_add_rejects_invalid_name() {
        let root = test_registry_root();
        let store = RegistrySourceStore::new(&root);

        let err = store
            .add_source(source_record("bad name", 10))
            .expect_err("must reject invalid source name");
        assert!(err.to_string().contains("invalid source name"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_add_rejects_name_with_leading_separator() {
        let root = test_registry_root();
        let store = RegistrySourceStore::new(&root);

        let err = store
            .add_source(source_record("-bad", 10))
            .expect_err("must reject invalid source name");
        assert!(err.to_string().contains("invalid source name"));

        let err = store
            .add_source(source_record("_bad", 10))
            .expect_err("must reject invalid source name");
        assert!(err.to_string().contains("invalid source name"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_add_rejects_name_longer_than_sixty_four_characters() {
        let root = test_registry_root();
        let store = RegistrySourceStore::new(&root);

        let too_long_name = "a".repeat(65);
        let err = store
            .add_source(source_record(&too_long_name, 10))
            .expect_err("must reject invalid source name");
        assert!(err.to_string().contains("invalid source name"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_add_rejects_invalid_fingerprint() {
        let root = test_registry_root();
        let store = RegistrySourceStore::new(&root);

        let mut record = source_record("official", 10);
        record.fingerprint_sha256 = "xyz".to_string();
        let err = store
            .add_source(record)
            .expect_err("must reject invalid fingerprint");
        assert!(err.to_string().contains("invalid source fingerprint"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_list_sorts_by_priority_then_name() {
        let root = test_registry_root();
        let store = RegistrySourceStore::new(&root);

        store
            .add_source(source_record("zeta", 10))
            .expect("must add source");
        store
            .add_source(source_record("alpha", 1))
            .expect("must add source");
        store
            .add_source(source_record("beta", 10))
            .expect("must add source");

        let listed = store.list_sources().expect("must list sources");
        let names: Vec<&str> = listed.iter().map(|record| record.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "beta", "zeta"]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_remove_reports_missing_source() {
        let root = test_registry_root();
        let store = RegistrySourceStore::new(&root);

        let err = store
            .remove_source("missing")
            .expect_err("must report missing source");
        assert!(err.to_string().contains("not found"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_writes_versioned_sorted_state_file() {
        let root = test_registry_root();
        let store = RegistrySourceStore::new(&root);

        store
            .add_source(source_record("zeta", 10))
            .expect("must add source");
        store
            .add_source(source_record("alpha", 0))
            .expect("must add source");
        store
            .add_source(source_record("beta", 10))
            .expect("must add source");

        let content = fs::read_to_string(root.join("sources.toml")).expect("must read state file");
        let expected = concat!(
            "version = 1\n",
            "\n",
            "[[sources]]\n",
            "name = \"alpha\"\n",
            "kind = \"git\"\n",
            "location = \"https://example.com/alpha.git\"\n",
            "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
            "enabled = true\n",
            "priority = 0\n",
            "\n",
            "[[sources]]\n",
            "name = \"beta\"\n",
            "kind = \"git\"\n",
            "location = \"https://example.com/beta.git\"\n",
            "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
            "enabled = true\n",
            "priority = 10\n",
            "\n",
            "[[sources]]\n",
            "name = \"zeta\"\n",
            "kind = \"git\"\n",
            "location = \"https://example.com/zeta.git\"\n",
            "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
            "enabled = true\n",
            "priority = 10\n"
        );
        assert_eq!(content, expected);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_loads_unversioned_state_file_for_back_compat() {
        let root = test_registry_root();
        fs::create_dir_all(&root).expect("must create state root");
        fs::write(
            root.join("sources.toml"),
            concat!(
                "[[sources]]\n",
                "name = \"zeta\"\n",
                "kind = \"git\"\n",
                "location = \"https://example.com/zeta.git\"\n",
                "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
                "priority = 10\n",
                "\n",
                "[[sources]]\n",
                "name = \"alpha\"\n",
                "kind = \"git\"\n",
                "location = \"https://example.com/alpha.git\"\n",
                "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
                "priority = 0\n"
            ),
        )
        .expect("must write legacy state file");

        let store = RegistrySourceStore::new(&root);
        let listed = store.list_sources().expect("must list sources");
        let names: Vec<&str> = listed.iter().map(|record| record.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "zeta"]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_rejects_negative_priority_in_legacy_state() {
        let root = test_registry_root();
        fs::create_dir_all(&root).expect("must create state root");
        fs::write(
            root.join("sources.toml"),
            concat!(
                "[[sources]]\n",
                "name = \"official\"\n",
                "kind = \"git\"\n",
                "location = \"https://example.com/official.git\"\n",
                "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
                "priority = -1\n"
            ),
        )
        .expect("must write legacy state file");

        let store = RegistrySourceStore::new(&root);
        let err = store
            .list_sources()
            .expect_err("must reject negative source priority");
        assert!(err.to_string().contains("failed parsing source state"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_defaults_enabled_to_true_when_missing_in_loaded_state() {
        let root = test_registry_root();
        fs::create_dir_all(&root).expect("must create state root");
        fs::write(
            root.join("sources.toml"),
            concat!(
                "version = 1\n",
                "\n",
                "[[sources]]\n",
                "name = \"official\"\n",
                "kind = \"git\"\n",
                "location = \"https://example.com/official.git\"\n",
                "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
                "priority = 1\n"
            ),
        )
        .expect("must write state file without enabled flag");

        let store = RegistrySourceStore::new(&root);
        store
            .add_source(source_record("mirror", 2))
            .expect("must add source");

        let content = fs::read_to_string(root.join("sources.toml")).expect("must read state file");
        assert!(
            content.contains(
                "name = \"official\"\nkind = \"git\"\nlocation = \"https://example.com/official.git\"\nfingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\nenabled = true\npriority = 1\n"
            ),
            "expected missing enabled flag to default to true when persisted\n{content}"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_first_write_from_empty_uses_version_one() {
        let root = test_registry_root();
        let store = RegistrySourceStore::new(&root);

        store
            .add_source(source_record("official", 0))
            .expect("must add first source");

        let content = fs::read_to_string(root.join("sources.toml")).expect("must read state file");
        assert!(
            content.starts_with("version = 1\n"),
            "expected first write to persist version 1\n{content}"
        );

        let _ = fs::remove_dir_all(&root);
    }

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

    fn source_record(name: &str, priority: u32) -> RegistrySourceRecord {
        RegistrySourceRecord {
            name: name.to_string(),
            kind: RegistrySourceKind::Git,
            location: format!("https://example.com/{name}.git"),
            fingerprint_sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .to_string(),
            enabled: true,
            priority,
        }
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
