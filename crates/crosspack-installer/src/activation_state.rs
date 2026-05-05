use anyhow::{anyhow, Context, Result};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use crate::fs_utils::remove_file_if_exists;
use crate::{
    IntegrationActivationRecord, IntegrationActivationScope, IntegrationAdapterKind,
    IntegrationAppliedState, IntegrationDesiredState, IntegrationReasonCode, PrefixLayout,
};

const INTEGRATION_ACTIVATION_STATE_VERSION: u32 = 1;

pub fn write_integration_activation_state(
    layout: &PrefixLayout,
    records: &[IntegrationActivationRecord],
) -> Result<PathBuf> {
    let path = layout.integration_activation_state_path();
    if records.is_empty() {
        remove_file_if_exists(&path).with_context(|| {
            format!(
                "failed to remove integration activation state: {}",
                path.display()
            )
        })?;
        return Ok(path);
    }

    let mut payload = String::new();
    payload.push_str(&format!("version={INTEGRATION_ACTIVATION_STATE_VERSION}\n"));
    let mut seen = HashSet::new();
    for record in records {
        validate_record_fields(record)?;
        if !seen.insert((
            record.package_state_key.as_str(),
            record.integration_key.as_str(),
        )) {
            return Err(anyhow!("duplicate integration activation state record"));
        }
        payload.push_str(&format!(
            "activation={}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            record.package_state_key,
            record.package,
            record.integration_key,
            record.kind,
            record.adapter.as_str(),
            record.scope.as_str(),
            record.desired_state.as_str(),
            record.applied_state.as_str(),
            record.host_path.as_deref().unwrap_or(""),
            record.reason_code.as_str()
        ));
    }

    fs::write(&path, payload.as_bytes()).with_context(|| {
        format!(
            "failed to write integration activation state: {}",
            path.display()
        )
    })?;
    Ok(path)
}

pub fn read_integration_activation_state(
    layout: &PrefixLayout,
) -> Result<Vec<IntegrationActivationRecord>> {
    let path = layout.integration_activation_state_path();
    if !path.exists() {
        return Ok(Vec::new());
    }

    let raw = fs::read_to_string(&path).with_context(|| {
        format!(
            "failed to read integration activation state: {}",
            path.display()
        )
    })?;
    parse_integration_activation_state(&raw).with_context(|| {
        format!(
            "failed to parse integration activation state: {}",
            path.display()
        )
    })
}

fn parse_integration_activation_state(raw: &str) -> Result<Vec<IntegrationActivationRecord>> {
    let mut version = None;
    let mut records = Vec::new();
    let mut seen = HashSet::new();

    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        let Some((key, value)) = line.split_once('=') else {
            return Err(anyhow!("invalid integration activation state row format"));
        };
        match key {
            "version" => {
                if version.is_some() {
                    return Err(anyhow!("duplicate integration activation state version"));
                }
                version = Some(
                    value
                        .parse::<u32>()
                        .context("invalid integration activation state version")?,
                );
            }
            "activation" => {
                let record = parse_activation_row(value)?;
                if !seen.insert((
                    record.package_state_key.clone(),
                    record.integration_key.clone(),
                )) {
                    return Err(anyhow!("duplicate integration activation state record"));
                }
                records.push(record);
            }
            _ => return Err(anyhow!("invalid integration activation state row format")),
        }
    }

    let Some(version) = version else {
        return Err(anyhow!("missing integration activation state version"));
    };
    if version != INTEGRATION_ACTIVATION_STATE_VERSION {
        return Err(anyhow!("unsupported integration activation state version"));
    }

    Ok(records)
}

fn parse_activation_row(value: &str) -> Result<IntegrationActivationRecord> {
    let fields = value.split('\t').collect::<Vec<_>>();
    if fields.len() != 10 {
        return Err(anyhow!("invalid integration activation state row format"));
    }

    Ok(IntegrationActivationRecord {
        package_state_key: fields[0].to_string(),
        package: fields[1].to_string(),
        integration_key: fields[2].to_string(),
        kind: fields[3].to_string(),
        adapter: IntegrationAdapterKind::parse(fields[4])?,
        scope: IntegrationActivationScope::parse(fields[5])?,
        desired_state: IntegrationDesiredState::parse(fields[6])?,
        applied_state: IntegrationAppliedState::parse(fields[7])?,
        host_path: if fields[8].is_empty() {
            None
        } else {
            Some(fields[8].to_string())
        },
        reason_code: IntegrationReasonCode::parse(fields[9])?,
    })
}

fn validate_record_fields(record: &IntegrationActivationRecord) -> Result<()> {
    for field in [
        record.package_state_key.as_str(),
        record.package.as_str(),
        record.integration_key.as_str(),
        record.kind.as_str(),
        record.host_path.as_deref().unwrap_or(""),
    ] {
        if field.contains(['\t', '\n']) {
            return Err(anyhow!(
                "integration activation state values must not contain tabs or newlines"
            ));
        }
    }
    Ok(())
}
