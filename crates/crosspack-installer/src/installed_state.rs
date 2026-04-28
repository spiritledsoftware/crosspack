use anyhow::{Context, Result};
use crosspack_core::ServiceDeclaration;
use std::fs;

use crate::receipts::parse_receipt;
use crate::{
    read_declared_services_state, read_gui_exposure_state, read_gui_native_state,
    read_integration_state, GuiExposureAsset, GuiNativeRegistrationRecord, InstallReceipt,
    IntegrationProjection, PrefixLayout,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPackageState {
    pub version: String,
    pub receipt: InstallReceipt,
    pub gui_assets: Vec<GuiExposureAsset>,
    pub native_gui_records: Vec<GuiNativeRegistrationRecord>,
    pub services: Vec<ServiceDeclaration>,
    pub integrations: Vec<IntegrationProjection>,
}

pub fn read_installed_package_state(
    layout: &PrefixLayout,
    package_name: &str,
) -> Result<Option<InstalledPackageState>> {
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
        version,
        receipt,
        gui_assets: read_gui_exposure_state(layout, package_name)?,
        native_gui_records: read_gui_native_state(layout, package_name)?,
        services: read_declared_services_state(layout, package_name)?,
        integrations: read_integration_state(layout, package_name)?,
    }))
}
