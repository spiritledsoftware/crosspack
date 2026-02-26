use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(unix)]
use std::process::Stdio;

use anyhow::{anyhow, Context, Result};
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use crosspack_core::{
    ArchiveType, Artifact, ArtifactCompletionShell, ArtifactGuiApp, PackageManifest,
};
use crosspack_installer::{
    append_transaction_journal_entry, bin_path, clear_active_transaction, current_unix_timestamp,
    default_user_prefix, expose_binary, expose_completion, expose_gui_app, exposed_completion_path,
    gui_asset_path, install_from_artifact, projected_exposed_completion_path, projected_gui_assets,
    read_active_transaction, read_all_gui_exposure_states, read_all_pins, read_gui_exposure_state,
    read_gui_native_state, read_install_receipts, read_transaction_metadata,
    register_native_gui_app_best_effort, remove_exposed_binary, remove_exposed_completion,
    remove_exposed_gui_asset, remove_file_if_exists, remove_native_gui_registration_best_effort,
    run_package_native_uninstall_actions, set_active_transaction,
    uninstall_blocked_by_roots_with_dependency_overrides_and_ignored_roots, uninstall_package,
    uninstall_package_with_dependency_overrides_and_ignored_roots, update_transaction_status,
    write_gui_exposure_state, write_gui_native_state, write_install_receipt, write_pin,
    write_transaction_metadata, ArtifactInstallOptions, GuiExposureAsset,
    GuiNativeRegistrationRecord, InstallInteractionPolicy, InstallMode, InstallReason,
    InstallReceipt, PrefixLayout, TransactionJournalEntry, TransactionMetadata, UninstallResult,
    UninstallStatus,
};
use crosspack_registry::{
    ConfiguredRegistryIndex, RegistryIndex, RegistrySourceKind, RegistrySourceRecord,
    RegistrySourceSnapshotState, RegistrySourceStore, RegistrySourceWithSnapshotState,
    SourceUpdateResult, SourceUpdateStatus,
};
use crosspack_resolver::{resolve_dependency_graph, RootRequirement};
use crosspack_security::verify_sha256_file;
use semver::{Version, VersionReq};
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
    "no configured registry snapshots available; bootstrap trusted source `core` with `crosspack registry add core https://github.com/spiritledsoftware/crosspack-registry.git --kind git --priority 100 --fingerprint <64-hex>` then run `crosspack update` (see https://github.com/spiritledsoftware/crosspack/blob/main/docs/registry-bootstrap-runbook.md and https://github.com/spiritledsoftware/crosspack/blob/main/docs/trust/core-registry-fingerprint.txt)";
const SNAPSHOT_ID_MISMATCH_ERROR_CODE: &str = "snapshot-id-mismatch";
const SEARCH_METADATA_GUIDANCE: &str =
    "search metadata unavailable; run `crosspack update` to refresh local snapshots and `crosspack registry list` to inspect source status";

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
        #[command(flatten)]
        escalation: EscalationArgs,
    },
    List,
    Pin {
        spec: String,
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

include!("render.rs");

include!("dispatch.rs");

include!("metadata.rs");

include!("command_flows.rs");

include!("core_flows.rs");

include!("tests.rs");
