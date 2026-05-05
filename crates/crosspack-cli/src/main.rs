use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{IsTerminal, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
#[cfg(unix)]
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use crosspack_core::{
    ArchiveType, Artifact, ArtifactCompletionShell, ArtifactGuiApp, PackageIntegration,
    PackageManifest, ServiceDeclaration,
};
#[cfg(test)]
use crosspack_installer::read_declared_services_state;
#[cfg(test)]
use crosspack_installer::read_installed_package_state;
#[cfg(test)]
use crosspack_installer::MemoryActivationFs;
use crosspack_installer::{
    append_transaction_journal_entry, apply_integration_plan_with_fs, apply_service_plan, bin_path,
    clear_active_transaction, current_unix_timestamp, default_user_prefix,
    disable_integration_plan_with_fs, disable_service_plan, expose_binary, expose_completion,
    expose_gui_app, expose_integrations, exposed_completion_path, gui_asset_path,
    install_from_artifact_to_dir, install_from_source_archive_to_dir,
    plan_docker_cli_plugin_activation, plan_path_plugin_activation, plan_service_activation,
    projected_exposed_completion_path, projected_gui_assets, projected_integrations,
    read_active_transaction_marker, read_all_declared_services_states,
    read_all_gui_exposure_states, read_all_installed_package_states, read_all_integration_states,
    read_all_pins, read_gui_exposure_state, read_gui_native_state, read_identity_integration_state,
    read_install_receipts, read_integration_activation_state, read_integration_state,
    read_transaction_metadata, register_native_gui_app_best_effort, remove_exposed_binary,
    remove_exposed_completion, remove_exposed_gui_asset, remove_exposed_integration,
    remove_file_if_exists, remove_native_gui_registration_best_effort,
    replay_activation_rollback_entry_with_fs, resolve_installed_package_selector,
    run_package_native_uninstall_actions, run_service_action_plan,
    uninstall_blocked_by_roots_with_dependency_overrides_and_ignored_roots, uninstall_package,
    uninstall_package_identity, uninstall_package_with_dependency_overrides_and_ignored_roots,
    update_transaction_status, write_declared_services_state, write_gui_exposure_state,
    write_gui_native_state, write_identity_declared_services_state,
    write_identity_gui_exposure_state, write_identity_gui_native_state,
    write_identity_install_receipt, write_identity_integration_state, write_identity_pin,
    write_install_receipt, write_installed_package_state, write_integration_activation_state,
    write_integration_state, write_pin, ActivationAdapterOutcome, ActivationFilesystem,
    ActivationFsEntry, ActivationOwner, ActivationRollbackEntry, ActivationRollbackOperation,
    ActiveTransactionMarker, ArtifactInstallOptions, GuiExposureAsset, GuiNativeRegistrationRecord,
    HostActivationContext, HostPlatform, InstallInteractionPolicy, InstallMode, InstallReason,
    InstallReceipt, InstalledPackageIdentity, InstalledPackageSelector, InstalledPackageState,
    IntegrationActivationPlan, IntegrationActivationRecord, IntegrationActivationScope,
    IntegrationAdapterKind, IntegrationAppliedState, IntegrationDesiredState,
    IntegrationProjection, IntegrationReasonCode, NativeServiceAction, PrefixLayout,
    RealActivationFs, ServiceActivationMetadata, SystemActivationCommandExecutor,
    TransactionCoordinator, TransactionJournalEntry, TransactionMetadata,
    TransactionRecoveryAction, TransactionRepairReason, TransactionStatus, UninstallResult,
    UninstallStatus,
};
#[cfg(test)]
use crosspack_installer::{
    read_active_transaction, set_active_transaction, write_transaction_metadata,
};
#[cfg(test)]
use crosspack_installer::{run_native_service_action, NativeServiceOutcome};
use crosspack_registry::{
    ConfiguredRegistryIndex, PackageSkipDiagnostic, RegistryIndex, RegistrySourceKind,
    RegistrySourceRecord, RegistrySourceSnapshotState, RegistrySourceStore,
    RegistrySourceWithSnapshotState, SourceUpdateResult, SourceUpdateStatus,
};
use crosspack_resolver::{
    plan_from_resolved_graph_with_installed, resolve_dependency_graph_with_installed_manifests,
    InstallPlan, InstalledPackageSummary, PlanOperation, PlannedPackage as InstallPlanPackage,
    PlannedRemoval as InstallPlanRemoval, PlannedReplacement as InstallPlanReplacement,
    PlannedTransition as InstallPlanTransition, ResolvedGraph, RootRequirement,
};
use crosspack_security::verify_sha256_file;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Parser, Debug)]
#[command(name = "crosspack")]
#[command(version)]
#[command(about = "Native cross-platform package manager", long_about = None)]
struct Cli {
    #[arg(long)]
    registry_root: Option<PathBuf>,
    #[command(subcommand)]
    command: Commands,
}

const NO_ROOT_PACKAGES_TO_UPGRADE: &str = "No root packages installed";
const METADATA_CONFIG_GUIDANCE: &str =
    "no configured registry snapshots available; bootstrap trusted source `core` with `crosspack registry add core https://github.com/spiritledsoftware/crosspack-registry.git --kind git --priority 100 --fingerprint <64-hex>` then run `crosspack update` (see https://github.com/spiritledsoftware/crosspack/blob/main/docs/registry-bootstrap-runbook.md)";
const SNAPSHOT_ID_MISMATCH_ERROR_CODE: &str = "snapshot-id-mismatch";
const SEARCH_METADATA_GUIDANCE: &str =
    "search metadata unavailable; run `crosspack update` to refresh local snapshots and `crosspack registry list` to inspect source status";

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum OutputStyle {
    Plain,
    Rich,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)]
enum ProgressMode {
    Auto,
    // Reserved for the planned public `--progress=always|never|auto` flag.
    Always,
    Never,
}

#[derive(Args, Copy, Clone, Debug, Default, Eq, PartialEq)]
struct EscalationArgs {
    #[arg(long)]
    non_interactive: bool,
    #[arg(long, conflicts_with = "no_escalation")]
    allow_escalation: bool,
    #[arg(long, conflicts_with = "allow_escalation")]
    no_escalation: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct EscalationPolicy {
    allow_prompt_escalation: bool,
    allow_non_prompt_escalation: bool,
}

fn resolve_escalation_policy(args: EscalationArgs) -> EscalationPolicy {
    if args.no_escalation {
        return EscalationPolicy {
            allow_prompt_escalation: false,
            allow_non_prompt_escalation: false,
        };
    }

    if args.non_interactive {
        return EscalationPolicy {
            allow_prompt_escalation: false,
            allow_non_prompt_escalation: args.allow_escalation,
        };
    }

    EscalationPolicy {
        allow_prompt_escalation: true,
        allow_non_prompt_escalation: true,
    }
}

fn install_interaction_policy(escalation_policy: EscalationPolicy) -> InstallInteractionPolicy {
    InstallInteractionPolicy {
        allow_prompt_escalation: escalation_policy.allow_prompt_escalation,
        allow_non_prompt_escalation: escalation_policy.allow_non_prompt_escalation,
    }
}

fn install_mode_for_archive_type(archive_type: ArchiveType) -> InstallMode {
    match archive_type {
        ArchiveType::Zip
        | ArchiveType::TarGz
        | ArchiveType::TarZst
        | ArchiveType::Bin
        | ArchiveType::Dmg
        | ArchiveType::AppImage => InstallMode::Managed,
        ArchiveType::Msi
        | ArchiveType::Exe
        | ArchiveType::Pkg
        | ArchiveType::Msix
        | ArchiveType::Appx => InstallMode::Native,
    }
}

fn build_artifact_install_options<'a>(
    resolved: &'a ResolvedInstall,
    interaction_policy: InstallInteractionPolicy,
) -> ArtifactInstallOptions<'a> {
    ArtifactInstallOptions {
        strip_components: resolved.artifact.strip_components.unwrap_or(0),
        artifact_root: resolved.artifact.artifact_root.as_deref(),
        install_mode: install_mode_for_archive_type(resolved.archive_type),
        interaction_policy,
    }
}

fn resolve_output_style(stdout_is_tty: bool, stderr_is_tty: bool) -> OutputStyle {
    if internal_ui_snapshot_enabled() {
        return OutputStyle::Rich;
    }

    if stdout_is_tty && stderr_is_tty {
        OutputStyle::Rich
    } else {
        OutputStyle::Plain
    }
}

fn internal_ui_snapshot_enabled() -> bool {
    std::env::var("CROSSPACK_INTERNAL_UI_SNAPSHOT").is_ok_and(|value| value == "1")
}

fn internal_no_color_enabled() -> bool {
    std::env::var("CROSSPACK_INTERNAL_NO_COLOR").is_ok_and(|value| value == "1")
}

fn internal_terminal_width() -> Option<usize> {
    if !internal_ui_snapshot_enabled() {
        return None;
    }

    std::env::var("CROSSPACK_INTERNAL_TERM_WIDTH")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|width| *width > 0)
}

fn resolve_progress_enabled(style: OutputStyle, stderr_is_tty: bool) -> bool {
    style == OutputStyle::Rich && stderr_is_tty
}

fn resolve_progress_mode(mode: ProgressMode, style: OutputStyle, stderr_is_tty: bool) -> bool {
    match mode {
        ProgressMode::Auto => resolve_progress_enabled(style, stderr_is_tty),
        ProgressMode::Always => style == OutputStyle::Rich,
        ProgressMode::Never => false,
    }
}

fn current_progress_enabled(style: OutputStyle) -> bool {
    resolve_progress_mode(ProgressMode::Auto, style, std::io::stderr().is_terminal())
}

fn render_status_line(style: OutputStyle, status: &str, message: &str) -> String {
    match style {
        OutputStyle::Plain => message.to_string(),
        OutputStyle::Rich => {
            let marker = match status {
                "ok" => "✓",
                "warn" => "!",
                "error" => "×",
                "step" => "•",
                _ => "•",
            };
            format!("{marker} {message}")
        }
    }
}

fn render_status_lines(
    style: OutputStyle,
    entries: impl IntoIterator<Item = (&'static str, String)>,
) -> Vec<String> {
    entries
        .into_iter()
        .map(|(status, message)| render_status_line(style, status, &message))
        .collect()
}

fn render_update_line(style: OutputStyle, line: &str) -> String {
    if line.contains(": failed") {
        return render_status_line(style, "error", line);
    }
    if line.contains(": updated") {
        return render_status_line(style, "ok", line);
    }
    if line.contains(": up-to-date") {
        return render_status_line(style, "step", line);
    }
    render_status_line(style, "step", line)
}

fn format_update_output_lines(report: &UpdateReport, style: OutputStyle) -> Vec<String> {
    report
        .lines
        .iter()
        .map(|line| render_update_line(style, line))
        .collect()
}

fn current_output_style() -> OutputStyle {
    resolve_output_style(
        std::io::stdout().is_terminal(),
        std::io::stderr().is_terminal(),
    )
}

#[derive(Subcommand, Debug)]
enum Commands {
    Search {
        query: String,
    },
    Info {
        name: String,
    },
    Install {
        spec: String,
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        explain: bool,
        #[arg(long)]
        build_from_source: bool,
        #[arg(long)]
        force_redownload: bool,
        #[arg(long = "provider", value_name = "capability=package")]
        provider: Vec<String>,
        #[command(flatten)]
        escalation: EscalationArgs,
    },
    Upgrade {
        spec: Option<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        explain: bool,
        #[arg(long)]
        build_from_source: bool,
        #[arg(long = "provider", value_name = "capability=package")]
        provider: Vec<String>,
        #[command(flatten)]
        escalation: EscalationArgs,
    },
    Rollback {
        txid: Option<String>,
        #[command(flatten)]
        escalation: EscalationArgs,
    },
    Repair {
        #[command(flatten)]
        escalation: EscalationArgs,
    },
    Uninstall {
        name: String,
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        source: Option<String>,
        #[command(flatten)]
        escalation: EscalationArgs,
    },
    List {
        #[arg(long)]
        identity: bool,
    },
    Pin {
        spec: String,
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        source: Option<String>,
    },
    Outdated,
    Depends {
        name: String,
    },
    Uses {
        name: String,
    },
    Why {
        name: String,
    },
    Services {
        #[command(subcommand)]
        command: ServicesCommands,
    },
    Integrations {
        #[command(subcommand)]
        command: IntegrationsCommands,
    },
    Cache {
        #[command(subcommand)]
        command: CacheCommands,
    },
    Bundle {
        #[command(subcommand)]
        command: BundleCommands,
    },
    Registry {
        #[command(subcommand)]
        command: RegistryCommands,
    },
    Update {
        #[arg(long = "registry")]
        registry: Vec<String>,
    },
    SelfUpdate {
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        force_redownload: bool,
        #[command(flatten)]
        escalation: EscalationArgs,
    },
    Doctor,
    Version,
    Completions {
        shell: CliCompletionShell,
    },
    InitShell {
        #[arg(long)]
        shell: Option<CliCompletionShell>,
    },
}

#[derive(Subcommand, Debug)]
enum RegistryCommands {
    Add {
        name: String,
        location: String,
        #[arg(long)]
        kind: CliRegistryKind,
        #[arg(long)]
        priority: u32,
        #[arg(long)]
        fingerprint: String,
    },
    List,
    Remove {
        name: String,
        #[arg(long)]
        purge_cache: bool,
    },
}

#[derive(Subcommand, Debug)]
enum CacheCommands {
    List,
    Prune,
    Gc,
}

#[derive(Subcommand, Debug)]
enum ServicesCommands {
    List,
    Status { package: String, service: String },
    Start { package: String, service: String },
    Stop { package: String, service: String },
    Restart { package: String, service: String },
}

#[derive(Subcommand, Debug)]
enum IntegrationsCommands {
    List,
    Status {
        package: String,
        integration: String,
    },
    Enable {
        package: String,
        integration: String,
    },
    Disable {
        package: String,
        integration: String,
    },
}

#[derive(Subcommand, Debug)]
enum BundleCommands {
    Export {
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Apply {
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        explain: bool,
        #[arg(long)]
        build_from_source: bool,
        #[arg(long)]
        force_redownload: bool,
        #[arg(long = "provider", value_name = "capability=package")]
        provider: Vec<String>,
    },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum CliRegistryKind {
    Git,
    Filesystem,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum CliCompletionShell {
    Bash,
    Zsh,
    Fish,
    Powershell,
}

impl From<CliRegistryKind> for RegistrySourceKind {
    fn from(value: CliRegistryKind) -> Self {
        match value {
            CliRegistryKind::Git => RegistrySourceKind::Git,
            CliRegistryKind::Filesystem => RegistrySourceKind::Filesystem,
        }
    }
}

impl From<CliCompletionShell> for Shell {
    fn from(value: CliCompletionShell) -> Self {
        match value {
            CliCompletionShell::Bash => Shell::Bash,
            CliCompletionShell::Zsh => Shell::Zsh,
            CliCompletionShell::Fish => Shell::Fish,
            CliCompletionShell::Powershell => Shell::PowerShell,
        }
    }
}

impl CliCompletionShell {
    fn completion_filename(self) -> &'static str {
        match self {
            Self::Bash => "crosspack.bash",
            Self::Zsh => "crosspack.zsh",
            Self::Fish => "crosspack.fish",
            Self::Powershell => "crosspack.ps1",
        }
    }

    fn package_completion_shell(self) -> ArtifactCompletionShell {
        match self {
            Self::Bash => ArtifactCompletionShell::Bash,
            Self::Zsh => ArtifactCompletionShell::Zsh,
            Self::Fish => ArtifactCompletionShell::Fish,
            Self::Powershell => ArtifactCompletionShell::Powershell,
        }
    }
}

fn main() -> Result<()> {
    run_cli(Cli::parse())
}

include!("completion.rs");

include!("dispatch.rs");

include!("metadata.rs");

include!("render.rs");

include!("lifecycle_service.rs");

include!("lifecycle_render.rs");

include!("command_flows.rs");

include!("core_flows.rs");

include!("bundle_flows.rs");

include!("tests.rs");
