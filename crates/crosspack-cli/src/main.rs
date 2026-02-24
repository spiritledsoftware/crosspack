use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(unix)]
use std::process::Stdio;

use anyhow::{anyhow, Context, Result};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use crosspack_core::{ArchiveType, Artifact, ArtifactCompletionShell, PackageManifest};
use crosspack_installer::{
    append_transaction_journal_entry, bin_path, clear_active_transaction, current_unix_timestamp,
    default_user_prefix, expose_binary, expose_completion, exposed_completion_path,
    install_from_artifact, projected_exposed_completion_path, read_active_transaction,
    read_all_pins, read_install_receipts, read_transaction_metadata, remove_exposed_binary,
    remove_exposed_completion, remove_file_if_exists, set_active_transaction,
    uninstall_blocked_by_roots_with_dependency_overrides_and_ignored_roots, uninstall_package,
    uninstall_package_with_dependency_overrides_and_ignored_roots, update_transaction_status,
    write_install_receipt, write_pin, write_transaction_metadata, InstallReason, InstallReceipt,
    PrefixLayout, TransactionJournalEntry, TransactionMetadata, UninstallResult, UninstallStatus,
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
        force_redownload: bool,
        #[arg(long = "provider", value_name = "capability=package")]
        provider: Vec<String>,
    },
    Upgrade {
        spec: Option<String>,
        #[arg(long = "provider", value_name = "capability=package")]
        provider: Vec<String>,
    },
    Rollback {
        txid: Option<String>,
    },
    Repair,
    Uninstall {
        name: String,
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
    Doctor,
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
    let cli = Cli::parse();

    match cli.command {
        Commands::Search { query } => {
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            let backend = select_metadata_backend(cli.registry_root.as_deref(), &layout)?;
            for name in backend.search_names(&query)? {
                println!("{name}");
            }
        }
        Commands::Info { name } => {
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            let backend = select_metadata_backend(cli.registry_root.as_deref(), &layout)?;
            let versions = backend.package_versions(&name)?;

            if versions.is_empty() {
                println!("No package found: {name}");
            } else {
                for line in format_info_lines(&name, &versions) {
                    println!("{line}");
                }
            }
        }
        Commands::Install {
            spec,
            target,
            force_redownload,
            provider,
        } => {
            let (name, requirement) = parse_spec(&spec)?;
            let provider_overrides = parse_provider_overrides(&provider)?;

            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            layout.ensure_base_dirs()?;
            ensure_no_active_transaction_for(&layout, "install")?;
            let backend = select_metadata_backend(cli.registry_root.as_deref(), &layout)?;

            let snapshot_id = match cli.registry_root.as_deref() {
                Some(_) => None,
                None => Some(resolve_transaction_snapshot_id(&layout, "install")?),
            };
            execute_with_transaction(&layout, "install", snapshot_id.as_deref(), |tx| {
                let mut journal_seq = 1_u64;
                let roots = vec![RootInstallRequest { name, requirement }];
                let root_names = roots
                    .iter()
                    .map(|root| root.name.clone())
                    .collect::<Vec<_>>();
                let resolved = resolve_install_graph(
                    &layout,
                    &backend,
                    &roots,
                    target.as_deref(),
                    &provider_overrides,
                )?;

                append_transaction_journal_entry(
                    &layout,
                    &tx.txid,
                    &TransactionJournalEntry {
                        seq: journal_seq,
                        step: "resolve_plan".to_string(),
                        state: "done".to_string(),
                        path: None,
                    },
                )?;
                journal_seq += 1;

                let planned_dependency_overrides = build_planned_dependency_overrides(&resolved);

                for package in &resolved {
                    let snapshot_path =
                        capture_package_state_snapshot(&layout, &tx.txid, &package.manifest.name)?;
                    append_transaction_journal_entry(
                        &layout,
                        &tx.txid,
                        &TransactionJournalEntry {
                            seq: journal_seq,
                            step: format!("backup_package_state:{}", package.manifest.name),
                            state: "done".to_string(),
                            path: Some(snapshot_path.display().to_string()),
                        },
                    )?;
                    journal_seq += 1;

                    append_transaction_journal_entry(
                        &layout,
                        &tx.txid,
                        &TransactionJournalEntry {
                            seq: journal_seq,
                            step: format!("install_package:{}", package.manifest.name),
                            state: "done".to_string(),
                            path: Some(package.manifest.name.clone()),
                        },
                    )?;
                    journal_seq += 1;

                    let dependencies = build_dependency_receipts(package, &resolved);
                    let outcome = install_resolved(
                        &layout,
                        package,
                        &dependencies,
                        &root_names,
                        &planned_dependency_overrides,
                        snapshot_id.as_deref(),
                        force_redownload,
                    )?;
                    print_install_outcome(&outcome);
                }

                append_transaction_journal_entry(
                    &layout,
                    &tx.txid,
                    &TransactionJournalEntry {
                        seq: journal_seq,
                        step: "apply_complete".to_string(),
                        state: "done".to_string(),
                        path: None,
                    },
                )?;

                Ok(())
            })?;
            if let Err(err) = sync_completion_assets_best_effort(&layout, "install") {
                eprintln!("{err}");
            }
        }
        Commands::Upgrade { spec, provider } => {
            let provider_overrides = parse_provider_overrides(&provider)?;
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            run_upgrade_command(
                &layout,
                cli.registry_root.as_deref(),
                spec,
                &provider_overrides,
            )?;
        }
        Commands::Rollback { txid } => {
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            run_rollback_command(&layout, txid)?;
        }
        Commands::Repair => {
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            run_repair_command(&layout)?;
        }
        Commands::Uninstall { name } => {
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            run_uninstall_command(&layout, name)?;
        }
        Commands::List => {
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            let receipts = read_install_receipts(&layout)?;
            if receipts.is_empty() {
                println!("No installed packages");
            } else {
                for receipt in receipts {
                    println!("{} {}", receipt.name, receipt.version);
                }
            }
        }
        Commands::Pin { spec } => {
            let (name, requirement) = parse_pin_spec(&spec)?;
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            layout.ensure_base_dirs()?;
            let pin_path = write_pin(&layout, &name, &requirement.to_string())?;
            println!("pinned {name} to {requirement}");
            println!("pin: {}", pin_path.display());
        }
        Commands::Registry { command } => {
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            let source_state_root = registry_state_root(&layout);
            let store = RegistrySourceStore::new(&source_state_root);

            match command {
                RegistryCommands::Add {
                    name,
                    location,
                    kind,
                    priority,
                    fingerprint,
                } => {
                    let source_kind: RegistrySourceKind = kind.into();
                    let kind_label = format_registry_kind(source_kind.clone());
                    let output_lines =
                        format_registry_add_lines(&name, kind_label, priority, &fingerprint);
                    store.add_source(RegistrySourceRecord {
                        name,
                        kind: source_kind,
                        location,
                        fingerprint_sha256: fingerprint,
                        enabled: true,
                        priority,
                    })?;
                    for line in output_lines {
                        println!("{line}");
                    }
                }
                RegistryCommands::List => {
                    let sources = store.list_sources_with_snapshot_state()?;
                    for line in format_registry_list_lines(sources) {
                        println!("{line}");
                    }
                }
                RegistryCommands::Remove { name, purge_cache } => {
                    store.remove_source_with_cache_purge(&name, purge_cache)?;
                    for line in format_registry_remove_lines(&name, purge_cache) {
                        println!("{line}");
                    }
                }
            }
        }
        Commands::Update { registry } => {
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            let source_state_root = registry_state_root(&layout);
            let store = RegistrySourceStore::new(&source_state_root);
            run_update_command(&store, &registry)?;
        }
        Commands::Doctor => {
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            println!("prefix: {}", layout.prefix().display());
            println!("bin: {}", layout.bin_dir().display());
            println!("cache: {}", layout.cache_dir().display());
            println!("{}", doctor_transaction_health_line(&layout)?);
        }
        Commands::Completions { shell } => {
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            let mut stdout = std::io::stdout();
            write_completions_script(shell, &layout, &mut stdout)?;
        }
        Commands::InitShell { shell } => {
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            let resolved_shell =
                resolve_init_shell(shell, std::env::var("SHELL").ok().as_deref(), cfg!(windows));
            print_init_shell_snippet(&layout, resolved_shell);
        }
    }

    Ok(())
}

fn write_completions_script<W: Write>(
    shell: CliCompletionShell,
    layout: &PrefixLayout,
    writer: &mut W,
) -> Result<()> {
    let mut command = Cli::command();
    let generator: Shell = shell.into();
    let mut generated = Vec::new();
    clap_complete::generate(generator, &mut command, "crosspack", &mut generated);
    writer
        .write_all(&generated)
        .with_context(|| "failed writing generated completion script")?;
    writer
        .write_all(b"\n")
        .with_context(|| "failed writing completion script delimiter")?;
    writer
        .write_all(package_completion_loader_snippet(layout, shell).as_bytes())
        .with_context(|| "failed writing package completion loader block")?;
    Ok(())
}

fn crosspack_completion_script_path(layout: &PrefixLayout, shell: CliCompletionShell) -> PathBuf {
    layout.completions_dir().join(shell.completion_filename())
}

fn package_completion_loader_snippet(layout: &PrefixLayout, shell: CliCompletionShell) -> String {
    let package_completion_dir =
        layout.package_completions_shell_dir(shell.package_completion_shell());
    match shell {
        CliCompletionShell::Bash | CliCompletionShell::Zsh => {
            let escaped_dir =
                escape_single_quote_shell(&package_completion_dir.display().to_string());
            format!(
                "# crosspack package completions\nif [ -d '{escaped_dir}' ]; then\n  find '{escaped_dir}' -mindepth 1 -maxdepth 1 -type f -print 2>/dev/null \\\n    | LC_ALL=C sort \\\n    | while IFS= read -r _crosspack_pkg_completion_path; do\n        . \"${{_crosspack_pkg_completion_path}}\"\n      done\nfi\n"
            )
        }
        CliCompletionShell::Fish => {
            let escaped_dir =
                escape_single_quote_shell(&package_completion_dir.display().to_string());
            format!(
                "# crosspack package completions\nif test -d '{escaped_dir}'\n    for _crosspack_pkg_completion_path in (find '{escaped_dir}' -mindepth 1 -maxdepth 1 -type f -print 2>/dev/null | sort)\n        source \"$_crosspack_pkg_completion_path\"\n    end\nend\nset -e _crosspack_pkg_completion_path\n"
            )
        }
        CliCompletionShell::Powershell => {
            let escaped_dir = escape_ps_single_quote(&package_completion_dir.display().to_string());
            format!(
                "# crosspack package completions\n$crosspackPackageCompletionDir = '{escaped_dir}'\nif (Test-Path $crosspackPackageCompletionDir) {{\n  Get-ChildItem -Path $crosspackPackageCompletionDir -File | Sort-Object Name | ForEach-Object {{\n    . $_.FullName\n  }}\n}}\nRemove-Variable crosspackPackageCompletionDir -ErrorAction SilentlyContinue\n"
            )
        }
    }
}

fn refresh_crosspack_completion_assets(layout: &PrefixLayout) -> Result<()> {
    fs::create_dir_all(layout.completions_dir()).with_context(|| {
        format!(
            "failed to create completion directory: {}",
            layout.completions_dir().display()
        )
    })?;

    for shell in [
        CliCompletionShell::Bash,
        CliCompletionShell::Zsh,
        CliCompletionShell::Fish,
        CliCompletionShell::Powershell,
    ] {
        let mut output = Vec::new();
        write_completions_script(shell, layout, &mut output)?;
        let path = crosspack_completion_script_path(layout, shell);
        fs::write(&path, &output)
            .with_context(|| format!("failed writing completion asset: {}", path.display()))?;
    }

    Ok(())
}

fn sync_completion_assets_best_effort(layout: &PrefixLayout, operation: &str) -> Result<()> {
    if let Err(err) = refresh_crosspack_completion_assets(layout) {
        return Err(anyhow!(
            "warning: completion sync skipped (operation={operation} reason={})",
            err
        ));
    }
    Ok(())
}

fn escape_single_quote_shell(value: &str) -> String {
    value.replace('\'', "'\"'\"'")
}

fn detect_shell_from_env(shell_env: Option<&str>) -> Option<CliCompletionShell> {
    let shell_value = shell_env?;
    let shell_token = Path::new(shell_value)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(shell_value)
        .to_ascii_lowercase();
    match shell_token.as_str() {
        "bash" => Some(CliCompletionShell::Bash),
        "zsh" => Some(CliCompletionShell::Zsh),
        "fish" => Some(CliCompletionShell::Fish),
        "powershell" | "pwsh" => Some(CliCompletionShell::Powershell),
        _ => None,
    }
}

fn resolve_init_shell(
    requested_shell: Option<CliCompletionShell>,
    shell_env: Option<&str>,
    is_windows: bool,
) -> CliCompletionShell {
    if let Some(shell) = requested_shell {
        return shell;
    }
    if let Some(shell) = detect_shell_from_env(shell_env) {
        return shell;
    }
    if is_windows {
        CliCompletionShell::Powershell
    } else {
        CliCompletionShell::Bash
    }
}

fn print_init_shell_snippet(layout: &PrefixLayout, shell: CliCompletionShell) {
    let bin = layout.bin_dir();
    let completion_path = crosspack_completion_script_path(layout, shell);
    match shell {
        CliCompletionShell::Bash | CliCompletionShell::Zsh => {
            let escaped_completion =
                escape_single_quote_shell(&completion_path.display().to_string());
            println!("export PATH=\"{}:$PATH\"", bin.display());
            println!("if [ -f '{escaped_completion}' ]; then");
            println!("  . '{escaped_completion}'");
            println!("fi");
        }
        CliCompletionShell::Fish => {
            let escaped_bin = escape_single_quote_shell(&bin.display().to_string());
            let escaped_completion =
                escape_single_quote_shell(&completion_path.display().to_string());
            println!("if test -d '{escaped_bin}'");
            println!("    if not contains -- '{escaped_bin}' $PATH");
            println!("        set -gx PATH '{escaped_bin}' $PATH");
            println!("    end");
            println!("end");
            println!("if test -f '{escaped_completion}'");
            println!("    source '{escaped_completion}'");
            println!("end");
        }
        CliCompletionShell::Powershell => {
            let escaped_bin = escape_ps_single_quote(&bin.display().to_string());
            let escaped_completion = escape_ps_single_quote(&completion_path.display().to_string());
            println!("if (Test-Path '{escaped_bin}') {{");
            println!(
                "  if (-not ($env:PATH -split ';' | Where-Object {{ $_ -eq '{escaped_bin}' }})) {{"
            );
            println!("    $env:PATH = '{escaped_bin};' + $env:PATH");
            println!("  }}");
            println!("}}");
            println!("if (Test-Path '{escaped_completion}') {{");
            println!("  . '{escaped_completion}'");
            println!("}}");
        }
    }
}

fn registry_state_root(layout: &PrefixLayout) -> PathBuf {
    layout.state_dir().join("registries")
}

#[derive(Debug)]
enum MetadataBackend {
    Legacy(RegistryIndex),
    Configured(ConfiguredRegistryIndex),
}

impl MetadataBackend {
    fn search_names(&self, query: &str) -> Result<Vec<String>> {
        match self {
            Self::Legacy(index) => index.search_names(query),
            Self::Configured(index) => index.search_names(query),
        }
    }

    fn package_versions(&self, name: &str) -> Result<Vec<PackageManifest>> {
        match self {
            Self::Legacy(index) => index.package_versions(name),
            Self::Configured(index) => index.package_versions(name),
        }
    }
}

fn select_metadata_backend(
    registry_root_override: Option<&Path>,
    layout: &PrefixLayout,
) -> Result<MetadataBackend> {
    if let Some(root) = registry_root_override {
        return Ok(MetadataBackend::Legacy(RegistryIndex::open(root)));
    }

    let source_state_root = registry_state_root(layout);
    let store = RegistrySourceStore::new(&source_state_root);
    let sources = store.list_sources_with_snapshot_state()?;
    let has_ready_snapshot = sources
        .iter()
        .any(|source| matches!(source.snapshot, RegistrySourceSnapshotState::Ready { .. }));
    if sources.is_empty() || !has_ready_snapshot {
        anyhow::bail!(METADATA_CONFIG_GUIDANCE);
    }

    let configured = ConfiguredRegistryIndex::open(source_state_root)
        .with_context(|| "failed loading configured registry snapshots for metadata commands")?;
    Ok(MetadataBackend::Configured(configured))
}

fn resolve_transaction_snapshot_id(layout: &PrefixLayout, operation: &str) -> Result<String> {
    let source_state_root = registry_state_root(layout);
    let store = RegistrySourceStore::new(&source_state_root);
    let sources = store.list_sources_with_snapshot_state()?;

    let mut ready = sources
        .into_iter()
        .filter(|source| source.source.enabled)
        .filter_map(|source| match source.snapshot {
            RegistrySourceSnapshotState::Ready { snapshot_id } => {
                Some((source.source.name, snapshot_id))
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    if ready.is_empty() {
        anyhow::bail!(METADATA_CONFIG_GUIDANCE);
    }

    ready.sort_by(|left, right| left.0.cmp(&right.0));
    let snapshot_id = ready[0].1.clone();
    if ready.iter().any(|(_, candidate)| candidate != &snapshot_id) {
        let _ = record_snapshot_id_mismatch(layout, operation, &ready);
        let summary = ready
            .iter()
            .map(|(name, snapshot)| format!("{name}={snapshot}"))
            .collect::<Vec<_>>()
            .join(", ");
        anyhow::bail!("metadata snapshot mismatch across configured sources: {summary}");
    }

    Ok(snapshot_id)
}

fn snapshot_monitor_log_path(layout: &PrefixLayout) -> PathBuf {
    layout.transactions_dir().join("snapshot-monitor.log")
}

fn record_snapshot_id_mismatch(
    layout: &PrefixLayout,
    operation: &str,
    ready: &[(String, String)],
) -> Result<()> {
    fs::create_dir_all(layout.transactions_dir()).with_context(|| {
        format!(
            "failed creating snapshot monitor state dir: {}",
            layout.transactions_dir().display()
        )
    })?;

    let timestamp_unix = current_unix_timestamp()?;
    let source_count = ready.len();
    let unique_snapshot_ids = ready
        .iter()
        .map(|(_, snapshot_id)| snapshot_id.as_str())
        .collect::<HashSet<_>>()
        .len();
    let source_summary = ready
        .iter()
        .map(|(name, snapshot)| format!("{name}={snapshot}"))
        .collect::<Vec<_>>()
        .join(",");
    let monitor_path = snapshot_monitor_log_path(layout);

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&monitor_path)
        .with_context(|| {
            format!(
                "failed opening snapshot monitor log for append: {}",
                monitor_path.display()
            )
        })?;

    writeln!(
        file,
        "timestamp_unix={timestamp_unix} level=error event=snapshot_id_consistency_mismatch error_code={} operation={operation} source_count={source_count} unique_snapshot_ids={unique_snapshot_ids} sources={source_summary}",
        SNAPSHOT_ID_MISMATCH_ERROR_CODE
    )
    .with_context(|| {
        format!(
            "failed writing snapshot monitor log entry: {}",
            monitor_path.display()
        )
    })?;

    Ok(())
}

struct UpdateReport {
    lines: Vec<String>,
    updated: u32,
    up_to_date: u32,
    failed: u32,
}

fn build_update_report(results: &[SourceUpdateResult]) -> UpdateReport {
    let mut updated = 0_u32;
    let mut up_to_date = 0_u32;
    let mut failed = 0_u32;
    let mut lines = Vec::with_capacity(results.len());

    for result in results {
        match result.status {
            SourceUpdateStatus::Updated => {
                updated += 1;
                lines.push(format!("{}: updated", result.name));
            }
            SourceUpdateStatus::UpToDate => {
                up_to_date += 1;
                lines.push(format!("{}: up-to-date", result.name));
            }
            SourceUpdateStatus::Failed => {
                failed += 1;
                let reason = update_failure_reason_code(result.error.as_deref());
                lines.push(format!("{}: failed (reason={reason})", result.name));
            }
        }
    }

    UpdateReport {
        lines,
        updated,
        up_to_date,
        failed,
    }
}

fn ensure_update_succeeded(failed: u32) -> Result<()> {
    if failed > 0 {
        return Err(anyhow!("source update failed"));
    }
    Ok(())
}

fn format_update_summary_line(updated: u32, up_to_date: u32, failed: u32) -> String {
    format!("update summary: updated={updated} up-to-date={up_to_date} failed={failed}")
}

fn update_failure_reason_code(error: Option<&str>) -> String {
    let Some(error) = error else {
        return "unknown".to_string();
    };

    for segment in error.split(':') {
        let candidate = segment.trim();
        if !candidate.is_empty()
            && candidate
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch == '-' || ch.is_ascii_digit())
        {
            return candidate.to_string();
        }
    }

    "unknown".to_string()
}

fn ensure_upgrade_command_ready(layout: &PrefixLayout) -> Result<()> {
    layout.ensure_base_dirs()?;
    ensure_no_active_transaction_for(layout, "upgrade")
}

fn run_upgrade_command(
    layout: &PrefixLayout,
    registry_root: Option<&Path>,
    spec: Option<String>,
    provider_overrides: &BTreeMap<String, String>,
) -> Result<()> {
    ensure_upgrade_command_ready(layout)?;
    let backend = select_metadata_backend(registry_root, layout)?;

    let receipts = read_install_receipts(layout)?;
    if receipts.is_empty() {
        println!("No installed packages");
        return Ok(());
    }

    let snapshot_id = match registry_root {
        Some(_) => None,
        None => Some(resolve_transaction_snapshot_id(layout, "upgrade")?),
    };
    execute_with_transaction(layout, "upgrade", snapshot_id.as_deref(), |tx| {
        let mut journal_seq = 1_u64;

        match spec.as_deref() {
            Some(single) => {
                let (name, requirement) = parse_spec(single)?;
                let installed = receipts.iter().find(|receipt| receipt.name == name);
                let Some(installed_receipt) = installed else {
                    println!("{name} is not installed");
                    return Ok(());
                };

                let roots = vec![RootInstallRequest {
                    name: installed_receipt.name.clone(),
                    requirement,
                }];
                let root_names = Vec::new();
                let resolved = resolve_install_graph(
                    layout,
                    &backend,
                    &roots,
                    installed_receipt.target.as_deref(),
                    provider_overrides,
                )?;
                let planned_dependency_overrides = build_planned_dependency_overrides(&resolved);
                enforce_no_downgrades(&receipts, &resolved, "upgrade")?;

                append_transaction_journal_entry(
                    layout,
                    &tx.txid,
                    &TransactionJournalEntry {
                        seq: journal_seq,
                        step: format!("resolve_plan:{}", installed_receipt.name),
                        state: "done".to_string(),
                        path: Some(installed_receipt.name.clone()),
                    },
                )?;
                journal_seq += 1;

                for package in &resolved {
                    if let Some(old) = receipts.iter().find(|r| r.name == package.manifest.name) {
                        let old_version = Version::parse(&old.version).with_context(|| {
                            format!(
                                "installed receipt for '{}' has invalid version: {}",
                                old.name, old.version
                            )
                        })?;
                        if package.manifest.version <= old_version {
                            println!("{} is up-to-date ({})", package.manifest.name, old.version);
                            continue;
                        }
                    }

                    let snapshot_path =
                        capture_package_state_snapshot(layout, &tx.txid, &package.manifest.name)?;
                    append_transaction_journal_entry(
                        layout,
                        &tx.txid,
                        &TransactionJournalEntry {
                            seq: journal_seq,
                            step: format!("backup_package_state:{}", package.manifest.name),
                            state: "done".to_string(),
                            path: Some(snapshot_path.display().to_string()),
                        },
                    )?;
                    journal_seq += 1;

                    append_transaction_journal_entry(
                        layout,
                        &tx.txid,
                        &TransactionJournalEntry {
                            seq: journal_seq,
                            step: format!("upgrade_package:{}", package.manifest.name),
                            state: "done".to_string(),
                            path: Some(package.manifest.name.clone()),
                        },
                    )?;
                    journal_seq += 1;

                    let dependencies = build_dependency_receipts(package, &resolved);
                    let outcome = install_resolved(
                        layout,
                        package,
                        &dependencies,
                        &root_names,
                        &planned_dependency_overrides,
                        snapshot_id.as_deref(),
                        false,
                    )?;
                    if let Some(old) = receipts.iter().find(|r| r.name == package.manifest.name) {
                        println!(
                            "upgraded {} from {} to {}",
                            package.manifest.name, old.version, package.manifest.version
                        );
                    }
                    println!("receipt: {}", outcome.receipt_path.display());
                }
            }
            None => {
                let plans = build_upgrade_plans(&receipts);
                if plans.is_empty() {
                    println!("{NO_ROOT_PACKAGES_TO_UPGRADE}");
                    return Ok(());
                }

                let mut grouped_resolved = Vec::new();
                let mut resolved_dependency_tokens = HashSet::new();
                for plan in &plans {
                    let (resolved, plan_tokens) = resolve_install_graph_with_tokens(
                        layout,
                        &backend,
                        &plan.roots,
                        plan.target.as_deref(),
                        provider_overrides,
                        false,
                    )?;
                    enforce_no_downgrades(&receipts, &resolved, "upgrade")?;

                    append_transaction_journal_entry(
                        layout,
                        &tx.txid,
                        &TransactionJournalEntry {
                            seq: journal_seq,
                            step: format!(
                                "resolve_plan:{}",
                                plan.target.as_deref().unwrap_or("host")
                            ),
                            state: "done".to_string(),
                            path: plan.target.clone(),
                        },
                    )?;
                    journal_seq += 1;

                    resolved_dependency_tokens.extend(plan_tokens);
                    grouped_resolved.push(resolved);
                }

                validate_provider_overrides_used(provider_overrides, &resolved_dependency_tokens)?;

                let overlap_check = grouped_resolved
                    .iter()
                    .zip(plans.iter())
                    .map(|(resolved, plan)| {
                        (
                            plan.target.as_deref(),
                            resolved
                                .iter()
                                .map(|package| package.manifest.name.clone())
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect::<Vec<_>>();
                enforce_disjoint_multi_target_upgrade(&overlap_check)?;

                for (resolved, plan) in grouped_resolved.iter().zip(plans.iter()) {
                    let planned_dependency_overrides = build_planned_dependency_overrides(resolved);

                    for package in resolved {
                        if let Some(old) = receipts.iter().find(|r| r.name == package.manifest.name)
                        {
                            let old_version = Version::parse(&old.version).with_context(|| {
                                format!(
                                    "installed receipt for '{}' has invalid version: {}",
                                    old.name, old.version
                                )
                            })?;
                            if package.manifest.version <= old_version {
                                println!(
                                    "{} is up-to-date ({})",
                                    package.manifest.name, old.version
                                );
                                continue;
                            }
                        }

                        let snapshot_path = capture_package_state_snapshot(
                            layout,
                            &tx.txid,
                            &package.manifest.name,
                        )?;
                        append_transaction_journal_entry(
                            layout,
                            &tx.txid,
                            &TransactionJournalEntry {
                                seq: journal_seq,
                                step: format!("backup_package_state:{}", package.manifest.name),
                                state: "done".to_string(),
                                path: Some(snapshot_path.display().to_string()),
                            },
                        )?;
                        journal_seq += 1;

                        append_transaction_journal_entry(
                            layout,
                            &tx.txid,
                            &TransactionJournalEntry {
                                seq: journal_seq,
                                step: format!("upgrade_package:{}", package.manifest.name),
                                state: "done".to_string(),
                                path: Some(package.manifest.name.clone()),
                            },
                        )?;
                        journal_seq += 1;

                        let dependencies = build_dependency_receipts(package, resolved);
                        let outcome = install_resolved(
                            layout,
                            package,
                            &dependencies,
                            &plan.root_names,
                            &planned_dependency_overrides,
                            snapshot_id.as_deref(),
                            false,
                        )?;
                        if let Some(old) = receipts.iter().find(|r| r.name == package.manifest.name)
                        {
                            println!(
                                "upgraded {} from {} to {}",
                                package.manifest.name, old.version, package.manifest.version
                            );
                        } else {
                            println!(
                                "installed dependency {} {}",
                                package.manifest.name, package.manifest.version
                            );
                        }
                        println!("receipt: {}", outcome.receipt_path.display());
                    }
                }
            }
        }

        append_transaction_journal_entry(
            layout,
            &tx.txid,
            &TransactionJournalEntry {
                seq: journal_seq,
                step: "apply_complete".to_string(),
                state: "done".to_string(),
                path: None,
            },
        )?;

        Ok(())
    })?;

    if let Err(err) = sync_completion_assets_best_effort(layout, "upgrade") {
        eprintln!("{err}");
    }

    Ok(())
}

fn is_valid_txid_input(txid: &str) -> bool {
    !txid.is_empty()
        && txid.starts_with("tx-")
        && txid.len() <= 128
        && txid
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn txid_process_id(txid: &str) -> Option<u32> {
    txid.rsplit('-').next()?.parse().ok()
}

fn transaction_owner_process_alive(txid: &str) -> Result<bool> {
    let Some(pid) = txid_process_id(txid) else {
        return Ok(false);
    };

    #[cfg(unix)]
    {
        let status = Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .with_context(|| format!("failed executing owner liveness probe for pid={pid}"))?;
        Ok(status.success())
    }

    #[cfg(windows)]
    {
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()
            .with_context(|| format!("failed executing owner liveness probe for pid={pid}"))?;

        if !output.status.success() {
            return Err(anyhow!(
                "owner liveness probe failed for pid={pid}: status={} stderr='{}'",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(stdout.contains(&format!(",\"{pid}\""))
            && !stdout.to_ascii_lowercase().contains("no tasks are running"))
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        Ok(true)
    }
}

fn read_transaction_journal_records(
    layout: &PrefixLayout,
    txid: &str,
) -> Result<Vec<TransactionJournalRecord>> {
    let path = layout.transaction_journal_path(txid);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(err).with_context(|| {
                format!("failed reading transaction journal: {}", path.display())
            });
        }
    };

    let mut records = Vec::new();
    for (line_no, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let value: Value = serde_json::from_str(trimmed).with_context(|| {
            format!(
                "failed parsing transaction journal entry: {} line={}",
                path.display(),
                line_no + 1
            )
        })?;
        let Some(object) = value.as_object() else {
            return Err(anyhow!(
                "failed parsing transaction journal entry: {} line={} is not an object",
                path.display(),
                line_no + 1
            ));
        };

        let seq = object
            .get("seq")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("missing journal field 'seq' line={}", line_no + 1))?;
        let step = object
            .get("step")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("missing journal field 'step' line={}", line_no + 1))?
            .to_string();
        let state = object
            .get("state")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("missing journal field 'state' line={}", line_no + 1))?
            .to_string();
        let path_value = object
            .get("path")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);

        records.push(TransactionJournalRecord {
            seq,
            step,
            state,
            path: path_value,
        });
    }

    records.sort_by_key(|record| record.seq);
    Ok(records)
}

fn rollback_package_from_step(step: &str) -> Option<&str> {
    step.strip_prefix("install_package:")
        .or_else(|| step.strip_prefix("upgrade_package:"))
        .or_else(|| step.strip_prefix("uninstall_target:"))
        .or_else(|| step.strip_prefix("prune_dependency:"))
}

fn backup_package_from_step(step: &str) -> Option<&str> {
    step.strip_prefix("backup_package_state:")
}

fn snapshot_manifest_path(snapshot_root: &Path) -> PathBuf {
    snapshot_root.join("manifest.txt")
}

fn snapshot_package_root(snapshot_root: &Path) -> PathBuf {
    snapshot_root.join("package")
}

fn snapshot_receipt_path(snapshot_root: &Path, package_name: &str) -> PathBuf {
    snapshot_root
        .join("receipt")
        .join(format!("{package_name}.receipt"))
}

fn snapshot_bin_path(snapshot_root: &Path, bin_name: &str) -> PathBuf {
    snapshot_root.join("bins").join(bin_name)
}

fn read_snapshot_manifest(snapshot_root: &Path) -> Result<PackageSnapshotManifest> {
    let path = snapshot_manifest_path(snapshot_root);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PackageSnapshotManifest {
                package_exists: false,
                receipt_exists: false,
                bins: Vec::new(),
            });
        }
        Err(err) => {
            return Err(err)
                .with_context(|| format!("failed reading snapshot manifest: {}", path.display()));
        }
    };

    let mut manifest = PackageSnapshotManifest {
        package_exists: false,
        receipt_exists: false,
        bins: Vec::new(),
    };

    for line in raw.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(value) = line.strip_prefix("package_exists=") {
            manifest.package_exists = value == "1";
        } else if let Some(value) = line.strip_prefix("receipt_exists=") {
            manifest.receipt_exists = value == "1";
        } else if let Some(bin_name) = line.strip_prefix("bin=") {
            manifest.bins.push(bin_name.to_string());
        }
    }

    Ok(manifest)
}

fn write_snapshot_manifest(snapshot_root: &Path, manifest: &PackageSnapshotManifest) -> Result<()> {
    let path = snapshot_manifest_path(snapshot_root);
    let mut lines = Vec::new();
    lines.push(format!(
        "package_exists={}",
        if manifest.package_exists { "1" } else { "0" }
    ));
    lines.push(format!(
        "receipt_exists={}",
        if manifest.receipt_exists { "1" } else { "0" }
    ));
    for bin in &manifest.bins {
        lines.push(format!("bin={bin}"));
    }
    std::fs::write(&path, lines.join("\n"))
        .with_context(|| format!("failed writing snapshot manifest: {}", path.display()))
}

fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(src)
        .with_context(|| format!("failed to stat source path: {}", src.display()))?;

    if metadata.is_dir() {
        std::fs::create_dir_all(dst)
            .with_context(|| format!("failed to create directory: {}", dst.display()))?;
        for entry in std::fs::read_dir(src)
            .with_context(|| format!("failed to read directory: {}", src.display()))?
        {
            let entry =
                entry.with_context(|| format!("failed to iterate directory: {}", src.display()))?;
            let child_src = entry.path();
            let child_dst = dst.join(entry.file_name());
            copy_tree(&child_src, &child_dst)?;
        }
        return Ok(());
    }

    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory: {}", parent.display()))?;
    }

    #[cfg(unix)]
    if metadata.file_type().is_symlink() {
        let target = std::fs::read_link(src)
            .with_context(|| format!("failed to read symlink: {}", src.display()))?;
        std::os::unix::fs::symlink(&target, dst).with_context(|| {
            format!(
                "failed to copy symlink {} -> {}",
                dst.display(),
                target.display()
            )
        })?;
        return Ok(());
    }

    std::fs::copy(src, dst)
        .with_context(|| format!("failed to copy {} to {}", src.display(), dst.display()))?;
    Ok(())
}

fn capture_package_state_snapshot(
    layout: &PrefixLayout,
    txid: &str,
    package_name: &str,
) -> Result<PathBuf> {
    let snapshot_root = layout
        .transaction_staging_path(txid)
        .join("rollback")
        .join(package_name);
    if snapshot_root.exists() {
        std::fs::remove_dir_all(&snapshot_root).with_context(|| {
            format!(
                "failed clearing existing rollback snapshot dir: {}",
                snapshot_root.display()
            )
        })?;
    }

    std::fs::create_dir_all(snapshot_package_root(&snapshot_root)).with_context(|| {
        format!(
            "failed creating rollback snapshot package dir: {}",
            snapshot_package_root(&snapshot_root).display()
        )
    })?;
    std::fs::create_dir_all(snapshot_root.join("receipt")).with_context(|| {
        format!(
            "failed creating rollback snapshot receipt dir: {}",
            snapshot_root.join("receipt").display()
        )
    })?;
    std::fs::create_dir_all(snapshot_root.join("bins")).with_context(|| {
        format!(
            "failed creating rollback snapshot bins dir: {}",
            snapshot_root.join("bins").display()
        )
    })?;

    let mut manifest = PackageSnapshotManifest {
        package_exists: false,
        receipt_exists: false,
        bins: Vec::new(),
    };

    let package_root = layout.pkgs_dir().join(package_name);
    if package_root.exists() {
        manifest.package_exists = true;
        copy_tree(&package_root, &snapshot_package_root(&snapshot_root))?;
    }

    let receipt_path = layout.receipt_path(package_name);
    if receipt_path.exists() {
        manifest.receipt_exists = true;
        std::fs::copy(
            &receipt_path,
            snapshot_receipt_path(&snapshot_root, package_name),
        )
        .with_context(|| {
            format!(
                "failed copying receipt snapshot {}",
                snapshot_receipt_path(&snapshot_root, package_name).display()
            )
        })?;

        if let Some(receipt) = read_install_receipts(layout)?
            .into_iter()
            .find(|receipt| receipt.name == package_name)
        {
            manifest.bins = receipt.exposed_bins.clone();
            for bin_name in &manifest.bins {
                let source = bin_path(layout, bin_name);
                if source.exists() {
                    std::fs::copy(&source, snapshot_bin_path(&snapshot_root, bin_name))
                        .with_context(|| {
                            format!(
                                "failed copying binary snapshot {}",
                                snapshot_bin_path(&snapshot_root, bin_name).display()
                            )
                        })?;
                }
            }
        }
    }

    write_snapshot_manifest(&snapshot_root, &manifest)?;
    Ok(snapshot_root)
}

fn binary_entry_points_to_package_root(bin_entry: &Path, package_root: &Path) -> Result<bool> {
    #[cfg(unix)]
    {
        let metadata = std::fs::symlink_metadata(bin_entry)
            .with_context(|| format!("failed to inspect binary entry: {}", bin_entry.display()))?;
        if metadata.file_type().is_symlink() {
            let target = std::fs::read_link(bin_entry).with_context(|| {
                format!(
                    "failed to read binary symlink target: {}",
                    bin_entry.display()
                )
            })?;
            let resolved = if target.is_absolute() {
                target
            } else {
                bin_entry
                    .parent()
                    .map(|parent| parent.join(&target))
                    .unwrap_or(target)
            };
            return Ok(resolved.starts_with(package_root));
        }
        Ok(false)
    }

    #[cfg(windows)]
    {
        let metadata = std::fs::metadata(bin_entry)
            .with_context(|| format!("failed to inspect binary entry: {}", bin_entry.display()))?;
        if !metadata.is_file() {
            return Ok(false);
        }

        let shim = std::fs::read_to_string(bin_entry)
            .with_context(|| format!("failed to read binary shim: {}", bin_entry.display()))?;
        let Some(start) = shim.find('"') else {
            return Ok(false);
        };
        let rest = &shim[start + 1..];
        let Some(end) = rest.find('"') else {
            return Ok(false);
        };

        let source = PathBuf::from(&rest[..end]);
        Ok(source.starts_with(package_root))
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = bin_entry;
        let _ = package_root;
        Ok(false)
    }
}

fn remove_binary_entries_for_package_root(
    layout: &PrefixLayout,
    package_root: &Path,
) -> Result<()> {
    let entries = match std::fs::read_dir(layout.bin_dir()) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "failed to read bin directory: {}",
                    layout.bin_dir().display()
                )
            });
        }
    };

    for entry in entries {
        let entry = entry.with_context(|| {
            format!(
                "failed to iterate bin directory: {}",
                layout.bin_dir().display()
            )
        })?;
        let path = entry.path();
        if binary_entry_points_to_package_root(&path, package_root)? {
            remove_file_if_exists(&path)?;
        }
    }

    Ok(())
}

fn restore_package_state_snapshot(
    layout: &PrefixLayout,
    package_name: &str,
    snapshot_root: Option<&Path>,
) -> Result<()> {
    let package_root = layout.pkgs_dir().join(package_name);
    remove_binary_entries_for_package_root(layout, &package_root)?;

    let existing_receipt = read_install_receipts(layout)?
        .into_iter()
        .find(|receipt| receipt.name == package_name);
    let existing_bins = existing_receipt
        .as_ref()
        .map(|receipt| receipt.exposed_bins.clone())
        .unwrap_or_default();
    for bin_name in existing_bins {
        remove_exposed_binary(layout, &bin_name)?;
    }

    if package_root.exists() {
        std::fs::remove_dir_all(&package_root).with_context(|| {
            format!("failed to remove package path: {}", package_root.display())
        })?;
    }

    remove_file_if_exists(&layout.receipt_path(package_name))?;

    let Some(snapshot_root) = snapshot_root else {
        return Ok(());
    };

    let manifest = read_snapshot_manifest(snapshot_root)?;
    if manifest.package_exists && snapshot_package_root(snapshot_root).exists() {
        copy_tree(&snapshot_package_root(snapshot_root), &package_root)?;
    }

    if manifest.receipt_exists {
        let src = snapshot_receipt_path(snapshot_root, package_name);
        if src.exists() {
            if let Some(parent) = layout.receipt_path(package_name).parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            std::fs::copy(&src, layout.receipt_path(package_name)).with_context(|| {
                format!(
                    "failed restoring receipt from {}",
                    snapshot_receipt_path(snapshot_root, package_name).display()
                )
            })?;
        }
    }

    for bin_name in manifest.bins {
        let dst = bin_path(layout, &bin_name);
        remove_file_if_exists(&dst)?;
        let src = snapshot_bin_path(snapshot_root, &bin_name);
        if src.exists() {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            std::fs::copy(&src, &dst).with_context(|| {
                format!(
                    "failed restoring binary '{}' from {}",
                    bin_name,
                    src.display()
                )
            })?;
        }
    }

    Ok(())
}

fn replay_rollback_journal(layout: &PrefixLayout, txid: &str) -> Result<bool> {
    let records = read_transaction_journal_records(layout, txid)?;
    if records.is_empty() {
        return Ok(false);
    }

    let mut backups = HashMap::new();
    for record in &records {
        if record.state != "done" {
            continue;
        }
        if let Some(package_name) = backup_package_from_step(&record.step) {
            if let Some(path) = &record.path {
                backups.insert(package_name.to_string(), PathBuf::from(path));
            }
        }
    }

    let mut compensating_steps = records
        .iter()
        .filter(|record| record.state == "done")
        .filter_map(|record| {
            rollback_package_from_step(&record.step)
                .map(|package_name| (record.seq, package_name.to_string()))
        })
        .collect::<Vec<_>>();
    compensating_steps.sort_by(|left, right| right.0.cmp(&left.0));

    if compensating_steps.is_empty() {
        return Ok(false);
    }

    for (_, package_name) in &compensating_steps {
        if !backups.contains_key(package_name) {
            return Err(anyhow!(
                "transaction journal missing rollback payload for package '{package_name}'"
            ));
        }
    }

    for (_, package_name) in compensating_steps {
        let snapshot_root = backups.get(&package_name).map(PathBuf::as_path);
        restore_package_state_snapshot(layout, &package_name, snapshot_root)?;
    }

    Ok(true)
}

fn latest_rollback_candidate_txid(layout: &PrefixLayout) -> Result<Option<String>> {
    let entries = match std::fs::read_dir(layout.transactions_dir()) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "failed to read transactions directory: {}",
                    layout.transactions_dir().display()
                )
            })
        }
    };

    let mut latest: Option<(u64, String)> = None;
    for entry in entries {
        let entry = entry.with_context(|| {
            format!(
                "failed to iterate transactions directory: {}",
                layout.transactions_dir().display()
            )
        })?;
        let path = entry.path();

        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }

        let Some(txid) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };

        let Some(metadata) = read_transaction_metadata(layout, txid)? else {
            continue;
        };
        if matches!(metadata.status.as_str(), "committed" | "rolled_back") {
            continue;
        }

        match &latest {
            None => latest = Some((metadata.started_at_unix, metadata.txid)),
            Some((best_started_at, best_txid)) => {
                if metadata.started_at_unix > *best_started_at
                    || (metadata.started_at_unix == *best_started_at && metadata.txid > *best_txid)
                {
                    latest = Some((metadata.started_at_unix, metadata.txid));
                }
            }
        }
    }

    Ok(latest.map(|(_, txid)| txid))
}

fn run_rollback_command(layout: &PrefixLayout, txid: Option<String>) -> Result<()> {
    layout.ensure_base_dirs()?;

    let target_txid = match txid {
        Some(txid) => {
            if !is_valid_txid_input(&txid) {
                return Err(anyhow!("invalid rollback txid: {txid}"));
            }
            txid
        }
        None => {
            if let Some(active_txid) = read_active_transaction(layout)? {
                active_txid
            } else if let Some(candidate_txid) = latest_rollback_candidate_txid(layout)? {
                candidate_txid
            } else {
                println!("no rollback needed");
                return Ok(());
            }
        }
    };

    let metadata = read_transaction_metadata(layout, &target_txid)?
        .ok_or_else(|| anyhow!("transaction metadata missing for rollback txid={target_txid}"))?;
    let active_txid = read_active_transaction(layout)?;

    if matches!(metadata.status.as_str(), "planning" | "applying")
        && active_txid.as_deref() == Some(target_txid.as_str())
        && transaction_owner_process_alive(&target_txid)?
    {
        return Err(anyhow!(
            "cannot rollback while transaction is active (status={})",
            metadata.status
        ));
    }

    if metadata.status == "committed" || metadata.status == "rolled_back" {
        if active_txid.as_deref() == Some(target_txid.as_str()) {
            clear_active_transaction(layout)?;
        }
        println!("no rollback needed");
        return Ok(());
    }

    let journal_records = read_transaction_journal_records(layout, &target_txid)?;
    let has_completed_mutating_steps = journal_records
        .iter()
        .any(|record| record.state == "done" && rollback_package_from_step(&record.step).is_some());

    set_transaction_status(layout, &target_txid, "rolling_back")?;
    let replayed = match replay_rollback_journal(layout, &target_txid) {
        Ok(replayed) => replayed,
        Err(err) => {
            let _ = set_transaction_status(layout, &target_txid, "failed");
            return Err(err).with_context(|| {
                format!("rollback failed {target_txid}: transaction journal replay required")
            });
        }
    };

    if !replayed && has_completed_mutating_steps {
        let _ = set_transaction_status(layout, &target_txid, "failed");
        return Err(anyhow!(
            "rollback failed {target_txid}: transaction journal replay required"
        ));
    }

    set_transaction_status(layout, &target_txid, "rolled_back")?;

    if active_txid.as_deref() == Some(target_txid.as_str()) {
        clear_active_transaction(layout)?;
    }

    if let Err(err) = sync_completion_assets_best_effort(layout, "rollback") {
        eprintln!("{err}");
    }

    println!("rolled back {target_txid}");
    Ok(())
}

fn run_repair_command(layout: &PrefixLayout) -> Result<()> {
    layout.ensure_base_dirs()?;

    let Some(txid) = read_active_transaction(layout)? else {
        println!("repair: no action needed");
        return Ok(());
    };

    let metadata = read_transaction_metadata(layout, &txid)?;
    let Some(metadata) = metadata else {
        clear_active_transaction(layout)?;
        println!("repair: cleared stale marker {txid}");
        return Ok(());
    };

    if status_allows_stale_marker_cleanup(&metadata.status) {
        clear_active_transaction(layout)?;
        println!("repair: cleared stale marker {txid}");
        return Ok(());
    }

    match metadata.status.as_str() {
        "planning" | "applying" | "failed" | "rolling_back" => {
            run_rollback_command(layout, Some(txid.clone()))?;
            println!("recovered interrupted transaction {txid}: rolled back");
            Ok(())
        }
        status => Err(anyhow!(
            "transaction {txid} requires manual repair (reason=unsupported_status status={status})"
        )),
    }
}

fn run_uninstall_command(layout: &PrefixLayout, name: String) -> Result<()> {
    layout.ensure_base_dirs()?;
    ensure_no_active_transaction_for(layout, "uninstall")?;

    execute_with_transaction(layout, "uninstall", None, |tx| {
        let mut journal_seq = 1_u64;
        let mut snapshot_paths = HashMap::new();
        for receipt in read_install_receipts(layout)? {
            let snapshot_path = capture_package_state_snapshot(layout, &tx.txid, &receipt.name)?;
            snapshot_paths.insert(receipt.name, snapshot_path);
        }

        let result = uninstall_package(layout, &name)?;

        if let Some(snapshot_path) = snapshot_paths.get(&name) {
            append_transaction_journal_entry(
                layout,
                &tx.txid,
                &TransactionJournalEntry {
                    seq: journal_seq,
                    step: format!("backup_package_state:{}", name),
                    state: "done".to_string(),
                    path: Some(snapshot_path.display().to_string()),
                },
            )?;
            journal_seq += 1;
        }

        append_transaction_journal_entry(
            layout,
            &tx.txid,
            &TransactionJournalEntry {
                seq: journal_seq,
                step: format!("uninstall_target:{}", name),
                state: "done".to_string(),
                path: Some(name.clone()),
            },
        )?;
        journal_seq += 1;

        for dependency in &result.pruned_dependencies {
            if let Some(snapshot_path) = snapshot_paths.get(dependency) {
                append_transaction_journal_entry(
                    layout,
                    &tx.txid,
                    &TransactionJournalEntry {
                        seq: journal_seq,
                        step: format!("backup_package_state:{dependency}"),
                        state: "done".to_string(),
                        path: Some(snapshot_path.display().to_string()),
                    },
                )?;
                journal_seq += 1;
            }

            append_transaction_journal_entry(
                layout,
                &tx.txid,
                &TransactionJournalEntry {
                    seq: journal_seq,
                    step: format!("prune_dependency:{dependency}"),
                    state: "done".to_string(),
                    path: Some(dependency.clone()),
                },
            )?;
            journal_seq += 1;
        }

        append_transaction_journal_entry(
            layout,
            &tx.txid,
            &TransactionJournalEntry {
                seq: journal_seq,
                step: "apply_complete".to_string(),
                state: "done".to_string(),
                path: None,
            },
        )?;

        for line in format_uninstall_messages(&result) {
            println!("{line}");
        }

        Ok(())
    })?;

    if let Err(err) = sync_completion_assets_best_effort(layout, "uninstall") {
        eprintln!("{err}");
    }

    Ok(())
}

fn run_update_command(store: &RegistrySourceStore, registry: &[String]) -> Result<()> {
    let results = store.update_sources(registry)?;
    let report = build_update_report(&results);
    for line in report.lines {
        println!("{line}");
    }
    println!(
        "{}",
        format_update_summary_line(report.updated, report.up_to_date, report.failed)
    );
    ensure_update_succeeded(report.failed)
}

fn format_registry_kind(kind: RegistrySourceKind) -> &'static str {
    match kind {
        RegistrySourceKind::Git => "git",
        RegistrySourceKind::Filesystem => "filesystem",
    }
}

fn format_registry_add_lines(
    name: &str,
    kind: &str,
    priority: u32,
    fingerprint: &str,
) -> Vec<String> {
    let prefix: String = fingerprint.chars().take(16).collect();
    vec![
        format!("added registry {name}"),
        format!("kind: {kind}"),
        format!("priority: {priority}"),
        format!("fingerprint: {prefix}..."),
    ]
}

fn format_registry_remove_lines(name: &str, purge_cache: bool) -> Vec<String> {
    let cache_state = if purge_cache { "purged" } else { "kept" };
    vec![
        format!("removed registry {name}"),
        format!("cache: {cache_state}"),
    ]
}

fn format_registry_list_snapshot_state(snapshot: &RegistrySourceSnapshotState) -> String {
    match snapshot {
        RegistrySourceSnapshotState::None => "none".to_string(),
        RegistrySourceSnapshotState::Ready { snapshot_id } => format!("ready:{snapshot_id}"),
        RegistrySourceSnapshotState::Error { reason_code, .. } => format!("error:{reason_code}"),
    }
}

fn format_registry_list_lines(mut sources: Vec<RegistrySourceWithSnapshotState>) -> Vec<String> {
    sources.sort_by(|left, right| {
        left.source
            .priority
            .cmp(&right.source.priority)
            .then_with(|| left.source.name.cmp(&right.source.name))
    });

    sources
        .into_iter()
        .map(|source| {
            let kind = format_registry_kind(source.source.kind.clone());
            format!(
                "{} kind={} priority={} location={} snapshot={}",
                source.source.name,
                kind,
                source.source.priority,
                source.source.location,
                format_registry_list_snapshot_state(&source.snapshot)
            )
        })
        .collect()
}

fn parse_spec(spec: &str) -> Result<(String, VersionReq)> {
    let (name, req) = match spec.split_once('@') {
        Some((name, req)) => (name, req),
        None => (spec, "*"),
    };
    if name.trim().is_empty() {
        return Err(anyhow!("package name must not be empty"));
    }
    let requirement = VersionReq::parse(req)
        .with_context(|| format!("invalid version requirement for '{name}': {req}"))?;
    Ok((name.to_string(), requirement))
}

fn parse_pin_spec(spec: &str) -> Result<(String, VersionReq)> {
    let Some((name, req)) = spec.split_once('@') else {
        return Err(anyhow!(
            "pin requires explicit constraint: use '<name>@<requirement>'"
        ));
    };
    if name.trim().is_empty() {
        return Err(anyhow!("package name must not be empty"));
    }
    if req.trim().is_empty() {
        return Err(anyhow!("pin requirement must not be empty"));
    }

    let requirement = VersionReq::parse(req)
        .with_context(|| format!("invalid pin requirement for '{name}': {req}"))?;
    Ok((name.to_string(), requirement))
}

fn parse_provider_overrides(values: &[String]) -> Result<BTreeMap<String, String>> {
    let mut overrides = BTreeMap::new();
    for value in values {
        let (capability, package) = value.split_once('=').ok_or_else(|| {
            anyhow!(
                "invalid provider override '{}': expected capability=package",
                value
            )
        })?;

        if !is_policy_token(capability) {
            return Err(anyhow!(
                "invalid provider override '{}': capability '{}' must use package-name grammar",
                value,
                capability
            ));
        }
        if !is_policy_token(package) {
            return Err(anyhow!(
                "invalid provider override '{}': package '{}' must use package-name grammar",
                value,
                package
            ));
        }

        if overrides
            .insert(capability.to_string(), package.to_string())
            .is_some()
        {
            return Err(anyhow!(
                "invalid provider override '{}': duplicate override for capability '{}': use one binding per capability",
                value,
                capability
            ));
        }
    }

    Ok(overrides)
}

fn is_policy_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 {
        return false;
    }

    let starts_valid = bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit();
    starts_valid
        && bytes[1..]
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._+-".contains(b))
}

fn format_info_lines(name: &str, versions: &[PackageManifest]) -> Vec<String> {
    let mut manifests = versions.iter().collect::<Vec<_>>();
    manifests.sort_by(|left, right| right.version.cmp(&left.version));

    let mut lines = vec![format!("Package: {name}")];
    for manifest in manifests {
        lines.push(format!("- {}", manifest.version));

        if !manifest.provides.is_empty() {
            lines.push(format!("  Provides: {}", manifest.provides.join(", ")));
        }

        if !manifest.conflicts.is_empty() {
            let conflicts = manifest
                .conflicts
                .iter()
                .map(|(name, req)| format!("{}({})", name, req))
                .collect::<Vec<_>>();
            lines.push(format!("  Conflicts: {}", conflicts.join(", ")));
        }

        if !manifest.replaces.is_empty() {
            let replaces = manifest
                .replaces
                .iter()
                .map(|(name, req)| format!("{}({})", name, req))
                .collect::<Vec<_>>();
            lines.push(format!("  Replaces: {}", replaces.join(", ")));
        }
    }

    lines
}

fn apply_provider_override(
    requested_name: &str,
    candidates: Vec<PackageManifest>,
    provider_overrides: &BTreeMap<String, String>,
) -> Result<Vec<PackageManifest>> {
    let Some(provider_name) = provider_overrides.get(requested_name) else {
        return Ok(candidates);
    };

    let has_direct_package_candidates = candidates
        .iter()
        .any(|manifest| manifest.name == requested_name);
    if has_direct_package_candidates && provider_name != requested_name {
        return Err(anyhow!(
            "provider override '{}={}' is invalid: '{}' resolves directly to package manifests; direct package names cannot be overridden",
            requested_name,
            provider_name,
            requested_name
        ));
    }

    let filtered = candidates
        .into_iter()
        .filter(|manifest| {
            manifest.name == *provider_name
                && (manifest.name == requested_name
                    || manifest
                        .provides
                        .iter()
                        .any(|provided| provided == requested_name))
        })
        .collect::<Vec<_>>();

    if filtered.is_empty() {
        return Err(anyhow!(
            "provider override '{}={}' did not match any candidate packages",
            requested_name,
            provider_name
        ));
    }

    Ok(filtered)
}

fn validate_provider_overrides_used(
    provider_overrides: &BTreeMap<String, String>,
    resolved_dependency_tokens: &HashSet<String>,
) -> Result<()> {
    let unused = provider_overrides
        .iter()
        .filter(|(capability, _)| !resolved_dependency_tokens.contains(*capability))
        .map(|(capability, provider)| format!("{capability}={provider}"))
        .collect::<Vec<_>>();

    if unused.is_empty() {
        return Ok(());
    }

    Err(anyhow!(
        "unused provider override(s): {}",
        unused.join(", ")
    ))
}

#[cfg(test)]
fn select_manifest_with_pin<'a>(
    versions: &'a [PackageManifest],
    request_requirement: &VersionReq,
    pin_requirement: Option<&VersionReq>,
) -> Option<&'a PackageManifest> {
    versions
        .iter()
        .filter(|manifest| request_requirement.matches(&manifest.version))
        .filter(|manifest| {
            pin_requirement
                .map(|pin| pin.matches(&manifest.version))
                .unwrap_or(true)
        })
        .max_by(|a, b| a.version.cmp(&b.version))
}

#[derive(Debug, Clone)]
struct ResolvedInstall {
    manifest: PackageManifest,
    artifact: Artifact,
    resolved_target: String,
    archive_type: ArchiveType,
}

#[derive(Debug, Clone)]
struct InstallOutcome {
    name: String,
    version: String,
    resolved_target: String,
    archive_type: ArchiveType,
    artifact_url: String,
    cache_path: PathBuf,
    download_status: &'static str,
    install_root: PathBuf,
    receipt_path: PathBuf,
    exposed_bins: Vec<String>,
    exposed_completions: Vec<String>,
}

#[derive(Debug, Clone)]
struct RootInstallRequest {
    name: String,
    requirement: VersionReq,
}

#[derive(Debug, Clone)]
struct UpgradePlan {
    target: Option<String>,
    roots: Vec<RootInstallRequest>,
    root_names: Vec<String>,
}

#[derive(Debug, Clone)]
struct TransactionJournalRecord {
    seq: u64,
    step: String,
    state: String,
    path: Option<String>,
}

#[derive(Debug, Clone)]
struct PackageSnapshotManifest {
    package_exists: bool,
    receipt_exists: bool,
    bins: Vec<String>,
}

fn begin_transaction(
    layout: &PrefixLayout,
    operation: &str,
    snapshot_id: Option<&str>,
    started_at_unix: u64,
) -> Result<TransactionMetadata> {
    let txid = format!("tx-{started_at_unix}-{}", std::process::id());
    let metadata = TransactionMetadata {
        version: 1,
        txid,
        operation: operation.to_string(),
        status: "planning".to_string(),
        started_at_unix,
        snapshot_id: snapshot_id.map(ToOwned::to_owned),
    };

    write_transaction_metadata(layout, &metadata)?;
    if let Err(err) = set_active_transaction(layout, &metadata.txid) {
        let _ = remove_file_if_exists(&layout.transaction_metadata_path(&metadata.txid));
        let _ = std::fs::remove_dir_all(layout.transaction_staging_path(&metadata.txid));
        return Err(err);
    }

    Ok(metadata)
}

fn set_transaction_status(layout: &PrefixLayout, txid: &str, status: &str) -> Result<()> {
    update_transaction_status(layout, txid, status)
}

fn execute_with_transaction<F>(
    layout: &PrefixLayout,
    operation: &str,
    snapshot_id: Option<&str>,
    run: F,
) -> Result<()>
where
    F: FnOnce(&TransactionMetadata) -> Result<()>,
{
    let started_at_unix = current_unix_timestamp()?;
    let tx = begin_transaction(layout, operation, snapshot_id, started_at_unix)?;

    let run_result = (|| -> Result<()> {
        set_transaction_status(layout, &tx.txid, "applying")?;
        run(&tx)?;
        set_transaction_status(layout, &tx.txid, "committed")?;
        clear_active_transaction(layout)?;
        Ok(())
    })();

    match run_result {
        Ok(()) => Ok(()),
        Err(err) => {
            let current_status = read_transaction_metadata(layout, &tx.txid)
                .ok()
                .flatten()
                .map(|metadata| metadata.status);
            let preserve_recovery_state = current_status
                .as_deref()
                .map(|status| {
                    matches!(
                        status,
                        "rolling_back" | "rolled_back" | "committed" | "failed"
                    )
                })
                .unwrap_or(false);
            if matches!(current_status.as_deref(), Some("rolled_back" | "committed")) {
                let _ = clear_active_transaction(layout);
            }
            if !preserve_recovery_state {
                let _ = set_transaction_status(layout, &tx.txid, "failed");
            }
            Err(err)
        }
    }
}

fn status_allows_stale_marker_cleanup(status: &str) -> bool {
    matches!(status, "committed" | "rolled_back")
}

fn normalize_command_token(command: &str) -> String {
    let command = command.trim().to_ascii_lowercase();
    if command.is_empty() {
        "unknown".to_string()
    } else {
        command
    }
}

fn ensure_no_active_transaction_for(layout: &PrefixLayout, command: &str) -> Result<()> {
    let command = normalize_command_token(command);
    ensure_no_active_transaction(layout).map_err(|err| {
        anyhow!("cannot {command} (reason=active_transaction command={command}): {err}")
    })
}

fn ensure_no_active_transaction(layout: &PrefixLayout) -> Result<()> {
    let active_txid = match read_active_transaction(layout) {
        Ok(active_txid) => active_txid,
        Err(_) => {
            return Err(anyhow!(
                "transaction state requires repair (reason=active_marker_unreadable path={})",
                layout.transaction_active_path().display()
            ));
        }
    };

    if let Some(txid) = active_txid {
        let metadata = match read_transaction_metadata(layout, &txid) {
            Ok(metadata) => metadata,
            Err(_) => {
                return Err(anyhow!(
                    "transaction {txid} requires repair (reason=metadata_unreadable path={})",
                    layout.transaction_metadata_path(&txid).display()
                ));
            }
        };

        if let Some(metadata) = metadata {
            if status_allows_stale_marker_cleanup(&metadata.status) {
                clear_active_transaction(layout)?;
                return Ok(());
            }
            if metadata.status == "rolling_back" {
                return Err(anyhow!(
                    "transaction {txid} requires repair (reason=rolling_back)"
                ));
            }
            if metadata.status == "failed" {
                return Err(anyhow!(
                    "transaction {txid} requires repair (reason=failed)"
                ));
            }

            return Err(anyhow!(
                "transaction {txid} is active (reason=active_status status={})",
                metadata.status
            ));
        }

        return Err(anyhow!(
            "transaction {txid} requires repair (reason=metadata_missing path={})",
            layout.transaction_metadata_path(&txid).display()
        ));
    }

    Ok(())
}

fn doctor_transaction_health_line(layout: &PrefixLayout) -> Result<String> {
    let active_txid = match read_active_transaction(layout) {
        Ok(active_txid) => active_txid,
        Err(_) => {
            return Ok(format!(
                "transaction: failed (reason=active_marker_unreadable path={})",
                layout.transaction_active_path().display()
            ));
        }
    };

    let Some(txid) = active_txid else {
        return Ok("transaction: clean".to_string());
    };

    let metadata = match read_transaction_metadata(layout, &txid) {
        Ok(metadata) => metadata,
        Err(_) => {
            return Ok(format!(
                "transaction: failed {txid} (reason=metadata_unreadable path={})",
                layout.transaction_metadata_path(&txid).display()
            ));
        }
    };

    let Some(metadata) = metadata else {
        return Ok(format!(
            "transaction: failed {txid} (reason=metadata_missing path={})",
            layout.transaction_metadata_path(&txid).display()
        ));
    };

    if metadata.status == "rolling_back" {
        return Ok(format!("transaction: failed {txid} (reason=rolling_back)"));
    }
    if metadata.status == "failed" {
        return Ok(format!("transaction: failed {txid} (reason=failed)"));
    }
    if status_allows_stale_marker_cleanup(&metadata.status) {
        clear_active_transaction(layout)?;
        return Ok("transaction: clean".to_string());
    }

    Ok(format!("transaction: active {txid}"))
}

fn resolve_install_graph(
    layout: &PrefixLayout,
    index: &MetadataBackend,
    roots: &[RootInstallRequest],
    requested_target: Option<&str>,
    provider_overrides: &BTreeMap<String, String>,
) -> Result<Vec<ResolvedInstall>> {
    let (resolved, _) = resolve_install_graph_with_tokens(
        layout,
        index,
        roots,
        requested_target,
        provider_overrides,
        true,
    )?;
    Ok(resolved)
}

fn resolve_install_graph_with_tokens(
    layout: &PrefixLayout,
    index: &MetadataBackend,
    roots: &[RootInstallRequest],
    requested_target: Option<&str>,
    provider_overrides: &BTreeMap<String, String>,
    validate_overrides: bool,
) -> Result<(Vec<ResolvedInstall>, HashSet<String>)> {
    let mut pins = BTreeMap::new();
    for (name, raw_req) in read_all_pins(layout)? {
        let parsed = VersionReq::parse(&raw_req)
            .with_context(|| format!("invalid pin requirement for '{name}' in state: {raw_req}"))?;
        pins.insert(name, parsed);
    }

    let root_reqs: Vec<RootRequirement> = roots
        .iter()
        .map(|root| RootRequirement {
            name: root.name.clone(),
            requirement: root.requirement.clone(),
        })
        .collect();

    let graph = resolve_dependency_graph(&root_reqs, &pins, |package_name| {
        let versions = index.package_versions(package_name)?;
        apply_provider_override(package_name, versions, provider_overrides)
    })?;

    let resolved_dependency_tokens = graph.manifests.keys().cloned().collect::<HashSet<_>>();
    if validate_overrides {
        validate_provider_overrides_used(provider_overrides, &resolved_dependency_tokens)?;
    }

    let resolved_target = requested_target
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| host_target_triple().to_string());

    let resolved = graph
        .install_order
        .iter()
        .map(|name| {
            let manifest = graph
                .manifests
                .get(name)
                .ok_or_else(|| anyhow!("resolver selected package missing from graph: {name}"))?
                .clone();

            let artifact = manifest
                .artifacts
                .iter()
                .find(|artifact| artifact.target == resolved_target)
                .ok_or_else(|| {
                    anyhow!(
                        "no artifact available for target {} in {} {}",
                        resolved_target,
                        manifest.name,
                        manifest.version
                    )
                })?
                .clone();
            let archive_type = artifact.archive_type()?;

            Ok(ResolvedInstall {
                manifest,
                artifact,
                resolved_target: resolved_target.clone(),
                archive_type,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok((resolved, resolved_dependency_tokens))
}

fn install_resolved(
    layout: &PrefixLayout,
    resolved: &ResolvedInstall,
    dependency_receipts: &[String],
    root_names: &[String],
    planned_dependency_overrides: &HashMap<String, Vec<String>>,
    snapshot_id: Option<&str>,
    force_redownload: bool,
) -> Result<InstallOutcome> {
    let receipts = read_install_receipts(layout)?;
    let replacement_receipts = collect_replacement_receipts(&resolved.manifest, &receipts)?;
    let replacement_targets = replacement_receipts
        .iter()
        .map(|receipt| receipt.name.as_str())
        .collect::<HashSet<_>>();

    let exposed_bins = collect_declared_binaries(&resolved.artifact)?;
    let declared_completions = collect_declared_completions(&resolved.artifact)?;
    let projected_completion_paths = declared_completions
        .iter()
        .map(|completion| {
            projected_exposed_completion_path(
                &resolved.manifest.name,
                completion.shell,
                &completion.path,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    validate_binary_preflight(
        layout,
        &resolved.manifest.name,
        &exposed_bins,
        &receipts,
        &replacement_targets,
    )?;
    validate_completion_preflight(
        layout,
        &resolved.manifest.name,
        &projected_completion_paths,
        &receipts,
    )?;

    let cache_path = layout.artifact_cache_path(
        &resolved.manifest.name,
        &resolved.manifest.version.to_string(),
        &resolved.resolved_target,
        resolved.archive_type,
    );
    let download_status = download_artifact(&resolved.artifact.url, &cache_path, force_redownload)?;

    let checksum_ok = verify_sha256_file(&cache_path, &resolved.artifact.sha256)?;
    if !checksum_ok {
        let _ = remove_file_if_exists(&cache_path);
        return Err(anyhow!(
            "sha256 mismatch for {} (expected {})",
            cache_path.display(),
            resolved.artifact.sha256
        ));
    }

    let install_root = install_from_artifact(
        layout,
        &resolved.manifest.name,
        &resolved.manifest.version.to_string(),
        &cache_path,
        resolved.archive_type,
        resolved.artifact.strip_components.unwrap_or(0),
        resolved.artifact.artifact_root.as_deref(),
    )?;

    if let Err(err) =
        apply_replacement_handoff(layout, &replacement_receipts, planned_dependency_overrides)
    {
        let _ = std::fs::remove_dir_all(&install_root);
        return Err(err);
    }

    let receipts = read_install_receipts(layout)?;

    for binary in &resolved.artifact.binaries {
        expose_binary(layout, &install_root, &binary.name, &binary.path)?;
    }

    let mut exposed_completions = Vec::with_capacity(declared_completions.len());
    for completion in &declared_completions {
        let storage_path = expose_completion(
            layout,
            &install_root,
            &resolved.manifest.name,
            completion.shell,
            &completion.path,
        )?;
        exposed_completions.push(storage_path);
    }

    if let Some(previous_receipt) = receipts
        .iter()
        .find(|receipt| receipt.name == resolved.manifest.name)
    {
        for stale_bin in previous_receipt
            .exposed_bins
            .iter()
            .filter(|old| !exposed_bins.contains(old))
        {
            remove_exposed_binary(layout, stale_bin)?;
        }
        for stale_completion in previous_receipt
            .exposed_completions
            .iter()
            .filter(|old| !exposed_completions.contains(old))
        {
            remove_exposed_completion(layout, stale_completion)?;
        }
    }

    let receipt = InstallReceipt {
        name: resolved.manifest.name.clone(),
        version: resolved.manifest.version.to_string(),
        dependencies: dependency_receipts.to_vec(),
        target: Some(resolved.resolved_target.clone()),
        artifact_url: Some(resolved.artifact.url.clone()),
        artifact_sha256: Some(resolved.artifact.sha256.clone()),
        cache_path: Some(cache_path.display().to_string()),
        exposed_bins: exposed_bins.clone(),
        exposed_completions: exposed_completions.clone(),
        snapshot_id: snapshot_id.map(ToOwned::to_owned),
        install_reason: determine_install_reason(
            &resolved.manifest.name,
            root_names,
            &receipts,
            &replacement_receipts,
        ),
        install_status: "installed".to_string(),
        installed_at_unix: current_unix_timestamp()?,
    };
    let receipt_path = write_install_receipt(layout, &receipt)?;

    Ok(InstallOutcome {
        name: resolved.manifest.name.clone(),
        version: resolved.manifest.version.to_string(),
        resolved_target: resolved.resolved_target.clone(),
        archive_type: resolved.archive_type,
        artifact_url: resolved.artifact.url.clone(),
        cache_path,
        download_status,
        install_root,
        receipt_path,
        exposed_bins,
        exposed_completions,
    })
}

fn print_install_outcome(outcome: &InstallOutcome) {
    println!(
        "resolved {} {} for {}",
        outcome.name, outcome.version, outcome.resolved_target
    );
    println!("archive: {}", outcome.archive_type.as_str());
    println!("artifact: {}", outcome.artifact_url);
    println!(
        "cache: {} ({})",
        outcome.cache_path.display(),
        outcome.download_status
    );
    println!("install_root: {}", outcome.install_root.display());
    if !outcome.exposed_bins.is_empty() {
        println!("exposed_bins: {}", outcome.exposed_bins.join(", "));
    }
    if !outcome.exposed_completions.is_empty() {
        println!(
            "exposed_completions: {}",
            outcome.exposed_completions.join(", ")
        );
    }
    println!("receipt: {}", outcome.receipt_path.display());
}

fn collect_declared_binaries(artifact: &Artifact) -> Result<Vec<String>> {
    let mut names = Vec::with_capacity(artifact.binaries.len());
    let mut seen = HashSet::new();
    for binary in &artifact.binaries {
        validate_binary_name(&binary.name)?;
        if !seen.insert(binary.name.clone()) {
            return Err(anyhow!(
                "duplicate binary declaration '{}' for target '{}'",
                binary.name,
                artifact.target
            ));
        }
        names.push(binary.name.clone());
    }
    Ok(names)
}

#[derive(Debug, Clone)]
struct DeclaredCompletion {
    shell: ArtifactCompletionShell,
    path: String,
}

fn collect_declared_completions(artifact: &Artifact) -> Result<Vec<DeclaredCompletion>> {
    let mut declared = Vec::with_capacity(artifact.completions.len());
    let mut seen = HashSet::new();
    for completion in &artifact.completions {
        let key = (completion.shell, completion.path.clone());
        if !seen.insert(key) {
            return Err(anyhow!(
                "duplicate completion declaration for shell '{}' and path '{}' in target '{}'",
                completion.shell.as_str(),
                completion.path,
                artifact.target
            ));
        }
        declared.push(DeclaredCompletion {
            shell: completion.shell,
            path: completion.path.clone(),
        });
    }
    Ok(declared)
}

fn validate_binary_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(anyhow!("binary name must not be empty"));
    }
    if name.contains('/') || name.contains('\\') {
        return Err(anyhow!(
            "binary name must not contain path separators: {name}"
        ));
    }
    Ok(())
}

fn validate_completion_preflight(
    layout: &PrefixLayout,
    package_name: &str,
    desired_completion_paths: &[String],
    receipts: &[InstallReceipt],
) -> Result<()> {
    let owned_by_self: HashSet<&str> = receipts
        .iter()
        .find(|receipt| receipt.name == package_name)
        .map(|receipt| {
            receipt
                .exposed_completions
                .iter()
                .map(String::as_str)
                .collect()
        })
        .unwrap_or_default();

    for desired in desired_completion_paths {
        for receipt in receipts {
            if receipt.name == package_name {
                continue;
            }
            if receipt
                .exposed_completions
                .iter()
                .any(|owned| owned == desired)
            {
                return Err(anyhow!(
                    "completion '{}' is already owned by package '{}'",
                    desired,
                    receipt.name
                ));
            }
        }

        let path = exposed_completion_path(layout, desired)?;
        if path.exists() && !owned_by_self.contains(desired.as_str()) {
            return Err(anyhow!(
                "completion '{}' at {} already exists and is not managed by crosspack",
                desired,
                path.display()
            ));
        }
    }

    Ok(())
}

fn collect_replacement_receipts(
    manifest: &PackageManifest,
    receipts: &[InstallReceipt],
) -> Result<Vec<InstallReceipt>> {
    let mut matched = receipts
        .iter()
        .filter_map(|receipt| {
            let requirement = manifest.replaces.get(&receipt.name)?;
            let installed = Version::parse(&receipt.version).ok()?;
            requirement.matches(&installed).then_some(receipt.clone())
        })
        .collect::<Vec<_>>();

    for receipt in receipts {
        if manifest.replaces.contains_key(&receipt.name) {
            Version::parse(&receipt.version).with_context(|| {
                format!(
                    "installed receipt for '{}' has invalid version for replacement preflight: {}",
                    receipt.name, receipt.version
                )
            })?;
        }
    }

    matched.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(matched)
}

fn apply_replacement_handoff(
    layout: &PrefixLayout,
    replacement_receipts: &[InstallReceipt],
    planned_dependency_overrides: &HashMap<String, Vec<String>>,
) -> Result<()> {
    let replacement_root_names = replacement_receipts
        .iter()
        .filter(|receipt| receipt.install_reason == InstallReason::Root)
        .map(|receipt| receipt.name.clone())
        .collect::<HashSet<_>>();

    for replacement in replacement_receipts {
        let blocked_by_roots =
            uninstall_blocked_by_roots_with_dependency_overrides_and_ignored_roots(
                layout,
                &replacement.name,
                planned_dependency_overrides,
                &replacement_root_names,
            )?;
        if !blocked_by_roots.is_empty() {
            return Err(anyhow!(
                "cannot replace '{}' {}: still required by roots {}",
                replacement.name,
                replacement.version,
                blocked_by_roots.join(", ")
            ));
        }
    }

    for replacement in replacement_receipts {
        let result = uninstall_package_with_dependency_overrides_and_ignored_roots(
            layout,
            &replacement.name,
            planned_dependency_overrides,
            &replacement_root_names,
        )?;
        if result.status == UninstallStatus::BlockedByDependents {
            return Err(anyhow!(
                "cannot replace '{}' {}: still required by roots {}",
                replacement.name,
                replacement.version,
                result.blocked_by_roots.join(", ")
            ));
        }
    }

    Ok(())
}

fn validate_binary_preflight(
    layout: &PrefixLayout,
    package_name: &str,
    desired_bins: &[String],
    receipts: &[InstallReceipt],
    replacement_targets: &HashSet<&str>,
) -> Result<()> {
    let owned_by_self: HashSet<&str> = receipts
        .iter()
        .find(|receipt| receipt.name == package_name)
        .map(|receipt| receipt.exposed_bins.iter().map(String::as_str).collect())
        .unwrap_or_default();

    let owned_by_replacements: HashSet<&str> = receipts
        .iter()
        .filter(|receipt| replacement_targets.contains(receipt.name.as_str()))
        .flat_map(|receipt| receipt.exposed_bins.iter().map(String::as_str))
        .collect();

    for desired in desired_bins {
        for receipt in receipts {
            if receipt.name == package_name || replacement_targets.contains(receipt.name.as_str()) {
                continue;
            }
            if receipt.exposed_bins.iter().any(|bin| bin == desired) {
                return Err(anyhow!(
                    "binary '{}' is already owned by package '{}'",
                    desired,
                    receipt.name
                ));
            }
        }

        let path = bin_path(layout, desired);
        if path.exists()
            && !owned_by_self.contains(desired.as_str())
            && !owned_by_replacements.contains(desired.as_str())
        {
            return Err(anyhow!(
                "binary '{}' at {} already exists and is not managed by crosspack",
                desired,
                path.display()
            ));
        }
    }

    Ok(())
}

fn build_dependency_receipts(
    resolved: &ResolvedInstall,
    selected: &[ResolvedInstall],
) -> Vec<String> {
    let mut deps = resolved
        .manifest
        .dependencies
        .keys()
        .filter_map(|name| {
            selected
                .iter()
                .find(|candidate| candidate.manifest.name == *name)
                .map(|candidate| {
                    format!("{}@{}", candidate.manifest.name, candidate.manifest.version)
                })
        })
        .collect::<Vec<_>>();
    deps.sort();
    deps
}

fn build_planned_dependency_overrides(
    selected: &[ResolvedInstall],
) -> HashMap<String, Vec<String>> {
    selected
        .iter()
        .map(|package| {
            let mut dependencies = package
                .manifest
                .dependencies
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            dependencies.sort();
            dependencies.dedup();
            (package.manifest.name.clone(), dependencies)
        })
        .collect()
}

fn determine_install_reason(
    package_name: &str,
    root_names: &[String],
    existing_receipts: &[InstallReceipt],
    replacement_receipts: &[InstallReceipt],
) -> InstallReason {
    if root_names.iter().any(|root| root == package_name) {
        return InstallReason::Root;
    }

    let promotes_from_replacement_root = replacement_receipts
        .iter()
        .any(|receipt| receipt.install_reason == InstallReason::Root);

    if let Some(existing) = existing_receipts
        .iter()
        .find(|receipt| receipt.name == package_name)
    {
        if promotes_from_replacement_root {
            return InstallReason::Root;
        }
        return existing.install_reason.clone();
    }

    if promotes_from_replacement_root {
        return InstallReason::Root;
    }

    InstallReason::Dependency
}

#[cfg(test)]
fn build_upgrade_roots(receipts: &[InstallReceipt]) -> Vec<RootInstallRequest> {
    receipts
        .iter()
        .filter(|receipt| receipt.install_reason == InstallReason::Root)
        .map(|receipt| RootInstallRequest {
            name: receipt.name.clone(),
            requirement: VersionReq::STAR,
        })
        .collect()
}

fn build_upgrade_plans(receipts: &[InstallReceipt]) -> Vec<UpgradePlan> {
    let mut grouped_roots: BTreeMap<Option<String>, Vec<String>> = BTreeMap::new();

    for receipt in receipts {
        if receipt.install_reason != InstallReason::Root {
            continue;
        }
        grouped_roots
            .entry(receipt.target.clone())
            .or_default()
            .push(receipt.name.clone());
    }

    grouped_roots
        .into_iter()
        .map(|(target, mut root_names)| {
            root_names.sort();
            root_names.dedup();

            let roots = root_names
                .iter()
                .map(|name| RootInstallRequest {
                    name: name.clone(),
                    requirement: VersionReq::STAR,
                })
                .collect::<Vec<_>>();

            UpgradePlan {
                target,
                roots,
                root_names,
            }
        })
        .collect()
}

fn enforce_disjoint_multi_target_upgrade(
    resolved_by_target: &[(Option<&str>, Vec<String>)],
) -> Result<()> {
    let mut package_targets = BTreeMap::new();

    for (target, packages) in resolved_by_target {
        let target_name = target.unwrap_or("host-default").to_string();
        for package in packages {
            if let Some(previous_target) =
                package_targets.insert(package.clone(), target_name.clone())
            {
                if previous_target != target_name {
                    return Err(anyhow!(
                        "upgrade cannot safely process package '{}' across multiple targets ({} and {}); install state is currently keyed by package name. Use separate prefixes for cross-target installs.",
                        package,
                        previous_target,
                        target_name
                    ));
                }
            }
        }
    }

    Ok(())
}

fn format_uninstall_messages(result: &UninstallResult) -> Vec<String> {
    let version = result.version.as_deref().unwrap_or("unknown");
    let mut lines = match result.status {
        UninstallStatus::NotInstalled => vec![format!("{} is not installed", result.name)],
        UninstallStatus::Uninstalled => vec![format!("uninstalled {} {}", result.name, version)],
        UninstallStatus::RepairedStaleState => vec![format!(
            "removed stale state for {} {} (package files already missing)",
            result.name, version
        )],
        UninstallStatus::BlockedByDependents => vec![format!(
            "cannot uninstall {} {}: still required by roots {}",
            result.name,
            version,
            result.blocked_by_roots.join(", ")
        )],
    };

    if !result.pruned_dependencies.is_empty() {
        lines.push(format!(
            "pruned orphan dependencies: {}",
            result.pruned_dependencies.join(", ")
        ));
    }

    lines
}

fn enforce_no_downgrades(
    receipts: &[InstallReceipt],
    resolved: &[ResolvedInstall],
    operation: &str,
) -> Result<()> {
    for receipt in receipts {
        let Some(candidate) = resolved
            .iter()
            .find(|entry| entry.manifest.name == receipt.name)
        else {
            continue;
        };

        let current = Version::parse(&receipt.version).with_context(|| {
            format!(
                "installed receipt for '{}' has invalid version: {}",
                receipt.name, receipt.version
            )
        })?;
        if candidate.manifest.version < current {
            return Err(anyhow!(
                "{} would downgrade '{}' from {} to {}; run `crosspack install '{}@={}'` to perform an explicit downgrade",
                operation,
                receipt.name,
                receipt.version,
                candidate.manifest.version,
                receipt.name,
                candidate.manifest.version
            ));
        }
    }
    Ok(())
}

fn host_target_triple() -> &'static str {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("x86_64", "linux") => "x86_64-unknown-linux-gnu",
        ("aarch64", "linux") => "aarch64-unknown-linux-gnu",
        ("x86_64", "macos") => "x86_64-apple-darwin",
        ("aarch64", "macos") => "aarch64-apple-darwin",
        ("x86_64", "windows") => "x86_64-pc-windows-msvc",
        ("aarch64", "windows") => "aarch64-pc-windows-msvc",
        _ => "unknown-unknown-unknown",
    }
}

fn download_artifact(url: &str, cache_path: &Path, force_redownload: bool) -> Result<&'static str> {
    if cache_path.exists() && !force_redownload {
        return Ok("cache-hit");
    }

    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create cache dir: {}", parent.display()))?;
    }

    let part_path = cache_path.with_file_name(format!(
        "{}.part",
        cache_path
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("artifact")
    ));

    let result = if cfg!(windows) {
        download_with_powershell(url, &part_path)
    } else {
        download_with_curl(url, &part_path).or_else(|_| download_with_wget(url, &part_path))
    };

    if let Err(err) = result {
        let _ = std::fs::remove_file(&part_path);
        return Err(err);
    }

    if cache_path.exists() {
        std::fs::remove_file(cache_path)
            .with_context(|| format!("failed to replace cache file: {}", cache_path.display()))?;
    }
    std::fs::rename(&part_path, cache_path).with_context(|| {
        format!(
            "failed to move downloaded artifact into cache: {}",
            cache_path.display()
        )
    })?;

    Ok("downloaded")
}

fn download_with_curl(url: &str, out_path: &Path) -> Result<()> {
    let mut command = Command::new("curl");
    command
        .arg("-fL")
        .arg("--retry")
        .arg("2")
        .arg("-o")
        .arg(out_path)
        .arg(url);
    run_command(&mut command, "curl download failed")
}

fn download_with_wget(url: &str, out_path: &Path) -> Result<()> {
    let mut command = Command::new("wget");
    command.arg("-O").arg(out_path).arg(url);
    run_command(&mut command, "wget download failed")
}

fn download_with_powershell(url: &str, out_path: &Path) -> Result<()> {
    let mut command = Command::new("powershell");
    command.arg("-NoProfile").arg("-Command").arg(format!(
        "Invoke-WebRequest -Uri '{}' -OutFile '{}'",
        escape_ps_single_quote(url),
        escape_ps_single_quote_path(out_path)
    ));
    run_command(&mut command, "powershell download failed")
}

fn run_command(command: &mut Command, context_message: &str) -> Result<()> {
    let output = command
        .output()
        .with_context(|| format!("{context_message}: command failed to start"))?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    Err(anyhow!(
        "{context_message}: status={} stdout='{}' stderr='{}'",
        output.status,
        stdout.trim(),
        stderr.trim()
    ))
}

fn escape_ps_single_quote(value: &str) -> String {
    value.replace('\'', "''")
}

fn escape_ps_single_quote_path(path: &Path) -> String {
    let mut os = OsString::new();
    os.push(path.as_os_str());
    os.to_string_lossy().replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::{
        apply_provider_override, apply_replacement_handoff, begin_transaction, build_update_report,
        build_upgrade_plans, build_upgrade_roots, collect_replacement_receipts,
        current_unix_timestamp, determine_install_reason, doctor_transaction_health_line,
        enforce_disjoint_multi_target_upgrade, enforce_no_downgrades, ensure_no_active_transaction,
        ensure_no_active_transaction_for, ensure_update_succeeded, ensure_upgrade_command_ready,
        execute_with_transaction, format_info_lines, format_registry_add_lines,
        format_registry_list_lines, format_registry_list_snapshot_state,
        format_registry_remove_lines, format_uninstall_messages, format_update_summary_line,
        normalize_command_token, parse_pin_spec, parse_provider_overrides, registry_state_root,
        resolve_init_shell, resolve_transaction_snapshot_id, run_repair_command,
        run_rollback_command, run_uninstall_command, run_update_command, run_upgrade_command,
        select_manifest_with_pin, select_metadata_backend, set_transaction_status,
        update_failure_reason_code, validate_binary_preflight, validate_completion_preflight,
        validate_provider_overrides_used, write_completions_script, Cli, CliCompletionShell,
        CliRegistryKind, Commands, MetadataBackend, ResolvedInstall,
    };
    use clap::Parser;
    use crosspack_core::{ArchiveType, PackageManifest};
    use crosspack_installer::{
        append_transaction_journal_entry, bin_path, expose_binary, exposed_completion_path,
        read_active_transaction, read_install_receipts, read_transaction_metadata,
        set_active_transaction, write_install_receipt, write_transaction_metadata, InstallReason,
        InstallReceipt, PrefixLayout, TransactionJournalEntry, TransactionMetadata,
        UninstallResult, UninstallStatus,
    };
    use crosspack_registry::{
        RegistrySourceKind, RegistrySourceRecord, RegistrySourceSnapshotState, RegistrySourceStore,
        RegistrySourceWithSnapshotState, RegistrySourceWithSnapshotStatus, SourceUpdateResult,
        SourceUpdateStatus,
    };
    use semver::VersionReq;
    use std::collections::{BTreeMap, HashMap, HashSet};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn begin_transaction_writes_planning_metadata_and_active_marker() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let tx = begin_transaction(
            &layout,
            "install",
            Some("git:5f1b3d8a1f2a4d0e"),
            1_771_001_234,
        )
        .expect("must start transaction");

        assert_eq!(tx.operation, "install");
        assert_eq!(tx.status, "planning");
        assert_eq!(tx.snapshot_id.as_deref(), Some("git:5f1b3d8a1f2a4d0e"));

        let active =
            std::fs::read_to_string(layout.transaction_active_path()).expect("must read active");
        assert_eq!(active.trim(), tx.txid);

        let metadata = std::fs::read_to_string(layout.transaction_metadata_path(&tx.txid))
            .expect("must read metadata");
        assert!(metadata.contains("\"status\": \"planning\""));
        assert!(metadata.contains("\"operation\": \"install\""));

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn begin_transaction_cleans_up_metadata_when_active_claim_fails() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        set_active_transaction(&layout, "tx-existing").expect("must seed existing active marker");

        let started_at_unix = 1_771_001_256;
        let expected_txid = format!("tx-{started_at_unix}-{}", std::process::id());
        let err = begin_transaction(&layout, "install", None, started_at_unix)
            .expect_err("existing active marker should block transaction start");
        assert!(
            err.to_string()
                .contains("active transaction marker already exists (txid=tx-existing)"),
            "unexpected error: {err}"
        );

        assert!(
            !layout.transaction_metadata_path(&expected_txid).exists(),
            "metadata file should be cleaned up when active claim fails"
        );
        assert!(
            !layout.transaction_staging_path(&expected_txid).exists(),
            "staging dir should be cleaned up when active claim fails"
        );

        assert_eq!(
            read_active_transaction(&layout)
                .expect("must read active transaction")
                .as_deref(),
            Some("tx-existing"),
            "existing active marker should remain unchanged"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_reports_unreadable_active_marker() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        std::fs::create_dir_all(layout.transaction_active_path())
            .expect("must create unreadable active marker fixture");

        let err = ensure_no_active_transaction(&layout)
            .expect_err("unreadable active marker should return repair-required reason");
        let expected = format!(
            "transaction state requires repair (reason=active_marker_unreadable path={})",
            layout.transaction_active_path().display()
        );
        assert!(
            err.to_string().contains(&expected),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_upgrade_command_ready_reports_preflight_context_when_transaction_active() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-blocked-upgrade-command".to_string(),
            operation: "install".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_258,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-blocked-upgrade-command")
            .expect("must write active marker");

        let err = ensure_upgrade_command_ready(&layout)
            .expect_err("active transaction should block upgrade preflight");
        assert!(
            err.to_string().contains(
                "cannot upgrade (reason=active_transaction command=upgrade): transaction tx-blocked-upgrade-command requires repair (reason=failed)"
            ),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_upgrade_command_reports_preflight_context_when_transaction_active() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-blocked-upgrade-dispatch".to_string(),
            operation: "install".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_258,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-blocked-upgrade-dispatch")
            .expect("must write active marker");

        let err = run_upgrade_command(&layout, None, None, &BTreeMap::new())
            .expect_err("active transaction should block upgrade command");
        assert!(
            err.to_string().contains(
                "cannot upgrade (reason=active_transaction command=upgrade): transaction tx-blocked-upgrade-dispatch requires repair (reason=failed)"
            ),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_uninstall_command_reports_preflight_context_when_transaction_active() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-blocked-uninstall-command".to_string(),
            operation: "upgrade".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_259,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-blocked-uninstall-command")
            .expect("must write active marker");

        let err = run_uninstall_command(&layout, "ripgrep".to_string())
            .expect_err("active transaction should block uninstall command");
        assert!(
            err.to_string().contains(
                "cannot uninstall (reason=active_transaction command=uninstall): transaction tx-blocked-uninstall-command requires repair (reason=failed)"
            ),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_rollback_command_transitions_active_transaction_to_rolled_back() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-needs-rollback".to_string(),
            operation: "upgrade".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_262,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-needs-rollback").expect("must write active marker");

        let snapshot_root = layout
            .transaction_staging_path("tx-needs-rollback")
            .join("rollback")
            .join("demo");
        std::fs::create_dir_all(snapshot_root.join("package"))
            .expect("must create snapshot package dir");
        std::fs::create_dir_all(snapshot_root.join("receipt"))
            .expect("must create snapshot receipt dir");
        std::fs::create_dir_all(snapshot_root.join("bins")).expect("must create snapshot bins dir");
        std::fs::write(
            snapshot_root.join("manifest.txt"),
            "package_exists=0\nreceipt_exists=0\n",
        )
        .expect("must write snapshot manifest");
        append_transaction_journal_entry(
            &layout,
            "tx-needs-rollback",
            &TransactionJournalEntry {
                seq: 1,
                step: "backup_package_state:demo".to_string(),
                state: "done".to_string(),
                path: Some(snapshot_root.display().to_string()),
            },
        )
        .expect("must append backup journal step");
        append_transaction_journal_entry(
            &layout,
            "tx-needs-rollback",
            &TransactionJournalEntry {
                seq: 2,
                step: "upgrade_package:demo".to_string(),
                state: "done".to_string(),
                path: Some("demo".to_string()),
            },
        )
        .expect("must append mutating journal step");

        run_rollback_command(&layout, None).expect("rollback command must succeed");

        let updated = read_transaction_metadata(&layout, "tx-needs-rollback")
            .expect("must read updated metadata")
            .expect("metadata should still exist");
        assert_eq!(updated.status, "rolled_back");
        assert_eq!(
            read_active_transaction(&layout).expect("must read active marker"),
            None,
            "rollback should clear active transaction marker"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_repair_command_recovers_failed_active_transaction() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-needs-repair".to_string(),
            operation: "install".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_263,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-needs-repair").expect("must write active marker");

        let snapshot_root = layout
            .transaction_staging_path("tx-needs-repair")
            .join("rollback")
            .join("demo");
        std::fs::create_dir_all(snapshot_root.join("package"))
            .expect("must create snapshot package dir");
        std::fs::create_dir_all(snapshot_root.join("receipt"))
            .expect("must create snapshot receipt dir");
        std::fs::create_dir_all(snapshot_root.join("bins")).expect("must create snapshot bins dir");
        std::fs::write(
            snapshot_root.join("manifest.txt"),
            "package_exists=0\nreceipt_exists=0\n",
        )
        .expect("must write snapshot manifest");
        append_transaction_journal_entry(
            &layout,
            "tx-needs-repair",
            &TransactionJournalEntry {
                seq: 1,
                step: "backup_package_state:demo".to_string(),
                state: "done".to_string(),
                path: Some(snapshot_root.display().to_string()),
            },
        )
        .expect("must append backup step");
        append_transaction_journal_entry(
            &layout,
            "tx-needs-repair",
            &TransactionJournalEntry {
                seq: 2,
                step: "install_package:demo".to_string(),
                state: "done".to_string(),
                path: Some("demo".to_string()),
            },
        )
        .expect("must append mutating step");

        run_repair_command(&layout).expect("repair command must succeed");

        let updated = read_transaction_metadata(&layout, "tx-needs-repair")
            .expect("must read updated metadata")
            .expect("metadata should still exist");
        assert_eq!(updated.status, "rolled_back");
        assert_eq!(
            read_active_transaction(&layout).expect("must read active marker"),
            None,
            "repair should clear active marker for recovered tx"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_repair_command_recovers_active_applying_transaction() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-applying-repair".to_string(),
            operation: "install".to_string(),
            status: "applying".to_string(),
            started_at_unix: 1_771_001_265,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-applying-repair").expect("must write active marker");

        let snapshot_root = layout
            .transaction_staging_path("tx-applying-repair")
            .join("rollback")
            .join("demo");
        std::fs::create_dir_all(snapshot_root.join("package"))
            .expect("must create snapshot package dir");
        std::fs::create_dir_all(snapshot_root.join("receipt"))
            .expect("must create snapshot receipt dir");
        std::fs::create_dir_all(snapshot_root.join("bins")).expect("must create snapshot bins dir");
        std::fs::write(
            snapshot_root.join("manifest.txt"),
            "package_exists=0\nreceipt_exists=0\n",
        )
        .expect("must write snapshot manifest");
        append_transaction_journal_entry(
            &layout,
            "tx-applying-repair",
            &TransactionJournalEntry {
                seq: 1,
                step: "backup_package_state:demo".to_string(),
                state: "done".to_string(),
                path: Some(snapshot_root.display().to_string()),
            },
        )
        .expect("must append backup step");
        append_transaction_journal_entry(
            &layout,
            "tx-applying-repair",
            &TransactionJournalEntry {
                seq: 2,
                step: "install_package:demo".to_string(),
                state: "done".to_string(),
                path: Some("demo".to_string()),
            },
        )
        .expect("must append mutating step");

        run_repair_command(&layout).expect("repair must recover active applying tx");

        let updated = read_transaction_metadata(&layout, "tx-applying-repair")
            .expect("must read updated metadata")
            .expect("metadata should still exist");
        assert_eq!(updated.status, "rolled_back");
        assert_eq!(
            read_active_transaction(&layout).expect("must read active marker"),
            None,
            "repair should clear active marker after recovery"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_rollback_command_fails_when_journal_replay_required() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-needs-replay".to_string(),
            operation: "install".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_266,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-needs-replay").expect("must write active marker");

        std::fs::write(
            layout.transaction_journal_path("tx-needs-replay"),
            r#"{"seq":1,"step":"install_package:demo","state":"done"}"#,
        )
        .expect("must write journal fixture");

        let err = run_rollback_command(&layout, Some("tx-needs-replay".to_string()))
            .expect_err("rollback should fail when replay is required");
        assert!(
            err.to_string().contains("rollback failed tx-needs-replay"),
            "unexpected error: {err}"
        );

        let active = read_active_transaction(&layout).expect("must read active marker");
        assert_eq!(active.as_deref(), Some("tx-needs-replay"));
        let updated = read_transaction_metadata(&layout, "tx-needs-replay")
            .expect("must read metadata")
            .expect("metadata should still exist");
        assert_eq!(updated.status, "failed");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_rollback_command_replays_compensating_steps_and_restores_filesystem_state() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let txid = "tx-replay-filesystem";
        let metadata = TransactionMetadata {
            version: 1,
            txid: txid.to_string(),
            operation: "install".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_266,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, txid).expect("must write active marker");

        let package_name = "demo";
        let previous_pkg_file = layout
            .pkgs_dir()
            .join(package_name)
            .join("1.0.0")
            .join("old.txt");
        std::fs::create_dir_all(previous_pkg_file.parent().expect("must resolve parent"))
            .expect("must create old package path");
        std::fs::write(&previous_pkg_file, "old-state").expect("must write old package marker");

        let previous_receipt = InstallReceipt {
            name: package_name.to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: Some("x86_64-unknown-linux-gnu".to_string()),
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["demo".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        };
        write_install_receipt(&layout, &previous_receipt).expect("must write previous receipt");
        std::fs::write(bin_path(&layout, "demo"), "old-bin").expect("must write old binary");

        let snapshot_root = layout
            .transaction_staging_path(txid)
            .join("rollback")
            .join(package_name);
        std::fs::create_dir_all(snapshot_root.join("package").join("1.0.0"))
            .expect("must create snapshot package dir");
        std::fs::create_dir_all(snapshot_root.join("receipt"))
            .expect("must create snapshot receipts");
        std::fs::create_dir_all(snapshot_root.join("bins")).expect("must create snapshot bins");
        std::fs::copy(
            layout
                .pkgs_dir()
                .join(package_name)
                .join("1.0.0")
                .join("old.txt"),
            snapshot_root.join("package").join("1.0.0").join("old.txt"),
        )
        .expect("must copy package fixture into snapshot");
        std::fs::copy(
            layout.receipt_path(package_name),
            snapshot_root
                .join("receipt")
                .join(format!("{package_name}.receipt")),
        )
        .expect("must copy receipt fixture into snapshot");
        std::fs::copy(
            bin_path(&layout, "demo"),
            snapshot_root.join("bins").join("demo"),
        )
        .expect("must copy bin fixture into snapshot");
        std::fs::write(
            snapshot_root.join("manifest.txt"),
            "package_exists=1\nreceipt_exists=1\nbin=demo\n",
        )
        .expect("must write snapshot manifest");

        append_transaction_journal_entry(
            &layout,
            txid,
            &TransactionJournalEntry {
                seq: 1,
                step: format!("backup_package_state:{package_name}"),
                state: "done".to_string(),
                path: Some(snapshot_root.display().to_string()),
            },
        )
        .expect("must append backup step");

        std::fs::remove_file(bin_path(&layout, "demo")).expect("must remove old binary");
        std::fs::remove_file(layout.receipt_path(package_name)).expect("must remove old receipt");
        std::fs::remove_dir_all(layout.pkgs_dir().join(package_name))
            .expect("must remove old package state");
        std::fs::create_dir_all(layout.pkgs_dir().join(package_name).join("2.0.0"))
            .expect("must create new package state");
        std::fs::write(
            layout
                .pkgs_dir()
                .join(package_name)
                .join("2.0.0")
                .join("new.txt"),
            "new-state",
        )
        .expect("must write new package marker");
        let new_receipt = InstallReceipt {
            name: package_name.to_string(),
            version: "2.0.0".to_string(),
            dependencies: Vec::new(),
            target: Some("x86_64-unknown-linux-gnu".to_string()),
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["demo".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 2,
        };
        write_install_receipt(&layout, &new_receipt).expect("must write new receipt");
        std::fs::write(bin_path(&layout, "demo"), "new-bin").expect("must write new binary");

        append_transaction_journal_entry(
            &layout,
            txid,
            &TransactionJournalEntry {
                seq: 2,
                step: format!("install_package:{package_name}"),
                state: "done".to_string(),
                path: Some(package_name.to_string()),
            },
        )
        .expect("must append mutating step");

        run_rollback_command(&layout, Some(txid.to_string()))
            .expect("rollback command should replay journal and succeed");

        let updated = read_transaction_metadata(&layout, txid)
            .expect("must read updated metadata")
            .expect("metadata should still exist");
        assert_eq!(updated.status, "rolled_back");
        assert!(
            read_active_transaction(&layout)
                .expect("must read active marker")
                .is_none(),
            "rollback should clear active transaction marker"
        );
        assert!(
            layout
                .pkgs_dir()
                .join(package_name)
                .join("1.0.0")
                .join("old.txt")
                .exists(),
            "rollback should restore previous package tree"
        );
        assert!(
            !layout
                .pkgs_dir()
                .join(package_name)
                .join("2.0.0")
                .join("new.txt")
                .exists(),
            "rollback should remove interrupted package tree"
        );
        let restored_receipt = read_install_receipts(&layout).expect("must load receipts");
        let restored = restored_receipt
            .iter()
            .find(|receipt| receipt.name == package_name)
            .expect("previous receipt must be restored");
        assert_eq!(restored.version, "1.0.0");
        assert_eq!(
            std::fs::read_to_string(bin_path(&layout, "demo")).expect("must read restored binary"),
            "old-bin"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_repair_command_recovers_interrupted_statuses_when_rollback_possible() {
        for status in ["planning", "applying", "rolling_back", "failed"] {
            let layout = test_layout();
            layout.ensure_base_dirs().expect("must create dirs");

            let txid = format!("tx-repair-{}", status.replace('_', "-"));
            let metadata = TransactionMetadata {
                version: 1,
                txid: txid.clone(),
                operation: "install".to_string(),
                status: status.to_string(),
                started_at_unix: 1_771_001_267,
                snapshot_id: None,
            };
            write_transaction_metadata(&layout, &metadata).expect("must write metadata");
            set_active_transaction(&layout, &txid).expect("must set active marker");

            let package_name = format!("pkg-{status}");
            let snapshot_root = layout
                .transaction_staging_path(&txid)
                .join("rollback")
                .join(&package_name);
            std::fs::create_dir_all(snapshot_root.join("package"))
                .expect("must create snapshot package directory");
            std::fs::create_dir_all(snapshot_root.join("receipt"))
                .expect("must create snapshot receipt directory");
            std::fs::create_dir_all(snapshot_root.join("bins"))
                .expect("must create snapshot bins directory");
            std::fs::write(snapshot_root.join("manifest.txt"), "")
                .expect("must create placeholder snapshot manifest");

            std::fs::create_dir_all(layout.pkgs_dir().join(&package_name).join("9.9.9"))
                .expect("must create interrupted package dir");
            std::fs::write(
                layout
                    .pkgs_dir()
                    .join(&package_name)
                    .join("9.9.9")
                    .join("partial.txt"),
                "interrupted",
            )
            .expect("must write interrupted package marker");

            append_transaction_journal_entry(
                &layout,
                &txid,
                &TransactionJournalEntry {
                    seq: 1,
                    step: format!("backup_package_state:{package_name}"),
                    state: "done".to_string(),
                    path: Some(snapshot_root.display().to_string()),
                },
            )
            .expect("must append backup step");
            append_transaction_journal_entry(
                &layout,
                &txid,
                &TransactionJournalEntry {
                    seq: 2,
                    step: format!("install_package:{package_name}"),
                    state: "done".to_string(),
                    path: Some(package_name.clone()),
                },
            )
            .expect("must append interrupted step");

            run_repair_command(&layout)
                .expect("repair should recover interrupted transaction by rollback replay");

            let updated = read_transaction_metadata(&layout, &txid)
                .expect("must read updated metadata")
                .expect("metadata should exist");
            assert_eq!(updated.status, "rolled_back", "status={status}");
            assert!(
                read_active_transaction(&layout)
                    .expect("must read active transaction")
                    .is_none(),
                "status={status}: active marker should be cleared"
            );
            assert!(
                !layout
                    .pkgs_dir()
                    .join(&package_name)
                    .join("9.9.9")
                    .join("partial.txt")
                    .exists(),
                "status={status}: interrupted package state should be rolled back"
            );

            let _ = std::fs::remove_dir_all(layout.prefix());
        }
    }

    #[test]
    fn run_rollback_command_succeeds_when_failed_tx_has_no_journal_entries() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-uninstall-no-journal".to_string(),
            operation: "uninstall".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_267,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-uninstall-no-journal")
            .expect("must write active marker");

        run_rollback_command(&layout, Some("tx-uninstall-no-journal".to_string()))
            .expect("rollback should succeed when no mutating journal entries were recorded");

        let active = read_active_transaction(&layout).expect("must read active marker");
        assert!(active.is_none(), "active marker should be cleared");
        let updated = read_transaction_metadata(&layout, "tx-uninstall-no-journal")
            .expect("must read metadata")
            .expect("metadata should still exist");
        assert_eq!(updated.status, "rolled_back");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_rollback_command_removes_orphan_bins_when_no_receipt_snapshot_exists() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let txid = "tx-install-no-receipt";
        let package_name = "demo";

        let metadata = TransactionMetadata {
            version: 1,
            txid: txid.to_string(),
            operation: "install".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_267,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, txid).expect("must write active marker");

        let snapshot_root = layout
            .transaction_staging_path(txid)
            .join("rollback")
            .join(package_name);
        std::fs::create_dir_all(snapshot_root.join("package"))
            .expect("must create snapshot package dir");
        std::fs::create_dir_all(snapshot_root.join("receipt"))
            .expect("must create snapshot receipt dir");
        std::fs::create_dir_all(snapshot_root.join("bins")).expect("must create snapshot bins dir");
        std::fs::write(
            snapshot_root.join("manifest.txt"),
            "package_exists=0\nreceipt_exists=0\n",
        )
        .expect("must write snapshot manifest");

        let install_root = layout.pkgs_dir().join(package_name).join("2.0.0");
        std::fs::create_dir_all(&install_root).expect("must create install root");
        std::fs::write(install_root.join("demo"), "new-bin").expect("must write binary payload");
        expose_binary(&layout, &install_root, "demo", "demo")
            .expect("must expose binary without receipt");
        assert!(
            bin_path(&layout, "demo").exists(),
            "binary should exist before rollback"
        );

        append_transaction_journal_entry(
            &layout,
            txid,
            &TransactionJournalEntry {
                seq: 1,
                step: format!("backup_package_state:{package_name}"),
                state: "done".to_string(),
                path: Some(snapshot_root.display().to_string()),
            },
        )
        .expect("must append backup journal step");
        append_transaction_journal_entry(
            &layout,
            txid,
            &TransactionJournalEntry {
                seq: 2,
                step: format!("install_package:{package_name}"),
                state: "done".to_string(),
                path: Some(package_name.to_string()),
            },
        )
        .expect("must append mutating journal step");

        run_rollback_command(&layout, Some(txid.to_string()))
            .expect("rollback should remove orphaned binaries for unsnapshotted install");

        assert!(
            !bin_path(&layout, "demo").exists(),
            "rollback should remove stale binary entry"
        );
        assert!(
            !layout.pkgs_dir().join(package_name).exists(),
            "rollback should remove interrupted package directory"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_rollback_command_rejects_invalid_txid_path_components() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let err = run_rollback_command(&layout, Some("../escape".to_string()))
            .expect_err("rollback must reject invalid txid input");
        assert!(
            err.to_string().contains("invalid rollback txid"),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_rollback_command_without_active_marker_uses_latest_non_final_transaction() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let older = TransactionMetadata {
            version: 1,
            txid: "tx-old-failed".to_string(),
            operation: "install".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_100,
            snapshot_id: None,
        };
        let newer = TransactionMetadata {
            version: 1,
            txid: "tx-new-failed".to_string(),
            operation: "upgrade".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_200,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &older).expect("must write older metadata");
        write_transaction_metadata(&layout, &newer).expect("must write newer metadata");

        let snapshot_root = layout
            .transaction_staging_path("tx-new-failed")
            .join("rollback")
            .join("demo");
        std::fs::create_dir_all(snapshot_root.join("package"))
            .expect("must create snapshot package dir");
        std::fs::create_dir_all(snapshot_root.join("receipt"))
            .expect("must create snapshot receipt dir");
        std::fs::create_dir_all(snapshot_root.join("bins")).expect("must create snapshot bins dir");
        std::fs::write(
            snapshot_root.join("manifest.txt"),
            "package_exists=0\nreceipt_exists=0\n",
        )
        .expect("must write snapshot manifest");
        append_transaction_journal_entry(
            &layout,
            "tx-new-failed",
            &TransactionJournalEntry {
                seq: 1,
                step: "backup_package_state:demo".to_string(),
                state: "done".to_string(),
                path: Some(snapshot_root.display().to_string()),
            },
        )
        .expect("must append backup journal step");
        append_transaction_journal_entry(
            &layout,
            "tx-new-failed",
            &TransactionJournalEntry {
                seq: 2,
                step: "upgrade_package:demo".to_string(),
                state: "done".to_string(),
                path: Some("demo".to_string()),
            },
        )
        .expect("must append mutating journal step");

        run_rollback_command(&layout, None)
            .expect("rollback without active marker should use latest non-final tx");

        let updated_newer = read_transaction_metadata(&layout, "tx-new-failed")
            .expect("must read newer metadata")
            .expect("newer metadata should exist");
        assert_eq!(updated_newer.status, "rolled_back");

        let updated_older = read_transaction_metadata(&layout, "tx-old-failed")
            .expect("must read older metadata")
            .expect("older metadata should exist");
        assert_eq!(updated_older.status, "failed");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_rollback_command_rejects_active_applying_transaction() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let txid = format!("tx-live-applying-{}", std::process::id());
        let metadata = TransactionMetadata {
            version: 1,
            txid: txid.clone(),
            operation: "install".to_string(),
            status: "applying".to_string(),
            started_at_unix: current_unix_timestamp().expect("must read current timestamp"),
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, &txid).expect("must write active marker");

        let snapshot_root = layout
            .transaction_staging_path(&txid)
            .join("rollback")
            .join("demo");
        std::fs::create_dir_all(snapshot_root.join("package"))
            .expect("must create snapshot package dir");
        std::fs::create_dir_all(snapshot_root.join("receipt"))
            .expect("must create snapshot receipt dir");
        std::fs::create_dir_all(snapshot_root.join("bins")).expect("must create snapshot bins dir");
        std::fs::write(
            snapshot_root.join("manifest.txt"),
            "package_exists=0\nreceipt_exists=0\n",
        )
        .expect("must write snapshot manifest");
        append_transaction_journal_entry(
            &layout,
            &txid,
            &TransactionJournalEntry {
                seq: 1,
                step: "backup_package_state:demo".to_string(),
                state: "done".to_string(),
                path: Some(snapshot_root.display().to_string()),
            },
        )
        .expect("must append backup journal step");
        append_transaction_journal_entry(
            &layout,
            &txid,
            &TransactionJournalEntry {
                seq: 2,
                step: "install_package:demo".to_string(),
                state: "done".to_string(),
                path: Some("demo".to_string()),
            },
        )
        .expect("must append mutating journal step");

        let err = run_rollback_command(&layout, Some(txid.clone()))
            .expect_err("rollback must reject active applying transactions");
        assert!(
            err.to_string()
                .contains("cannot rollback while transaction is active (status=applying)"),
            "unexpected error: {err}"
        );

        let active = read_active_transaction(&layout).expect("must read active marker");
        assert_eq!(active.as_deref(), Some(txid.as_str()));
        let updated = read_transaction_metadata(&layout, &txid)
            .expect("must read updated metadata")
            .expect("metadata should still exist");
        assert_eq!(updated.status, "applying");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_rollback_command_allows_stale_active_applying_transaction() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let txid = "tx-stale-applying-99999999".to_string();
        let metadata = TransactionMetadata {
            version: 1,
            txid: txid.clone(),
            operation: "install".to_string(),
            status: "applying".to_string(),
            started_at_unix: current_unix_timestamp().expect("must read current timestamp"),
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, &txid).expect("must write active marker");

        let snapshot_root = layout
            .transaction_staging_path(&txid)
            .join("rollback")
            .join("demo");
        std::fs::create_dir_all(snapshot_root.join("package"))
            .expect("must create snapshot package dir");
        std::fs::create_dir_all(snapshot_root.join("receipt"))
            .expect("must create snapshot receipt dir");
        std::fs::create_dir_all(snapshot_root.join("bins")).expect("must create snapshot bins dir");
        std::fs::write(
            snapshot_root.join("manifest.txt"),
            "package_exists=0\nreceipt_exists=0\n",
        )
        .expect("must write snapshot manifest");
        append_transaction_journal_entry(
            &layout,
            &txid,
            &TransactionJournalEntry {
                seq: 1,
                step: "backup_package_state:demo".to_string(),
                state: "done".to_string(),
                path: Some(snapshot_root.display().to_string()),
            },
        )
        .expect("must append backup journal step");
        append_transaction_journal_entry(
            &layout,
            &txid,
            &TransactionJournalEntry {
                seq: 2,
                step: "install_package:demo".to_string(),
                state: "done".to_string(),
                path: Some("demo".to_string()),
            },
        )
        .expect("must append mutating journal step");

        run_rollback_command(&layout, Some(txid.clone()))
            .expect("rollback should recover stale active transaction");

        let active = read_active_transaction(&layout).expect("must read active marker");
        assert_eq!(active, None);
        let updated = read_transaction_metadata(&layout, &txid)
            .expect("must read updated metadata")
            .expect("metadata should still exist");
        assert_eq!(updated.status, "rolled_back");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn normalize_command_token_trims_lowercases_and_falls_back() {
        assert_eq!(normalize_command_token("  UnInstall  "), "uninstall");
        assert_eq!(normalize_command_token("   \t  "), "unknown");
    }

    #[test]
    fn ensure_no_active_transaction_for_includes_command_context() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-blocked".to_string(),
            operation: "upgrade".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_260,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-blocked").expect("must write active marker");

        let err = ensure_no_active_transaction_for(&layout, "uninstall")
            .expect_err("blocked transaction should include command context");
        assert!(
            err.to_string().contains(
                "cannot uninstall (reason=active_transaction command=uninstall): transaction tx-blocked requires repair (reason=failed)"
            ),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_for_normalizes_command_token() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-blocked-normalized".to_string(),
            operation: "upgrade".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_261,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-blocked-normalized").expect("must write active marker");

        let err = ensure_no_active_transaction_for(&layout, "  UnInstall  ")
            .expect_err("blocked transaction should normalize command token");
        assert!(
            err.to_string().contains(
                "cannot uninstall (reason=active_transaction command=uninstall): transaction tx-blocked-normalized requires repair (reason=failed)"
            ),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_for_uses_unknown_when_command_missing() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-blocked-empty-command".to_string(),
            operation: "upgrade".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_262,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-blocked-empty-command")
            .expect("must write active marker");

        let err = ensure_no_active_transaction_for(&layout, "   ")
            .expect_err("blocked transaction should fallback command token");
        assert!(
            err.to_string().contains(
                "cannot unknown (reason=active_transaction command=unknown): transaction tx-blocked-empty-command requires repair (reason=failed)"
            ),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_rejects_when_marker_exists() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        set_active_transaction(&layout, "tx-abc").expect("must write active marker");

        let err = ensure_no_active_transaction(&layout)
            .expect_err("active transaction must block mutating command");
        assert!(
            err.to_string()
                .contains("transaction tx-abc requires repair"),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_reports_rolling_back_status_in_error() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-rolling-diagnostic".to_string(),
            operation: "upgrade".to_string(),
            status: "rolling_back".to_string(),
            started_at_unix: 1_771_001_700,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-rolling-diagnostic").expect("must write active marker");

        let err = ensure_no_active_transaction(&layout)
            .expect_err("rolling_back transaction should block mutation");
        assert!(
            err.to_string().contains(
                "transaction tx-rolling-diagnostic requires repair (reason=rolling_back)"
            ),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_reports_failed_reason_in_error() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-failed-diagnostic".to_string(),
            operation: "upgrade".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_710,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-failed-diagnostic").expect("must write active marker");

        let err = ensure_no_active_transaction(&layout)
            .expect_err("failed transaction should block mutation");
        assert!(
            err.to_string()
                .contains("transaction tx-failed-diagnostic requires repair (reason=failed)"),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_reports_unreadable_metadata_in_error() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let txid = "tx-corrupt-meta";
        std::fs::write(layout.transaction_metadata_path(txid), "{invalid-json")
            .expect("must write corrupt metadata");
        set_active_transaction(&layout, txid).expect("must write active marker");

        let err = ensure_no_active_transaction(&layout)
            .expect_err("corrupt metadata should block mutating command");
        let expected = format!(
            "transaction tx-corrupt-meta requires repair (reason=metadata_unreadable path={})",
            layout.transaction_metadata_path(txid).display()
        );
        assert!(
            err.to_string().contains(&expected),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_reports_missing_metadata_in_error() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        set_active_transaction(&layout, "tx-missing-meta").expect("must write active marker");

        let err = ensure_no_active_transaction(&layout)
            .expect_err("missing metadata should block mutating command");
        let expected = format!(
            "transaction tx-missing-meta requires repair (reason=metadata_missing path={})",
            layout
                .transaction_metadata_path("tx-missing-meta")
                .display()
        );
        assert!(
            err.to_string().contains(&expected),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_includes_status_when_metadata_exists() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-abc".to_string(),
            operation: "install".to_string(),
            status: "paused".to_string(),
            started_at_unix: 1_771_001_300,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-abc").expect("must write active marker");

        let err = ensure_no_active_transaction(&layout)
            .expect_err("active transaction must include status context");
        assert!(
            err.to_string()
                .contains("transaction tx-abc is active (reason=active_status status=paused)"),
            "unexpected error: {err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_clears_committed_marker() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-committed".to_string(),
            operation: "install".to_string(),
            status: "committed".to_string(),
            started_at_unix: 1_771_001_360,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-committed").expect("must write active marker");

        ensure_no_active_transaction(&layout)
            .expect("committed transaction marker should be auto-cleaned");

        assert!(
            read_active_transaction(&layout)
                .expect("must read active transaction")
                .is_none(),
            "committed active marker should be cleared"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_blocks_planning_without_mutating_status() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-planning".to_string(),
            operation: "install".to_string(),
            status: "planning".to_string(),
            started_at_unix: 1_771_001_420,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-planning").expect("must write active marker");

        let err = ensure_no_active_transaction(&layout)
            .expect_err("planning transaction should block concurrent mutation");
        assert!(
            err.to_string().contains(
                "transaction tx-planning is active (reason=active_status status=planning)"
            ),
            "unexpected error: {err}"
        );

        let updated = read_transaction_metadata(&layout, "tx-planning")
            .expect("must read metadata")
            .expect("metadata should exist");
        assert_eq!(updated.status, "planning");
        assert_eq!(
            read_active_transaction(&layout)
                .expect("must read active transaction")
                .as_deref(),
            Some("tx-planning"),
            "planning marker should remain active"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_clears_rolled_back_marker() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-rolled-back".to_string(),
            operation: "upgrade".to_string(),
            status: "rolled_back".to_string(),
            started_at_unix: 1_771_001_430,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-rolled-back").expect("must write active marker");

        ensure_no_active_transaction(&layout)
            .expect("rolled_back transaction marker should be auto-cleaned");

        assert!(
            read_active_transaction(&layout)
                .expect("must read active transaction")
                .is_none(),
            "rolled_back active marker should be cleared"
        );

        ensure_no_active_transaction(&layout)
            .expect("cleanup path should remain idempotent after marker is removed");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn set_transaction_status_updates_metadata_via_helper() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let tx = begin_transaction(&layout, "install", None, 1_771_001_500)
            .expect("must create transaction");

        set_transaction_status(&layout, &tx.txid, "applying").expect("must update status");

        let metadata = read_transaction_metadata(&layout, &tx.txid)
            .expect("must read metadata")
            .expect("metadata must exist");
        assert_eq!(metadata.status, "applying");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn execute_with_transaction_commits_and_clears_active_marker() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let mut txid = None;
        execute_with_transaction(&layout, "upgrade", None, |tx| {
            txid = Some(tx.txid.clone());
            Ok(())
        })
        .expect("transaction should commit");

        let txid = txid.expect("txid should be captured");
        let metadata = read_transaction_metadata(&layout, &txid)
            .expect("must read metadata")
            .expect("metadata should exist");
        assert_eq!(metadata.status, "committed");
        assert_eq!(metadata.operation, "upgrade");
        assert!(
            read_active_transaction(&layout)
                .expect("must read active transaction")
                .is_none(),
            "active marker should be cleared"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn execute_with_transaction_marks_failed_on_error() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let mut txid = None;
        let err = execute_with_transaction(&layout, "uninstall", None, |tx| {
            txid = Some(tx.txid.clone());
            Err(anyhow::anyhow!("boom"))
        })
        .expect_err("failing transaction must return error");
        assert!(err.to_string().contains("boom"));

        let txid = txid.expect("txid should be captured");
        let metadata = read_transaction_metadata(&layout, &txid)
            .expect("must read metadata")
            .expect("metadata should exist");
        assert_eq!(metadata.status, "failed");
        assert_eq!(metadata.operation, "uninstall");
        assert_eq!(
            read_active_transaction(&layout)
                .expect("must read active transaction")
                .as_deref(),
            Some(txid.as_str()),
            "failed transaction should retain active marker for repair"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn execute_with_transaction_preserves_rolling_back_status_on_error() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let mut txid = None;
        let err = execute_with_transaction(&layout, "upgrade", None, |tx| {
            txid = Some(tx.txid.clone());
            set_transaction_status(&layout, &tx.txid, "rolling_back")?;
            Err(anyhow::anyhow!("rollback in progress"))
        })
        .expect_err("failing rollback transaction must return error");
        assert!(err.to_string().contains("rollback in progress"));

        let txid = txid.expect("txid should be captured");
        let metadata = read_transaction_metadata(&layout, &txid)
            .expect("must read metadata")
            .expect("metadata should exist");
        assert_eq!(metadata.status, "rolling_back");
        assert_eq!(metadata.operation, "upgrade");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn execute_with_transaction_preserves_rolled_back_status_on_error() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let mut txid = None;
        let err = execute_with_transaction(&layout, "uninstall", None, |tx| {
            txid = Some(tx.txid.clone());
            set_transaction_status(&layout, &tx.txid, "rolled_back")?;
            Err(anyhow::anyhow!("post-rollback cleanup failed"))
        })
        .expect_err("rolled_back transaction should preserve status on error");
        assert!(err.to_string().contains("post-rollback cleanup failed"));

        let txid = txid.expect("txid should be captured");
        let metadata = read_transaction_metadata(&layout, &txid)
            .expect("must read metadata")
            .expect("metadata should exist");
        assert_eq!(metadata.status, "rolled_back");
        assert_eq!(metadata.operation, "uninstall");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn execute_with_transaction_clears_active_marker_when_rolled_back_on_error() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let mut txid = None;
        let err = execute_with_transaction(&layout, "upgrade", None, |tx| {
            txid = Some(tx.txid.clone());
            set_transaction_status(&layout, &tx.txid, "rolled_back")?;
            Err(anyhow::anyhow!("cleanup warning"))
        })
        .expect_err("rolled_back error path should still return original error");
        assert!(err.to_string().contains("cleanup warning"));

        let txid = txid.expect("txid should be captured");
        let metadata = read_transaction_metadata(&layout, &txid)
            .expect("must read metadata")
            .expect("metadata should exist");
        assert_eq!(metadata.status, "rolled_back");
        assert!(
            read_active_transaction(&layout)
                .expect("must read active transaction")
                .is_none(),
            "rolled_back final state should clear active marker"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn execute_with_transaction_preserves_committed_status_on_error() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let mut txid = None;
        let err = execute_with_transaction(&layout, "install", None, |tx| {
            txid = Some(tx.txid.clone());
            set_transaction_status(&layout, &tx.txid, "committed")?;
            Err(anyhow::anyhow!("post-commit warning"))
        })
        .expect_err("committed transaction should preserve final status on error");
        assert!(err.to_string().contains("post-commit warning"));

        let txid = txid.expect("txid should be captured");
        let metadata = read_transaction_metadata(&layout, &txid)
            .expect("must read metadata")
            .expect("metadata should exist");
        assert_eq!(metadata.status, "committed");
        assert!(
            read_active_transaction(&layout)
                .expect("must read active transaction")
                .is_none(),
            "committed final state should clear active marker"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_blocks_applying_without_mutating_status() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-applying".to_string(),
            operation: "install".to_string(),
            status: "applying".to_string(),
            started_at_unix: 1_771_001_560,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-applying").expect("must write active marker");

        let err = ensure_no_active_transaction(&layout)
            .expect_err("applying transaction should block concurrent mutation");
        assert!(
            err.to_string().contains(
                "transaction tx-applying is active (reason=active_status status=applying)"
            ),
            "unexpected error: {err}"
        );

        let updated = read_transaction_metadata(&layout, "tx-applying")
            .expect("must read metadata")
            .expect("metadata should exist");
        assert_eq!(updated.status, "applying");

        let second_err = ensure_no_active_transaction(&layout)
            .expect_err("second preflight call should remain blocked and deterministic");
        assert!(
            second_err.to_string().contains(
                "transaction tx-applying is active (reason=active_status status=applying)"
            ),
            "unexpected second error: {second_err}"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn ensure_no_active_transaction_blocks_rolling_back_without_mutating_status() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-rolling-back".to_string(),
            operation: "install".to_string(),
            status: "rolling_back".to_string(),
            started_at_unix: 1_771_001_580,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-rolling-back").expect("must write active marker");

        let err = ensure_no_active_transaction(&layout)
            .expect_err("rolling_back transaction should block and preserve status");
        assert!(
            err.to_string()
                .contains("transaction tx-rolling-back requires repair"),
            "unexpected error: {err}"
        );

        let updated = read_transaction_metadata(&layout, "tx-rolling-back")
            .expect("must read metadata")
            .expect("metadata should exist");
        assert_eq!(updated.status, "rolling_back");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn doctor_transaction_health_line_reports_failed_state() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-failed".to_string(),
            operation: "install".to_string(),
            status: "failed".to_string(),
            started_at_unix: 1_771_001_620,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-failed").expect("must write active marker");

        let line = doctor_transaction_health_line(&layout)
            .expect("doctor line should resolve for failed tx");
        assert_eq!(line, "transaction: failed tx-failed (reason=failed)");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn doctor_transaction_health_line_treats_rolling_back_as_failed() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-rolling-back".to_string(),
            operation: "uninstall".to_string(),
            status: "rolling_back".to_string(),
            started_at_unix: 1_771_001_630,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-rolling-back").expect("must write active marker");

        let line = doctor_transaction_health_line(&layout)
            .expect("doctor line should resolve for rolling_back tx");
        assert_eq!(
            line,
            "transaction: failed tx-rolling-back (reason=rolling_back)"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn doctor_transaction_health_line_reports_failed_when_active_marker_unreadable() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        std::fs::create_dir_all(layout.transaction_active_path())
            .expect("must create unreadable active marker fixture");

        let line = doctor_transaction_health_line(&layout)
            .expect("doctor line should map unreadable active marker to failed");
        let expected = format!(
            "transaction: failed (reason=active_marker_unreadable path={})",
            layout.transaction_active_path().display()
        );
        assert_eq!(line, expected);

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn doctor_transaction_health_line_reports_failed_when_active_marker_has_no_metadata() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        set_active_transaction(&layout, "tx-missing").expect("must write active marker");

        let line = doctor_transaction_health_line(&layout)
            .expect("doctor line should resolve for missing metadata");
        let expected = format!(
            "transaction: failed tx-missing (reason=metadata_missing path={})",
            layout.transaction_metadata_path("tx-missing").display()
        );
        assert_eq!(line, expected);

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn doctor_transaction_health_line_reports_failed_when_metadata_unreadable() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let txid = "tx-unreadable";
        std::fs::write(layout.transaction_metadata_path(txid), "{not-json")
            .expect("must write corrupt metadata");
        set_active_transaction(&layout, txid).expect("must write active marker");

        let line = doctor_transaction_health_line(&layout)
            .expect("doctor line should map unreadable metadata to failed");
        let expected = format!(
            "transaction: failed tx-unreadable (reason=metadata_unreadable path={})",
            layout.transaction_metadata_path(txid).display()
        );
        assert_eq!(line, expected);

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn doctor_transaction_health_line_treats_applying_as_active() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-applying-health".to_string(),
            operation: "install".to_string(),
            status: "applying".to_string(),
            started_at_unix: 1_771_001_645,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-applying-health").expect("must write active marker");

        let line = doctor_transaction_health_line(&layout)
            .expect("doctor line should resolve for applying tx");
        assert_eq!(line, "transaction: active tx-applying-health");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn doctor_transaction_health_line_reports_active_state_without_status_suffix() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-active".to_string(),
            operation: "upgrade".to_string(),
            status: "paused".to_string(),
            started_at_unix: 1_771_001_640,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-active").expect("must write active marker");

        let line = doctor_transaction_health_line(&layout)
            .expect("doctor line should resolve for active tx");
        assert_eq!(line, "transaction: active tx-active");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn doctor_transaction_health_line_treats_committed_marker_as_clean() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-committed".to_string(),
            operation: "install".to_string(),
            status: "committed".to_string(),
            started_at_unix: 1_771_001_660,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-committed").expect("must write active marker");

        let line = doctor_transaction_health_line(&layout)
            .expect("doctor line should resolve for committed marker");
        assert_eq!(line, "transaction: clean");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn doctor_transaction_health_line_treats_planning_marker_as_active() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-planning".to_string(),
            operation: "install".to_string(),
            status: "planning".to_string(),
            started_at_unix: 1_771_001_670,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-planning").expect("must write active marker");

        let line = doctor_transaction_health_line(&layout)
            .expect("doctor line should resolve for planning marker");
        assert_eq!(line, "transaction: active tx-planning");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn doctor_transaction_health_line_clears_stale_marker_when_status_is_final() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-stale".to_string(),
            operation: "upgrade".to_string(),
            status: "committed".to_string(),
            started_at_unix: 1_771_001_680,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-stale").expect("must write active marker");

        let line = doctor_transaction_health_line(&layout)
            .expect("doctor line should resolve for stale marker");
        assert_eq!(line, "transaction: clean");
        assert!(
            read_active_transaction(&layout)
                .expect("must read active transaction")
                .is_none(),
            "doctor should clear stale final-state marker"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn parse_pin_spec_requires_constraint() {
        let err = parse_pin_spec("ripgrep").expect_err("must require constraint");
        assert!(err.to_string().contains("pin requires"));
    }

    #[test]
    fn select_manifest_with_pin_applies_both_constraints() {
        let one = PackageManifest::from_toml_str(
            r#"
name = "tool"
version = "1.2.0"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/tool-1.2.0.tar.zst"
sha256 = "abc"
"#,
        )
        .expect("manifest must parse");
        let two = PackageManifest::from_toml_str(
            r#"
name = "tool"
version = "1.3.0"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/tool-1.3.0.tar.zst"
sha256 = "def"
"#,
        )
        .expect("manifest must parse");

        let versions = vec![one, two];
        let request = VersionReq::parse("^1").expect("request req");
        let pin = VersionReq::parse("<1.3.0").expect("pin req");

        let selected =
            select_manifest_with_pin(&versions, &request, Some(&pin)).expect("must select");
        assert_eq!(selected.version.to_string(), "1.2.0");
    }

    #[test]
    fn validate_binary_preflight_rejects_other_package_owner() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let receipts = vec![InstallReceipt {
            name: "fd".to_string(),
            version: "10.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["rg".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];

        let err = validate_binary_preflight(
            &layout,
            "ripgrep",
            &["rg".to_string()],
            &receipts,
            &HashSet::new(),
        )
        .expect_err("must reject conflict");
        assert!(err.to_string().contains("already owned by package 'fd'"));

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn validate_binary_preflight_rejects_unmanaged_existing_file() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let existing = bin_path(&layout, "rg");
        fs::write(&existing, b"#!/bin/sh\n").expect("must write existing file");

        let err = validate_binary_preflight(
            &layout,
            "ripgrep",
            &["rg".to_string()],
            &[],
            &HashSet::new(),
        )
        .expect_err("must reject unmanaged file");
        assert!(err
            .to_string()
            .contains("already exists and is not managed by crosspack"));

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn validate_binary_preflight_allows_replacement_owned_binary() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let existing = bin_path(&layout, "rg");
        fs::write(&existing, b"#!/bin/sh\n").expect("must write existing file");

        let receipts = vec![InstallReceipt {
            name: "ripgrep-legacy".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["rg".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];

        let replacement_targets = HashSet::from(["ripgrep-legacy"]);
        validate_binary_preflight(
            &layout,
            "ripgrep",
            &["rg".to_string()],
            &receipts,
            &replacement_targets,
        )
        .expect("replacement-owned binary should be allowed");

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn validate_completion_preflight_rejects_other_package_owner() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let desired = "packages/bash/zoxide--completions--zoxide.bash".to_string();
        let receipts = vec![InstallReceipt {
            name: "zoxide".to_string(),
            version: "0.9.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["zoxide".to_string()],
            exposed_completions: vec![desired.clone()],
            snapshot_id: None,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];

        let err = validate_completion_preflight(
            &layout,
            "ripgrep",
            std::slice::from_ref(&desired),
            &receipts,
        )
        .expect_err("must reject completion ownership conflict");
        assert!(err
            .to_string()
            .contains("is already owned by package 'zoxide'"));

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn validate_completion_preflight_rejects_unmanaged_existing_file() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let desired = "packages/bash/ripgrep--completions--rg.bash".to_string();
        let path =
            exposed_completion_path(&layout, &desired).expect("must resolve completion path");
        fs::create_dir_all(path.parent().expect("must have parent"))
            .expect("must create completion parent");
        fs::write(&path, b"complete -F _rg rg\n").expect("must write completion file");

        let err =
            validate_completion_preflight(&layout, "ripgrep", std::slice::from_ref(&desired), &[])
                .expect_err("must reject unmanaged completion file");
        assert!(err
            .to_string()
            .contains("already exists and is not managed by crosspack"));

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn validate_completion_preflight_allows_self_owned_existing_file() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let desired = "packages/bash/ripgrep--completions--rg.bash".to_string();
        let path =
            exposed_completion_path(&layout, &desired).expect("must resolve completion path");
        fs::create_dir_all(path.parent().expect("must have parent"))
            .expect("must create completion parent");
        fs::write(&path, b"complete -F _rg rg\n").expect("must write completion file");

        let receipts = vec![InstallReceipt {
            name: "ripgrep".to_string(),
            version: "14.1.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["rg".to_string()],
            exposed_completions: vec![desired.clone()],
            snapshot_id: None,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];

        validate_completion_preflight(
            &layout,
            "ripgrep",
            std::slice::from_ref(&desired),
            &receipts,
        )
        .expect("self-owned completion file should be allowed");

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn collect_replacement_receipts_matches_manifest_rules() {
        let manifest = PackageManifest::from_toml_str(
            r#"
name = "ripgrep"
version = "2.0.0"

[replaces]
ripgrep-legacy = "<2.0.0"
"#,
        )
        .expect("manifest should parse");

        let receipts = vec![
            InstallReceipt {
                name: "ripgrep-legacy".to_string(),
                version: "1.5.0".to_string(),
                dependencies: Vec::new(),
                target: None,
                artifact_url: None,
                artifact_sha256: None,
                cache_path: None,
                exposed_bins: vec!["rg".to_string()],
                exposed_completions: Vec::new(),
                snapshot_id: None,
                install_reason: InstallReason::Root,
                install_status: "installed".to_string(),
                installed_at_unix: 1,
            },
            InstallReceipt {
                name: "other".to_string(),
                version: "3.0.0".to_string(),
                dependencies: Vec::new(),
                target: None,
                artifact_url: None,
                artifact_sha256: None,
                cache_path: None,
                exposed_bins: vec!["other".to_string()],
                exposed_completions: Vec::new(),
                snapshot_id: None,
                install_reason: InstallReason::Dependency,
                install_status: "installed".to_string(),
                installed_at_unix: 1,
            },
        ];

        let replacements =
            collect_replacement_receipts(&manifest, &receipts).expect("replacement match expected");
        assert_eq!(replacements.len(), 1);
        assert_eq!(replacements[0].name, "ripgrep-legacy");
    }

    #[test]
    fn collect_replacement_receipts_rejects_invalid_installed_version() {
        let manifest = PackageManifest::from_toml_str(
            r#"
name = "ripgrep"
version = "2.0.0"

[replaces]
ripgrep-legacy = "*"
"#,
        )
        .expect("manifest should parse");

        let receipts = vec![InstallReceipt {
            name: "ripgrep-legacy".to_string(),
            version: "not-a-semver".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["rg".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];

        let err = collect_replacement_receipts(&manifest, &receipts)
            .expect_err("invalid installed semver should fail replacement preflight");
        assert!(err
            .to_string()
            .contains("invalid version for replacement preflight"));
    }

    #[test]
    fn apply_replacement_handoff_blocks_when_dependents_remain() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let app = InstallReceipt {
            name: "app".to_string(),
            version: "1.0.0".to_string(),
            dependencies: vec!["ripgrep-legacy@1.0.0".to_string()],
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["app".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        };
        let replaced = InstallReceipt {
            name: "ripgrep-legacy".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["rg".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        };
        write_install_receipt(&layout, &app).expect("must seed app receipt");
        write_install_receipt(&layout, &replaced).expect("must seed replaced receipt");

        let err =
            apply_replacement_handoff(&layout, std::slice::from_ref(&replaced), &HashMap::new())
                .expect_err("replacement must fail while rooted dependents remain");
        assert!(err.to_string().contains("still required by roots app"));

        let remaining = read_install_receipts(&layout).expect("must read receipts");
        assert_eq!(
            remaining.len(),
            2,
            "blocked replacement must not mutate state"
        );

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn apply_replacement_handoff_preflights_all_targets_before_mutation() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let app = InstallReceipt {
            name: "app".to_string(),
            version: "1.0.0".to_string(),
            dependencies: vec!["legacy-b@1.0.0".to_string()],
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["app".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        };
        let legacy_a = InstallReceipt {
            name: "legacy-a".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["legacy-a".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_reason: InstallReason::Dependency,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        };
        let legacy_b = InstallReceipt {
            name: "legacy-b".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["legacy-b".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_reason: InstallReason::Dependency,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        };
        write_install_receipt(&layout, &app).expect("must seed app receipt");
        write_install_receipt(&layout, &legacy_a).expect("must seed first replacement target");
        write_install_receipt(&layout, &legacy_b).expect("must seed second replacement target");

        let err = apply_replacement_handoff(
            &layout,
            &[legacy_a.clone(), legacy_b.clone()],
            &HashMap::new(),
        )
        .expect_err("blocked replacement must fail before any uninstall mutation");
        assert!(err.to_string().contains("still required by roots app"));

        let remaining = read_install_receipts(&layout).expect("must read receipts");
        let remaining_names = remaining
            .iter()
            .map(|receipt| receipt.name.as_str())
            .collect::<HashSet<_>>();
        assert!(
            remaining_names.contains("legacy-a") && remaining_names.contains("legacy-b"),
            "preflight failure must keep every replacement target installed"
        );

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn apply_replacement_handoff_allows_interdependent_replacement_roots() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let legacy_a = InstallReceipt {
            name: "legacy-a".to_string(),
            version: "1.0.0".to_string(),
            dependencies: vec!["legacy-b@1.0.0".to_string()],
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["legacy-a".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        };
        let legacy_b = InstallReceipt {
            name: "legacy-b".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["legacy-b".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        };
        write_install_receipt(&layout, &legacy_a).expect("must seed first replacement root");
        write_install_receipt(&layout, &legacy_b).expect("must seed second replacement root");

        apply_replacement_handoff(
            &layout,
            &[legacy_a.clone(), legacy_b.clone()],
            &HashMap::new(),
        )
        .expect("replacement handoff should allow roots that are all being replaced");

        let remaining = read_install_receipts(&layout).expect("must read receipts");
        assert!(
            remaining.is_empty(),
            "all replacement roots should be removed in a successful handoff"
        );

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn apply_replacement_handoff_uses_planned_dependency_overrides() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let app = InstallReceipt {
            name: "app".to_string(),
            version: "1.0.0".to_string(),
            dependencies: vec!["ripgrep-legacy@1.0.0".to_string()],
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["app".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        };
        let replaced = InstallReceipt {
            name: "ripgrep-legacy".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["rg".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_reason: InstallReason::Dependency,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        };
        write_install_receipt(&layout, &app).expect("must seed app receipt");
        write_install_receipt(&layout, &replaced).expect("must seed replaced receipt");

        let planned_dependency_overrides =
            HashMap::from([("app".to_string(), vec!["ripgrep".to_string()])]);

        apply_replacement_handoff(
            &layout,
            std::slice::from_ref(&replaced),
            &planned_dependency_overrides,
        )
        .expect("planned dependency graph should allow replacement handoff");

        let remaining = read_install_receipts(&layout).expect("must read receipts");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].name, "app");

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn apply_replacement_handoff_uninstalls_safe_target() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let replaced = InstallReceipt {
            name: "ripgrep-legacy".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["rg".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        };
        write_install_receipt(&layout, &replaced).expect("must seed replaced receipt");

        apply_replacement_handoff(&layout, std::slice::from_ref(&replaced), &HashMap::new())
            .expect("safe replacement handoff should uninstall target");

        let remaining = read_install_receipts(&layout).expect("must read receipts");
        assert!(
            remaining.is_empty(),
            "replacement handoff must remove target receipt"
        );

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn enforce_no_downgrades_rejects_lower_version() {
        let receipts = vec![InstallReceipt {
            name: "tool".to_string(),
            version: "2.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: Vec::new(),
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];
        let resolved = vec![resolved_install("tool", "1.9.0")];

        let err = enforce_no_downgrades(&receipts, &resolved, "upgrade").expect_err("must fail");
        assert!(err.to_string().contains("would downgrade 'tool'"));
    }

    #[test]
    fn enforce_no_downgrades_allows_upgrade() {
        let receipts = vec![InstallReceipt {
            name: "tool".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: Vec::new(),
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];
        let resolved = vec![resolved_install("tool", "1.2.0")];
        enforce_no_downgrades(&receipts, &resolved, "upgrade").expect("must pass");
    }

    #[test]
    fn determine_install_reason_sets_requested_root() {
        let reason = determine_install_reason("tool", &["tool".to_string()], &[], &[]);
        assert_eq!(reason, InstallReason::Root);
    }

    #[test]
    fn determine_install_reason_sets_dependency_for_non_root() {
        let reason = determine_install_reason("shared", &["app".to_string()], &[], &[]);
        assert_eq!(reason, InstallReason::Dependency);
    }

    #[test]
    fn determine_install_reason_preserves_existing_root() {
        let existing = vec![InstallReceipt {
            name: "shared".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: Vec::new(),
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];

        let reason = determine_install_reason("shared", &["app".to_string()], &existing, &[]);
        assert_eq!(reason, InstallReason::Root);
    }

    #[test]
    fn determine_install_reason_promotes_to_root_when_requested() {
        let existing = vec![InstallReceipt {
            name: "shared".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: Vec::new(),
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_reason: InstallReason::Dependency,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];

        let reason = determine_install_reason("shared", &["shared".to_string()], &existing, &[]);
        assert_eq!(reason, InstallReason::Root);
    }

    #[test]
    fn determine_install_reason_promotes_existing_dependency_when_replacing_root() {
        let existing = vec![InstallReceipt {
            name: "ripgrep".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["rg".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_reason: InstallReason::Dependency,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];
        let replacement = vec![InstallReceipt {
            name: "ripgrep-legacy".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["rg".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];

        let reason = determine_install_reason("ripgrep", &[], &existing, &replacement);
        assert_eq!(reason, InstallReason::Root);
    }

    #[test]
    fn determine_install_reason_preserves_root_from_replacement_target() {
        let replacement = vec![InstallReceipt {
            name: "ripgrep-legacy".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["rg".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];

        let reason = determine_install_reason("ripgrep", &[], &[], &replacement);
        assert_eq!(reason, InstallReason::Root);
    }

    #[test]
    fn build_upgrade_roots_uses_only_root_receipts() {
        let receipts = vec![
            InstallReceipt {
                name: "app".to_string(),
                version: "1.0.0".to_string(),
                dependencies: vec!["shared@1.0.0".to_string()],
                target: None,
                artifact_url: None,
                artifact_sha256: None,
                cache_path: None,
                exposed_bins: Vec::new(),
                exposed_completions: Vec::new(),
                snapshot_id: None,
                install_reason: InstallReason::Root,
                install_status: "installed".to_string(),
                installed_at_unix: 1,
            },
            InstallReceipt {
                name: "shared".to_string(),
                version: "1.0.0".to_string(),
                dependencies: Vec::new(),
                target: None,
                artifact_url: None,
                artifact_sha256: None,
                cache_path: None,
                exposed_bins: Vec::new(),
                exposed_completions: Vec::new(),
                snapshot_id: None,
                install_reason: InstallReason::Dependency,
                install_status: "installed".to_string(),
                installed_at_unix: 1,
            },
        ];

        let roots = build_upgrade_roots(&receipts);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].name, "app");
    }

    #[test]
    fn build_upgrade_roots_is_empty_when_no_roots_installed() {
        let receipts = vec![InstallReceipt {
            name: "shared".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: Vec::new(),
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_reason: InstallReason::Dependency,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];

        let roots = build_upgrade_roots(&receipts);
        assert!(roots.is_empty());
    }

    #[test]
    fn build_upgrade_plans_groups_roots_by_target() {
        let receipts = vec![
            InstallReceipt {
                name: "linux-tool".to_string(),
                version: "1.0.0".to_string(),
                dependencies: Vec::new(),
                target: Some("x86_64-unknown-linux-gnu".to_string()),
                artifact_url: None,
                artifact_sha256: None,
                cache_path: None,
                exposed_bins: Vec::new(),
                exposed_completions: Vec::new(),
                snapshot_id: None,
                install_reason: InstallReason::Root,
                install_status: "installed".to_string(),
                installed_at_unix: 1,
            },
            InstallReceipt {
                name: "mac-tool".to_string(),
                version: "1.0.0".to_string(),
                dependencies: Vec::new(),
                target: Some("aarch64-apple-darwin".to_string()),
                artifact_url: None,
                artifact_sha256: None,
                cache_path: None,
                exposed_bins: Vec::new(),
                exposed_completions: Vec::new(),
                snapshot_id: None,
                install_reason: InstallReason::Root,
                install_status: "installed".to_string(),
                installed_at_unix: 1,
            },
        ];

        let plans = build_upgrade_plans(&receipts);
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].target.as_deref(), Some("aarch64-apple-darwin"));
        assert_eq!(plans[0].root_names, vec!["mac-tool"]);
        assert_eq!(plans[1].target.as_deref(), Some("x86_64-unknown-linux-gnu"));
        assert_eq!(plans[1].root_names, vec!["linux-tool"]);
    }

    #[test]
    fn build_upgrade_plans_ignores_dependency_receipts() {
        let receipts = vec![
            InstallReceipt {
                name: "app".to_string(),
                version: "1.0.0".to_string(),
                dependencies: vec!["shared@1.0.0".to_string()],
                target: Some("x86_64-unknown-linux-gnu".to_string()),
                artifact_url: None,
                artifact_sha256: None,
                cache_path: None,
                exposed_bins: Vec::new(),
                exposed_completions: Vec::new(),
                snapshot_id: None,
                install_reason: InstallReason::Root,
                install_status: "installed".to_string(),
                installed_at_unix: 1,
            },
            InstallReceipt {
                name: "shared".to_string(),
                version: "1.0.0".to_string(),
                dependencies: Vec::new(),
                target: Some("x86_64-unknown-linux-gnu".to_string()),
                artifact_url: None,
                artifact_sha256: None,
                cache_path: None,
                exposed_bins: Vec::new(),
                exposed_completions: Vec::new(),
                snapshot_id: None,
                install_reason: InstallReason::Dependency,
                install_status: "installed".to_string(),
                installed_at_unix: 1,
            },
        ];

        let plans = build_upgrade_plans(&receipts);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].root_names, vec!["app"]);
        assert_eq!(plans[0].roots.len(), 1);
        assert_eq!(plans[0].roots[0].name, "app");
    }

    #[test]
    fn build_upgrade_plans_is_empty_when_no_roots_installed() {
        let receipts = vec![InstallReceipt {
            name: "shared".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: Some("x86_64-unknown-linux-gnu".to_string()),
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: Vec::new(),
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_reason: InstallReason::Dependency,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];

        let plans = build_upgrade_plans(&receipts);
        assert!(plans.is_empty());
    }

    #[test]
    fn enforce_disjoint_multi_target_upgrade_rejects_overlapping_package_names() {
        let err = enforce_disjoint_multi_target_upgrade(&[
            (
                Some("x86_64-unknown-linux-gnu"),
                vec!["shared".to_string(), "linux-tool".to_string()],
            ),
            (
                Some("aarch64-apple-darwin"),
                vec!["shared".to_string(), "mac-tool".to_string()],
            ),
        ])
        .expect_err("overlap must fail");

        assert!(err
            .to_string()
            .contains("cannot safely process package 'shared'"));
        assert!(err.to_string().contains("separate prefixes"));
    }

    #[test]
    fn enforce_disjoint_multi_target_upgrade_allows_disjoint_package_sets() {
        enforce_disjoint_multi_target_upgrade(&[
            (
                Some("x86_64-unknown-linux-gnu"),
                vec!["linux-tool".to_string(), "linux-lib".to_string()],
            ),
            (
                Some("aarch64-apple-darwin"),
                vec!["mac-tool".to_string(), "mac-lib".to_string()],
            ),
        ])
        .expect("disjoint groups must pass");
    }

    #[test]
    fn format_uninstall_messages_reports_blocking_roots() {
        let result = UninstallResult {
            name: "shared".to_string(),
            version: Some("1.0.0".to_string()),
            status: UninstallStatus::BlockedByDependents,
            pruned_dependencies: Vec::new(),
            blocked_by_roots: vec!["app-a".to_string(), "app-b".to_string()],
        };

        let lines = format_uninstall_messages(&result);
        assert_eq!(
            lines,
            vec!["cannot uninstall shared 1.0.0: still required by roots app-a, app-b".to_string()]
        );
    }

    #[test]
    fn format_uninstall_messages_reports_pruned_dependencies() {
        let result = UninstallResult {
            name: "app".to_string(),
            version: Some("1.0.0".to_string()),
            status: UninstallStatus::Uninstalled,
            pruned_dependencies: vec!["shared".to_string(), "zlib".to_string()],
            blocked_by_roots: Vec::new(),
        };

        let lines = format_uninstall_messages(&result);
        assert_eq!(lines[0], "uninstalled app 1.0.0");
        assert_eq!(lines[1], "pruned orphan dependencies: shared, zlib");
    }

    #[test]
    fn cli_parses_install_with_repeatable_provider_overrides() {
        let cli = Cli::try_parse_from([
            "crosspack",
            "install",
            "compiler@^2",
            "--provider",
            "c-compiler=clang",
            "--provider",
            "rust-toolchain=rustup",
        ])
        .expect("command must parse");

        match cli.command {
            Commands::Install { provider, .. } => {
                assert_eq!(provider, vec!["c-compiler=clang", "rust-toolchain=rustup"]);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_parses_upgrade_with_repeatable_provider_overrides() {
        let cli = Cli::try_parse_from([
            "crosspack",
            "upgrade",
            "compiler@^2",
            "--provider",
            "c-compiler=clang",
            "--provider",
            "rust-toolchain=rustup",
        ])
        .expect("command must parse");

        match cli.command {
            Commands::Upgrade { provider, .. } => {
                assert_eq!(provider, vec!["c-compiler=clang", "rust-toolchain=rustup"]);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_parses_completions_for_each_supported_shell() {
        let cases = vec![
            ("bash", CliCompletionShell::Bash),
            ("zsh", CliCompletionShell::Zsh),
            ("fish", CliCompletionShell::Fish),
            ("powershell", CliCompletionShell::Powershell),
        ];

        for (shell, expected) in cases {
            let cli =
                Cli::try_parse_from(["crosspack", "completions", shell]).expect("command parses");
            match cli.command {
                Commands::Completions { shell } => {
                    assert_eq!(shell, expected);
                }
                other => panic!("unexpected command: {other:?}"),
            }
        }
    }

    #[test]
    fn cli_rejects_completions_without_shell() {
        let err = Cli::try_parse_from(["crosspack", "completions"])
            .expect_err("missing shell argument must fail");
        assert!(err.to_string().contains("<SHELL>"));
    }

    #[test]
    fn cli_rejects_unsupported_completion_shell() {
        let err = Cli::try_parse_from(["crosspack", "completions", "elvish"])
            .expect_err("unsupported shell must fail");
        let rendered = err.to_string();
        assert!(rendered.contains("elvish"));
        assert!(rendered.contains("possible values"));
    }

    #[test]
    fn cli_parses_init_shell_with_optional_shell_override() {
        let cli = Cli::try_parse_from(["crosspack", "init-shell", "--shell", "zsh"])
            .expect("command must parse");
        match cli.command {
            Commands::InitShell { shell } => {
                assert_eq!(shell, Some(CliCompletionShell::Zsh));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn resolve_init_shell_prefers_requested_shell_over_env_detection() {
        let resolved = resolve_init_shell(Some(CliCompletionShell::Fish), Some("/bin/zsh"), false);
        assert_eq!(resolved, CliCompletionShell::Fish);
    }

    #[test]
    fn resolve_init_shell_uses_env_detection_when_request_missing() {
        let resolved = resolve_init_shell(None, Some("/usr/bin/pwsh"), false);
        assert_eq!(resolved, CliCompletionShell::Powershell);
    }

    #[test]
    fn resolve_init_shell_falls_back_deterministically_by_platform() {
        let unix_fallback = resolve_init_shell(None, Some("/usr/bin/unknown-shell"), false);
        assert_eq!(unix_fallback, CliCompletionShell::Bash);

        let windows_fallback = resolve_init_shell(None, None, true);
        assert_eq!(windows_fallback, CliCompletionShell::Powershell);
    }

    #[test]
    fn generate_completions_outputs_non_empty_script_for_each_shell() {
        let shells = [
            CliCompletionShell::Bash,
            CliCompletionShell::Zsh,
            CliCompletionShell::Fish,
            CliCompletionShell::Powershell,
        ];
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        for shell in shells {
            let mut output = Vec::new();
            write_completions_script(shell, &layout, &mut output)
                .expect("completion script generation should succeed");
            assert!(
                !output.is_empty(),
                "completion script should not be empty for {shell:?}"
            );
        }
        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn generate_completions_uses_crosspack_command_name() {
        let shells = [
            CliCompletionShell::Bash,
            CliCompletionShell::Zsh,
            CliCompletionShell::Fish,
            CliCompletionShell::Powershell,
        ];
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        for shell in shells {
            let mut output = Vec::new();
            write_completions_script(shell, &layout, &mut output)
                .expect("completion script generation should succeed");
            let rendered = String::from_utf8(output).expect("completion script should be utf-8");
            assert!(
                rendered.contains("crosspack"),
                "completion script should target canonical binary name for {shell:?}"
            );
        }
        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn parse_provider_overrides_rejects_invalid_shape() {
        let err = parse_provider_overrides(&["missing-equals".to_string()])
            .expect_err("override must require capability=package shape");
        assert!(err.to_string().contains("expected capability=package"));
    }

    #[test]
    fn parse_provider_overrides_rejects_invalid_capability_token() {
        let err = parse_provider_overrides(&["BadCap=clang".to_string()])
            .expect_err("invalid capability token must fail");
        assert!(err.to_string().contains("capability 'BadCap'"));
    }

    #[test]
    fn apply_provider_override_selects_requested_capability_provider() {
        let gcc = PackageManifest::from_toml_str(
            r#"
name = "gcc"
version = "2.0.0"
provides = ["compiler"]
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/gcc-2.0.0.tar.zst"
sha256 = "gcc"
"#,
        )
        .expect("gcc manifest must parse");
        let llvm = PackageManifest::from_toml_str(
            r#"
name = "llvm"
version = "2.1.0"
provides = ["compiler"]
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/llvm-2.1.0.tar.zst"
sha256 = "llvm"
"#,
        )
        .expect("llvm manifest must parse");

        let mut overrides = BTreeMap::new();
        overrides.insert("compiler".to_string(), "llvm".to_string());

        let selected = apply_provider_override("compiler", vec![gcc, llvm], &overrides)
            .expect("provider override must filter candidate set");
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name, "llvm");
    }

    #[test]
    fn apply_provider_override_errors_when_requested_provider_missing() {
        let gcc = PackageManifest::from_toml_str(
            r#"
name = "gcc"
version = "2.0.0"
provides = ["compiler"]
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/gcc-2.0.0.tar.zst"
sha256 = "gcc"
"#,
        )
        .expect("manifest must parse");

        let mut overrides = BTreeMap::new();
        overrides.insert("compiler".to_string(), "clang".to_string());

        let err = apply_provider_override("compiler", vec![gcc], &overrides)
            .expect_err("missing requested provider must fail early");
        assert!(
            err.to_string()
                .contains("provider override 'compiler=clang'"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn apply_provider_override_rejects_overriding_direct_package_tokens() {
        let foo = PackageManifest::from_toml_str(
            r#"
name = "foo"
version = "1.0.0"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/foo-1.0.0.tar.zst"
sha256 = "foo"
"#,
        )
        .expect("foo manifest must parse");
        let bar = PackageManifest::from_toml_str(
            r#"
name = "bar"
version = "1.0.0"
provides = ["foo"]
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/bar-1.0.0.tar.zst"
sha256 = "bar"
"#,
        )
        .expect("bar manifest must parse");

        let mut overrides = BTreeMap::new();
        overrides.insert("foo".to_string(), "bar".to_string());

        let err = apply_provider_override("foo", vec![foo, bar], &overrides)
            .expect_err("direct package tokens must not be overridable");
        assert!(
            err.to_string()
                .contains("direct package names cannot be overridden"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_provider_overrides_used_accepts_consumed_overrides() {
        let mut overrides = BTreeMap::new();
        overrides.insert("compiler".to_string(), "llvm".to_string());
        overrides.insert("rust-toolchain".to_string(), "rustup".to_string());

        let resolved_dependency_tokens = HashSet::from([
            "compiler".to_string(),
            "rust-toolchain".to_string(),
            "ripgrep".to_string(),
        ]);

        validate_provider_overrides_used(&overrides, &resolved_dependency_tokens)
            .expect("all overrides should be consumed by the resolved graph");
    }

    #[test]
    fn validate_provider_overrides_used_rejects_unused_overrides() {
        let mut overrides = BTreeMap::new();
        overrides.insert("compiler".to_string(), "llvm".to_string());
        overrides.insert("rust-toolchain".to_string(), "rustup".to_string());

        let resolved_dependency_tokens = HashSet::from(["compiler".to_string()]);

        let err = validate_provider_overrides_used(&overrides, &resolved_dependency_tokens)
            .expect_err("unused overrides must fail fast");
        assert!(
            err.to_string()
                .contains("unused provider override(s): rust-toolchain=rustup"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_provider_overrides_used_accepts_union_of_multi_plan_tokens() {
        let mut overrides = BTreeMap::new();
        overrides.insert("compiler".to_string(), "llvm".to_string());
        overrides.insert("rust-toolchain".to_string(), "rustup".to_string());

        let plan_a_tokens = HashSet::from(["compiler".to_string()]);
        let plan_b_tokens = HashSet::from(["rust-toolchain".to_string()]);

        let mut combined_tokens = HashSet::new();
        combined_tokens.extend(plan_a_tokens);
        combined_tokens.extend(plan_b_tokens);

        validate_provider_overrides_used(&overrides, &combined_tokens)
            .expect("overrides consumed across plans should pass");
    }

    #[test]
    fn format_info_lines_includes_policy_sections_when_present() {
        let manifest = PackageManifest::from_toml_str(
            r#"
name = "compiler"
version = "2.1.0"
provides = ["c-compiler", "cc"]

[conflicts]
legacy-cc = "*"

[replaces]
old-cc = "<2.0.0"
"#,
        )
        .expect("manifest must parse");

        let lines = format_info_lines("compiler", &[manifest]);
        assert_eq!(lines[0], "Package: compiler");
        assert_eq!(lines[1], "- 2.1.0");
        assert_eq!(lines[2], "  Provides: c-compiler, cc");
        assert_eq!(lines[3], "  Conflicts: legacy-cc(*)");
        assert_eq!(lines[4], "  Replaces: old-cc(<2.0.0)");
    }

    #[test]
    fn cli_parses_registry_add_command() {
        let cli = Cli::try_parse_from([
            "crosspack",
            "registry",
            "add",
            "official",
            "https://example.com/official.git",
            "--kind",
            "git",
            "--priority",
            "10",
            "--fingerprint",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ])
        .expect("command must parse");

        match cli.command {
            Commands::Registry {
                command:
                    super::RegistryCommands::Add {
                        name,
                        location,
                        kind,
                        priority,
                        fingerprint,
                    },
            } => {
                assert_eq!(name, "official");
                assert_eq!(location, "https://example.com/official.git");
                assert_eq!(kind, CliRegistryKind::Git);
                assert_eq!(priority, 10);
                assert_eq!(
                    fingerprint,
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                );
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_parses_registry_remove_with_purge_cache() {
        let cli = Cli::try_parse_from([
            "crosspack",
            "registry",
            "remove",
            "official",
            "--purge-cache",
        ])
        .expect("command must parse");

        match cli.command {
            Commands::Registry {
                command: super::RegistryCommands::Remove { name, purge_cache },
            } => {
                assert_eq!(name, "official");
                assert!(purge_cache);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_parses_registry_list_command() {
        let cli =
            Cli::try_parse_from(["crosspack", "registry", "list"]).expect("command must parse");

        match cli.command {
            Commands::Registry {
                command: super::RegistryCommands::List,
            } => {}
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn cli_rejects_registry_add_without_required_kind_flag() {
        let err = Cli::try_parse_from([
            "crosspack",
            "registry",
            "add",
            "official",
            "https://example.com/official.git",
            "--priority",
            "10",
            "--fingerprint",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ])
        .expect_err("missing --kind should fail");

        let rendered = err.to_string();
        assert!(rendered.contains("--kind <KIND>"));
    }

    #[test]
    fn cli_rejects_registry_add_when_priority_value_missing() {
        let err = Cli::try_parse_from([
            "crosspack",
            "registry",
            "add",
            "official",
            "https://example.com/official.git",
            "--kind",
            "git",
            "--priority",
            "--fingerprint",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ])
        .expect_err("missing --priority value should fail");

        let rendered = err.to_string();
        assert!(rendered.contains("--priority <PRIORITY>"));
    }

    #[test]
    fn cli_rejects_registry_remove_without_name() {
        let err = Cli::try_parse_from(["crosspack", "registry", "remove"])
            .expect_err("missing remove name should fail");

        let rendered = err.to_string();
        assert!(rendered.contains("<NAME>"));
    }

    #[test]
    fn cli_rejects_update_when_registry_value_missing() {
        let err = Cli::try_parse_from(["crosspack", "update", "--registry"])
            .expect_err("missing --registry value should fail");

        let rendered = err.to_string();
        assert!(rendered.contains("a value is required for '--registry <REGISTRY>'"));
    }

    #[test]
    fn cli_parses_update_with_multiple_registry_flags() {
        let cli = Cli::try_parse_from([
            "crosspack",
            "update",
            "--registry",
            "official",
            "--registry",
            "mirror",
        ])
        .expect("command must parse");

        match cli.command {
            Commands::Update { registry } => {
                assert_eq!(registry, vec!["official", "mirror"]);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn registry_list_output_is_sorted() {
        let sources = vec![
            RegistrySourceWithSnapshotState {
                source: RegistrySourceRecord {
                    name: "zeta".to_string(),
                    kind: RegistrySourceKind::Git,
                    location: "https://example.test/zeta.git".to_string(),
                    fingerprint_sha256:
                        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                            .to_string(),
                    enabled: true,
                    priority: 10,
                },
                snapshot: RegistrySourceSnapshotState::Ready {
                    snapshot_id: "git:0123456789abcdef".to_string(),
                },
            },
            RegistrySourceWithSnapshotState {
                source: RegistrySourceRecord {
                    name: "alpha".to_string(),
                    kind: RegistrySourceKind::Filesystem,
                    location: "/tmp/alpha".to_string(),
                    fingerprint_sha256:
                        "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
                            .to_string(),
                    enabled: true,
                    priority: 1,
                },
                snapshot: RegistrySourceSnapshotState::None,
            },
        ];

        let lines = format_registry_list_lines(sources);
        assert_eq!(
            lines[0],
            "alpha kind=filesystem priority=1 location=/tmp/alpha snapshot=none"
        );
        assert_eq!(
            lines[1],
            "zeta kind=git priority=10 location=https://example.test/zeta.git snapshot=ready:git:0123456789abcdef"
        );
    }

    #[test]
    fn format_registry_add_lines_matches_source_management_spec() {
        let lines = format_registry_add_lines(
            "official",
            "git",
            10,
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        );

        assert_eq!(
            lines,
            vec![
                "added registry official".to_string(),
                "kind: git".to_string(),
                "priority: 10".to_string(),
                "fingerprint: 0123456789abcdef...".to_string(),
            ]
        );
    }

    #[test]
    fn format_registry_remove_lines_matches_source_management_spec() {
        let lines = format_registry_remove_lines("official", true);
        assert_eq!(lines, vec!["removed registry official", "cache: purged"]);

        let lines = format_registry_remove_lines("official", false);
        assert_eq!(lines, vec!["removed registry official", "cache: kept"]);
    }

    #[test]
    fn format_registry_list_snapshot_error_line_uses_reason_code() {
        let line = format_registry_list_snapshot_state(&RegistrySourceSnapshotState::Error {
            status: RegistrySourceWithSnapshotStatus::Unreadable,
            reason_code: "snapshot-unreadable".to_string(),
        });
        assert_eq!(line, "error:snapshot-unreadable");
    }

    #[test]
    fn resolve_transaction_snapshot_id_ignores_disabled_sources() {
        let layout = test_layout();
        let state_root = registry_state_root(&layout);
        let store = RegistrySourceStore::new(&state_root);
        let snap_root = |name: &str| state_root.join("cache").join(name).join("snapshot.json");

        store
            .add_source(RegistrySourceRecord {
                name: "alpha".to_string(),
                kind: RegistrySourceKind::Filesystem,
                location: "/tmp/alpha".to_string(),
                fingerprint_sha256:
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
                enabled: true,
                priority: 1,
            })
            .expect("must add alpha source");
        store
            .add_source(RegistrySourceRecord {
                name: "beta".to_string(),
                kind: RegistrySourceKind::Filesystem,
                location: "/tmp/beta".to_string(),
                fingerprint_sha256:
                    "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210".to_string(),
                enabled: false,
                priority: 2,
            })
            .expect("must add beta source");

        std::fs::create_dir_all(state_root.join("cache/alpha"))
            .expect("must create alpha cache directory");
        std::fs::create_dir_all(state_root.join("cache/beta"))
            .expect("must create beta cache directory");
        std::fs::write(
            snap_root("alpha"),
            r#"{"version":1,"source":"alpha","snapshot_id":"snapshot-a","updated_at_unix":1,"manifest_count":0,"status":"ready"}"#,
        )
        .expect("must write alpha snapshot");
        std::fs::write(
            snap_root("beta"),
            r#"{"version":1,"source":"beta","snapshot_id":"snapshot-b","updated_at_unix":1,"manifest_count":0,"status":"ready"}"#,
        )
        .expect("must write beta snapshot");

        let snapshot_id = resolve_transaction_snapshot_id(&layout, "install")
            .expect("must ignore disabled source snapshot");
        assert_eq!(snapshot_id, "snapshot-a");

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn resolve_transaction_snapshot_id_rejects_mixed_ready_snapshots() {
        let layout = test_layout();
        let state_root = registry_state_root(&layout);
        let store = RegistrySourceStore::new(&state_root);
        let snap_root = |name: &str| state_root.join("cache").join(name).join("snapshot.json");

        store
            .add_source(RegistrySourceRecord {
                name: "alpha".to_string(),
                kind: RegistrySourceKind::Filesystem,
                location: "/tmp/alpha".to_string(),
                fingerprint_sha256:
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
                enabled: true,
                priority: 1,
            })
            .expect("must add alpha source");
        store
            .add_source(RegistrySourceRecord {
                name: "beta".to_string(),
                kind: RegistrySourceKind::Filesystem,
                location: "/tmp/beta".to_string(),
                fingerprint_sha256:
                    "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210".to_string(),
                enabled: true,
                priority: 2,
            })
            .expect("must add beta source");

        std::fs::create_dir_all(state_root.join("cache/alpha"))
            .expect("must create alpha cache directory");
        std::fs::create_dir_all(state_root.join("cache/beta"))
            .expect("must create beta cache directory");
        std::fs::write(
            snap_root("alpha"),
            r#"{"version":1,"source":"alpha","snapshot_id":"snapshot-a","updated_at_unix":1,"manifest_count":0,"status":"ready"}"#,
        )
        .expect("must write alpha snapshot");
        std::fs::write(
            snap_root("beta"),
            r#"{"version":1,"source":"beta","snapshot_id":"snapshot-b","updated_at_unix":1,"manifest_count":0,"status":"ready"}"#,
        )
        .expect("must write beta snapshot");

        let err = resolve_transaction_snapshot_id(&layout, "install")
            .expect_err("must fail mixed snapshots");
        let rendered = err.to_string();
        assert!(rendered.contains("metadata snapshot mismatch across configured sources"));
        assert!(rendered.contains("alpha=snapshot-a"));
        assert!(rendered.contains("beta=snapshot-b"));
        let monitor_raw =
            std::fs::read_to_string(layout.transactions_dir().join("snapshot-monitor.log"))
                .expect("must write mismatch telemetry log");
        assert!(monitor_raw.contains("event=snapshot_id_consistency_mismatch"));
        assert!(monitor_raw.contains("error_code=snapshot-id-mismatch"));
        assert!(monitor_raw.contains("operation=install"));
        assert!(monitor_raw.contains("source_count=2"));
        assert!(monitor_raw.contains("unique_snapshot_ids=2"));
        assert!(monitor_raw.contains("sources=alpha=snapshot-a,beta=snapshot-b"));

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn resolve_transaction_snapshot_id_uses_shared_snapshot_id() {
        let layout = test_layout();
        let state_root = registry_state_root(&layout);
        let store = RegistrySourceStore::new(&state_root);
        let snap_root = |name: &str| state_root.join("cache").join(name).join("snapshot.json");

        store
            .add_source(RegistrySourceRecord {
                name: "alpha".to_string(),
                kind: RegistrySourceKind::Filesystem,
                location: "/tmp/alpha".to_string(),
                fingerprint_sha256:
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
                enabled: true,
                priority: 1,
            })
            .expect("must add alpha source");
        store
            .add_source(RegistrySourceRecord {
                name: "beta".to_string(),
                kind: RegistrySourceKind::Filesystem,
                location: "/tmp/beta".to_string(),
                fingerprint_sha256:
                    "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210".to_string(),
                enabled: true,
                priority: 2,
            })
            .expect("must add beta source");

        std::fs::create_dir_all(state_root.join("cache/alpha"))
            .expect("must create alpha cache directory");
        std::fs::create_dir_all(state_root.join("cache/beta"))
            .expect("must create beta cache directory");
        std::fs::write(
            snap_root("alpha"),
            r#"{"version":1,"source":"alpha","snapshot_id":"snapshot-shared","updated_at_unix":1,"manifest_count":0,"status":"ready"}"#,
        )
        .expect("must write alpha snapshot");
        std::fs::write(
            snap_root("beta"),
            r#"{"version":1,"source":"beta","snapshot_id":"snapshot-shared","updated_at_unix":1,"manifest_count":0,"status":"ready"}"#,
        )
        .expect("must write beta snapshot");

        let snapshot_id = resolve_transaction_snapshot_id(&layout, "upgrade")
            .expect("must choose shared snapshot id");
        assert_eq!(snapshot_id, "snapshot-shared");
        assert!(
            !layout
                .transactions_dir()
                .join("snapshot-monitor.log")
                .exists(),
            "shared snapshot id should not emit mismatch telemetry"
        );

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn resolve_transaction_snapshot_id_requires_ready_snapshot() {
        let layout = test_layout();
        let state_root = registry_state_root(&layout);
        let store = RegistrySourceStore::new(&state_root);

        store
            .add_source(RegistrySourceRecord {
                name: "alpha".to_string(),
                kind: RegistrySourceKind::Filesystem,
                location: "/tmp/alpha".to_string(),
                fingerprint_sha256:
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
                enabled: true,
                priority: 1,
            })
            .expect("must add alpha source");

        let err = resolve_transaction_snapshot_id(&layout, "install")
            .expect_err("must fail without ready snapshot");
        assert!(err.to_string().contains(
            "no configured registry snapshots available; bootstrap trusted source `core`"
        ));

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn run_update_command_returns_err_on_partial_failure() {
        let root = test_layout();
        let store = RegistrySourceStore::new(registry_state_root(&root));

        let ok_source = test_registry_source_dir("ok-source", true);
        let bad_source = test_registry_source_dir("bad-source", false);

        store
            .add_source(RegistrySourceRecord {
                name: "ok".to_string(),
                kind: RegistrySourceKind::Filesystem,
                location: ok_source.display().to_string(),
                fingerprint_sha256:
                    "f0cf90f634c31f8f43f56f3576d2f23f9f66d4b041e92f788bcbdbdbf4dcd89f".to_string(),
                enabled: true,
                priority: 1,
            })
            .expect("must add ok source");
        store
            .add_source(RegistrySourceRecord {
                name: "bad".to_string(),
                kind: RegistrySourceKind::Filesystem,
                location: bad_source.display().to_string(),
                fingerprint_sha256:
                    "f0cf90f634c31f8f43f56f3576d2f23f9f66d4b041e92f788bcbdbdbf4dcd89f".to_string(),
                enabled: true,
                priority: 2,
            })
            .expect("must add bad source");

        let err = run_update_command(&store, &[]).expect_err("partial failure must return err");
        assert_eq!(err.to_string(), "source update failed");

        let _ = std::fs::remove_dir_all(root.prefix());
        let _ = std::fs::remove_dir_all(ok_source);
        let _ = std::fs::remove_dir_all(bad_source);
    }

    #[test]
    fn search_uses_registry_root_override_when_present() {
        let layout = test_layout();
        let override_root = PathBuf::from("/tmp/override-registry");

        let backend = select_metadata_backend(Some(override_root.as_path()), &layout)
            .expect("override backend must resolve");
        assert!(matches!(backend, MetadataBackend::Legacy(_)));
    }

    #[test]
    fn search_uses_configured_sources_without_registry_root() {
        let layout = test_layout();
        let state_root = registry_state_root(&layout);
        std::fs::create_dir_all(state_root.join("cache/official/index/ripgrep"))
            .expect("must create source cache structure");
        std::fs::write(
            state_root.join("sources.toml"),
            concat!(
                "version = 1\n",
                "\n",
                "[[sources]]\n",
                "name = \"official\"\n",
                "kind = \"filesystem\"\n",
                "location = \"/tmp/official\"\n",
                "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
                "enabled = true\n",
                "priority = 1\n"
            ),
        )
        .expect("must write configured sources file");
        std::fs::write(
            state_root.join("cache/official/snapshot.json"),
            r#"{
  "version": 1,
  "source": "official",
  "snapshot_id": "fs:test",
  "updated_at_unix": 1,
  "manifest_count": 0,
  "status": "ready"
}"#,
        )
        .expect("must write snapshot metadata");

        let backend = select_metadata_backend(None, &layout)
            .expect("configured backend must resolve without override");
        assert!(matches!(backend, MetadataBackend::Configured(_)));

        let _ = std::fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn metadata_commands_fail_with_guidance_when_no_sources_or_snapshots() {
        let layout = test_layout();

        let err = select_metadata_backend(None, &layout)
            .expect_err("must fail when no configured metadata backend is available");
        let rendered = err.to_string();
        assert!(rendered.contains("crosspack registry add"));
        assert!(rendered.contains("crosspack update"));
    }

    #[test]
    fn update_failure_reason_code_prefers_deterministic_reason_prefix() {
        let reason = update_failure_reason_code(Some(
            "source-sync-failed: source 'official' git fetch failed: fatal: bad object",
        ));
        assert_eq!(reason, "source-sync-failed");
    }

    #[test]
    fn update_failure_reason_code_falls_back_to_unknown_for_unstructured_error() {
        let reason = update_failure_reason_code(Some("failed to sync source with weird error"));
        assert_eq!(reason, "unknown");
    }

    #[test]
    fn build_update_report_formats_failed_result_with_reason_code_only() {
        let results = vec![SourceUpdateResult {
            name: "official".to_string(),
            status: SourceUpdateStatus::Failed,
            snapshot_id: String::new(),
            error: Some(
                "source-metadata-invalid: source 'official' package 'ripgrep' failed signature validation: nested detail"
                    .to_string(),
            ),
        }];

        let report = build_update_report(&results);
        assert_eq!(
            report.lines,
            vec!["official: failed (reason=source-metadata-invalid)"]
        );
        assert_eq!(report.failed, 1);
    }

    #[test]
    fn ensure_update_succeeded_returns_err_when_any_source_failed() {
        let err = ensure_update_succeeded(1).expect_err("must return err when failures exist");
        assert_eq!(err.to_string(), "source update failed");
    }

    #[test]
    fn format_update_summary_line_matches_contract() {
        let line = format_update_summary_line(2, 5, 1);
        assert_eq!(line, "update summary: updated=2 up-to-date=5 failed=1");
    }

    fn resolved_install(name: &str, version: &str) -> ResolvedInstall {
        let manifest = PackageManifest::from_toml_str(&format!(
            r#"
name = "{name}"
version = "{version}"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/{name}-{version}.tar.zst"
sha256 = "abc"
"#
        ))
        .expect("manifest parse");
        let artifact = manifest.artifacts[0].clone();

        ResolvedInstall {
            manifest,
            artifact,
            resolved_target: "x86_64-unknown-linux-gnu".to_string(),
            archive_type: ArchiveType::TarZst,
        }
    }

    static TEST_LAYOUT_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn build_test_layout_path(nanos: u128) -> PathBuf {
        let mut path = std::env::temp_dir();
        let sequence = TEST_LAYOUT_COUNTER.fetch_add(1, Ordering::Relaxed);
        path.push(format!(
            "crosspack-cli-tests-{}-{}-{}",
            std::process::id(),
            nanos,
            sequence
        ));
        path
    }

    #[test]
    fn build_test_layout_path_disambiguates_same_timestamp_calls() {
        let first = build_test_layout_path(42);
        let second = build_test_layout_path(42);
        assert_ne!(
            first, second,
            "test layout paths must remain unique when timestamp granularity is coarse"
        );
    }

    fn test_layout() -> PrefixLayout {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        PrefixLayout::new(build_test_layout_path(nanos))
    }

    fn test_registry_source_dir(name: &str, with_registry_pub: bool) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        path.push(format!("crosspack-cli-test-registry-{name}-{nanos}"));
        std::fs::create_dir_all(path.join("index")).expect("must create index dir");
        if with_registry_pub {
            std::fs::write(path.join("registry.pub"), "test-key\n")
                .expect("must write registry key");
        }
        path
    }
}
