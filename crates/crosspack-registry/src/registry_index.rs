use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use crosspack_core::PackageManifest;
use crosspack_security::verify_ed25519_signature_hex;

use crate::{
    parse_source_state_file, sort_sources, source_has_ready_snapshot, RegistrySourceRecord,
    RegistrySourceStateFile,
};

#[derive(Debug, Clone)]
pub struct RegistryIndex {
    root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ConfiguredRegistryIndex {
    sources: Vec<ConfiguredSnapshotSource>,
}

#[derive(Debug, Clone)]
struct ConfiguredSnapshotSource {
    name: String,
    index: RegistryIndex,
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

impl ConfiguredRegistryIndex {
    pub fn open(state_root: impl Into<PathBuf>) -> Result<Self> {
        let state_root = state_root.into();
        let sources_path = state_root.join("sources.toml");
        let (state, has_sources_file) = match fs::read_to_string(&sources_path) {
            Ok(content) => {
                let state = parse_source_state_file(&content).with_context(|| {
                    format!(
                        "failed parsing configured registry sources: {}",
                        sources_path.display()
                    )
                })?;
                (state, true)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                (RegistrySourceStateFile::default(), false)
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "failed reading configured registry sources: {}",
                        sources_path.display()
                    )
                });
            }
        };

        let mut enabled_sources: Vec<RegistrySourceRecord> = state
            .sources
            .into_iter()
            .filter(|source| source.enabled)
            .collect();
        let enabled_count = enabled_sources.len();
        sort_sources(&mut enabled_sources);

        let mut configured = Vec::new();
        for source in enabled_sources {
            let cache_root = state_root.join("cache").join(&source.name);
            if !source_has_ready_snapshot(&cache_root)? {
                continue;
            }
            configured.push(ConfiguredSnapshotSource {
                name: source.name,
                index: RegistryIndex::open(cache_root),
            });
        }

        if !configured.is_empty() {
            return Ok(Self {
                sources: configured,
            });
        }

        if !has_sources_file || enabled_count == 0 {
            return Ok(Self {
                sources: Vec::new(),
            });
        }

        anyhow::bail!("no ready snapshot exists for enabled sources")
    }

    pub fn search_names(&self, needle: &str) -> Result<Vec<String>> {
        let mut deduped = HashSet::new();
        for source in &self.sources {
            for name in source.index.search_names(needle)? {
                deduped.insert(name);
            }
        }

        let mut names: Vec<String> = deduped.into_iter().collect();
        names.sort();
        Ok(names)
    }

    pub fn package_versions(&self, package: &str) -> Result<Vec<PackageManifest>> {
        if let Some((_, manifests)) = self.package_versions_with_source(package)? {
            return Ok(manifests);
        }
        Ok(Vec::new())
    }

    pub fn package_versions_with_source(
        &self,
        package: &str,
    ) -> Result<Option<(String, Vec<PackageManifest>)>> {
        for source in &self.sources {
            let manifests = source.index.package_versions(package).with_context(|| {
                format!(
                    "failed loading package '{package}' from configured source '{}'",
                    source.name
                )
            })?;
            if !manifests.is_empty() {
                return Ok(Some((source.name.clone(), manifests)));
            }
        }
        Ok(None)
    }
}
