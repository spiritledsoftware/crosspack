use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path};

use anyhow::{anyhow, Context};
use semver::{Version, VersionReq};
use serde::{de, Deserialize, Deserializer, Serialize};

use crate::artifact::Artifact;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageManifest {
    pub name: String,
    #[serde(deserialize_with = "deserialize_manifest_version")]
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
    #[serde(default)]
    pub integrations: Vec<PackageIntegration>,
}

fn deserialize_manifest_version<'de, D>(deserializer: D) -> Result<Version, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    Version::parse(&value)
        .or_else(|_| parse_version_with_lenient_core_identifiers(&value))
        .map_err(de::Error::custom)
}

fn parse_version_with_lenient_core_identifiers(value: &str) -> Result<Version, semver::Error> {
    let (without_build, build) = value
        .split_once('+')
        .map_or((value, None), |(core, build)| (core, Some(build)));
    let (core, pre) = without_build
        .split_once('-')
        .map_or((without_build, None), |(core, pre)| (core, Some(pre)));
    let mut identifiers = core.split('.');
    let Some(major) = identifiers.next() else {
        return Version::parse(value);
    };
    let Some(minor) = identifiers.next() else {
        return Version::parse(value);
    };
    let Some(patch) = identifiers.next() else {
        return Version::parse(value);
    };
    if identifiers.next().is_some() {
        return Version::parse(value);
    }

    let Some(major) = normalize_numeric_identifier(major) else {
        return Version::parse(value);
    };
    let Some(minor) = normalize_numeric_identifier(minor) else {
        return Version::parse(value);
    };
    let Some(patch) = normalize_numeric_identifier(patch) else {
        return Version::parse(value);
    };

    let mut normalized = format!("{major}.{minor}.{patch}");
    if let Some(pre) = pre {
        normalized.push('-');
        normalized.push_str(pre);
    }
    if let Some(build) = build {
        normalized.push('+');
        normalized.push_str(build);
    }
    Version::parse(&normalized)
}

fn normalize_numeric_identifier(value: &str) -> Option<String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    let trimmed = value.trim_start_matches('0');
    if trimmed.is_empty() {
        Some("0".to_string())
    } else {
        Some(trimmed.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PackageIntegration {
    DockerCliPlugin {
        name: String,
        source: String,
    },
    PathPlugin {
        host: String,
        name: String,
        source: String,
    },
    Service {
        name: String,
        #[serde(default, alias = "source")]
        linux_systemd_user: Option<String>,
        #[serde(default)]
        macos_launch_agent: Option<String>,
        #[serde(default)]
        windows_service: Option<String>,
        #[serde(default)]
        enable: bool,
    },
}

impl PackageIntegration {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::DockerCliPlugin { .. } => "docker_cli_plugin",
            Self::PathPlugin { .. } => "path_plugin",
            Self::Service { .. } => "service",
        }
    }

    pub fn source(&self) -> &str {
        match self {
            Self::DockerCliPlugin { source, .. } | Self::PathPlugin { source, .. } => source,
            Self::Service {
                linux_systemd_user, ..
            } => linux_systemd_user.as_deref().unwrap_or(""),
        }
    }

    fn ownership_key(&self) -> String {
        match self {
            Self::DockerCliPlugin { name, .. } => format!("docker_cli_plugin:{name}"),
            Self::PathPlugin { host, name, .. } => format!("path_plugin:{host}:{name}"),
            Self::Service { name, .. } => format!("service:{name}"),
        }
    }

    fn validate(&self) -> anyhow::Result<()> {
        match self {
            Self::DockerCliPlugin { name, source } => {
                validate_integration_source_path(source)?;
                validate_service_token("integration name", name, false)
            }
            Self::Service {
                name,
                linux_systemd_user,
                macos_launch_agent,
                windows_service,
                ..
            } => {
                if linux_systemd_user.is_none()
                    && macos_launch_agent.is_none()
                    && windows_service.is_none()
                {
                    return Err(anyhow!(
                        "service integration '{name}' must declare at least one source"
                    ));
                }
                for (field, source) in [
                    ("linux_systemd_user", linux_systemd_user),
                    ("macos_launch_agent", macos_launch_agent),
                    ("windows_service", windows_service),
                ] {
                    if let Some(source) = source {
                        validate_integration_source_path(source)
                            .with_context(|| format!("invalid service {field} source"))?;
                    }
                }
                validate_service_token("integration name", name, false)
            }
            Self::PathPlugin { host, name, source } => {
                validate_integration_source_path(source)?;
                validate_service_token("integration host", host, false)?;
                validate_service_token("integration name", name, false)
            }
        }
    }
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
        let mut seen_integration_keys = HashSet::new();
        for integration in &manifest.integrations {
            integration.validate()?;
            let key = integration.ownership_key();
            if !seen_integration_keys.insert(key.clone()) {
                return Err(anyhow!(
                    "duplicate integration declaration '{}' in manifest '{}'",
                    key,
                    manifest.name
                ));
            }
        }
        Ok(manifest)
    }
}

fn validate_integration_source_path(value: &str) -> anyhow::Result<()> {
    let relative = Path::new(value);
    if relative.is_absolute() {
        return Err(anyhow!("integration source path must be relative: {value}"));
    }
    if relative.as_os_str().is_empty() {
        return Err(anyhow!("integration source path must not be empty"));
    }
    if value == "." {
        return Err(anyhow!(
            "integration source path must not be current dir: {value}"
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(anyhow!(
            "integration source path must not include control characters: {value:?}"
        ));
    }
    if value.contains('\\') {
        return Err(anyhow!(
            "integration source path must not include backslashes: {value}"
        ));
    }
    if value
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(anyhow!(
            "integration source path must not include empty, '.', or '..' components: {value}"
        ));
    }
    if looks_like_windows_drive_path(value) {
        return Err(anyhow!(
            "integration source path must not include Windows drive prefixes: {value}"
        ));
    }
    if relative
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(anyhow!(
            "integration source path must not include '..' or prefixes: {value}"
        ));
    }
    Ok(())
}

fn looks_like_windows_drive_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic()
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
