use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::{
    current_unix_timestamp, RegistrySourceSnapshotState, RegistrySourceWithSnapshotStatus,
};

#[derive(Debug, Serialize, Deserialize)]
struct SourceSnapshotFile {
    version: u32,
    source: String,
    snapshot_id: String,
    updated_at_unix: u64,
    manifest_count: u64,
    status: String,
}

pub(crate) fn write_snapshot_file(
    cache_root: &Path,
    source_name: &str,
    snapshot_id: &str,
    manifest_count: u64,
) -> Result<()> {
    let snapshot_path = cache_root.join("snapshot.json");
    let snapshot = SourceSnapshotFile {
        version: 1,
        source: source_name.to_string(),
        snapshot_id: snapshot_id.to_string(),
        updated_at_unix: current_unix_timestamp(),
        manifest_count,
        status: "ready".to_string(),
    };
    let content = serde_json::to_string_pretty(&snapshot).with_context(|| {
        format!(
            "source-sync-failed: source '{}' failed serializing snapshot {}",
            source_name,
            snapshot_path.display()
        )
    })?;
    fs::write(&snapshot_path, content).with_context(|| {
        format!(
            "source-sync-failed: source '{}' failed writing snapshot {}",
            source_name,
            snapshot_path.display()
        )
    })
}

pub(crate) fn read_snapshot_id(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    let parsed = serde_json::from_str::<SourceSnapshotFile>(&content).ok()?;
    Some(parsed.snapshot_id)
}

pub(crate) fn read_snapshot_state(cache_root: &Path) -> RegistrySourceSnapshotState {
    let snapshot_path = cache_root.join("snapshot.json");
    let content = match fs::read_to_string(&snapshot_path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return RegistrySourceSnapshotState::None;
        }
        Err(_) => {
            return RegistrySourceSnapshotState::Error {
                status: RegistrySourceWithSnapshotStatus::Unreadable,
                reason_code: "snapshot-unreadable".to_string(),
            };
        }
    };

    let snapshot = match serde_json::from_str::<SourceSnapshotFile>(&content) {
        Ok(snapshot) => snapshot,
        Err(_) => {
            return RegistrySourceSnapshotState::Error {
                status: RegistrySourceWithSnapshotStatus::Unreadable,
                reason_code: "snapshot-unreadable".to_string(),
            };
        }
    };

    if snapshot.status == "ready" {
        return RegistrySourceSnapshotState::Ready {
            snapshot_id: snapshot.snapshot_id,
        };
    }

    RegistrySourceSnapshotState::Error {
        status: RegistrySourceWithSnapshotStatus::Invalid,
        reason_code: "snapshot-invalid".to_string(),
    }
}

pub(crate) fn source_has_ready_snapshot(cache_root: &Path) -> Result<bool> {
    let snapshot_path = cache_root.join("snapshot.json");
    let content = match fs::read_to_string(&snapshot_path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "failed reading source snapshot metadata: {}",
                    snapshot_path.display()
                )
            });
        }
    };

    let snapshot: SourceSnapshotFile = serde_json::from_str(&content).with_context(|| {
        format!(
            "failed parsing source snapshot metadata: {}",
            snapshot_path.display()
        )
    })?;
    Ok(snapshot.status == "ready")
}
