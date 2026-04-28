use anyhow::{Context, Result};
use crosspack_core::ServiceDeclaration;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::receipts::parse_receipt;
use crate::{
    read_declared_services_state, read_gui_exposure_state, read_gui_native_state,
    read_install_receipts, read_integration_state, GuiExposureAsset, GuiNativeRegistrationRecord,
    InstallMode, InstallReason, InstallReceipt, InstalledPackageIdentity, IntegrationProjection,
    PrefixLayout,
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
    package: String,
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
            package: identity.package.clone(),
        }
    }
}

impl From<InstalledPackageIdentityDocument> for InstalledPackageIdentity {
    fn from(document: InstalledPackageIdentityDocument) -> Self {
        Self {
            profile: document.profile,
            target: document.target,
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

fn read_installed_package_state_document(
    layout: &PrefixLayout,
    package_name: &str,
) -> Result<Option<InstalledPackageState>> {
    let receipt_path = layout.receipt_path(package_name);
    let identity_path = if receipt_path.exists() {
        let raw = fs::read_to_string(&receipt_path).with_context(|| {
            format!("failed to read install receipt: {}", receipt_path.display())
        })?;
        let receipt = parse_receipt(&raw).with_context(|| {
            format!(
                "failed to parse install receipt: {}",
                receipt_path.display()
            )
        })?;
        Some(layout.installed_identity_state_document_path(
            &InstalledPackageIdentity::from_legacy_receipt(&receipt),
        ))
    } else {
        None
    };
    let legacy_path = layout.installed_state_document_path(package_name);
    let path = identity_path
        .filter(|path| path.exists())
        .unwrap_or(legacy_path);
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
        return Ok(None);
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

pub fn read_all_installed_package_states(
    layout: &PrefixLayout,
) -> Result<Vec<InstalledPackageState>> {
    let receipts = read_install_receipts(layout)?;
    let mut states = Vec::new();
    let mut state_keys = std::collections::BTreeSet::new();
    for receipt in receipts {
        if let Some(state) = read_installed_package_state(layout, &receipt.name)? {
            state_keys.insert(state.identity.state_key());
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
        if state_keys.insert(state.identity.state_key()) {
            states.push(state);
        }
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
