use anyhow::{Context, Result};
use crosspack_core::ServiceDeclaration;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::receipts::parse_receipt;
use crate::{
    read_declared_services_state, read_gui_exposure_state, read_gui_native_state,
    read_install_receipts, read_integration_state, GuiExposureAsset, GuiNativeRegistrationRecord,
    InstallMode, InstallReason, InstallReceipt, InstalledPackageIdentity, InstalledPackageSelector,
    IntegrationProjection, PrefixLayout,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPackageState {
    pub identity: InstalledPackageIdentity,
    pub version: String,
    pub receipt: InstallReceipt,
    pub gui_assets: Vec<GuiExposureAsset>,
    pub native_gui_records: Vec<GuiNativeRegistrationRecord>,
    pub services: Vec<ServiceDeclaration>,
    pub integrations: Vec<IntegrationProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPackageSelectionAmbiguity {
    pub selector: InstalledPackageSelector,
    pub matches: Vec<InstalledPackageState>,
}

impl std::fmt::Display for InstalledPackageSelectionAmbiguity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "installed package name '{}' is ambiguous",
            self.selector.package
        )
    }
}

impl std::error::Error for InstalledPackageSelectionAmbiguity {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstalledPackageStateDocument {
    version: u32,
    identity: InstalledPackageIdentityDocument,
    receipt: InstallReceiptDocument,
    gui_assets: Vec<GuiExposureAssetDocument>,
    native_gui_records: Vec<GuiNativeRegistrationRecordDocument>,
    services: Vec<ServiceDeclaration>,
    integrations: Vec<IntegrationProjectionDocument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallReceiptDocument {
    name: String,
    version: String,
    dependencies: Vec<String>,
    target: Option<String>,
    artifact_url: Option<String>,
    artifact_sha256: Option<String>,
    cache_path: Option<String>,
    exposed_bins: Vec<String>,
    exposed_completions: Vec<String>,
    snapshot_id: Option<String>,
    install_mode: String,
    install_reason: String,
    install_status: String,
    installed_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstalledPackageIdentityDocument {
    profile: String,
    target: Option<String>,
    #[serde(default = "default_installed_source_namespace")]
    source_namespace: String,
    #[serde(default = "default_installed_source_provenance")]
    source_provenance: Option<String>,
    package: String,
}

fn default_installed_source_namespace() -> String {
    "default".to_string()
}

fn default_installed_source_provenance() -> Option<String> {
    Some("unknown".to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GuiExposureAssetDocument {
    key: String,
    rel_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GuiNativeRegistrationRecordDocument {
    key: String,
    kind: String,
    path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IntegrationProjectionDocument {
    kind: String,
    key: String,
    rel_path: String,
}

impl From<&InstalledPackageState> for InstalledPackageStateDocument {
    fn from(state: &InstalledPackageState) -> Self {
        Self {
            version: 1,
            identity: InstalledPackageIdentityDocument::from(&state.identity),
            receipt: InstallReceiptDocument::from(&state.receipt),
            gui_assets: state.gui_assets.iter().map(Into::into).collect(),
            native_gui_records: state.native_gui_records.iter().map(Into::into).collect(),
            services: state.services.clone(),
            integrations: state.integrations.iter().map(Into::into).collect(),
        }
    }
}

impl TryFrom<InstalledPackageStateDocument> for InstalledPackageState {
    type Error = anyhow::Error;

    fn try_from(document: InstalledPackageStateDocument) -> Result<Self> {
        let receipt = InstallReceipt::try_from(document.receipt)?;
        Ok(Self {
            identity: InstalledPackageIdentity::from(document.identity),
            version: receipt.version.clone(),
            receipt,
            gui_assets: document.gui_assets.into_iter().map(Into::into).collect(),
            native_gui_records: document
                .native_gui_records
                .into_iter()
                .map(Into::into)
                .collect(),
            services: document.services,
            integrations: document.integrations.into_iter().map(Into::into).collect(),
        })
    }
}

impl From<&InstalledPackageIdentity> for InstalledPackageIdentityDocument {
    fn from(identity: &InstalledPackageIdentity) -> Self {
        Self {
            profile: identity.profile.clone(),
            target: identity.target.clone(),
            source_namespace: identity.source_namespace.clone(),
            source_provenance: identity.source_provenance.clone(),
            package: identity.package.clone(),
        }
    }
}

impl From<InstalledPackageIdentityDocument> for InstalledPackageIdentity {
    fn from(document: InstalledPackageIdentityDocument) -> Self {
        Self {
            profile: document.profile,
            target: document.target,
            source_namespace: document.source_namespace,
            source_provenance: document
                .source_provenance
                .or_else(default_installed_source_provenance),
            package: document.package,
        }
    }
}

impl From<&InstallReceipt> for InstallReceiptDocument {
    fn from(receipt: &InstallReceipt) -> Self {
        Self {
            name: receipt.name.clone(),
            version: receipt.version.clone(),
            dependencies: receipt.dependencies.clone(),
            target: receipt.target.clone(),
            artifact_url: receipt.artifact_url.clone(),
            artifact_sha256: receipt.artifact_sha256.clone(),
            cache_path: receipt.cache_path.clone(),
            exposed_bins: receipt.exposed_bins.clone(),
            exposed_completions: receipt.exposed_completions.clone(),
            snapshot_id: receipt.snapshot_id.clone(),
            install_mode: receipt.install_mode.as_str().to_string(),
            install_reason: receipt.install_reason.as_str().to_string(),
            install_status: receipt.install_status.clone(),
            installed_at_unix: receipt.installed_at_unix,
        }
    }
}

impl TryFrom<InstallReceiptDocument> for InstallReceipt {
    type Error = anyhow::Error;

    fn try_from(document: InstallReceiptDocument) -> Result<Self> {
        Ok(Self {
            name: document.name,
            version: document.version,
            dependencies: document.dependencies,
            target: document.target,
            artifact_url: document.artifact_url,
            artifact_sha256: document.artifact_sha256,
            cache_path: document.cache_path,
            exposed_bins: document.exposed_bins,
            exposed_completions: document.exposed_completions,
            snapshot_id: document.snapshot_id,
            install_mode: InstallMode::parse(&document.install_mode)?,
            install_reason: InstallReason::parse(&document.install_reason)?,
            install_status: document.install_status,
            installed_at_unix: document.installed_at_unix,
        })
    }
}

impl From<&GuiExposureAsset> for GuiExposureAssetDocument {
    fn from(value: &GuiExposureAsset) -> Self {
        Self {
            key: value.key.clone(),
            rel_path: value.rel_path.clone(),
        }
    }
}

impl From<GuiExposureAssetDocument> for GuiExposureAsset {
    fn from(value: GuiExposureAssetDocument) -> Self {
        Self {
            key: value.key,
            rel_path: value.rel_path,
        }
    }
}

impl From<&GuiNativeRegistrationRecord> for GuiNativeRegistrationRecordDocument {
    fn from(value: &GuiNativeRegistrationRecord) -> Self {
        Self {
            key: value.key.clone(),
            kind: value.kind.clone(),
            path: value.path.clone(),
        }
    }
}

impl From<GuiNativeRegistrationRecordDocument> for GuiNativeRegistrationRecord {
    fn from(value: GuiNativeRegistrationRecordDocument) -> Self {
        Self {
            key: value.key,
            kind: value.kind,
            path: value.path,
        }
    }
}

impl From<&IntegrationProjection> for IntegrationProjectionDocument {
    fn from(value: &IntegrationProjection) -> Self {
        Self {
            kind: value.kind.clone(),
            key: value.key.clone(),
            rel_path: value.rel_path.clone(),
        }
    }
}

impl From<IntegrationProjectionDocument> for IntegrationProjection {
    fn from(value: IntegrationProjectionDocument) -> Self {
        Self {
            kind: value.kind,
            key: value.key,
            rel_path: value.rel_path,
        }
    }
}

pub fn write_installed_package_state(
    layout: &PrefixLayout,
    state: &InstalledPackageState,
) -> Result<PathBuf> {
    let path = layout.installed_identity_state_document_path(&state.identity);
    let document = InstalledPackageStateDocument::from(state);
    let raw =
        serde_json::to_vec_pretty(&document).context("failed to serialize installed state")?;
    crate::atomic_write::write_file_atomically(&path, &raw)?;
    Ok(path)
}

pub fn clear_installed_package_state_document(
    layout: &PrefixLayout,
    receipt: &InstallReceipt,
) -> Result<()> {
    let identity_path = layout.installed_identity_state_document_path(
        &InstalledPackageIdentity::from_legacy_receipt(receipt),
    );
    crate::remove_file_if_exists(&identity_path).with_context(|| {
        format!(
            "failed to remove installed state document: {}",
            identity_path.display()
        )
    })?;

    let legacy_path = layout.installed_state_document_path(&receipt.name);
    crate::remove_file_if_exists(&legacy_path).with_context(|| {
        format!(
            "failed to remove legacy installed state document: {}",
            legacy_path.display()
        )
    })?;

    Ok(())
}

fn read_installed_package_state_document(
    layout: &PrefixLayout,
    package_name: &str,
) -> Result<Option<InstalledPackageState>> {
    let receipt_path = layout.receipt_path(package_name);
    let identity_paths = if receipt_path.exists() {
        let raw = fs::read_to_string(&receipt_path).with_context(|| {
            format!("failed to read install receipt: {}", receipt_path.display())
        })?;
        let receipt = parse_receipt(&raw).with_context(|| {
            format!(
                "failed to parse install receipt: {}",
                receipt_path.display()
            )
        })?;
        let identity = InstalledPackageIdentity::from_legacy_receipt(&receipt);
        vec![
            layout.installed_identity_state_document_path(&identity),
            layout.installed_legacy_identity_state_document_path(&identity),
        ]
    } else {
        Vec::new()
    };
    let legacy_path = layout.installed_state_document_path(package_name);
    let path = identity_paths
        .into_iter()
        .chain(std::iter::once(legacy_path))
        .find(|path| path.exists());
    let Some(path) = path else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }

    read_installed_package_state_document_path(&path).map(Some)
}

fn read_installed_package_state_document_path(
    path: &std::path::Path,
) -> Result<InstalledPackageState> {
    let raw = fs::read_to_string(path).with_context(|| {
        format!(
            "failed to read installed state document: {}",
            path.display()
        )
    })?;
    let document: InstalledPackageStateDocument =
        serde_json::from_str(&raw).with_context(|| {
            format!(
                "failed to parse installed state document: {}",
                path.display()
            )
        })?;
    InstalledPackageState::try_from(document)
}

pub fn read_installed_package_state(
    layout: &PrefixLayout,
    package_name: &str,
) -> Result<Option<InstalledPackageState>> {
    if let Some(state) = read_installed_package_state_document(layout, package_name)? {
        return Ok(Some(state));
    }

    let receipt_path = layout.receipt_path(package_name);
    if !receipt_path.exists() {
        return read_installed_package_state_document_by_package_name(layout, package_name);
    }

    let raw = fs::read_to_string(&receipt_path)
        .with_context(|| format!("failed to read install receipt: {}", receipt_path.display()))?;
    let receipt = parse_receipt(&raw).with_context(|| {
        format!(
            "failed to parse install receipt: {}",
            receipt_path.display()
        )
    })?;
    let version = receipt.version.clone();

    Ok(Some(InstalledPackageState {
        identity: InstalledPackageIdentity::from_legacy_receipt(&receipt),
        version,
        receipt,
        gui_assets: read_gui_exposure_state(layout, package_name)?,
        native_gui_records: read_gui_native_state(layout, package_name)?,
        services: read_declared_services_state(layout, package_name)?,
        integrations: read_integration_state(layout, package_name)?,
    }))
}

fn read_installed_package_state_document_by_package_name(
    layout: &PrefixLayout,
    package_name: &str,
) -> Result<Option<InstalledPackageState>> {
    let installed_dir = layout.installed_state_dir();
    let entries = match fs::read_dir(&installed_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read {}", installed_dir.display()));
        }
    };
    let mut matches = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("failed to read {}", installed_dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let state = read_installed_package_state_document_path(&path)?;
        if state.identity.package == package_name {
            matches.push(state);
        }
    }
    matches.sort_by(|left, right| left.identity.cmp(&right.identity));
    Ok(matches.into_iter().next())
}

pub fn read_all_installed_package_states(
    layout: &PrefixLayout,
) -> Result<Vec<InstalledPackageState>> {
    let receipts = read_install_receipts(layout)?;
    let mut states = Vec::new();
    let mut state_keys = std::collections::BTreeSet::new();
    let mut document_state_keys = std::collections::BTreeMap::new();
    for receipt in receipts {
        if let Some(state) = read_installed_package_state(layout, &receipt.name)? {
            let state_key = state.identity.state_key();
            if !state_keys.insert(state_key.clone()) {
                return Err(anyhow::anyhow!("duplicate installed identity: {state_key}"));
            }
            states.push(state);
        }
    }
    let installed_dir = layout.installed_state_dir();
    let entries = match fs::read_dir(&installed_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            states.sort_by(|left, right| {
                left.receipt
                    .name
                    .cmp(&right.receipt.name)
                    .then_with(|| left.identity.cmp(&right.identity))
            });
            return Ok(states);
        }
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read {}", installed_dir.display()));
        }
    };
    for entry in entries {
        let entry = entry.with_context(|| format!("failed to read {}", installed_dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let state = read_installed_package_state_document_path(&path)?;
        let state_key = state.identity.state_key();
        let legacy_path = layout.installed_state_document_path(&state.identity.package);
        if let Some(previous_path) = document_state_keys.insert(state_key.clone(), path.clone()) {
            if path == legacy_path || previous_path == legacy_path {
                continue;
            }
            return Err(anyhow::anyhow!("duplicate installed identity: {state_key}"));
        }
        if !state_keys.insert(state_key) {
            continue;
        }
        states.push(state);
    }
    states.sort_by(|left, right| {
        left.receipt
            .name
            .cmp(&right.receipt.name)
            .then_with(|| left.identity.cmp(&right.identity))
    });
    Ok(states)
}

pub fn find_installed_states_by_package_name(
    layout: &PrefixLayout,
    package_name: &str,
) -> Result<Vec<InstalledPackageState>> {
    Ok(read_all_installed_package_states(layout)?
        .into_iter()
        .filter(|state| state.identity.package == package_name)
        .collect())
}

pub fn resolve_installed_package_selector(
    layout: &PrefixLayout,
    selector: &InstalledPackageSelector,
) -> Result<std::result::Result<Option<InstalledPackageState>, InstalledPackageSelectionAmbiguity>>
{
    let mut matches = read_all_installed_package_states(layout)?
        .into_iter()
        .filter(|state| selector.matches(&state.identity))
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| left.identity.cmp(&right.identity));

    Ok(match matches.len() {
        0 => Ok(None),
        1 => Ok(matches.into_iter().next()),
        _ => Err(InstalledPackageSelectionAmbiguity {
            selector: selector.clone(),
            matches,
        }),
    })
}
