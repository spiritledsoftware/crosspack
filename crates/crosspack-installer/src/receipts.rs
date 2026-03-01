use anyhow::{anyhow, Context, Result};
use crosspack_core::ServiceDeclaration;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use crate::{InstallMode, InstallReason, InstallReceipt, PrefixLayout};

pub fn write_install_receipt(layout: &PrefixLayout, receipt: &InstallReceipt) -> Result<PathBuf> {
    let mut payload = String::new();
    payload.push_str(&format!("name={}\n", receipt.name));
    payload.push_str(&format!("version={}\n", receipt.version));
    for dependency in &receipt.dependencies {
        payload.push_str(&format!("dependency={}\n", dependency));
    }
    if let Some(target) = &receipt.target {
        payload.push_str(&format!("target={}\n", target));
    }
    if let Some(url) = &receipt.artifact_url {
        payload.push_str(&format!("artifact_url={}\n", url));
    }
    if let Some(sha256) = &receipt.artifact_sha256 {
        payload.push_str(&format!("artifact_sha256={}\n", sha256));
    }
    if let Some(cache_path) = &receipt.cache_path {
        payload.push_str(&format!("cache_path={}\n", cache_path));
    }
    for exposed_bin in &receipt.exposed_bins {
        payload.push_str(&format!("exposed_bin={}\n", exposed_bin));
    }
    for exposed_completion in &receipt.exposed_completions {
        payload.push_str(&format!("exposed_completion={}\n", exposed_completion));
    }
    if let Some(snapshot_id) = &receipt.snapshot_id {
        payload.push_str(&format!("snapshot_id={}\n", snapshot_id));
    }
    payload.push_str(&format!("install_mode={}\n", receipt.install_mode.as_str()));
    payload.push_str(&format!(
        "install_reason={}\n",
        receipt.install_reason.as_str()
    ));
    payload.push_str(&format!("install_status={}\n", receipt.install_status));
    payload.push_str(&format!(
        "installed_at_unix={}\n",
        receipt.installed_at_unix
    ));

    let path = layout.receipt_path(&receipt.name);
    fs::write(&path, payload.as_bytes())
        .with_context(|| format!("failed to write install receipt: {}", path.display()))?;
    Ok(path)
}

pub fn read_install_receipts(layout: &PrefixLayout) -> Result<Vec<InstallReceipt>> {
    let dir = layout.installed_state_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut receipts = Vec::new();
    for entry in fs::read_dir(&dir)
        .with_context(|| format!("failed to read install state directory: {}", dir.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }

        let path = entry.path();
        if path.extension().and_then(|v| v.to_str()) != Some("receipt") {
            continue;
        }

        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read install receipt: {}", path.display()))?;
        let receipt = parse_receipt(&raw)
            .with_context(|| format!("failed to parse install receipt: {}", path.display()))?;
        receipts.push(receipt);
    }

    receipts.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(receipts)
}

pub(crate) fn parse_receipt(raw: &str) -> Result<InstallReceipt> {
    let mut name = None;
    let mut version = None;
    let mut dependencies = Vec::new();
    let mut target = None;
    let mut artifact_url = None;
    let mut artifact_sha256 = None;
    let mut cache_path = None;
    let mut exposed_bins = Vec::new();
    let mut exposed_completions = Vec::new();
    let mut snapshot_id = None;
    let mut install_mode = None;
    let mut install_reason = None;
    let mut install_status = None;
    let mut installed_at_unix = None;

    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k {
            "name" => name = Some(v.to_string()),
            "version" => version = Some(v.to_string()),
            "dependency" => dependencies.push(v.to_string()),
            "target" => target = Some(v.to_string()),
            "artifact_url" => artifact_url = Some(v.to_string()),
            "artifact_sha256" => artifact_sha256 = Some(v.to_string()),
            "cache_path" => cache_path = Some(v.to_string()),
            "exposed_bin" => exposed_bins.push(v.to_string()),
            "exposed_completion" => exposed_completions.push(v.to_string()),
            "snapshot_id" => snapshot_id = Some(v.to_string()),
            "install_mode" => install_mode = Some(InstallMode::parse_receipt_token(v)),
            "install_reason" => install_reason = Some(InstallReason::parse(v)?),
            "install_status" => install_status = Some(v.to_string()),
            "installed_at_unix" => {
                installed_at_unix = Some(v.parse().context("installed_at_unix must be u64")?)
            }
            _ => {}
        }
    }

    Ok(InstallReceipt {
        name: name.context("missing name")?,
        version: version.context("missing version")?,
        dependencies,
        target,
        artifact_url,
        artifact_sha256,
        cache_path,
        exposed_bins,
        exposed_completions,
        snapshot_id,
        install_mode: install_mode.unwrap_or(InstallMode::Managed),
        install_reason: install_reason.unwrap_or(InstallReason::Root),
        install_status: install_status.unwrap_or_else(|| "installed".to_string()),
        installed_at_unix: installed_at_unix.context("missing installed_at_unix")?,
    })
}

const DECLARED_SERVICES_STATE_VERSION: u32 = 1;

pub fn write_declared_services_state(
    layout: &PrefixLayout,
    package_name: &str,
    services: &[ServiceDeclaration],
) -> Result<PathBuf> {
    let path = layout.declared_services_state_path(package_name);
    if services.is_empty() {
        let _ = fs::remove_file(&path);
        return Ok(path);
    }

    let mut payload = String::new();
    payload.push_str(&format!("version={}\n", DECLARED_SERVICES_STATE_VERSION));
    for service in services {
        if service.name.contains('\t')
            || service.name.contains('\n')
            || service
                .native_id
                .as_deref()
                .is_some_and(|id| id.contains('\t') || id.contains('\n'))
        {
            return Err(anyhow!(
                "declared service values must not contain tabs or newlines"
            ));
        }
        payload.push_str(&format!(
            "service={}\t{}\n",
            service.name,
            service.native_id.as_deref().unwrap_or("")
        ));
    }

    fs::write(&path, payload.as_bytes()).with_context(|| {
        format!(
            "failed to write declared services state: {}",
            path.display()
        )
    })?;
    Ok(path)
}

pub fn read_declared_services_state(
    layout: &PrefixLayout,
    package_name: &str,
) -> Result<Vec<ServiceDeclaration>> {
    let path = layout.declared_services_state_path(package_name);
    if !path.exists() {
        return Ok(Vec::new());
    }

    let raw = fs::read_to_string(&path)
        .with_context(|| format!("failed to read declared services state: {}", path.display()))?;
    parse_declared_services_state(&raw).with_context(|| {
        format!(
            "failed to parse declared services state: {}",
            path.display()
        )
    })
}

pub fn read_all_declared_services_states(
    layout: &PrefixLayout,
) -> Result<BTreeMap<String, Vec<ServiceDeclaration>>> {
    let dir = layout.installed_state_dir();
    if !dir.exists() {
        return Ok(BTreeMap::new());
    }

    let mut states = BTreeMap::new();
    for entry in fs::read_dir(&dir)
        .with_context(|| format!("failed to read install state directory: {}", dir.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        if !path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.ends_with(".services"))
        {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|v| v.to_str()) else {
            continue;
        };
        let raw = fs::read_to_string(&path).with_context(|| {
            format!("failed to read declared services state: {}", path.display())
        })?;
        let services = parse_declared_services_state(&raw).with_context(|| {
            format!(
                "failed to parse declared services state: {}",
                path.display()
            )
        })?;
        states.insert(stem.to_string(), services);
    }

    Ok(states)
}

pub fn clear_declared_services_state(layout: &PrefixLayout, package_name: &str) -> Result<()> {
    let path = layout.declared_services_state_path(package_name);
    let _ = fs::remove_file(path);
    Ok(())
}

fn parse_declared_services_state(raw: &str) -> Result<Vec<ServiceDeclaration>> {
    let mut version = None;
    let mut services = Vec::new();

    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        let Some((key, value)) = line.split_once('=') else {
            return Err(anyhow!("invalid declared services row format: {line}"));
        };
        match key {
            "version" => {
                version = Some(
                    value
                        .parse::<u32>()
                        .context("declared services version must be u32")?,
                );
            }
            "service" => {
                let parts = value.split('\t').collect::<Vec<_>>();
                if parts.len() != 2 {
                    return Err(anyhow!("invalid declared service row format"));
                }
                if parts[0].trim().is_empty() {
                    return Err(anyhow!("declared service name must not be empty"));
                }
                let native_id = if parts[1].trim().is_empty() {
                    None
                } else {
                    Some(parts[1].to_string())
                };
                services.push(ServiceDeclaration {
                    name: parts[0].to_string(),
                    native_id,
                });
            }
            _ => {}
        }
    }

    if let Some(found_version) = version {
        if found_version != DECLARED_SERVICES_STATE_VERSION {
            return Err(anyhow!(
                "unsupported declared services state version: {found_version}"
            ));
        }
    }

    Ok(services)
}
