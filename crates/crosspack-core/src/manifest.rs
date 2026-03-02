use std::collections::{BTreeMap, HashSet};

use anyhow::{anyhow, Context};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};

use crate::artifact::Artifact;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageManifest {
    pub name: String,
    pub version: Version,
    pub description: Option<String>,
    pub license: Option<String>,
    pub homepage: Option<String>,
    #[serde(default)]
    pub provides: Vec<String>,
    #[serde(default)]
    pub conflicts: BTreeMap<String, VersionReq>,
    #[serde(default)]
    pub replaces: BTreeMap<String, VersionReq>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, VersionReq>,
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
    #[serde(default)]
    pub source_build: Option<SourceBuildMetadata>,
    #[serde(default)]
    pub services: Vec<ServiceDeclaration>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ServiceDeclaration {
    pub name: String,
    #[serde(default)]
    pub native_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceBuildMetadata {
    #[serde(alias = "source_url")]
    pub url: String,
    pub archive_sha256: String,
    pub build_system: String,
    #[serde(default)]
    pub build_commands: Vec<String>,
    #[serde(default)]
    pub install_commands: Vec<String>,
}

impl PackageManifest {
    pub fn from_toml_str(input: &str) -> anyhow::Result<Self> {
        let manifest: Self = toml::from_str(input).context("failed to parse crosspack manifest")?;
        if manifest.conflicts.contains_key(&manifest.name) {
            return Err(anyhow!(
                "manifest '{}' conflicts with itself",
                manifest.name
            ));
        }
        if manifest.replaces.contains_key(&manifest.name) {
            return Err(anyhow!("manifest '{}' replaces itself", manifest.name));
        }
        for artifact in &manifest.artifacts {
            let mut seen_app_ids = HashSet::new();
            for gui_app in &artifact.gui_apps {
                if gui_app.app_id.trim().is_empty() {
                    return Err(anyhow!(
                        "gui app id must not be empty for target '{}'",
                        artifact.target
                    ));
                }
                if !seen_app_ids.insert(gui_app.app_id.clone()) {
                    return Err(anyhow!(
                        "duplicate gui app declaration '{}' for target '{}'",
                        gui_app.app_id,
                        artifact.target
                    ));
                }
                for protocol in &gui_app.protocols {
                    validate_protocol_scheme(&protocol.scheme).with_context(|| {
                        format!(
                            "invalid gui protocol scheme '{}' for app '{}' target '{}'",
                            protocol.scheme, gui_app.app_id, artifact.target
                        )
                    })?;
                }
            }
        }
        let mut seen_service_names = HashSet::new();
        for service in &manifest.services {
            validate_service_name_token(&service.name)?;
            if !seen_service_names.insert(service.name.clone()) {
                return Err(anyhow!(
                    "duplicate service declaration '{}' in manifest '{}'",
                    service.name,
                    manifest.name
                ));
            }
            if let Some(native_id) = service.native_id.as_deref() {
                validate_native_service_id_token(native_id)?;
            }
        }
        Ok(manifest)
    }
}

fn validate_service_name_token(value: &str) -> anyhow::Result<()> {
    validate_service_token("service name", value, false)
}

fn validate_native_service_id_token(value: &str) -> anyhow::Result<()> {
    validate_service_token("native service id", value, true)
}

fn validate_service_token(kind: &str, value: &str, allow_at: bool) -> anyhow::Result<()> {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 {
        return Err(anyhow!(
            "invalid {kind} '{value}': use package-token grammar"
        ));
    }

    let starts_valid = bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit();
    let allowed_symbols: &[u8] = if allow_at { b"._+-@" } else { b"._+-" };
    let remainder_valid = bytes[1..]
        .iter()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || allowed_symbols.contains(b));
    if !starts_valid || !remainder_valid {
        return Err(anyhow!(
            "invalid {kind} '{value}': use package-token grammar"
        ));
    }

    Ok(())
}

fn validate_protocol_scheme(scheme: &str) -> anyhow::Result<()> {
    let trimmed = scheme.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("protocol scheme must not be empty"));
    }

    let mut chars = trimmed.chars();
    let Some(first) = chars.next() else {
        return Err(anyhow!("protocol scheme must not be empty"));
    };
    if !first.is_ascii_alphabetic() {
        return Err(anyhow!(
            "protocol scheme must start with an ASCII letter: {scheme}"
        ));
    }
    if chars.any(|ch| !(ch.is_ascii_alphanumeric() || ch == '+' || ch == '-' || ch == '.')) {
        return Err(anyhow!(
            "protocol scheme contains invalid character(s): {scheme}"
        ));
    }

    Ok(())
}
