use std::collections::{BTreeMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use crosspack_core::{ArchiveType, Artifact, PackageManifest};
use crosspack_installer::{
    append_transaction_journal_entry, bin_path, clear_active_transaction, current_unix_timestamp,
    default_user_prefix, expose_binary, install_from_artifact, read_active_transaction,
    read_all_pins, read_install_receipts, read_transaction_metadata, remove_exposed_binary,
    remove_file_if_exists, set_active_transaction, uninstall_package, write_install_receipt,
    write_pin, write_transaction_metadata, InstallReason, InstallReceipt, PrefixLayout,
    TransactionJournalEntry, TransactionMetadata, UninstallResult, UninstallStatus,
};
use crosspack_registry::{
    ConfiguredRegistryIndex, RegistryIndex, RegistrySourceKind, RegistrySourceRecord,
    RegistrySourceSnapshotState, RegistrySourceStore, RegistrySourceWithSnapshotState,
    SourceUpdateResult, SourceUpdateStatus,
};
use crosspack_resolver::{resolve_dependency_graph, RootRequirement};
use crosspack_security::verify_sha256_file;
use semver::{Version, VersionReq};

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
    "no configured registry snapshots available; run `crosspack registry add` and `crosspack update`";

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
    },
    Upgrade {
        spec: Option<String>,
    },
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
    InitShell,
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

impl From<CliRegistryKind> for RegistrySourceKind {
    fn from(value: CliRegistryKind) -> Self {
        match value {
            CliRegistryKind::Git => RegistrySourceKind::Git,
            CliRegistryKind::Filesystem => RegistrySourceKind::Filesystem,
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
                println!("Package: {name}");
                for manifest in versions {
                    println!("- {}", manifest.version);
                }
            }
        }
        Commands::Install {
            spec,
            target,
            force_redownload,
        } => {
            let (name, requirement) = parse_spec(&spec)?;

            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            layout.ensure_base_dirs()?;
            ensure_no_active_transaction(&layout)?;
            let backend = select_metadata_backend(cli.registry_root.as_deref(), &layout)?;
            let started_at_unix = current_unix_timestamp()?;
            let mut tx = begin_transaction(&layout, "install", None, started_at_unix)?;
            let mut journal_seq = 1_u64;

            let install_result = (|| -> Result<()> {
                let roots = vec![RootInstallRequest { name, requirement }];
                let root_names = roots
                    .iter()
                    .map(|root| root.name.clone())
                    .collect::<Vec<_>>();
                let resolved = resolve_install_graph(&layout, &backend, &roots, target.as_deref())?;

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

                tx.status = "applying".to_string();
                write_transaction_metadata(&layout, &tx)?;

                for package in &resolved {
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
            })();

            match install_result {
                Ok(()) => {
                    tx.status = "committed".to_string();
                    write_transaction_metadata(&layout, &tx)?;
                    clear_active_transaction(&layout)?;
                }
                Err(err) => {
                    tx.status = "failed".to_string();
                    let _ = write_transaction_metadata(&layout, &tx);
                    return Err(err);
                }
            }
        }
        Commands::Upgrade { spec } => {
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            layout.ensure_base_dirs()?;
            ensure_no_active_transaction(&layout)?;
            let backend = select_metadata_backend(cli.registry_root.as_deref(), &layout)?;

            let receipts = read_install_receipts(&layout)?;
            if receipts.is_empty() {
                println!("No installed packages");
                return Ok(());
            }

            match spec {
                Some(single) => {
                    let (name, requirement) = parse_spec(&single)?;
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
                        &layout,
                        &backend,
                        &roots,
                        installed_receipt.target.as_deref(),
                    )?;
                    enforce_no_downgrades(&receipts, &resolved, "upgrade")?;
                    for package in &resolved {
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

                        let dependencies = build_dependency_receipts(package, &resolved);
                        let outcome =
                            install_resolved(&layout, package, &dependencies, &root_names, false)?;
                        if let Some(old) = receipts.iter().find(|r| r.name == package.manifest.name)
                        {
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
                    for plan in &plans {
                        let resolved = resolve_install_graph(
                            &layout,
                            &backend,
                            &plan.roots,
                            plan.target.as_deref(),
                        )?;
                        enforce_no_downgrades(&receipts, &resolved, "upgrade")?;
                        grouped_resolved.push(resolved);
                    }

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
                        for package in resolved {
                            if let Some(old) =
                                receipts.iter().find(|r| r.name == package.manifest.name)
                            {
                                let old_version =
                                    Version::parse(&old.version).with_context(|| {
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

                            let dependencies = build_dependency_receipts(package, resolved);
                            let outcome = install_resolved(
                                &layout,
                                package,
                                &dependencies,
                                &plan.root_names,
                                false,
                            )?;
                            if let Some(old) =
                                receipts.iter().find(|r| r.name == package.manifest.name)
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
        }
        Commands::Uninstall { name } => {
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            ensure_no_active_transaction(&layout)?;
            let result = uninstall_package(&layout, &name)?;

            for line in format_uninstall_messages(&result) {
                println!("{line}");
            }
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
        }
        Commands::InitShell => {
            let prefix = default_user_prefix()?;
            let bin = PrefixLayout::new(prefix).bin_dir();
            if cfg!(windows) {
                println!("setx PATH \"%PATH%;{}\"", bin.display());
            } else {
                println!("export PATH=\"{}:$PATH\"", bin.display());
            }
        }
    }

    Ok(())
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
    set_active_transaction(layout, &metadata.txid)?;

    Ok(metadata)
}

fn ensure_no_active_transaction(layout: &PrefixLayout) -> Result<()> {
    if let Some(txid) = read_active_transaction(layout)? {
        if let Some(metadata) = read_transaction_metadata(layout, &txid)? {
            if metadata.status == "committed" {
                clear_active_transaction(layout)?;
                return Ok(());
            }
            if metadata.status == "failed" {
                return Err(anyhow!("transaction {txid} requires repair"));
            }

            return Err(anyhow!(
                "transaction {txid} is active (status={})",
                metadata.status
            ));
        }

        return Err(anyhow!("transaction {txid} requires repair"));
    }

    Ok(())
}

fn resolve_install_graph(
    layout: &PrefixLayout,
    index: &MetadataBackend,
    roots: &[RootInstallRequest],
    requested_target: Option<&str>,
) -> Result<Vec<ResolvedInstall>> {
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
        index.package_versions(package_name)
    })?;

    let resolved_target = requested_target
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| host_target_triple().to_string());

    graph
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
        .collect()
}

fn install_resolved(
    layout: &PrefixLayout,
    resolved: &ResolvedInstall,
    dependency_receipts: &[String],
    root_names: &[String],
    force_redownload: bool,
) -> Result<InstallOutcome> {
    let receipts = read_install_receipts(layout)?;
    let exposed_bins = collect_declared_binaries(&resolved.artifact)?;
    validate_binary_preflight(layout, &resolved.manifest.name, &exposed_bins, &receipts)?;

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

    for binary in &resolved.artifact.binaries {
        expose_binary(layout, &install_root, &binary.name, &binary.path)?;
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
        snapshot_id: None,
        install_reason: determine_install_reason(&resolved.manifest.name, root_names, &receipts),
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

fn validate_binary_preflight(
    layout: &PrefixLayout,
    package_name: &str,
    desired_bins: &[String],
    receipts: &[InstallReceipt],
) -> Result<()> {
    let owned_by_self: HashSet<&str> = receipts
        .iter()
        .find(|receipt| receipt.name == package_name)
        .map(|receipt| receipt.exposed_bins.iter().map(String::as_str).collect())
        .unwrap_or_default();

    for desired in desired_bins {
        for receipt in receipts {
            if receipt.name == package_name {
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
        if path.exists() && !owned_by_self.contains(desired.as_str()) {
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

fn determine_install_reason(
    package_name: &str,
    root_names: &[String],
    existing_receipts: &[InstallReceipt],
) -> InstallReason {
    if root_names.iter().any(|root| root == package_name) {
        return InstallReason::Root;
    }

    if let Some(existing) = existing_receipts
        .iter()
        .find(|receipt| receipt.name == package_name)
    {
        return existing.install_reason.clone();
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
        begin_transaction, build_update_report, build_upgrade_plans, build_upgrade_roots,
        determine_install_reason, enforce_disjoint_multi_target_upgrade, enforce_no_downgrades,
        ensure_no_active_transaction, ensure_update_succeeded, format_registry_add_lines,
        format_registry_list_lines, format_registry_list_snapshot_state,
        format_registry_remove_lines, format_uninstall_messages, format_update_summary_line,
        parse_pin_spec, registry_state_root, run_update_command, select_manifest_with_pin,
        select_metadata_backend, update_failure_reason_code, validate_binary_preflight, Cli,
        CliRegistryKind, Commands, MetadataBackend, ResolvedInstall,
    };
    use clap::Parser;
    use crosspack_core::{ArchiveType, PackageManifest};
    use crosspack_installer::{
        bin_path, read_active_transaction, set_active_transaction, write_transaction_metadata,
        InstallReason, InstallReceipt, PrefixLayout, TransactionMetadata, UninstallResult,
        UninstallStatus,
    };
    use crosspack_registry::{
        RegistrySourceKind, RegistrySourceRecord, RegistrySourceSnapshotState, RegistrySourceStore,
        RegistrySourceWithSnapshotState, RegistrySourceWithSnapshotStatus, SourceUpdateResult,
        SourceUpdateStatus,
    };
    use semver::VersionReq;
    use std::fs;
    use std::path::PathBuf;

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
    fn ensure_no_active_transaction_includes_status_when_metadata_exists() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-abc".to_string(),
            operation: "install".to_string(),
            status: "applying".to_string(),
            started_at_unix: 1_771_001_300,
            snapshot_id: None,
        };
        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        set_active_transaction(&layout, "tx-abc").expect("must write active marker");

        let err = ensure_no_active_transaction(&layout)
            .expect_err("active transaction must include status context");
        assert!(
            err.to_string()
                .contains("transaction tx-abc is active (status=applying)"),
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
            snapshot_id: None,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];

        let err = validate_binary_preflight(&layout, "ripgrep", &["rg".to_string()], &receipts)
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

        let err = validate_binary_preflight(&layout, "ripgrep", &["rg".to_string()], &[])
            .expect_err("must reject unmanaged file");
        assert!(err
            .to_string()
            .contains("already exists and is not managed by crosspack"));

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
        let reason = determine_install_reason("tool", &["tool".to_string()], &[]);
        assert_eq!(reason, InstallReason::Root);
    }

    #[test]
    fn determine_install_reason_sets_dependency_for_non_root() {
        let reason = determine_install_reason("shared", &["app".to_string()], &[]);
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
            snapshot_id: None,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];

        let reason = determine_install_reason("shared", &["app".to_string()], &existing);
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
            snapshot_id: None,
            install_reason: InstallReason::Dependency,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        }];

        let reason = determine_install_reason("shared", &["shared".to_string()], &existing);
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

    fn test_layout() -> PrefixLayout {
        let mut path = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        path.push(format!(
            "crosspack-cli-tests-{}-{}",
            std::process::id(),
            nanos
        ));
        PrefixLayout::new(path)
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
