use std::collections::HashSet;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::RegistrySourceRecord;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RegistrySourceStateFile {
    #[serde(default = "state_file_version")]
    pub(crate) version: u32,
    #[serde(default)]
    pub(crate) sources: Vec<RegistrySourceRecord>,
}

#[derive(Debug, Deserialize)]
struct RegistrySourceStateFileLegacy {
    #[serde(default)]
    sources: Vec<RegistrySourceRecord>,
}

impl Default for RegistrySourceStateFile {
    fn default() -> Self {
        Self {
            version: state_file_version(),
            sources: Vec::new(),
        }
    }
}

pub(crate) fn parse_source_state_file(content: &str) -> Result<RegistrySourceStateFile> {
    let value = toml::from_str::<toml::Value>(content)?;
    let mut state = if value.get("version").is_some() {
        let parsed = value
            .clone()
            .try_into::<RegistrySourceStateFile>()
            .context("failed parsing versioned source state")?;
        let expected = state_file_version();
        if parsed.version != expected {
            anyhow::bail!(
                "unsupported source state version {} (expected {}): update sources.toml to version {}",
                parsed.version,
                expected,
                expected
            );
        }
        parsed
    } else {
        let parsed = value
            .try_into::<RegistrySourceStateFileLegacy>()
            .context("failed parsing legacy source state")?;
        RegistrySourceStateFile {
            version: state_file_version(),
            sources: parsed.sources,
        }
    };

    validate_loaded_sources(&state.sources)?;
    state.version = state_file_version();
    Ok(state)
}

pub(crate) fn state_file_version() -> u32 {
    1
}

pub(crate) fn sort_sources(sources: &mut [RegistrySourceRecord]) {
    sources.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.name.cmp(&right.name))
    });
}

pub(crate) fn validate_source_name(name: &str) -> Result<()> {
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

pub(crate) fn validate_source_fingerprint(fingerprint: &str) -> Result<()> {
    if fingerprint.len() != 64 || !fingerprint.chars().all(|ch| ch.is_ascii_hexdigit()) {
        anyhow::bail!("invalid source fingerprint: '{fingerprint}'");
    }

    Ok(())
}

pub(crate) fn validate_loaded_sources(sources: &[RegistrySourceRecord]) -> Result<()> {
    let mut seen_names: HashSet<&str> = HashSet::with_capacity(sources.len());
    for source in sources {
        validate_source_name(&source.name)?;
        validate_source_fingerprint(&source.fingerprint_sha256)?;

        if !seen_names.insert(source.name.as_str()) {
            anyhow::bail!(
                "duplicate source name '{}' in sources.toml: remove or rename one entry",
                source.name
            );
        }
    }

    Ok(())
}

pub(crate) fn select_update_sources(
    sources: &[RegistrySourceRecord],
    target_names: &[String],
) -> Result<Vec<RegistrySourceRecord>> {
    if target_names.is_empty() {
        return Ok(sources.to_vec());
    }

    let known_names: HashSet<&str> = sources.iter().map(|source| source.name.as_str()).collect();
    for name in target_names {
        if !known_names.contains(name.as_str()) {
            anyhow::bail!("source-not-found: source '{}' not found", name);
        }
    }

    let target_set: HashSet<&str> = target_names.iter().map(String::as_str).collect();
    Ok(sources
        .iter()
        .filter(|source| target_set.contains(source.name.as_str()))
        .cloned()
        .collect())
}
