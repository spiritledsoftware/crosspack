use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RegistrySourceKind {
    Git,
    Filesystem,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistrySourceRecord {
    pub name: String,
    pub kind: RegistrySourceKind,
    pub location: String,
    #[serde(alias = "fingerprint")]
    pub fingerprint_sha256: String,
    #[serde(default = "crate::source_types::source_enabled_default")]
    pub enabled: bool,
    pub priority: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceUpdateStatus {
    Updated,
    UpToDate,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceUpdateResult {
    pub name: String,
    pub status: SourceUpdateStatus,
    pub snapshot_id: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrySourceWithSnapshotState {
    pub source: RegistrySourceRecord,
    pub snapshot: RegistrySourceSnapshotState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistrySourceSnapshotState {
    None,
    Ready {
        snapshot_id: String,
    },
    Error {
        status: RegistrySourceWithSnapshotStatus,
        reason_code: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistrySourceWithSnapshotStatus {
    Unreadable,
    Invalid,
}

pub(crate) fn source_enabled_default() -> bool {
    true
}
