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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostPlatform {
    Linux,
    Macos,
    Windows,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrationAdapterKind {
    None,
    DockerCli,
    PathPluginBin,
    SystemdUser,
    LaunchdUser,
    WindowsServiceUser,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrationActivationScope {
    None,
    User,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrationDesiredState {
    Projected,
    Enabled,
    Running,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrationAppliedState {
    Projected,
    Installed,
    Enabled,
    Running,
    Stopped,
    Failed,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrationReasonCode {
    Ok,
    NotEnabled,
    UnsupportedHost,
    AdapterToolMissing,
    HostPathConflict,
    EscalationRequired,
    NativeCommandFailed,
    InvalidServiceMetadata,
    StateMissing,
    StateAmbiguous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationActivationRecord {
    pub package_state_key: String,
    pub package: String,
    pub integration_key: String,
    pub kind: String,
    pub adapter: IntegrationAdapterKind,
    pub scope: IntegrationActivationScope,
    pub desired_state: IntegrationDesiredState,
    pub applied_state: IntegrationAppliedState,
    pub host_path: Option<String>,
    pub reason_code: IntegrationReasonCode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationActivationPlan {
    pub package_state_key: String,
    pub package: String,
    pub integration_key: String,
    pub kind: String,
    pub adapter: IntegrationAdapterKind,
    pub scope: IntegrationActivationScope,
    pub desired_state: IntegrationDesiredState,
    pub host_path: String,
    pub source_path: String,
}

impl IntegrationActivationPlan {
    pub fn with_package_state_key(mut self, package_state_key: impl Into<String>) -> Self {
        self.package_state_key = package_state_key.into();
        self
    }
}

impl IntegrationAdapterKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::DockerCli => "docker-cli",
            Self::PathPluginBin => "path-plugin-bin",
            Self::SystemdUser => "systemd-user",
            Self::LaunchdUser => "launchd-user",
            Self::WindowsServiceUser => "windows-service-user",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "none" => Ok(Self::None),
            "docker-cli" => Ok(Self::DockerCli),
            "path-plugin-bin" => Ok(Self::PathPluginBin),
            "systemd-user" => Ok(Self::SystemdUser),
            "launchd-user" => Ok(Self::LaunchdUser),
            "windows-service-user" => Ok(Self::WindowsServiceUser),
            _ => Err(anyhow!("invalid integration adapter kind: {value}")),
        }
    }
}

impl IntegrationActivationScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::User => "user",
            Self::System => "system",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "none" => Ok(Self::None),
            "user" => Ok(Self::User),
            "system" => Ok(Self::System),
            _ => Err(anyhow!("invalid integration activation scope: {value}")),
        }
    }
}

impl IntegrationDesiredState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Projected => "projected",
            Self::Enabled => "enabled",
            Self::Running => "running",
            Self::Disabled => "disabled",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "projected" => Ok(Self::Projected),
            "enabled" => Ok(Self::Enabled),
            "running" => Ok(Self::Running),
            "disabled" => Ok(Self::Disabled),
            _ => Err(anyhow!("invalid integration desired state: {value}")),
        }
    }
}

impl IntegrationAppliedState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Projected => "projected",
            Self::Installed => "installed",
            Self::Enabled => "enabled",
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
            Self::Unsupported => "unsupported",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "projected" => Ok(Self::Projected),
            "installed" => Ok(Self::Installed),
            "enabled" => Ok(Self::Enabled),
            "running" => Ok(Self::Running),
            "stopped" => Ok(Self::Stopped),
            "failed" => Ok(Self::Failed),
            "unsupported" => Ok(Self::Unsupported),
            _ => Err(anyhow!("invalid integration applied state: {value}")),
        }
    }
}

impl IntegrationReasonCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::NotEnabled => "not-enabled",
            Self::UnsupportedHost => "unsupported-host",
            Self::AdapterToolMissing => "adapter-tool-missing",
            Self::HostPathConflict => "host-path-conflict",
            Self::EscalationRequired => "escalation-required",
            Self::NativeCommandFailed => "native-command-failed",
            Self::InvalidServiceMetadata => "invalid-service-metadata",
            Self::StateMissing => "state-missing",
            Self::StateAmbiguous => "state-ambiguous",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "ok" => Ok(Self::Ok),
            "not-enabled" => Ok(Self::NotEnabled),
            "unsupported-host" => Ok(Self::UnsupportedHost),
            "adapter-tool-missing" => Ok(Self::AdapterToolMissing),
            "host-path-conflict" => Ok(Self::HostPathConflict),
            "escalation-required" => Ok(Self::EscalationRequired),
            "native-command-failed" => Ok(Self::NativeCommandFailed),
            "invalid-service-metadata" => Ok(Self::InvalidServiceMetadata),
            "state-missing" => Ok(Self::StateMissing),
            "state-ambiguous" => Ok(Self::StateAmbiguous),
            _ => Err(anyhow!("invalid integration reason code: {value}")),
        }
    }
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
