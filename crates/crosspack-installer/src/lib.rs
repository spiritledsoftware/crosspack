use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use crosspack_core::ArchiveType;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixLayout {
    prefix: PathBuf,
}

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
    pub snapshot_id: Option<String>,
    pub install_reason: InstallReason,
    pub install_status: String,
    pub installed_at_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionMetadata {
    pub version: u32,
    pub txid: String,
    pub operation: String,
    pub status: String,
    pub started_at_unix: u64,
    pub snapshot_id: Option<String>,
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

impl InstallReason {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Dependency => "dependency",
        }
    }

    fn parse(value: &str) -> Result<Self> {
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

impl PrefixLayout {
    pub fn new(prefix: impl Into<PathBuf>) -> Self {
        Self {
            prefix: prefix.into(),
        }
    }

    pub fn prefix(&self) -> &Path {
        &self.prefix
    }

    pub fn pkgs_dir(&self) -> PathBuf {
        self.prefix.join("pkgs")
    }

    pub fn bin_dir(&self) -> PathBuf {
        self.prefix.join("bin")
    }

    pub fn state_dir(&self) -> PathBuf {
        self.prefix.join("state")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.prefix.join("cache")
    }

    pub fn artifacts_cache_dir(&self) -> PathBuf {
        self.cache_dir().join("artifacts")
    }

    pub fn tmp_state_dir(&self) -> PathBuf {
        self.state_dir().join("tmp")
    }

    pub fn installed_state_dir(&self) -> PathBuf {
        self.state_dir().join("installed")
    }

    pub fn pins_dir(&self) -> PathBuf {
        self.state_dir().join("pins")
    }

    pub fn pin_path(&self, name: &str) -> PathBuf {
        self.pins_dir().join(format!("{name}.pin"))
    }

    pub fn package_dir(&self, name: &str, version: &str) -> PathBuf {
        self.pkgs_dir().join(name).join(version)
    }

    pub fn receipt_path(&self, name: &str) -> PathBuf {
        self.installed_state_dir().join(format!("{name}.receipt"))
    }

    pub fn transactions_dir(&self) -> PathBuf {
        self.state_dir().join("transactions")
    }

    pub fn transactions_staging_dir(&self) -> PathBuf {
        self.transactions_dir().join("staging")
    }

    pub fn transaction_active_path(&self) -> PathBuf {
        self.transactions_dir().join("active")
    }

    pub fn transaction_metadata_path(&self, txid: &str) -> PathBuf {
        self.transactions_dir().join(format!("{txid}.json"))
    }

    pub fn transaction_journal_path(&self, txid: &str) -> PathBuf {
        self.transactions_dir().join(format!("{txid}.journal"))
    }

    pub fn transaction_staging_path(&self, txid: &str) -> PathBuf {
        self.transactions_staging_dir().join(txid)
    }

    pub fn artifact_cache_path(
        &self,
        name: &str,
        version: &str,
        target: &str,
        archive_type: ArchiveType,
    ) -> PathBuf {
        self.artifacts_cache_dir()
            .join(name)
            .join(version)
            .join(target)
            .join(format!("artifact.{}", archive_type.cache_extension()))
    }

    pub fn ensure_base_dirs(&self) -> Result<()> {
        for dir in [
            self.pkgs_dir(),
            self.bin_dir(),
            self.state_dir(),
            self.cache_dir(),
            self.artifacts_cache_dir(),
            self.tmp_state_dir(),
            self.installed_state_dir(),
            self.pins_dir(),
            self.transactions_dir(),
            self.transactions_staging_dir(),
        ] {
            fs::create_dir_all(&dir)
                .with_context(|| format!("failed to create {}", dir.display()))?;
        }
        Ok(())
    }
}

pub fn default_user_prefix() -> Result<PathBuf> {
    if cfg!(windows) {
        let app_data = std::env::var("LOCALAPPDATA")
            .context("LOCALAPPDATA is not set; cannot resolve Windows user prefix")?;
        return Ok(PathBuf::from(app_data).join("Crosspack"));
    }

    let home = std::env::var("HOME").context("HOME is not set; cannot resolve user prefix")?;
    Ok(PathBuf::from(home).join(".crosspack"))
}

pub fn set_active_transaction(layout: &PrefixLayout, txid: &str) -> Result<PathBuf> {
    let path = layout.transaction_active_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(&path, format!("{txid}\n")).with_context(|| {
        format!(
            "failed to write active transaction file: {}",
            path.display()
        )
    })?;
    Ok(path)
}

pub fn read_active_transaction(layout: &PrefixLayout) -> Result<Option<String>> {
    let path = layout.transaction_active_path();
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err).with_context(|| {
                format!("failed to read active transaction file: {}", path.display())
            });
        }
    };

    let txid = raw.trim();
    if txid.is_empty() {
        return Ok(None);
    }

    Ok(Some(txid.to_string()))
}

pub fn clear_active_transaction(layout: &PrefixLayout) -> Result<()> {
    let path = layout.transaction_active_path();
    if path.exists() {
        fs::remove_file(&path).with_context(|| {
            format!(
                "failed to clear active transaction file: {}",
                path.display()
            )
        })?;
    }
    Ok(())
}

pub fn write_transaction_metadata(
    layout: &PrefixLayout,
    metadata: &TransactionMetadata,
) -> Result<PathBuf> {
    let path = layout.transaction_metadata_path(&metadata.txid);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::create_dir_all(layout.transaction_staging_path(&metadata.txid)).with_context(|| {
        format!(
            "failed to create transaction staging dir: {}",
            layout.transaction_staging_path(&metadata.txid).display()
        )
    })?;

    fs::write(&path, serialize_transaction_metadata(metadata)).with_context(|| {
        format!(
            "failed to write transaction metadata file: {}",
            path.display()
        )
    })?;
    Ok(path)
}

pub fn read_transaction_metadata(
    layout: &PrefixLayout,
    txid: &str,
) -> Result<Option<TransactionMetadata>> {
    let path = layout.transaction_metadata_path(txid);
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "failed to read transaction metadata file: {}",
                    path.display()
                )
            });
        }
    };

    let metadata = parse_transaction_metadata(&raw).with_context(|| {
        format!(
            "failed parsing transaction metadata file: {}",
            path.display()
        )
    })?;
    Ok(Some(metadata))
}

pub fn update_transaction_status(layout: &PrefixLayout, txid: &str, status: &str) -> Result<()> {
    let mut metadata = read_transaction_metadata(layout, txid)?
        .ok_or_else(|| anyhow!("transaction metadata not found for '{txid}'"))?;
    metadata.status = status.to_string();
    write_transaction_metadata(layout, &metadata)?;
    Ok(())
}

pub fn append_transaction_journal_entry(
    layout: &PrefixLayout,
    txid: &str,
    entry: &TransactionJournalEntry,
) -> Result<PathBuf> {
    let path = layout.transaction_journal_path(txid);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("failed to open transaction journal: {}", path.display()))?;
    file.write_all(serialize_transaction_journal_entry(entry).as_bytes())
        .with_context(|| format!("failed to append transaction journal: {}", path.display()))?;
    file.write_all(b"\n").with_context(|| {
        format!(
            "failed to append transaction journal newline: {}",
            path.display()
        )
    })?;
    file.flush()
        .with_context(|| format!("failed to flush transaction journal: {}", path.display()))?;
    Ok(path)
}

pub fn install_from_artifact(
    layout: &PrefixLayout,
    name: &str,
    version: &str,
    archive_path: &Path,
    archive_type: ArchiveType,
    strip_components: u32,
    artifact_root: Option<&str>,
) -> Result<PathBuf> {
    let install_tmp = make_tmp_dir(layout, "install")?;
    let raw_dir = install_tmp.join("raw");
    let staged_dir = install_tmp.join("staged");
    fs::create_dir_all(&raw_dir)
        .with_context(|| format!("failed to create {}", raw_dir.display()))?;
    fs::create_dir_all(&staged_dir)
        .with_context(|| format!("failed to create {}", staged_dir.display()))?;

    extract_archive(archive_path, &raw_dir, archive_type)?;

    if let Some(root) = artifact_root {
        let root_path = raw_dir.join(root);
        if !root_path.exists() {
            return Err(anyhow!(
                "artifact_root '{}' was not found after extraction: {}",
                root,
                root_path.display()
            ));
        }
    }

    copy_with_strip(&raw_dir, &staged_dir, strip_components as usize)?;

    let dst = layout.package_dir(name, version);
    if dst.exists() {
        fs::remove_dir_all(&dst)
            .with_context(|| format!("failed to remove existing package dir: {}", dst.display()))?;
    }

    move_dir_or_copy(&staged_dir, &dst)?;

    let _ = fs::remove_dir_all(&install_tmp);
    Ok(dst)
}

pub fn write_install_receipt(layout: &PrefixLayout, receipt: &InstallReceipt) -> Result<PathBuf> {
    let mut payload = String::new();
    payload.push_str(&format!("name={}\n", receipt.name));
    payload.push_str(&format!("version={}\n", receipt.version));
    for dependency in &receipt.dependencies {
        payload.push_str(&format!("dependency={}\n", dependency));
    }
    if let Some(target) = &receipt.target {
        payload.push_str(&format!("target={}\n", target));
    }
    if let Some(url) = &receipt.artifact_url {
        payload.push_str(&format!("artifact_url={}\n", url));
    }
    if let Some(sha256) = &receipt.artifact_sha256 {
        payload.push_str(&format!("artifact_sha256={}\n", sha256));
    }
    if let Some(cache_path) = &receipt.cache_path {
        payload.push_str(&format!("cache_path={}\n", cache_path));
    }
    for exposed_bin in &receipt.exposed_bins {
        payload.push_str(&format!("exposed_bin={}\n", exposed_bin));
    }
    if let Some(snapshot_id) = &receipt.snapshot_id {
        payload.push_str(&format!("snapshot_id={}\n", snapshot_id));
    }
    payload.push_str(&format!(
        "install_reason={}\n",
        receipt.install_reason.as_str()
    ));
    payload.push_str(&format!("install_status={}\n", receipt.install_status));
    payload.push_str(&format!(
        "installed_at_unix={}\n",
        receipt.installed_at_unix
    ));

    let path = layout.receipt_path(&receipt.name);
    fs::write(&path, payload.as_bytes())
        .with_context(|| format!("failed to write install receipt: {}", path.display()))?;
    Ok(path)
}

pub fn read_install_receipts(layout: &PrefixLayout) -> Result<Vec<InstallReceipt>> {
    let dir = layout.installed_state_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut receipts = Vec::new();
    for entry in fs::read_dir(&dir)
        .with_context(|| format!("failed to read install state directory: {}", dir.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }

        let path = entry.path();
        if path.extension().and_then(|v| v.to_str()) != Some("receipt") {
            continue;
        }

        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read install receipt: {}", path.display()))?;
        let receipt = parse_receipt(&raw)
            .with_context(|| format!("failed to parse install receipt: {}", path.display()))?;
        receipts.push(receipt);
    }

    receipts.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(receipts)
}

pub fn current_unix_timestamp() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time is before unix epoch")?
        .as_secs())
}

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

pub fn uninstall_package(layout: &PrefixLayout, name: &str) -> Result<UninstallResult> {
    let receipts = read_install_receipts(layout)?;
    let Some(target_receipt) = receipts
        .iter()
        .find(|receipt| receipt.name == name)
        .cloned()
    else {
        return Ok(UninstallResult {
            name: name.to_string(),
            version: None,
            status: UninstallStatus::NotInstalled,
            pruned_dependencies: Vec::new(),
            blocked_by_roots: Vec::new(),
        });
    };

    let receipt_map: HashMap<String, InstallReceipt> = receipts
        .iter()
        .cloned()
        .map(|receipt| (receipt.name.clone(), receipt))
        .collect();
    let dependencies = dependency_map(&receipt_map);

    let remaining_roots = receipt_map
        .values()
        .filter(|receipt| receipt.name != name)
        .filter(|receipt| receipt.install_reason == InstallReason::Root)
        .map(|receipt| receipt.name.clone())
        .collect::<Vec<_>>();
    let reachable = reachable_packages(&remaining_roots, &dependencies);

    if reachable.contains(name) {
        let mut blocked_by_roots = remaining_roots
            .iter()
            .filter(|root| package_reachable(root, name, &dependencies))
            .cloned()
            .collect::<Vec<_>>();
        blocked_by_roots.sort();
        blocked_by_roots.dedup();
        return Ok(UninstallResult {
            name: target_receipt.name,
            version: Some(target_receipt.version),
            status: UninstallStatus::BlockedByDependents,
            pruned_dependencies: Vec::new(),
            blocked_by_roots,
        });
    }

    let target_closure = reachable_packages(&[name.to_string()], &dependencies);
    let mut pruned_dependencies = target_closure
        .iter()
        .filter(|entry| entry.as_str() != name)
        .filter(|entry| !reachable.contains(entry.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    pruned_dependencies.sort();

    let mut removal_names = Vec::with_capacity(pruned_dependencies.len() + 1);
    removal_names.push(name.to_string());
    removal_names.extend(pruned_dependencies.iter().cloned());
    let removal_names_set: HashSet<&str> = removal_names.iter().map(String::as_str).collect();

    let mut target_status = UninstallStatus::RepairedStaleState;
    let mut removed_cache_paths = Vec::new();
    for removal_name in &removal_names {
        let Some(receipt) = receipt_map.get(removal_name) else {
            continue;
        };

        if removal_name == name {
            target_status = remove_receipt_artifacts(layout, receipt)?;
        } else {
            let _ = remove_receipt_artifacts(layout, receipt)?;
        }
        if let Some(cache_path) = &receipt.cache_path {
            removed_cache_paths.push(cache_path.clone());
        }
    }

    let referenced_cache_paths: HashSet<String> = receipt_map
        .iter()
        .filter(|(receipt_name, _)| !removal_names_set.contains(receipt_name.as_str()))
        .filter_map(|(_, receipt)| receipt.cache_path.clone())
        .collect();
    for cache_path in removed_cache_paths {
        if referenced_cache_paths.contains(&cache_path) {
            continue;
        }
        if let Some(cache_path) = safe_cache_prune_path(layout, &cache_path) {
            remove_file_if_exists(&cache_path)
                .with_context(|| format!("failed to prune cache file: {}", cache_path.display()))?;
        }
    }

    Ok(UninstallResult {
        name: target_receipt.name,
        version: Some(target_receipt.version),
        status: target_status,
        pruned_dependencies,
        blocked_by_roots: Vec::new(),
    })
}

fn safe_cache_prune_path(layout: &PrefixLayout, cache_path: &str) -> Option<PathBuf> {
    let path = PathBuf::from(cache_path);
    if !path.is_absolute() {
        return None;
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return None;
    }

    let artifacts_dir = layout.artifacts_cache_dir();
    if !path.starts_with(&artifacts_dir) {
        return None;
    }

    Some(path)
}

fn parse_receipt(raw: &str) -> Result<InstallReceipt> {
    let mut name = None;
    let mut version = None;
    let mut dependencies = Vec::new();
    let mut target = None;
    let mut artifact_url = None;
    let mut artifact_sha256 = None;
    let mut cache_path = None;
    let mut exposed_bins = Vec::new();
    let mut snapshot_id = None;
    let mut install_reason = None;
    let mut install_status = None;
    let mut installed_at_unix = None;

    for line in raw.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k {
            "name" => name = Some(v.to_string()),
            "version" => version = Some(v.to_string()),
            "dependency" => dependencies.push(v.to_string()),
            "target" => target = Some(v.to_string()),
            "artifact_url" => artifact_url = Some(v.to_string()),
            "artifact_sha256" => artifact_sha256 = Some(v.to_string()),
            "cache_path" => cache_path = Some(v.to_string()),
            "exposed_bin" => exposed_bins.push(v.to_string()),
            "snapshot_id" => snapshot_id = Some(v.to_string()),
            "install_reason" => install_reason = Some(InstallReason::parse(v)?),
            "install_status" => install_status = Some(v.to_string()),
            "installed_at_unix" => {
                installed_at_unix = Some(v.parse().context("installed_at_unix must be u64")?)
            }
            _ => {}
        }
    }

    Ok(InstallReceipt {
        name: name.context("missing name")?,
        version: version.context("missing version")?,
        dependencies,
        target,
        artifact_url,
        artifact_sha256,
        cache_path,
        exposed_bins,
        snapshot_id,
        install_reason: install_reason.unwrap_or(InstallReason::Root),
        install_status: install_status.unwrap_or_else(|| "installed".to_string()),
        installed_at_unix: installed_at_unix.context("missing installed_at_unix")?,
    })
}

fn remove_receipt_artifacts(
    layout: &PrefixLayout,
    receipt: &InstallReceipt,
) -> Result<UninstallStatus> {
    let package_dir = layout.package_dir(&receipt.name, &receipt.version);
    let package_existed = package_dir.exists();
    if package_existed {
        fs::remove_dir_all(&package_dir)
            .with_context(|| format!("failed to remove package dir: {}", package_dir.display()))?;
    }

    for exposed_bin in &receipt.exposed_bins {
        remove_exposed_binary(layout, exposed_bin)?;
    }

    let receipt_path = layout.receipt_path(&receipt.name);
    fs::remove_file(&receipt_path).with_context(|| {
        format!(
            "failed to remove install receipt: {}",
            receipt_path.display()
        )
    })?;

    Ok(if package_existed {
        UninstallStatus::Uninstalled
    } else {
        UninstallStatus::RepairedStaleState
    })
}

fn dependency_map(receipts: &HashMap<String, InstallReceipt>) -> HashMap<String, BTreeSet<String>> {
    receipts
        .iter()
        .map(|(name, receipt)| {
            let deps = receipt
                .dependencies
                .iter()
                .filter_map(|entry| parse_dependency_name(entry))
                .filter(|dep| receipts.contains_key(*dep))
                .map(ToOwned::to_owned)
                .collect::<BTreeSet<_>>();
            (name.clone(), deps)
        })
        .collect()
}

fn parse_dependency_name(entry: &str) -> Option<&str> {
    entry.split_once('@').map(|(name, _)| name)
}

fn reachable_packages(
    roots: &[String],
    dependencies: &HashMap<String, BTreeSet<String>>,
) -> HashSet<String> {
    let mut visited = HashSet::new();
    let mut stack = roots.to_vec();
    while let Some(next) = stack.pop() {
        if !visited.insert(next.clone()) {
            continue;
        }
        if let Some(next_deps) = dependencies.get(&next) {
            stack.extend(next_deps.iter().cloned());
        }
    }
    visited
}

fn package_reachable(
    root: &str,
    target: &str,
    dependencies: &HashMap<String, BTreeSet<String>>,
) -> bool {
    let mut visited = HashSet::new();
    let mut stack = vec![root.to_string()];
    while let Some(next) = stack.pop() {
        if next == target {
            return true;
        }
        if !visited.insert(next.clone()) {
            continue;
        }
        if let Some(next_deps) = dependencies.get(&next) {
            stack.extend(next_deps.iter().cloned());
        }
    }
    false
}

pub fn bin_path(layout: &PrefixLayout, binary_name: &str) -> PathBuf {
    let mut file_name = binary_name.to_string();
    if cfg!(windows) {
        file_name.push_str(".cmd");
    }
    layout.bin_dir().join(file_name)
}

pub fn expose_binary(
    layout: &PrefixLayout,
    install_root: &Path,
    binary_name: &str,
    binary_rel_path: &str,
) -> Result<()> {
    let source_rel = validated_relative_binary_path(binary_rel_path)?;
    let source_path = install_root.join(source_rel);
    if !source_path.exists() {
        return Err(anyhow!(
            "declared binary path '{}' was not found in install root: {}",
            binary_rel_path,
            source_path.display()
        ));
    }

    let destination = bin_path(layout, binary_name);
    if destination.exists() {
        fs::remove_file(&destination).with_context(|| {
            format!(
                "failed to replace existing binary entry: {}",
                destination.display()
            )
        })?;
    }

    create_binary_entry(&source_path, &destination)
}

pub fn remove_exposed_binary(layout: &PrefixLayout, binary_name: &str) -> Result<()> {
    let destination = bin_path(layout, binary_name);
    if !destination.exists() {
        return Ok(());
    }

    fs::remove_file(&destination)
        .with_context(|| format!("failed to remove exposed binary: {}", destination.display()))?;
    Ok(())
}

fn validated_relative_binary_path(path: &str) -> Result<&Path> {
    let relative = Path::new(path);
    if relative.is_absolute() {
        return Err(anyhow!("binary path must be relative: {}", path));
    }
    if relative.as_os_str().is_empty() {
        return Err(anyhow!("binary path must not be empty"));
    }
    if relative
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(anyhow!("binary path must not include '..': {}", path));
    }
    Ok(relative)
}

fn create_binary_entry(source_path: &Path, destination: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source_path, destination).with_context(|| {
            format!(
                "failed to create symlink {} -> {}",
                destination.display(),
                source_path.display()
            )
        })
    }

    #[cfg(windows)]
    {
        let shim = format!("@echo off\r\n\"{}\" %*\r\n", source_path.display());
        fs::write(destination, shim.as_bytes())
            .with_context(|| format!("failed to write shim: {}", destination.display()))
    }
}

fn make_tmp_dir(layout: &PrefixLayout, prefix: &str) -> Result<PathBuf> {
    let mut dir = layout.tmp_state_dir();
    dir.push(format!(
        "{}-{}-{}",
        prefix,
        std::process::id(),
        current_unix_timestamp()?
    ));
    fs::create_dir_all(&dir)
        .with_context(|| format!("failed creating tmp dir: {}", dir.display()))?;
    Ok(dir)
}

fn extract_archive(archive_path: &Path, dst: &Path, archive_type: ArchiveType) -> Result<()> {
    match archive_type {
        ArchiveType::Zip => extract_zip(archive_path, dst),
        ArchiveType::TarGz | ArchiveType::TarZst => extract_tar(archive_path, dst),
    }
}

fn extract_tar(archive_path: &Path, dst: &Path) -> Result<()> {
    run_command(
        Command::new("tar")
            .arg("-xf")
            .arg(archive_path)
            .arg("-C")
            .arg(dst),
        "failed to extract tar archive",
    )
}

fn extract_zip(archive_path: &Path, dst: &Path) -> Result<()> {
    if cfg!(windows) {
        let mut command = Command::new("powershell");
        command.arg("-NoProfile").arg("-Command").arg(format!(
            "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
            escape_ps_single_quote(archive_path),
            escape_ps_single_quote(dst)
        ));
        if run_command(
            &mut command,
            "failed to extract zip archive with powershell",
        )
        .is_ok()
        {
            return Ok(());
        }
    }

    let mut unzip_command = Command::new("unzip");
    unzip_command.arg("-q").arg(archive_path).arg("-d").arg(dst);
    if run_command(
        &mut unzip_command,
        "failed to extract zip archive with unzip",
    )
    .is_ok()
    {
        return Ok(());
    }

    run_command(
        Command::new("tar")
            .arg("-xf")
            .arg(archive_path)
            .arg("-C")
            .arg(dst),
        "failed to extract zip archive with tar fallback",
    )
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

fn move_dir_or_copy(src: &Path, dst: &Path) -> Result<()> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create install parent: {}", parent.display()))?;
    }

    match fs::rename(src, dst) {
        Ok(_) => Ok(()),
        Err(_) => {
            copy_dir_recursive(src, dst)?;
            fs::remove_dir_all(src)
                .with_context(|| format!("failed to cleanup staging dir: {}", src.display()))?;
            Ok(())
        }
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst).with_context(|| format!("failed to create {}", dst.display()))?;
    for entry in fs::read_dir(src).with_context(|| format!("failed to read {}", src.display()))? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let metadata = fs::symlink_metadata(&src_path)
            .with_context(|| format!("failed to stat {}", src_path.display()))?;
        if metadata.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
            continue;
        }

        #[cfg(unix)]
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(&src_path)
                .with_context(|| format!("failed to read symlink {}", src_path.display()))?;
            std::os::unix::fs::symlink(&target, &dst_path).with_context(|| {
                format!(
                    "failed to create symlink {} -> {}",
                    dst_path.display(),
                    target.display()
                )
            })?;
            continue;
        }

        fs::copy(&src_path, &dst_path).with_context(|| {
            format!(
                "failed to copy {} to {}",
                src_path.display(),
                dst_path.display()
            )
        })?;
    }
    Ok(())
}

fn copy_with_strip(src_root: &Path, dst_root: &Path, strip_components: usize) -> Result<()> {
    let mut copied_any = false;
    copy_with_strip_recursive(
        src_root,
        src_root,
        dst_root,
        strip_components,
        &mut copied_any,
    )?;
    if !copied_any {
        return Err(anyhow!(
            "no files copied during extraction; strip_components={} may be too large",
            strip_components
        ));
    }
    Ok(())
}

fn copy_with_strip_recursive(
    src_root: &Path,
    current: &Path,
    dst_root: &Path,
    strip_components: usize,
    copied_any: &mut bool,
) -> Result<()> {
    for entry in
        fs::read_dir(current).with_context(|| format!("failed to read {}", current.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("failed to stat {}", path.display()))?;

        if metadata.is_dir() {
            copy_with_strip_recursive(src_root, &path, dst_root, strip_components, copied_any)?;
            continue;
        }

        let rel = path
            .strip_prefix(src_root)
            .with_context(|| format!("failed to relativize {}", path.display()))?;
        let stripped = strip_rel_components(rel, strip_components);
        let Some(stripped_rel) = stripped else {
            continue;
        };

        let dst_path = dst_root.join(&stripped_rel);
        if let Some(parent) = dst_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        #[cfg(unix)]
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(&path)
                .with_context(|| format!("failed to read symlink {}", path.display()))?;
            std::os::unix::fs::symlink(&target, &dst_path).with_context(|| {
                format!(
                    "failed to create symlink {} -> {}",
                    dst_path.display(),
                    target.display()
                )
            })?;
            *copied_any = true;
            continue;
        }

        fs::copy(&path, &dst_path).with_context(|| {
            format!(
                "failed to copy {} to {}",
                path.display(),
                dst_path.display()
            )
        })?;
        *copied_any = true;
    }

    Ok(())
}

fn serialize_transaction_metadata(metadata: &TransactionMetadata) -> String {
    let snapshot_id = metadata
        .snapshot_id
        .as_ref()
        .map(|value| format!("\n  \"snapshot_id\": \"{}\"", escape_json(value)))
        .unwrap_or_default();

    format!(
        "{{\n  \"version\": {},\n  \"txid\": \"{}\",\n  \"operation\": \"{}\",\n  \"status\": \"{}\",\n  \"started_at_unix\": {}{}\n}}\n",
        metadata.version,
        escape_json(&metadata.txid),
        escape_json(&metadata.operation),
        escape_json(&metadata.status),
        metadata.started_at_unix,
        snapshot_id
    )
}

fn serialize_transaction_journal_entry(entry: &TransactionJournalEntry) -> String {
    let mut fields = vec![
        format!("\"seq\":{}", entry.seq),
        format!("\"step\":\"{}\"", escape_json(&entry.step)),
        format!("\"state\":\"{}\"", escape_json(&entry.state)),
    ];
    if let Some(path) = &entry.path {
        fields.push(format!("\"path\":\"{}\"", escape_json(path)));
    }
    format!("{{{}}}", fields.join(","))
}

fn parse_transaction_metadata(raw: &str) -> Result<TransactionMetadata> {
    let mut string_fields = HashMap::new();
    let mut number_fields = HashMap::new();

    for line in raw.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if line == "{" || line == "}" {
            continue;
        }

        let normalized = line.strip_suffix(',').unwrap_or(line);
        let (raw_key, raw_value) = normalized
            .split_once(':')
            .ok_or_else(|| anyhow!("invalid transaction metadata line: {line}"))?;

        let key = raw_key.trim().trim_matches('"').to_string();
        let value = raw_value.trim();
        if value.starts_with('"') || value.ends_with('"') {
            if !(value.starts_with('"') && value.ends_with('"') && value.len() >= 2) {
                return Err(anyhow!(
                    "invalid quoted transaction metadata value for field: {key}"
                ));
            }

            let inner = &value[1..value.len() - 1];
            string_fields.insert(key, unescape_json(inner)?);
        } else {
            number_fields.insert(key, value.to_string());
        }
    }

    let parse_number = |field: &str| -> Result<u64> {
        number_fields
            .get(field)
            .with_context(|| format!("missing transaction metadata field: {field}"))?
            .parse::<u64>()
            .with_context(|| format!("invalid numeric transaction metadata field: {field}"))
    };

    Ok(TransactionMetadata {
        version: parse_number("version")? as u32,
        txid: string_fields
            .get("txid")
            .with_context(|| "missing transaction metadata field: txid")?
            .clone(),
        operation: string_fields
            .get("operation")
            .with_context(|| "missing transaction metadata field: operation")?
            .clone(),
        status: string_fields
            .get("status")
            .with_context(|| "missing transaction metadata field: status")?
            .clone(),
        started_at_unix: parse_number("started_at_unix")?,
        snapshot_id: string_fields.get("snapshot_id").cloned(),
    })
}

fn escape_json(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn unescape_json(value: &str) -> Result<String> {
    let mut out = String::new();
    let mut chars = value.chars();

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }

        let escaped = chars
            .next()
            .ok_or_else(|| anyhow!("unterminated JSON escape sequence"))?;
        match escaped {
            '\\' => out.push('\\'),
            '"' => out.push('"'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            other => {
                return Err(anyhow!("unsupported JSON escape sequence: \\{other}"));
            }
        }
    }

    Ok(out)
}

fn strip_rel_components(path: &Path, strip_components: usize) -> Option<PathBuf> {
    let components: Vec<_> = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(v) => Some(v.to_os_string()),
            _ => None,
        })
        .collect();

    if components.len() <= strip_components {
        return None;
    }

    let mut out = PathBuf::new();
    for component in components.into_iter().skip(strip_components) {
        out.push(component);
    }
    Some(out)
}

fn escape_ps_single_quote(path: &Path) -> String {
    let mut os = OsString::new();
    os.push(path.as_os_str());
    os.to_string_lossy().replace('\'', "''")
}

pub fn remove_file_if_exists(path: &Path) -> io::Result<()> {
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        append_transaction_journal_entry, bin_path, clear_active_transaction, expose_binary,
        parse_receipt, read_active_transaction, read_all_pins, read_pin, read_transaction_metadata,
        remove_exposed_binary, remove_pin, set_active_transaction, strip_rel_components,
        uninstall_package, update_transaction_status, write_install_receipt, write_pin,
        write_transaction_metadata, InstallReason, InstallReceipt, PrefixLayout,
        TransactionJournalEntry, TransactionMetadata, UninstallStatus,
    };
    use std::fs;
    use std::path::Path;

    #[test]
    fn parse_old_receipt_shape() {
        let raw = "name=fd\nversion=10.2.0\ninstalled_at_unix=123\n";
        let receipt = parse_receipt(raw).expect("must parse");
        assert_eq!(receipt.name, "fd");
        assert_eq!(receipt.version, "10.2.0");
        assert!(receipt.dependencies.is_empty());
        assert_eq!(receipt.install_status, "installed");
        assert!(receipt.target.is_none());
        assert!(receipt.snapshot_id.is_none());
        assert_eq!(receipt.install_reason, InstallReason::Root);
    }

    #[test]
    fn parse_new_receipt_shape() {
        let raw = "name=fd\nversion=10.2.0\ndependency=zlib@2.1.0\ndependency=pcre2@10.44.0\ntarget=x86_64-unknown-linux-gnu\nartifact_url=https://example.test/fd.tgz\nartifact_sha256=abc\ncache_path=/tmp/fd.tgz\nexposed_bin=fd\nexposed_bin=fdfind\nsnapshot_id=git:5f1b3d8a1f2a4d0e\ninstall_reason=dependency\ninstall_status=installed\ninstalled_at_unix=123\n";
        let receipt = parse_receipt(raw).expect("must parse");
        assert_eq!(receipt.dependencies, vec!["zlib@2.1.0", "pcre2@10.44.0"]);
        assert_eq!(receipt.target.as_deref(), Some("x86_64-unknown-linux-gnu"));
        assert_eq!(receipt.artifact_sha256.as_deref(), Some("abc"));
        assert_eq!(receipt.exposed_bins, vec!["fd", "fdfind"]);
        assert_eq!(receipt.snapshot_id.as_deref(), Some("git:5f1b3d8a1f2a4d0e"));
        assert_eq!(receipt.install_reason, InstallReason::Dependency);
    }

    #[test]
    fn transaction_paths_match_spec_layout() {
        let layout = test_layout();
        assert_eq!(
            layout.transactions_dir(),
            layout.state_dir().join("transactions")
        );
        assert_eq!(
            layout.transaction_active_path(),
            layout.state_dir().join("transactions").join("active")
        );
        assert_eq!(
            layout.transaction_metadata_path("tx-1"),
            layout.state_dir().join("transactions").join("tx-1.json")
        );
        assert_eq!(
            layout.transaction_journal_path("tx-1"),
            layout.state_dir().join("transactions").join("tx-1.journal")
        );
        assert_eq!(
            layout.transaction_staging_path("tx-1"),
            layout
                .state_dir()
                .join("transactions")
                .join("staging")
                .join("tx-1")
        );
    }

    #[test]
    fn write_transaction_metadata_and_active_file() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-1771001234-000042".to_string(),
            operation: "upgrade".to_string(),
            status: "applying".to_string(),
            started_at_unix: 1_771_001_234,
            snapshot_id: Some("git:5f1b3d8a1f2a4d0e".to_string()),
        };

        let metadata_path = write_transaction_metadata(&layout, &metadata)
            .expect("must write transaction metadata");
        set_active_transaction(&layout, &metadata.txid).expect("must write active transaction");

        let metadata_raw = fs::read_to_string(metadata_path).expect("must read metadata file");
        assert!(metadata_raw.contains("\"txid\": \"tx-1771001234-000042\""));
        assert!(metadata_raw.contains("\"operation\": \"upgrade\""));
        assert!(metadata_raw.contains("\"status\": \"applying\""));
        assert!(metadata_raw.contains("\"snapshot_id\": \"git:5f1b3d8a1f2a4d0e\""));

        let active_raw =
            fs::read_to_string(layout.transaction_active_path()).expect("must read active file");
        assert_eq!(active_raw.trim(), "tx-1771001234-000042");

        clear_active_transaction(&layout).expect("must clear active transaction");
        assert!(!layout.transaction_active_path().exists());

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn read_transaction_metadata_round_trip() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-meta-1".to_string(),
            operation: "upgrade".to_string(),
            status: "applying".to_string(),
            started_at_unix: 1_771_001_240,
            snapshot_id: Some("git:abc123".to_string()),
        };

        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        let loaded = read_transaction_metadata(&layout, "tx-meta-1")
            .expect("must read metadata")
            .expect("metadata should exist");

        assert_eq!(loaded, metadata);

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn read_transaction_metadata_rejects_truncated_quoted_value() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let txid = "tx-corrupt-quote";
        let raw = "{\n  \"version\": 1,\n  \"txid\": \",\n  \"operation\": \"install\",\n  \"status\": \"planning\",\n  \"started_at_unix\": 1771001250\n}\n";
        fs::write(layout.transaction_metadata_path(txid), raw)
            .expect("must write malformed metadata file");

        let err = read_transaction_metadata(&layout, txid)
            .expect_err("truncated quoted value should be recoverable parse error");
        let err_text = format!("{err:#}");
        assert!(
            err_text.contains("invalid quoted transaction metadata value for field: txid"),
            "unexpected error: {err_text}"
        );

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn update_transaction_status_rewrites_metadata_status() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let metadata = TransactionMetadata {
            version: 1,
            txid: "tx-status-1".to_string(),
            operation: "install".to_string(),
            status: "planning".to_string(),
            started_at_unix: 1_771_001_250,
            snapshot_id: None,
        };

        write_transaction_metadata(&layout, &metadata).expect("must write metadata");
        update_transaction_status(&layout, "tx-status-1", "applying").expect("must update status");

        let loaded = read_transaction_metadata(&layout, "tx-status-1")
            .expect("must read metadata")
            .expect("metadata should exist");
        assert_eq!(loaded.status, "applying");

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn read_active_transaction_round_trip() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        assert!(read_active_transaction(&layout)
            .expect("must read active transaction")
            .is_none());

        set_active_transaction(&layout, "tx-abc").expect("must write active transaction");
        assert_eq!(
            read_active_transaction(&layout)
                .expect("must read active transaction")
                .as_deref(),
            Some("tx-abc")
        );

        clear_active_transaction(&layout).expect("must clear active transaction");
        assert!(read_active_transaction(&layout)
            .expect("must read active transaction")
            .is_none());

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn append_transaction_journal_entries_in_order() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        append_transaction_journal_entry(
            &layout,
            "tx-1",
            &TransactionJournalEntry {
                seq: 1,
                step: "backup_receipt".to_string(),
                state: "done".to_string(),
                path: Some("state/installed/tool.receipt.bak".to_string()),
            },
        )
        .expect("must append first entry");

        append_transaction_journal_entry(
            &layout,
            "tx-1",
            &TransactionJournalEntry {
                seq: 2,
                step: "remove_package_dir".to_string(),
                state: "done".to_string(),
                path: Some("pkgs/tool/1.0.0".to_string()),
            },
        )
        .expect("must append second entry");

        let journal_raw =
            fs::read_to_string(layout.transaction_journal_path("tx-1")).expect("must read journal");
        let lines: Vec<&str> = journal_raw.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[0],
            "{\"seq\":1,\"step\":\"backup_receipt\",\"state\":\"done\",\"path\":\"state/installed/tool.receipt.bak\"}"
        );
        assert_eq!(
            lines[1],
            "{\"seq\":2,\"step\":\"remove_package_dir\",\"state\":\"done\",\"path\":\"pkgs/tool/1.0.0\"}"
        );

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn expose_and_remove_binary_round_trip() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");
        let package_dir = layout.package_dir("demo", "1.0.0");
        fs::create_dir_all(&package_dir).expect("must create package dir");
        fs::write(package_dir.join("demo"), b"#!/bin/sh\n").expect("must write binary");

        expose_binary(&layout, &package_dir, "demo", "demo").expect("must expose binary");

        let exposed_path = bin_path(&layout, "demo");
        assert!(exposed_path.exists());

        remove_exposed_binary(&layout, "demo").expect("must remove binary");
        assert!(!exposed_path.exists());

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn strip_components_behavior() {
        let p = Path::new("top/inner/bin/tool");
        assert_eq!(
            strip_rel_components(p, 1).expect("must exist"),
            Path::new("inner/bin/tool")
        );
        assert!(strip_rel_components(p, 4).is_none());
    }

    #[test]
    fn uninstall_removes_package_dir_and_receipt() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let package_dir = layout.package_dir("demo", "1.0.0");
        fs::create_dir_all(&package_dir).expect("must create package dir");
        fs::write(package_dir.join("demo.txt"), b"hello").expect("must create package file");

        write_install_receipt(
            &layout,
            &InstallReceipt {
                name: "demo".to_string(),
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
            },
        )
        .expect("must write receipt");

        let result = uninstall_package(&layout, "demo").expect("must uninstall");
        assert_eq!(result.status, UninstallStatus::Uninstalled);
        assert_eq!(result.version.as_deref(), Some("1.0.0"));
        assert!(!layout.receipt_path("demo").exists());
        assert!(!package_dir.exists());

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn uninstall_is_idempotent_when_not_installed() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let result = uninstall_package(&layout, "missing").expect("must be ok");
        assert_eq!(result.status, UninstallStatus::NotInstalled);
        assert_eq!(result.version, None);

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn uninstall_cleans_stale_receipt_when_package_is_missing() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        write_install_receipt(
            &layout,
            &InstallReceipt {
                name: "demo".to_string(),
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
            },
        )
        .expect("must write receipt");

        let result = uninstall_package(&layout, "demo").expect("must uninstall stale state");
        assert_eq!(result.status, UninstallStatus::RepairedStaleState);
        assert!(!layout.receipt_path("demo").exists());

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn uninstall_parse_failure_preserves_files() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let package_dir = layout.package_dir("demo", "1.0.0");
        fs::create_dir_all(&package_dir).expect("must create package dir");
        let receipt_path = layout.receipt_path("demo");
        fs::write(&receipt_path, b"name=demo\nversion=1.0.0\n").expect("must write malformed");

        let err = uninstall_package(&layout, "demo").expect_err("must fail on malformed receipt");
        assert!(err.to_string().contains("failed to parse install receipt"));
        assert!(receipt_path.exists());
        assert!(package_dir.exists());

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn uninstall_blocks_when_required_by_remaining_root() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        write_receipt(
            &layout,
            "app",
            "1.0.0",
            &["shared@1.0.0"],
            InstallReason::Root,
            None,
        );
        write_receipt(
            &layout,
            "shared",
            "1.0.0",
            &[],
            InstallReason::Dependency,
            None,
        );

        let result = uninstall_package(&layout, "shared").expect("must evaluate dependencies");
        assert_eq!(result.status, UninstallStatus::BlockedByDependents);
        assert_eq!(result.blocked_by_roots, vec!["app"]);
        assert!(layout.receipt_path("shared").exists());

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn uninstall_prunes_orphans_when_root_removed() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        write_receipt(
            &layout,
            "app",
            "1.0.0",
            &["shared@1.0.0"],
            InstallReason::Root,
            None,
        );
        write_receipt(
            &layout,
            "shared",
            "1.0.0",
            &[],
            InstallReason::Dependency,
            None,
        );

        let result = uninstall_package(&layout, "app").expect("must uninstall root and orphan");
        assert_eq!(result.status, UninstallStatus::Uninstalled);
        assert_eq!(result.pruned_dependencies, vec!["shared"]);
        assert!(!layout.receipt_path("app").exists());
        assert!(!layout.receipt_path("shared").exists());

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn uninstall_keeps_shared_dependency_for_other_root() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        write_receipt(
            &layout,
            "app-a",
            "1.0.0",
            &["shared@1.0.0"],
            InstallReason::Root,
            None,
        );
        write_receipt(
            &layout,
            "app-b",
            "1.0.0",
            &["shared@1.0.0"],
            InstallReason::Root,
            None,
        );
        write_receipt(
            &layout,
            "shared",
            "1.0.0",
            &[],
            InstallReason::Dependency,
            None,
        );

        let result = uninstall_package(&layout, "app-a").expect("must uninstall app-a only");
        assert_eq!(result.status, UninstallStatus::Uninstalled);
        assert!(result.pruned_dependencies.is_empty());
        assert!(!layout.receipt_path("app-a").exists());
        assert!(layout.receipt_path("app-b").exists());
        assert!(layout.receipt_path("shared").exists());

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn uninstall_prunes_unreferenced_cache_paths() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let cache_path = layout
            .cache_dir()
            .join("artifacts")
            .join("shared")
            .join("1.0.0")
            .join("x86_64-unknown-linux-gnu")
            .join("artifact.tar.zst");
        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent).expect("must create cache dir");
        }
        fs::write(&cache_path, b"artifact").expect("must create cache file");

        write_receipt(
            &layout,
            "app",
            "1.0.0",
            &["shared@1.0.0"],
            InstallReason::Root,
            None,
        );
        write_receipt(
            &layout,
            "shared",
            "1.0.0",
            &[],
            InstallReason::Dependency,
            Some(cache_path.to_string_lossy().to_string()),
        );

        let result = uninstall_package(&layout, "app").expect("must uninstall root and orphan");
        assert_eq!(result.pruned_dependencies, vec!["shared"]);
        assert!(!cache_path.exists());

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn uninstall_keeps_cache_when_still_referenced() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let cache_path = layout
            .cache_dir()
            .join("artifacts")
            .join("shared")
            .join("1.0.0")
            .join("x86_64-unknown-linux-gnu")
            .join("artifact.tar.zst");
        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent).expect("must create cache dir");
        }
        fs::write(&cache_path, b"artifact").expect("must create cache file");

        write_receipt(
            &layout,
            "app-a",
            "1.0.0",
            &["shared@1.0.0"],
            InstallReason::Root,
            None,
        );
        write_receipt(
            &layout,
            "app-b",
            "1.0.0",
            &["shared@1.0.0"],
            InstallReason::Root,
            None,
        );
        write_receipt(
            &layout,
            "shared",
            "1.0.0",
            &[],
            InstallReason::Dependency,
            Some(cache_path.to_string_lossy().to_string()),
        );

        uninstall_package(&layout, "app-a").expect("must uninstall only app-a");
        assert!(cache_path.exists());

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn uninstall_skips_pruning_cache_path_outside_artifacts_dir() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let outside_cache_path = layout.prefix().join("outside-cache-file");
        fs::write(&outside_cache_path, b"artifact").expect("must create outside cache file");

        write_receipt(
            &layout,
            "app",
            "1.0.0",
            &["shared@1.0.0"],
            InstallReason::Root,
            None,
        );
        write_receipt(
            &layout,
            "shared",
            "1.0.0",
            &[],
            InstallReason::Dependency,
            Some(outside_cache_path.to_string_lossy().to_string()),
        );

        let result = uninstall_package(&layout, "app").expect("must ignore unsafe cache prune");
        assert_eq!(result.pruned_dependencies, vec!["shared"]);
        assert!(outside_cache_path.exists());
        assert!(!layout.receipt_path("app").exists());
        assert!(!layout.receipt_path("shared").exists());

        let _ = fs::remove_dir_all(layout.prefix());
    }

    fn write_receipt(
        layout: &PrefixLayout,
        name: &str,
        version: &str,
        dependencies: &[&str],
        install_reason: InstallReason,
        cache_path: Option<String>,
    ) {
        let package_dir = layout.package_dir(name, version);
        fs::create_dir_all(&package_dir).expect("must create package dir");
        write_install_receipt(
            layout,
            &InstallReceipt {
                name: name.to_string(),
                version: version.to_string(),
                dependencies: dependencies.iter().map(|v| (*v).to_string()).collect(),
                target: None,
                artifact_url: None,
                artifact_sha256: None,
                cache_path,
                exposed_bins: Vec::new(),
                snapshot_id: None,
                install_reason,
                install_status: "installed".to_string(),
                installed_at_unix: 1,
            },
        )
        .expect("must write receipt");
    }

    fn test_layout() -> PrefixLayout {
        let mut path = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        path.push(format!(
            "crosspack-installer-tests-{}-{}",
            std::process::id(),
            nanos
        ));
        PrefixLayout::new(path)
    }

    #[test]
    fn pin_round_trip() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        write_pin(&layout, "ripgrep", "^14").expect("must write pin");
        let pin = read_pin(&layout, "ripgrep").expect("must read pin");
        assert_eq!(pin.as_deref(), Some("^14"));

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn pin_overwrite_replaces_old_value() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        write_pin(&layout, "ripgrep", "^13").expect("must write pin");
        write_pin(&layout, "ripgrep", "^14").expect("must overwrite pin");
        let pin = read_pin(&layout, "ripgrep").expect("must read pin");
        assert_eq!(pin.as_deref(), Some("^14"));

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn remove_pin_returns_false_when_missing() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        let removed = remove_pin(&layout, "missing").expect("must handle missing");
        assert!(!removed);

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn remove_pin_returns_true_when_existing() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        write_pin(&layout, "ripgrep", "^14").expect("must write pin");
        let removed = remove_pin(&layout, "ripgrep").expect("must remove existing");
        assert!(removed);
        let pin = read_pin(&layout, "ripgrep").expect("must read pin");
        assert!(pin.is_none());

        let _ = fs::remove_dir_all(layout.prefix());
    }

    #[test]
    fn read_all_pins_reads_multiple_pin_files() {
        let layout = test_layout();
        layout.ensure_base_dirs().expect("must create dirs");

        write_pin(&layout, "ripgrep", "^14").expect("pin ripgrep");
        write_pin(&layout, "fd", "^10").expect("pin fd");

        let pins = read_all_pins(&layout).expect("must read pins");
        assert_eq!(pins.get("ripgrep").map(String::as_str), Some("^14"));
        assert_eq!(pins.get("fd").map(String::as_str), Some("^10"));

        let _ = fs::remove_dir_all(layout.prefix());
    }
}
