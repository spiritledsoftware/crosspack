use anyhow::{anyhow, Result};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReceipt {
    pub name: String,
    pub version: String,
    pub dependencies: Vec<String>,
    pub target: Option<String>,
    pub artifact_url: Option<String>,
    pub artifact_sha256: Option<String>,
    pub cache_path: Option<String>,
    pub exposed_bins: Vec<String>,
    pub exposed_completions: Vec<String>,
    pub snapshot_id: Option<String>,
    pub install_mode: InstallMode,
    pub install_reason: InstallReason,
    pub install_status: String,
    pub installed_at_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiExposureAsset {
    pub key: String,
    pub rel_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiNativeRegistrationRecord {
    pub key: String,
    pub kind: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationProjection {
    pub kind: String,
    pub key: String,
    pub rel_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeUninstallAction {
    pub key: String,
    pub kind: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSidecarState {
    pub uninstall_actions: Vec<NativeUninstallAction>,
}

impl From<GuiNativeRegistrationRecord> for NativeUninstallAction {
    fn from(value: GuiNativeRegistrationRecord) -> Self {
        Self {
            key: value.key,
            kind: value.kind,
            path: value.path,
        }
    }
}

impl From<NativeUninstallAction> for GuiNativeRegistrationRecord {
    fn from(value: NativeUninstallAction) -> Self {
        Self {
            key: value.key,
            kind: value.kind,
            path: value.path,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionMetadata {
    pub version: u32,
    pub txid: String,
    pub operation: String,
    pub status: TransactionStatus,
    pub started_at_unix: u64,
    pub snapshot_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionStatus {
    Planning,
    Applying,
    Completed,
    Committed,
    RollingBack,
    RolledBack,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransactionRecoveryAction {
    Clean,
    CleanupPlanning { txid: String },
    Rollback { txid: String },
    FinalizeCommitted { txid: String },
    ResumeRollback { txid: String },
    ClearRolledBack { txid: String },
    BlockedFailed { txid: String },
    RepairRequired(TransactionRepairReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransactionRepairReason {
    ActiveMarkerUnreadable,
    ActiveMarkerInvalid { path: String },
    ActiveMarkerWithoutMetadata { txid: String },
    MetadataUnreadable { txid: String },
    MetadataTxidMismatch { expected: String, actual: String },
    JournalUnreadable { txid: String },
    ApplyingWithoutActiveMarker { txid: String },
    RollbackEvidenceMissing { txid: String },
}

impl TransactionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Planning => "planning",
            Self::Applying => "applying",
            Self::Completed => "completed",
            Self::Committed => "committed",
            Self::RollingBack => "rolling_back",
            Self::RolledBack => "rolled_back",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "planning" => Ok(Self::Planning),
            "applying" => Ok(Self::Applying),
            "completed" => Ok(Self::Completed),
            "committed" => Ok(Self::Committed),
            "rolling_back" => Ok(Self::RollingBack),
            "rolled_back" => Ok(Self::RolledBack),
            "failed" => Ok(Self::Failed),
            _ => Err(anyhow!("invalid transaction status: {value}")),
        }
    }
}

impl fmt::Display for TransactionStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl PartialEq<&str> for TransactionStatus {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionJournalEntry {
    pub seq: u64,
    pub step: String,
    pub state: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallReason {
    Root,
    Dependency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMode {
    Managed,
    Native,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallInteractionPolicy {
    pub allow_prompt_escalation: bool,
    pub allow_non_prompt_escalation: bool,
}

impl Default for InstallInteractionPolicy {
    fn default() -> Self {
        Self {
            allow_prompt_escalation: true,
            allow_non_prompt_escalation: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtifactInstallOptions<'a> {
    pub strip_components: u32,
    pub artifact_root: Option<&'a str>,
    pub install_mode: InstallMode,
    pub interaction_policy: InstallInteractionPolicy,
}

impl InstallMode {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Managed => "managed",
            Self::Native => "native",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value {
            "managed" => Ok(Self::Managed),
            "native" => Ok(Self::Native),
            _ => Err(anyhow!("invalid install_mode: {value}")),
        }
    }

    pub(crate) fn parse_receipt_token(value: &str) -> Self {
        Self::parse(value).unwrap_or(Self::Managed)
    }
}

impl InstallReason {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Dependency => "dependency",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self> {
        match value {
            "root" => Ok(Self::Root),
            "dependency" => Ok(Self::Dependency),
            _ => Err(anyhow!("invalid install_reason: {value}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UninstallStatus {
    NotInstalled,
    Uninstalled,
    RepairedStaleState,
    BlockedByDependents,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallResult {
    pub name: String,
    pub version: Option<String>,
    pub status: UninstallStatus,
    pub pruned_dependencies: Vec<String>,
    pub blocked_by_roots: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeServiceAction {
    Status,
    Start,
    Stop,
    Restart,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeServiceOutcome {
    pub adapter: String,
    pub applied: bool,
    pub reason_code: String,
}
