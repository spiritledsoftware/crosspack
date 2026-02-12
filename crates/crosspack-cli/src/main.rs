use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use crosspack_core::{ArchiveType, Artifact, PackageManifest};
use crosspack_installer::{
    bin_path, current_unix_timestamp, default_user_prefix, expose_binary, install_from_artifact,
    read_install_receipts, read_pin, remove_exposed_binary, remove_file_if_exists,
    uninstall_package, write_install_receipt, write_pin, InstallReceipt, PrefixLayout,
    UninstallStatus,
};
use crosspack_registry::RegistryIndex;
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
    Doctor,
    InitShell,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Search { query } => {
            let root = cli.registry_root.unwrap_or_else(|| PathBuf::from("."));
            let index = RegistryIndex::open(root);
            for name in index.search_names(&query)? {
                println!("{name}");
            }
        }
        Commands::Info { name } => {
            let root = cli.registry_root.unwrap_or_else(|| PathBuf::from("."));
            let index = RegistryIndex::open(root);
            let versions = index.package_versions(&name)?;

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
            let root = cli.registry_root.unwrap_or_else(|| PathBuf::from("."));
            let index = RegistryIndex::open(root);

            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            layout.ensure_base_dirs()?;
            let resolved =
                resolve_install(&layout, &index, &name, &requirement, target.as_deref())?;
            let outcome = install_resolved(&layout, &resolved, force_redownload)?;
            print_install_outcome(&outcome);
        }
        Commands::Upgrade { spec } => {
            let root = cli.registry_root.unwrap_or_else(|| PathBuf::from("."));
            let index = RegistryIndex::open(root);

            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            layout.ensure_base_dirs()?;

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

                    upgrade_single(&layout, &index, installed_receipt, &requirement, false)?;
                }
                None => {
                    let requirement = VersionReq::STAR;
                    for receipt in &receipts {
                        upgrade_single(&layout, &index, receipt, &requirement, false)?;
                    }
                }
            }
        }
        Commands::Uninstall { name } => {
            let prefix = default_user_prefix()?;
            let layout = PrefixLayout::new(prefix);
            let result = uninstall_package(&layout, &name)?;

            match result.status {
                UninstallStatus::NotInstalled => {
                    println!("{name} is not installed");
                }
                UninstallStatus::Uninstalled => {
                    let version = result.version.unwrap_or_else(|| "unknown".to_string());
                    println!("uninstalled {} {}", result.name, version);
                }
                UninstallStatus::RepairedStaleState => {
                    let version = result.version.unwrap_or_else(|| "unknown".to_string());
                    println!(
                        "removed stale state for {} {} (package files already missing)",
                        result.name, version
                    );
                }
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

fn resolve_install(
    layout: &PrefixLayout,
    index: &RegistryIndex,
    name: &str,
    requirement: &VersionReq,
    requested_target: Option<&str>,
) -> Result<ResolvedInstall> {
    let versions = index.package_versions(name)?;
    let pin_requirement = load_pin_requirement(layout, name)?;

    let manifest = select_manifest_with_pin(&versions, requirement, pin_requirement.as_ref())
        .ok_or_else(|| {
            if let Some(pin) = pin_requirement {
                anyhow!(
                    "no matching version found for {name} with request {} and pin {}",
                    requirement,
                    pin
                )
            } else {
                anyhow!(
                    "no matching version found for {name} with requirement {}",
                    requirement
                )
            }
        })?
        .clone();

    let resolved_target = requested_target
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| host_target_triple().to_string());
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
        resolved_target,
        archive_type,
    })
}

fn install_resolved(
    layout: &PrefixLayout,
    resolved: &ResolvedInstall,
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
        target: Some(resolved.resolved_target.clone()),
        artifact_url: Some(resolved.artifact.url.clone()),
        artifact_sha256: Some(resolved.artifact.sha256.clone()),
        cache_path: Some(cache_path.display().to_string()),
        exposed_bins: exposed_bins.clone(),
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

fn load_pin_requirement(layout: &PrefixLayout, name: &str) -> Result<Option<VersionReq>> {
    let Some(raw) = read_pin(layout, name)? else {
        return Ok(None);
    };
    let requirement = VersionReq::parse(&raw)
        .with_context(|| format!("invalid pin requirement for '{name}' in state: {raw}"))?;
    Ok(Some(requirement))
}

fn upgrade_single(
    layout: &PrefixLayout,
    index: &RegistryIndex,
    receipt: &InstallReceipt,
    request_requirement: &VersionReq,
    force_redownload: bool,
) -> Result<()> {
    let current_version = Version::parse(&receipt.version).with_context(|| {
        format!(
            "installed receipt for '{}' has invalid version: {}",
            receipt.name, receipt.version
        )
    })?;

    let resolved = resolve_install(
        layout,
        index,
        &receipt.name,
        request_requirement,
        receipt.target.as_deref(),
    )?;
    if resolved.manifest.version <= current_version {
        println!("{} is up-to-date ({})", receipt.name, receipt.version);
        return Ok(());
    }

    let outcome = install_resolved(layout, &resolved, force_redownload)?;
    println!(
        "upgraded {} from {} to {}",
        receipt.name, receipt.version, outcome.version
    );
    println!("receipt: {}", outcome.receipt_path.display());
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
    use super::{parse_pin_spec, select_manifest_with_pin, validate_binary_preflight};
    use crosspack_core::PackageManifest;
    use crosspack_installer::{bin_path, InstallReceipt, PrefixLayout};
    use semver::VersionReq;
    use std::fs;

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
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: vec!["rg".to_string()],
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
}
