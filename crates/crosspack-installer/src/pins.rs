use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use crate::PrefixLayout;

pub fn write_pin(layout: &PrefixLayout, name: &str, requirement: &str) -> Result<PathBuf> {
    let pin_path = layout.pin_path(name);
    if let Some(parent) = pin_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create pin dir: {}", parent.display()))?;
    }

    fs::write(&pin_path, requirement.as_bytes())
        .with_context(|| format!("failed to write pin: {}", pin_path.display()))?;
    Ok(pin_path)
}

pub fn read_pin(layout: &PrefixLayout, name: &str) -> Result<Option<String>> {
    let pin_path = layout.pin_path(name);
    if !pin_path.exists() {
        return Ok(None);
    }

    let value = fs::read_to_string(&pin_path)
        .with_context(|| format!("failed to read pin: {}", pin_path.display()))?;
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        return Ok(None);
    }
    Ok(Some(trimmed))
}

pub fn read_all_pins(layout: &PrefixLayout) -> Result<BTreeMap<String, String>> {
    let dir = layout.pins_dir();
    if !dir.exists() {
        return Ok(BTreeMap::new());
    }

    let mut pins = BTreeMap::new();
    for entry in fs::read_dir(&dir)
        .with_context(|| format!("failed to read pin state directory: {}", dir.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }

        let path = entry.path();
        if path.extension().and_then(|v| v.to_str()) != Some("pin") {
            continue;
        }

        let Some(stem) = path.file_stem().and_then(|v| v.to_str()) else {
            continue;
        };
        let value = fs::read_to_string(&path)
            .with_context(|| format!("failed to read pin: {}", path.display()))?;
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        pins.insert(stem.to_string(), trimmed.to_string());
    }

    Ok(pins)
}

pub fn remove_pin(layout: &PrefixLayout, name: &str) -> Result<bool> {
    let pin_path = layout.pin_path(name);
    if !pin_path.exists() {
        return Ok(false);
    }

    fs::remove_file(&pin_path)
        .with_context(|| format!("failed to remove pin: {}", pin_path.display()))?;
    Ok(true)
}
