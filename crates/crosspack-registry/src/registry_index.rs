use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use crosspack_core::PackageManifest;
use crosspack_security::verify_ed25519_signature_hex;
use toml::value::Table;
use toml::Value;

use crate::{
    parse_source_state_file, sort_sources, source_has_ready_snapshot,
    verify_community_recipe_catalog_policy, RegistrySourceRecord, RegistrySourceStateFile,
};

#[derive(Debug, Clone)]
pub struct RegistryIndex {
    root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ConfiguredRegistryIndex {
    sources: Vec<ConfiguredSnapshotSource>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PackageSkipDiagnostic {
    pub package: String,
    pub reason_code: &'static str,
    pub source: String,
    pub detail: String,
}

type PackageManifestSet = (String, Vec<PackageManifest>);
type PackageDiagnostics = Vec<PackageSkipDiagnostic>;
type PackageManifestSetsWithDiagnostics = (Vec<PackageManifestSet>, PackageDiagnostics);
type SourcePackageVersionsWithDiagnostics = (Option<PackageManifestSet>, PackageDiagnostics);

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
        let (names, _) = self.search_names_with_diagnostics(needle)?;
        Ok(names)
    }

    pub fn search_names_with_diagnostics(
        &self,
        needle: &str,
    ) -> Result<(Vec<String>, Vec<PackageSkipDiagnostic>)> {
        let (mut names, diagnostics) = self.package_names_with_diagnostics()?;
        names.retain(|name| name.contains(needle));

        names.sort();
        Ok((names, diagnostics))
    }

    pub fn package_names(&self) -> Result<Vec<String>> {
        let (names, _) = self.package_names_with_diagnostics()?;
        Ok(names)
    }

    pub fn package_names_with_diagnostics(
        &self,
    ) -> Result<(Vec<String>, Vec<PackageSkipDiagnostic>)> {
        let (packages, diagnostics) = self.package_manifests_with_diagnostics()?;
        let mut names: Vec<String> = packages.into_iter().map(|(name, _)| name).collect();
        names.sort();
        Ok((names, diagnostics))
    }

    fn package_manifests_with_diagnostics(&self) -> Result<PackageManifestSetsWithDiagnostics> {
        let mut packages = Vec::new();
        let mut diagnostics = Vec::new();
        let source = self.root.display().to_string();
        for name in self.package_record_names()? {
            let manifests = self.package_versions_tolerant(&name, &source, &mut diagnostics)?;
            if !manifests.is_empty() {
                packages.push((name, manifests));
            }
        }
        packages.sort_by(|(left, _), (right, _)| left.cmp(right));
        Ok((packages, diagnostics))
    }

    fn package_record_names(&self) -> Result<Vec<String>> {
        let mut names = HashSet::new();
        let releases_root = self.root.join("releases");
        if releases_root.exists() {
            for entry in fs::read_dir(releases_root).context("failed to read registry releases")? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    names.insert(entry.file_name().to_string_lossy().to_string());
                }
            }
        }

        let packages_root = self.root.join("packages");
        if packages_root.exists() {
            for entry in fs::read_dir(packages_root).context("failed to read registry packages")? {
                let entry = entry?;
                if !entry.file_type()?.is_file() {
                    continue;
                }
                let path = entry.path();
                if path.extension().and_then(|value| value.to_str()) != Some("toml") {
                    continue;
                }
                let Some(package) = path.file_stem().and_then(|value| value.to_str()) else {
                    anyhow::bail!(
                        "failed reading registry packages: non-utf8 package filename {}",
                        path.display()
                    );
                };
                names.insert(package.to_string());
            }
        }

        let mut names: Vec<String> = names.into_iter().collect();
        names.sort();
        Ok(names)
    }

    pub fn package_versions(&self, package: &str) -> Result<Vec<PackageManifest>> {
        let release_dir = self.root.join("releases").join(package);
        let package_template_path = self.root.join("packages").join(format!("{package}.toml"));
        let has_release_dir = release_dir.exists();
        let has_package_template = package_template_path.exists();
        if !has_release_dir && !has_package_template {
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

        let package_template_bytes = fs::read(&package_template_path).with_context(|| {
            format!(
                "failed reading package template: {}",
                package_template_path.display()
            )
        })?;
        verify_signed_toml_document(
            &package_template_path,
            &package_template_bytes,
            trusted_public_key_hex,
            &key_identifier,
        )?;
        let package_template = parse_toml_table(
            &package_template_bytes,
            &package_template_path,
            "package template",
        )?;

        if !has_release_dir {
            anyhow::bail!(
                "orphaned package template without releases directory: package template {}, expected release directory {}",
                package_template_path.display(),
                release_dir.display()
            );
        }

        let mut manifests = Vec::new();
        for entry in fs::read_dir(&release_dir)
            .with_context(|| format!("failed to read release directory: {package}"))?
        {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }

            let path = entry.path();
            if path.extension().and_then(|v| v.to_str()) != Some("toml") {
                continue;
            }

            let release_bytes = fs::read(&path)
                .with_context(|| format!("failed reading release file: {}", path.display()))?;
            verify_signed_toml_document(
                &path,
                &release_bytes,
                trusted_public_key_hex,
                &key_identifier,
            )?;
            let release_document = parse_toml_table(&release_bytes, &path, "release metadata")?;
            let merged_document = merge_manifest_documents(&package_template, &release_document);
            let merged_manifest =
                toml::to_string(&Value::Table(merged_document)).with_context(|| {
                    format!(
                        "failed serializing merged manifest: package template {}, release {}",
                        package_template_path.display(),
                        path.display()
                    )
                })?;
            let manifest = PackageManifest::from_toml_str(&merged_manifest).with_context(|| {
                format!(
                    "failed parsing merged manifest: package template {}, release {}",
                    package_template_path.display(),
                    path.display()
                )
            })?;
            manifests.push(manifest);
        }

        manifests.sort_by(|a, b| b.version.cmp(&a.version));
        Ok(manifests)
    }

    pub fn provider_versions(&self, capability: &str) -> Result<Vec<PackageManifest>> {
        let (providers, _) = self.provider_versions_with_diagnostics(capability)?;
        Ok(providers)
    }

    pub fn provider_versions_with_diagnostics(
        &self,
        capability: &str,
    ) -> Result<(Vec<PackageManifest>, Vec<PackageSkipDiagnostic>)> {
        let mut providers = Vec::new();
        let (packages, diagnostics) = self.package_manifests_with_diagnostics()?;
        for (_, manifests) in packages {
            providers.extend(manifests.into_iter().filter(|manifest| {
                manifest
                    .provides
                    .iter()
                    .any(|provided| provided == capability)
            }));
        }
        Ok((providers, diagnostics))
    }

    pub fn package_versions_with_diagnostics(
        &self,
        package: &str,
    ) -> Result<(Vec<PackageManifest>, Vec<PackageSkipDiagnostic>)> {
        let mut diagnostics = Vec::new();
        let source = self.root.display().to_string();
        let manifests = self.package_versions_tolerant(package, &source, &mut diagnostics)?;
        Ok((manifests, diagnostics))
    }

    fn package_versions_tolerant(
        &self,
        package: &str,
        source: &str,
        diagnostics: &mut Vec<PackageSkipDiagnostic>,
    ) -> Result<Vec<PackageManifest>> {
        match self.package_versions(package) {
            Ok(manifests) => Ok(manifests),
            Err(error) => {
                if !is_skippable_package_poison(&error) {
                    return Err(error);
                }
                diagnostics.push(PackageSkipDiagnostic {
                    package: package.to_string(),
                    reason_code: "package-metadata-invalid",
                    source: source.to_string(),
                    detail: error.to_string(),
                });
                Ok(Vec::new())
            }
        }
    }
}

fn is_skippable_package_poison(error: &anyhow::Error) -> bool {
    let rendered = error.to_string();
    if rendered.contains("signature")
        || rendered.contains("trusted registry key")
        || rendered.contains("registry.pub")
        || rendered.contains("fingerprint")
    {
        return false;
    }
    rendered.contains("failed parsing package template")
        || rendered.contains("failed parsing release metadata")
        || rendered.contains("failed parsing merged manifest")
        || rendered.contains("failed serializing merged manifest")
        || rendered.contains("expected TOML table")
        || rendered.contains("not valid UTF-8")
}

fn verify_signed_toml_document(
    document_path: &Path,
    document_bytes: &[u8],
    trusted_public_key_hex: &str,
    key_identifier: &str,
) -> Result<()> {
    let signature_path = document_path.with_extension("toml.sig");
    let signature_hex = fs::read_to_string(&signature_path).with_context(|| {
        format!(
            "failed reading metadata signature for trusted key {}: {}",
            key_identifier,
            signature_path.display()
        )
    })?;
    let signature_hex = signature_hex.trim();

    let signature_is_valid =
        verify_ed25519_signature_hex(document_bytes, trusted_public_key_hex, signature_hex)
            .with_context(|| {
                format!(
                    "failed verifying metadata signature for trusted key {}: {}",
                    key_identifier,
                    signature_path.display()
                )
            })?;
    if !signature_is_valid {
        anyhow::bail!(
            "invalid metadata signature for trusted key {}: document {}, signature {}",
            key_identifier,
            document_path.display(),
            signature_path.display()
        );
    }

    Ok(())
}

fn parse_toml_table(document_bytes: &[u8], document_path: &Path, kind: &str) -> Result<Table> {
    let content = String::from_utf8(document_bytes.to_vec())
        .with_context(|| format!("{kind} is not valid UTF-8: {}", document_path.display()))?;
    let value: Value = toml::from_str(&content)
        .with_context(|| format!("failed parsing {kind}: {}", document_path.display()))?;
    let Value::Table(table) = value else {
        anyhow::bail!(
            "failed parsing {kind}: expected TOML table at root in {}",
            document_path.display()
        );
    };
    Ok(table)
}

fn merge_manifest_documents(package_template: &Table, release_document: &Table) -> Table {
    let mut merged = package_template.clone();
    merge_tables(&mut merged, release_document);
    merged
}

fn merge_tables(base: &mut Table, overlay: &Table) {
    for (key, overlay_value) in overlay {
        if key == "artifacts" {
            if let (Some(Value::Array(base_array)), Value::Array(overlay_array)) =
                (base.get(key), overlay_value)
            {
                base.insert(
                    key.clone(),
                    Value::Array(merge_artifacts(base_array, overlay_array)),
                );
                continue;
            }
        }

        if let Some(Value::Table(base_table)) = base.get_mut(key) {
            if let Value::Table(overlay_table) = overlay_value {
                merge_tables(base_table, overlay_table);
                continue;
            }
        }
        base.insert(key.clone(), overlay_value.clone());
    }
}

fn merge_artifacts(base: &[Value], overlay: &[Value]) -> Vec<Value> {
    overlay
        .iter()
        .map(|overlay_value| {
            let Some(overlay_table) = overlay_value.as_table() else {
                return overlay_value.clone();
            };
            let Some(target) = overlay_table.get("target").and_then(Value::as_str) else {
                return overlay_value.clone();
            };

            let Some(base_table) = base.iter().find_map(|base_value| {
                let base_table = base_value.as_table()?;
                let base_target = base_table.get("target")?.as_str()?;
                if base_target == target {
                    Some(base_table)
                } else {
                    None
                }
            }) else {
                return overlay_value.clone();
            };

            let mut merged = base_table.clone();
            merge_tables(&mut merged, overlay_table);
            Value::Table(merged)
        })
        .collect()
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
            verify_community_recipe_catalog_policy(&cache_root, &source).with_context(|| {
                format!(
                    "failed validating community recipe metadata for configured source '{}'",
                    source.name
                )
            })?;
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
        let (names, _) = self.search_names_with_diagnostics(needle)?;
        Ok(names)
    }

    pub fn search_names_with_diagnostics(
        &self,
        needle: &str,
    ) -> Result<(Vec<String>, Vec<PackageSkipDiagnostic>)> {
        let (mut names, diagnostics) = self.package_names_with_diagnostics()?;
        names.retain(|name| name.contains(needle));
        names.sort();
        Ok((names, diagnostics))
    }

    pub fn package_names(&self) -> Result<Vec<String>> {
        let (names, _) = self.package_names_with_diagnostics()?;
        Ok(names)
    }

    pub fn package_names_with_diagnostics(
        &self,
    ) -> Result<(Vec<String>, Vec<PackageSkipDiagnostic>)> {
        let mut deduped = HashSet::new();
        let mut diagnostics = Vec::new();
        for source in &self.sources {
            let (names, mut source_diagnostics) = source.index.package_names_with_diagnostics()?;
            retag_diagnostics_source(&mut source_diagnostics, &source.name);
            diagnostics.append(&mut source_diagnostics);
            for name in names {
                deduped.insert(name);
            }
        }

        let mut names: Vec<String> = deduped.into_iter().collect();
        names.sort();
        Ok((names, diagnostics))
    }

    pub fn package_versions(&self, package: &str) -> Result<Vec<PackageManifest>> {
        if let Some((_, manifests)) = self.package_versions_with_source(package)? {
            return Ok(manifests);
        }
        Ok(Vec::new())
    }

    pub fn provider_versions(&self, capability: &str) -> Result<Vec<PackageManifest>> {
        let (providers, _) = self.provider_versions_with_diagnostics(capability)?;
        Ok(providers)
    }

    pub fn provider_versions_with_diagnostics(
        &self,
        capability: &str,
    ) -> Result<(Vec<PackageManifest>, Vec<PackageSkipDiagnostic>)> {
        let mut providers = Vec::new();
        let mut diagnostics = Vec::new();
        for source in &self.sources {
            let (mut source_providers, mut source_diagnostics) = source
                .index
                .provider_versions_with_diagnostics(capability)
                .with_context(|| {
                    format!(
                        "failed loading provider metadata for capability '{capability}' from configured source '{}'",
                        source.name
                    )
                })?;
            retag_diagnostics_source(&mut source_diagnostics, &source.name);
            providers.append(&mut source_providers);
            diagnostics.append(&mut source_diagnostics);
        }
        Ok((providers, diagnostics))
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

    pub fn package_versions_with_source_and_diagnostics(
        &self,
        package: &str,
    ) -> Result<SourcePackageVersionsWithDiagnostics> {
        let mut diagnostics = Vec::new();
        for source in &self.sources {
            let (manifests, mut source_diagnostics) = source
                .index
                .package_versions_with_diagnostics(package)
                .with_context(|| {
                    format!(
                        "failed loading package '{package}' from configured source '{}'",
                        source.name
                    )
                })?;
            retag_diagnostics_source(&mut source_diagnostics, &source.name);
            diagnostics.append(&mut source_diagnostics);
            if !manifests.is_empty() {
                return Ok((Some((source.name.clone(), manifests)), diagnostics));
            }
        }
        Ok((None, diagnostics))
    }
}

fn retag_diagnostics_source(diagnostics: &mut [PackageSkipDiagnostic], source: &str) {
    for diagnostic in diagnostics {
        diagnostic.source = source.to_string();
    }
}
