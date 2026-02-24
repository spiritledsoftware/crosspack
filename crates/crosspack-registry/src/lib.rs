use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use crosspack_core::PackageManifest;
use crosspack_security::{sha256_hex, verify_ed25519_signature_hex};
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
    #[serde(default = "source_enabled_default")]
    pub enabled: bool,
    pub priority: u32,
}

#[derive(Debug, Clone)]
pub struct RegistrySourceStore {
    state_root: PathBuf,
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

impl RegistrySourceStore {
    pub fn new(state_root: impl Into<PathBuf>) -> Self {
        Self {
            state_root: state_root.into(),
        }
    }

    pub fn add_source(&self, source: RegistrySourceRecord) -> Result<()> {
        validate_source_name(&source.name)?;
        validate_source_fingerprint(&source.fingerprint_sha256)?;

        let mut state = self.load_state()?;
        if state
            .sources
            .iter()
            .any(|existing| existing.name == source.name)
        {
            anyhow::bail!("source '{}' already exists", source.name);
        }

        state.sources.push(source);
        sort_sources(&mut state.sources);
        self.save_state(&state)
    }

    pub fn list_sources(&self) -> Result<Vec<RegistrySourceRecord>> {
        let mut state = self.load_state()?;
        sort_sources(&mut state.sources);
        Ok(state.sources)
    }

    pub fn list_sources_with_snapshot_state(&self) -> Result<Vec<RegistrySourceWithSnapshotState>> {
        let mut state = self.load_state()?;
        sort_sources(&mut state.sources);

        let mut listed = Vec::with_capacity(state.sources.len());
        for source in state.sources {
            let cache_root = self.state_root.join("cache").join(&source.name);
            let snapshot = read_snapshot_state(&cache_root);
            listed.push(RegistrySourceWithSnapshotState { source, snapshot });
        }
        Ok(listed)
    }

    pub fn remove_source(&self, name: &str) -> Result<()> {
        let mut state = self.load_state()?;
        let before = state.sources.len();
        state.sources.retain(|source| source.name != name);
        if state.sources.len() == before {
            anyhow::bail!("source '{}' not found", name);
        }

        sort_sources(&mut state.sources);
        self.save_state(&state)
    }

    pub fn remove_source_with_cache_purge(&self, name: &str, purge_cache: bool) -> Result<()> {
        self.remove_source(name)?;
        if purge_cache {
            let cache_path = self.state_root.join("cache").join(name);
            if cache_path.exists() {
                fs::remove_dir_all(&cache_path).with_context(|| {
                    format!("failed purging source cache: {}", cache_path.display())
                })?;
            }
        }
        Ok(())
    }

    pub fn update_sources(&self, target_names: &[String]) -> Result<Vec<SourceUpdateResult>> {
        let state = self.load_state()?;
        let selected = select_update_sources(&state.sources, target_names)?;

        let mut results = Vec::with_capacity(selected.len());
        for source in selected {
            match self.update_source(&source) {
                Ok((status, snapshot_id)) => results.push(SourceUpdateResult {
                    name: source.name,
                    status,
                    snapshot_id,
                    error: None,
                }),
                Err(err) => results.push(SourceUpdateResult {
                    name: source.name,
                    status: SourceUpdateStatus::Failed,
                    snapshot_id: String::new(),
                    error: Some(format!("{err:#}")),
                }),
            }
        }

        Ok(results)
    }

    fn sources_file_path(&self) -> PathBuf {
        self.state_root.join("sources.toml")
    }

    fn load_state(&self) -> Result<RegistrySourceStateFile> {
        let path = self.sources_file_path();
        if !path.exists() {
            return Ok(RegistrySourceStateFile::default());
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed reading source state: {}", path.display()))?;
        let mut state = parse_source_state_file(&content)
            .with_context(|| format!("failed parsing source state: {}", path.display()))?;
        sort_sources(&mut state.sources);
        Ok(state)
    }

    fn save_state(&self, state: &RegistrySourceStateFile) -> Result<()> {
        fs::create_dir_all(&self.state_root).with_context(|| {
            format!(
                "failed creating source state root: {}",
                self.state_root.display()
            )
        })?;

        let path = self.sources_file_path();
        let mut state = state.clone();
        sort_sources(&mut state.sources);
        let content = toml::to_string(&state)
            .with_context(|| format!("failed serializing source state: {}", path.display()))?;
        fs::write(&path, content)
            .with_context(|| format!("failed writing source state: {}", path.display()))
    }

    fn update_source(&self, source: &RegistrySourceRecord) -> Result<(SourceUpdateStatus, String)> {
        match source.kind {
            RegistrySourceKind::Filesystem => self.update_filesystem_source(source),
            RegistrySourceKind::Git => self.update_git_source(source),
        }
    }

    fn update_filesystem_source(
        &self,
        source: &RegistrySourceRecord,
    ) -> Result<(SourceUpdateStatus, String)> {
        let staged_root = self
            .state_root
            .join(format!("tmp-{}-{}", source.name, unique_suffix()));

        let source_path = PathBuf::from(&source.location);
        if let Err(err) = copy_source_to_temp(&source_path, &staged_root, &source.name) {
            let _ = fs::remove_dir_all(&staged_root);
            return Err(err);
        }

        let snapshot_id = match compute_filesystem_snapshot_id(&staged_root) {
            Ok(snapshot_id) => snapshot_id,
            Err(err) => {
                let _ = fs::remove_dir_all(&staged_root);
                return Err(err);
            }
        };

        self.finalize_staged_source_update(source, staged_root, snapshot_id)
    }

    fn update_git_source(
        &self,
        source: &RegistrySourceRecord,
    ) -> Result<(SourceUpdateStatus, String)> {
        let staged_root = self
            .state_root
            .join(format!("tmp-{}-{}", source.name, unique_suffix()));
        let destination = self.state_root.join("cache").join(&source.name);

        let prepare_result = if destination.exists() {
            copy_source_to_temp(&destination, &staged_root, &source.name).and_then(|_| {
                run_git_command(
                    &staged_root,
                    &["fetch", "--prune", "--", source.location.as_str()],
                    &source.name,
                )?;
                run_git_command(
                    &staged_root,
                    &["reset", "--hard", "FETCH_HEAD"],
                    &source.name,
                )?;
                Ok(())
            })
        } else {
            run_git_clone(&source.location, &staged_root, &source.name)
        };

        if let Err(err) = prepare_result {
            let _ = fs::remove_dir_all(&staged_root);
            return Err(err);
        }

        let snapshot_id = match git_head_snapshot_id(&staged_root, &source.name) {
            Ok(snapshot_id) => snapshot_id,
            Err(err) => {
                let _ = fs::remove_dir_all(&staged_root);
                return Err(err);
            }
        };

        self.finalize_staged_source_update(source, staged_root, snapshot_id)
    }

    fn finalize_staged_source_update(
        &self,
        source: &RegistrySourceRecord,
        staged_root: PathBuf,
        snapshot_id: String,
    ) -> Result<(SourceUpdateStatus, String)> {
        let pipeline_result = (|| -> Result<(String, u64, Option<String>)> {
            validate_staged_registry_layout(&staged_root, &source.name)?;

            let registry_pub_path = staged_root.join("registry.pub");
            let registry_pub_raw = fs::read(&registry_pub_path).with_context(|| {
                format!(
                    "source-sync-failed: source '{}' failed reading {}",
                    source.name,
                    registry_pub_path.display()
                )
            })?;
            let actual_fingerprint = sha256_hex(&registry_pub_raw);
            if !actual_fingerprint.eq_ignore_ascii_case(&source.fingerprint_sha256) {
                anyhow::bail!(
                    "source-key-fingerprint-mismatch: source '{}' expected {}, got {}",
                    source.name,
                    source.fingerprint_sha256,
                    actual_fingerprint
                );
            }

            verify_metadata_signature_policy(&staged_root, &source.name)?;

            let manifest_count = count_manifest_files(&staged_root.join("index"))?;
            let existing_snapshot_id = read_snapshot_id(
                &self
                    .state_root
                    .join("cache")
                    .join(&source.name)
                    .join("snapshot.json"),
            );
            Ok((snapshot_id, manifest_count, existing_snapshot_id))
        })();

        if let Err(err) = pipeline_result {
            let _ = fs::remove_dir_all(&staged_root);
            return Err(err);
        }

        let (snapshot_id, manifest_count, existing_snapshot_id) = pipeline_result?;
        let cache_root = self.state_root.join("cache");
        fs::create_dir_all(&cache_root).with_context(|| {
            format!(
                "source-sync-failed: source '{}' failed creating cache root {}",
                source.name,
                cache_root.display()
            )
        })?;
        let destination = cache_root.join(&source.name);
        let backup = cache_root.join(format!(".{}-backup-{}", source.name, unique_suffix()));
        let had_existing = destination.exists();

        if had_existing {
            fs::rename(&destination, &backup).with_context(|| {
                format!(
                    "source-sync-failed: source '{}' failed backing up cache {}",
                    source.name,
                    destination.display()
                )
            })?;
        }

        if let Err(err) = fs::rename(&staged_root, &destination).with_context(|| {
            format!(
                "source-sync-failed: source '{}' failed replacing cache {}",
                source.name,
                destination.display()
            )
        }) {
            if had_existing {
                if let Err(restore_err) = fs::rename(&backup, &destination) {
                    return Err(combine_replace_restore_errors(
                        &source.name,
                        &destination,
                        &backup,
                        err,
                        restore_err,
                    ));
                }
            }
            return Err(err);
        }

        if let Err(err) =
            write_snapshot_file(&destination, &source.name, &snapshot_id, manifest_count)
        {
            let _ = fs::remove_dir_all(&destination);
            if had_existing {
                if let Err(restore_err) = fs::rename(&backup, &destination) {
                    return Err(combine_replace_restore_errors(
                        &source.name,
                        &destination,
                        &backup,
                        err,
                        restore_err,
                    ));
                }
            }
            return Err(err);
        }

        if had_existing {
            let _ = fs::remove_dir_all(&backup);
        }

        let status = if existing_snapshot_id.as_deref() == Some(snapshot_id.as_str()) {
            SourceUpdateStatus::UpToDate
        } else {
            SourceUpdateStatus::Updated
        };
        Ok((status, snapshot_id))
    }
}

fn base_git_command() -> Command {
    let mut command = Command::new("git");
    command
        .arg("-c")
        .arg("core.autocrlf=false")
        .arg("-c")
        .arg("core.eol=lf");
    if cfg!(windows) {
        command.arg("-c").arg("core.longpaths=true");
    }
    command
}

fn run_git_clone(location: &str, destination: &Path, source_name: &str) -> Result<()> {
    let output = base_git_command()
        .arg("clone")
        .arg("--")
        .arg(location)
        .arg(destination)
        .output()
        .with_context(|| {
            format!(
                "source-sync-failed: source '{}' failed launching git clone",
                source_name
            )
        })?;
    if !output.status.success() {
        anyhow::bail!(
            "source-sync-failed: source '{}' git clone failed: {}",
            source_name,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn run_git_command(repo_root: &Path, args: &[&str], source_name: &str) -> Result<()> {
    let output = base_git_command()
        .args(args)
        .current_dir(repo_root)
        .output()
        .with_context(|| {
            format!(
                "source-sync-failed: source '{}' failed launching git {}",
                source_name,
                args.join(" ")
            )
        })?;
    if !output.status.success() {
        anyhow::bail!(
            "source-sync-failed: source '{}' git {} failed: {}",
            source_name,
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn git_head_snapshot_id(repo_root: &Path, source_name: &str) -> Result<String> {
    let output = base_git_command()
        .arg("rev-parse")
        .arg("HEAD")
        .current_dir(repo_root)
        .output()
        .with_context(|| {
            format!(
                "source-sync-failed: source '{}' failed launching git rev-parse",
                source_name
            )
        })?;
    if !output.status.success() {
        anyhow::bail!(
            "source-sync-failed: source '{}' git rev-parse failed: {}",
            source_name,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let snapshot_id = String::from_utf8(output.stdout)
        .context("source-sync-failed: git rev-parse produced non-UTF-8 output")?
        .trim()
        .to_string();
    derive_snapshot_id_from_full_git_sha(&snapshot_id).with_context(|| {
        format!(
            "source-sync-failed: source '{}' git rev-parse returned invalid HEAD sha",
            source_name
        )
    })
}

fn derive_snapshot_id_from_full_git_sha(full_sha: &str) -> Result<String> {
    let normalized = full_sha.trim();
    if normalized.len() < 16 {
        anyhow::bail!("git HEAD sha too short for snapshot id: '{normalized}'");
    }
    if !normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        anyhow::bail!("git HEAD sha contains non-hex characters: '{normalized}'");
    }

    Ok(format!(
        "git:{}",
        normalized.chars().take(16).collect::<String>()
    ))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RegistrySourceStateFile {
    #[serde(default = "state_file_version")]
    version: u32,
    #[serde(default)]
    sources: Vec<RegistrySourceRecord>,
}

#[derive(Debug, Deserialize)]
struct RegistrySourceStateFileLegacy {
    #[serde(default)]
    sources: Vec<RegistrySourceRecord>,
}

impl Default for RegistrySourceStateFile {
    fn default() -> Self {
        Self {
            version: state_file_version(),
            sources: Vec::new(),
        }
    }
}

fn parse_source_state_file(content: &str) -> Result<RegistrySourceStateFile> {
    let value = toml::from_str::<toml::Value>(content)?;
    let mut state = if value.get("version").is_some() {
        let parsed = value
            .clone()
            .try_into::<RegistrySourceStateFile>()
            .context("failed parsing versioned source state")?;
        let expected = state_file_version();
        if parsed.version != expected {
            anyhow::bail!(
                "unsupported source state version {} (expected {}): update sources.toml to version {}",
                parsed.version,
                expected,
                expected
            );
        }
        parsed
    } else {
        let parsed = value
            .try_into::<RegistrySourceStateFileLegacy>()
            .context("failed parsing legacy source state")?;
        RegistrySourceStateFile {
            version: state_file_version(),
            sources: parsed.sources,
        }
    };

    validate_loaded_sources(&state.sources)?;
    state.version = state_file_version();
    Ok(state)
}

fn state_file_version() -> u32 {
    1
}

fn source_enabled_default() -> bool {
    true
}

fn sort_sources(sources: &mut [RegistrySourceRecord]) {
    sources.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| left.name.cmp(&right.name))
    });
}

fn validate_source_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 64 {
        anyhow::bail!("invalid source name: must not be empty");
    }

    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        anyhow::bail!("invalid source name: '{name}'");
    };

    let first_is_valid = first.is_ascii_lowercase() || first.is_ascii_digit();
    let rest_is_valid =
        chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_');
    if !first_is_valid || !rest_is_valid {
        anyhow::bail!("invalid source name: '{name}'");
    }

    Ok(())
}

fn validate_source_fingerprint(fingerprint: &str) -> Result<()> {
    if fingerprint.len() != 64 || !fingerprint.chars().all(|ch| ch.is_ascii_hexdigit()) {
        anyhow::bail!("invalid source fingerprint: '{fingerprint}'");
    }

    Ok(())
}

fn validate_loaded_sources(sources: &[RegistrySourceRecord]) -> Result<()> {
    let mut seen_names: HashSet<&str> = HashSet::with_capacity(sources.len());
    for source in sources {
        validate_source_name(&source.name)?;
        validate_source_fingerprint(&source.fingerprint_sha256)?;

        if !seen_names.insert(source.name.as_str()) {
            anyhow::bail!(
                "duplicate source name '{}' in sources.toml: remove or rename one entry",
                source.name
            );
        }
    }

    Ok(())
}

fn select_update_sources(
    sources: &[RegistrySourceRecord],
    target_names: &[String],
) -> Result<Vec<RegistrySourceRecord>> {
    if target_names.is_empty() {
        return Ok(sources.to_vec());
    }

    let known_names: HashSet<&str> = sources.iter().map(|source| source.name.as_str()).collect();
    for name in target_names {
        if !known_names.contains(name.as_str()) {
            anyhow::bail!("source-not-found: source '{}' not found", name);
        }
    }

    let target_set: HashSet<&str> = target_names.iter().map(String::as_str).collect();
    Ok(sources
        .iter()
        .filter(|source| target_set.contains(source.name.as_str()))
        .cloned()
        .collect())
}

fn copy_source_to_temp(source_path: &Path, staged_root: &Path, source_name: &str) -> Result<()> {
    if !source_path.exists() {
        anyhow::bail!(
            "source-sync-failed: source '{}' path does not exist: {}",
            source_name,
            source_path.display()
        );
    }

    copy_dir_recursive(source_path, staged_root).with_context(|| {
        format!(
            "source-sync-failed: source '{}' failed copying from {}",
            source_name,
            source_path.display()
        )
    })
}

fn copy_dir_recursive(source_root: &Path, destination_root: &Path) -> Result<()> {
    if !source_root.is_dir() {
        anyhow::bail!(
            "source location is not a directory: {}",
            source_root.display()
        );
    }

    if destination_root.exists() {
        fs::remove_dir_all(destination_root).with_context(|| {
            format!(
                "failed clearing temp directory {}",
                destination_root.display()
            )
        })?;
    }
    fs::create_dir_all(destination_root).with_context(|| {
        format!(
            "failed creating temp directory {}",
            destination_root.display()
        )
    })?;

    let mut queue: VecDeque<(PathBuf, PathBuf)> = VecDeque::new();
    queue.push_back((source_root.to_path_buf(), destination_root.to_path_buf()));

    while let Some((from_dir, to_dir)) = queue.pop_front() {
        for entry in fs::read_dir(&from_dir)
            .with_context(|| format!("failed reading source directory {}", from_dir.display()))?
        {
            let entry = entry?;
            let from_path = entry.path();
            let to_path = to_dir.join(entry.file_name());
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                fs::create_dir_all(&to_path)
                    .with_context(|| format!("failed creating directory {}", to_path.display()))?;
                queue.push_back((from_path, to_path));
            } else if file_type.is_file() {
                fs::copy(&from_path, &to_path).with_context(|| {
                    format!(
                        "failed copying file from {} to {}",
                        from_path.display(),
                        to_path.display()
                    )
                })?;
            }
        }
    }

    Ok(())
}

fn validate_staged_registry_layout(staged_root: &Path, source_name: &str) -> Result<()> {
    let registry_pub = staged_root.join("registry.pub");
    if !registry_pub.is_file() {
        anyhow::bail!(
            "source-snapshot-missing: source '{}' missing registry.pub in {}",
            source_name,
            staged_root.display()
        );
    }

    let index_root = staged_root.join("index");
    if !index_root.is_dir() {
        anyhow::bail!(
            "source-snapshot-missing: source '{}' missing index/ in {}",
            source_name,
            staged_root.display()
        );
    }

    Ok(())
}

fn verify_metadata_signature_policy(staged_root: &Path, source_name: &str) -> Result<()> {
    let index_root = staged_root.join("index");
    for entry in fs::read_dir(&index_root).with_context(|| {
        format!(
            "source-metadata-invalid: source '{}' failed reading index {}",
            source_name,
            index_root.display()
        )
    })? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let package = entry.file_name().to_string_lossy().to_string();
        RegistryIndex::open(staged_root)
            .package_versions(&package)
            .with_context(|| {
                format!(
                    "source-metadata-invalid: source '{}' package '{}' failed signature validation",
                    source_name, package
                )
            })?;
    }

    Ok(())
}

fn count_manifest_files(index_root: &Path) -> Result<u64> {
    let mut count = 0_u64;
    let mut queue: VecDeque<PathBuf> = VecDeque::new();
    queue.push_back(index_root.to_path_buf());

    while let Some(dir) = queue.pop_front() {
        for entry in fs::read_dir(&dir)
            .with_context(|| format!("failed reading index directory {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                queue.push_back(path);
            } else if file_type.is_file()
                && path.extension().and_then(|value| value.to_str()) == Some("toml")
            {
                count += 1;
            }
        }
    }

    Ok(count)
}

fn compute_filesystem_snapshot_id(staged_root: &Path) -> Result<String> {
    let mut file_paths = collect_relative_file_paths(staged_root)?;
    file_paths.sort();

    let mut snapshot_input = Vec::new();
    for relative_path in file_paths {
        let normalized_path = normalize_path_for_snapshot(&relative_path);
        let file_bytes = fs::read(staged_root.join(&relative_path)).with_context(|| {
            format!(
                "source-sync-failed: failed reading staged file for snapshot {}",
                staged_root.join(&relative_path).display()
            )
        })?;
        let file_digest = sha256_hex(&file_bytes);

        snapshot_input.extend_from_slice(normalized_path.as_bytes());
        snapshot_input.push(0);
        snapshot_input.extend_from_slice(file_digest.as_bytes());
        snapshot_input.push(0);
    }

    Ok(format!("fs:{}", sha256_hex(&snapshot_input)))
}

fn collect_relative_file_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let mut queue: VecDeque<PathBuf> = VecDeque::new();
    queue.push_back(root.to_path_buf());

    while let Some(dir) = queue.pop_front() {
        for entry in fs::read_dir(&dir)
            .with_context(|| format!("failed reading staged directory {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;

            if file_type.is_dir() {
                queue.push_back(path);
            } else if file_type.is_file() {
                let relative_path = path.strip_prefix(root).with_context(|| {
                    format!(
                        "failed deriving staged relative path {} from {}",
                        path.display(),
                        root.display()
                    )
                })?;
                paths.push(relative_path.to_path_buf());
            }
        }
    }

    Ok(paths)
}

fn normalize_path_for_snapshot(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn combine_replace_restore_errors(
    source_name: &str,
    destination: &Path,
    backup: &Path,
    replace_err: anyhow::Error,
    restore_err: std::io::Error,
) -> anyhow::Error {
    anyhow::anyhow!(
        "source-sync-failed: source '{}' failed replacing cache {}: {:#}; failed restoring backup {}: {}",
        source_name,
        destination.display(),
        replace_err,
        backup.display(),
        restore_err
    )
}

#[derive(Debug, Serialize, Deserialize)]
struct SourceSnapshotFile {
    version: u32,
    source: String,
    snapshot_id: String,
    updated_at_unix: u64,
    manifest_count: u64,
    status: String,
}

fn write_snapshot_file(
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

fn read_snapshot_id(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    let parsed = serde_json::from_str::<SourceSnapshotFile>(&content).ok()?;
    Some(parsed.snapshot_id)
}

fn read_snapshot_state(cache_root: &Path) -> RegistrySourceSnapshotState {
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

fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[derive(Debug, Clone)]
pub struct RegistryIndex {
    root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ConfiguredRegistryIndex {
    sources: Vec<ConfiguredSnapshotSource>,
}

#[derive(Debug, Clone)]
struct ConfiguredSnapshotSource {
    name: String,
    index: RegistryIndex,
}

impl RegistryIndex {
    pub fn open(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn search_names(&self, needle: &str) -> Result<Vec<String>> {
        let index_root = self.root.join("index");
        if !index_root.exists() {
            return Ok(Vec::new());
        }

        let mut names = Vec::new();
        for entry in fs::read_dir(index_root).context("failed to read registry index")? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.contains(needle) {
                    let manifests = self.package_versions(&name)?;
                    if !manifests.is_empty() {
                        names.push(name);
                    }
                }
            }
        }

        names.sort();
        Ok(names)
    }

    pub fn package_versions(&self, package: &str) -> Result<Vec<PackageManifest>> {
        let package_dir = self.root.join("index").join(package);
        if !package_dir.exists() {
            return Ok(Vec::new());
        }

        let trusted_key_path = self.root.join("registry.pub");
        let trusted_public_key_hex = fs::read_to_string(&trusted_key_path).with_context(|| {
            format!(
                "failed to read trusted registry key: {}",
                trusted_key_path.display()
            )
        })?;
        let trusted_public_key_hex = trusted_public_key_hex.trim();
        let key_identifier: String = trusted_public_key_hex.chars().take(16).collect();

        let mut manifests = Vec::new();
        for entry in fs::read_dir(&package_dir)
            .with_context(|| format!("failed to read package directory: {package}"))?
        {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }

            let path = entry.path();
            if path.extension().and_then(|v| v.to_str()) != Some("toml") {
                continue;
            }

            let manifest_bytes = fs::read(&path)
                .with_context(|| format!("failed reading manifest: {}", path.display()))?;

            let signature_path = path.with_extension("toml.sig");
            let signature_hex = fs::read_to_string(&signature_path).with_context(|| {
                format!(
                    "failed reading manifest signature for key {}: {}",
                    key_identifier,
                    signature_path.display()
                )
            })?;
            let signature_hex = signature_hex.trim();

            let signature_is_valid = verify_ed25519_signature_hex(
                &manifest_bytes,
                trusted_public_key_hex,
                signature_hex,
            )
            .with_context(|| {
                format!(
                    "failed verifying manifest signature for key {}: {}",
                    key_identifier,
                    signature_path.display()
                )
            })?;
            if !signature_is_valid {
                anyhow::bail!(
                    "invalid manifest signature for key {}: manifest {}, signature {}",
                    key_identifier,
                    path.display(),
                    signature_path.display()
                );
            }

            let content = String::from_utf8(manifest_bytes)
                .with_context(|| format!("manifest is not valid UTF-8: {}", path.display()))?;
            let manifest = PackageManifest::from_toml_str(&content)
                .with_context(|| format!("failed parsing manifest: {}", path.display()))?;
            manifests.push(manifest);
        }

        manifests.sort_by(|a, b| b.version.cmp(&a.version));
        Ok(manifests)
    }
}

impl ConfiguredRegistryIndex {
    pub fn open(state_root: impl Into<PathBuf>) -> Result<Self> {
        let state_root = state_root.into();
        let sources_path = state_root.join("sources.toml");
        let (state, has_sources_file) = match fs::read_to_string(&sources_path) {
            Ok(content) => {
                let state = parse_source_state_file(&content).with_context(|| {
                    format!(
                        "failed parsing configured registry sources: {}",
                        sources_path.display()
                    )
                })?;
                (state, true)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                (RegistrySourceStateFile::default(), false)
            }
            Err(err) => {
                return Err(err).with_context(|| {
                    format!(
                        "failed reading configured registry sources: {}",
                        sources_path.display()
                    )
                });
            }
        };

        let mut enabled_sources: Vec<RegistrySourceRecord> = state
            .sources
            .into_iter()
            .filter(|source| source.enabled)
            .collect();
        let enabled_count = enabled_sources.len();
        sort_sources(&mut enabled_sources);

        let mut configured = Vec::new();
        for source in enabled_sources {
            let cache_root = state_root.join("cache").join(&source.name);
            if !source_has_ready_snapshot(&cache_root)? {
                continue;
            }
            configured.push(ConfiguredSnapshotSource {
                name: source.name,
                index: RegistryIndex::open(cache_root),
            });
        }

        if !configured.is_empty() {
            return Ok(Self {
                sources: configured,
            });
        }

        if !has_sources_file || enabled_count == 0 {
            return Ok(Self {
                sources: Vec::new(),
            });
        }

        anyhow::bail!("no ready snapshot exists for enabled sources")
    }

    pub fn search_names(&self, needle: &str) -> Result<Vec<String>> {
        let mut deduped = HashSet::new();
        for source in &self.sources {
            for name in source.index.search_names(needle)? {
                deduped.insert(name);
            }
        }

        let mut names: Vec<String> = deduped.into_iter().collect();
        names.sort();
        Ok(names)
    }

    pub fn package_versions(&self, package: &str) -> Result<Vec<PackageManifest>> {
        if let Some((_, manifests)) = self.package_versions_with_source(package)? {
            return Ok(manifests);
        }
        Ok(Vec::new())
    }

    pub fn package_versions_with_source(
        &self,
        package: &str,
    ) -> Result<Option<(String, Vec<PackageManifest>)>> {
        for source in &self.sources {
            let manifests = source.index.package_versions(package).with_context(|| {
                format!(
                    "failed loading package '{package}' from configured source '{}'",
                    source.name
                )
            })?;
            if !manifests.is_empty() {
                return Ok(Some((source.name.clone(), manifests)));
            }
        }
        Ok(None)
    }
}

fn source_has_ready_snapshot(cache_root: &Path) -> Result<bool> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn source_store_add_rejects_duplicate_name() {
        let root = test_registry_root();
        let store = RegistrySourceStore::new(&root);

        store
            .add_source(source_record("official", 10))
            .expect("must add source");
        let err = store
            .add_source(source_record("official", 5))
            .expect_err("must reject duplicate source name");
        assert!(err.to_string().contains("already exists"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_add_rejects_invalid_name() {
        let root = test_registry_root();
        let store = RegistrySourceStore::new(&root);

        let err = store
            .add_source(source_record("bad name", 10))
            .expect_err("must reject invalid source name");
        assert!(err.to_string().contains("invalid source name"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_add_rejects_name_with_leading_separator() {
        let root = test_registry_root();
        let store = RegistrySourceStore::new(&root);

        let err = store
            .add_source(source_record("-bad", 10))
            .expect_err("must reject invalid source name");
        assert!(err.to_string().contains("invalid source name"));

        let err = store
            .add_source(source_record("_bad", 10))
            .expect_err("must reject invalid source name");
        assert!(err.to_string().contains("invalid source name"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_add_rejects_name_longer_than_sixty_four_characters() {
        let root = test_registry_root();
        let store = RegistrySourceStore::new(&root);

        let too_long_name = "a".repeat(65);
        let err = store
            .add_source(source_record(&too_long_name, 10))
            .expect_err("must reject invalid source name");
        assert!(err.to_string().contains("invalid source name"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_add_rejects_invalid_fingerprint() {
        let root = test_registry_root();
        let store = RegistrySourceStore::new(&root);

        let mut record = source_record("official", 10);
        record.fingerprint_sha256 = "xyz".to_string();
        let err = store
            .add_source(record)
            .expect_err("must reject invalid fingerprint");
        assert!(err.to_string().contains("invalid source fingerprint"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_list_sorts_by_priority_then_name() {
        let root = test_registry_root();
        let store = RegistrySourceStore::new(&root);

        store
            .add_source(source_record("zeta", 10))
            .expect("must add source");
        store
            .add_source(source_record("alpha", 1))
            .expect("must add source");
        store
            .add_source(source_record("beta", 10))
            .expect("must add source");

        let listed = store.list_sources().expect("must list sources");
        let names: Vec<&str> = listed.iter().map(|record| record.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "beta", "zeta"]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_list_sources_with_snapshot_state_returns_none_when_snapshot_missing() {
        let root = test_registry_root();
        let store = RegistrySourceStore::new(&root);
        store
            .add_source(source_record("official", 1))
            .expect("must add source");

        let listed = store
            .list_sources_with_snapshot_state()
            .expect("must list source states");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].source.name, "official");
        assert_eq!(listed[0].snapshot, RegistrySourceSnapshotState::None);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_list_sources_with_snapshot_state_returns_ready_snapshot() {
        let root = test_registry_root();
        let store = RegistrySourceStore::new(&root);
        store
            .add_source(source_record("official", 1))
            .expect("must add source");

        let cache_root = root.join("cache").join("official");
        fs::create_dir_all(&cache_root).expect("must create cache root");
        fs::write(
            cache_root.join("snapshot.json"),
            r#"{
  "version": 1,
  "source": "official",
  "snapshot_id": "git:0123456789abcdef",
  "updated_at_unix": 1,
  "manifest_count": 0,
  "status": "ready"
}"#,
        )
        .expect("must write snapshot file");

        let listed = store
            .list_sources_with_snapshot_state()
            .expect("must list source states");
        assert_eq!(listed.len(), 1);
        assert_eq!(
            listed[0].snapshot,
            RegistrySourceSnapshotState::Ready {
                snapshot_id: "git:0123456789abcdef".to_string()
            }
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_list_sources_with_snapshot_state_maps_invalid_snapshot_to_error_reason_code() {
        let root = test_registry_root();
        let store = RegistrySourceStore::new(&root);
        store
            .add_source(source_record("official", 1))
            .expect("must add source");

        let cache_root = root.join("cache").join("official");
        fs::create_dir_all(&cache_root).expect("must create cache root");
        fs::write(cache_root.join("snapshot.json"), "{not-json")
            .expect("must write invalid snapshot");

        let listed = store
            .list_sources_with_snapshot_state()
            .expect("must list source states");
        assert_eq!(listed.len(), 1);
        assert_eq!(
            listed[0].snapshot,
            RegistrySourceSnapshotState::Error {
                status: RegistrySourceWithSnapshotStatus::Unreadable,
                reason_code: "snapshot-unreadable".to_string(),
            }
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_remove_reports_missing_source() {
        let root = test_registry_root();
        let store = RegistrySourceStore::new(&root);

        let err = store
            .remove_source("missing")
            .expect_err("must report missing source");
        assert!(err.to_string().contains("not found"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_remove_with_cache_purge_removes_source_cache_directory() {
        let root = test_registry_root();
        let store = RegistrySourceStore::new(&root);
        store
            .add_source(source_record("official", 1))
            .expect("must add source");

        let cache_path = root.join("cache").join("official");
        fs::create_dir_all(&cache_path).expect("must create cache path");
        fs::write(cache_path.join("snapshot.json"), "{}").expect("must write cache fixture file");

        store
            .remove_source_with_cache_purge("official", true)
            .expect("must remove source and cache");

        assert!(!cache_path.exists(), "cache path must be removed");
        assert!(
            store
                .list_sources()
                .expect("must list sources")
                .into_iter()
                .all(|source| source.name != "official"),
            "source must be removed from source state"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_remove_without_cache_purge_keeps_source_cache_directory() {
        let root = test_registry_root();
        let store = RegistrySourceStore::new(&root);
        store
            .add_source(source_record("official", 1))
            .expect("must add source");

        let cache_path = root.join("cache").join("official");
        fs::create_dir_all(&cache_path).expect("must create cache path");
        fs::write(cache_path.join("snapshot.json"), "{}").expect("must write cache fixture file");

        store
            .remove_source_with_cache_purge("official", false)
            .expect("must remove source and keep cache");

        assert!(
            cache_path.exists(),
            "cache path must remain when purge disabled"
        );
        assert!(
            store
                .list_sources()
                .expect("must list sources")
                .into_iter()
                .all(|source| source.name != "official"),
            "source must be removed from source state"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_writes_versioned_sorted_state_file() {
        let root = test_registry_root();
        let store = RegistrySourceStore::new(&root);

        store
            .add_source(source_record("zeta", 10))
            .expect("must add source");
        store
            .add_source(source_record("alpha", 0))
            .expect("must add source");
        store
            .add_source(source_record("beta", 10))
            .expect("must add source");

        let content = fs::read_to_string(root.join("sources.toml")).expect("must read state file");
        let expected = concat!(
            "version = 1\n",
            "\n",
            "[[sources]]\n",
            "name = \"alpha\"\n",
            "kind = \"git\"\n",
            "location = \"https://example.com/alpha.git\"\n",
            "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
            "enabled = true\n",
            "priority = 0\n",
            "\n",
            "[[sources]]\n",
            "name = \"beta\"\n",
            "kind = \"git\"\n",
            "location = \"https://example.com/beta.git\"\n",
            "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
            "enabled = true\n",
            "priority = 10\n",
            "\n",
            "[[sources]]\n",
            "name = \"zeta\"\n",
            "kind = \"git\"\n",
            "location = \"https://example.com/zeta.git\"\n",
            "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
            "enabled = true\n",
            "priority = 10\n"
        );
        assert_eq!(content, expected);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_loads_unversioned_state_file_for_back_compat() {
        let root = test_registry_root();
        fs::create_dir_all(&root).expect("must create state root");
        fs::write(
            root.join("sources.toml"),
            concat!(
                "[[sources]]\n",
                "name = \"zeta\"\n",
                "kind = \"git\"\n",
                "location = \"https://example.com/zeta.git\"\n",
                "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
                "priority = 10\n",
                "\n",
                "[[sources]]\n",
                "name = \"alpha\"\n",
                "kind = \"git\"\n",
                "location = \"https://example.com/alpha.git\"\n",
                "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
                "priority = 0\n"
            ),
        )
        .expect("must write legacy state file");

        let store = RegistrySourceStore::new(&root);
        let listed = store.list_sources().expect("must list sources");
        let names: Vec<&str> = listed.iter().map(|record| record.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "zeta"]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_rejects_negative_priority_in_legacy_state() {
        let root = test_registry_root();
        fs::create_dir_all(&root).expect("must create state root");
        fs::write(
            root.join("sources.toml"),
            concat!(
                "[[sources]]\n",
                "name = \"official\"\n",
                "kind = \"git\"\n",
                "location = \"https://example.com/official.git\"\n",
                "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
                "priority = -1\n"
            ),
        )
        .expect("must write legacy state file");

        let store = RegistrySourceStore::new(&root);
        let err = store
            .list_sources()
            .expect_err("must reject negative source priority");
        assert!(err.to_string().contains("failed parsing source state"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_rejects_duplicate_names_in_loaded_state() {
        let root = test_registry_root();
        fs::create_dir_all(&root).expect("must create state root");
        fs::write(
            root.join("sources.toml"),
            concat!(
                "version = 1\n",
                "\n",
                "[[sources]]\n",
                "name = \"official\"\n",
                "kind = \"git\"\n",
                "location = \"https://example.com/official.git\"\n",
                "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
                "priority = 1\n",
                "\n",
                "[[sources]]\n",
                "name = \"official\"\n",
                "kind = \"git\"\n",
                "location = \"https://example.com/official-mirror.git\"\n",
                "fingerprint_sha256 = \"fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210\"\n",
                "priority = 2\n"
            ),
        )
        .expect("must write duplicate source state file");

        let store = RegistrySourceStore::new(&root);
        let err = store
            .list_sources()
            .expect_err("must reject duplicate source names from disk");
        let rendered = format!("{err:#}");
        assert!(rendered.contains("duplicate source name 'official'"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_rejects_invalid_fingerprint_in_loaded_state() {
        let root = test_registry_root();
        fs::create_dir_all(&root).expect("must create state root");
        fs::write(
            root.join("sources.toml"),
            concat!(
                "version = 1\n",
                "\n",
                "[[sources]]\n",
                "name = \"official\"\n",
                "kind = \"git\"\n",
                "location = \"https://example.com/official.git\"\n",
                "fingerprint_sha256 = \"xyz\"\n",
                "priority = 1\n"
            ),
        )
        .expect("must write invalid fingerprint state file");

        let store = RegistrySourceStore::new(&root);
        let err = store
            .list_sources()
            .expect_err("must reject invalid source fingerprint from disk");
        let rendered = format!("{err:#}");
        assert!(rendered.contains("invalid source fingerprint"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_rejects_unknown_state_file_version() {
        let root = test_registry_root();
        fs::create_dir_all(&root).expect("must create state root");
        fs::write(
            root.join("sources.toml"),
            concat!(
                "version = 7\n",
                "\n",
                "[[sources]]\n",
                "name = \"official\"\n",
                "kind = \"git\"\n",
                "location = \"https://example.com/official.git\"\n",
                "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
                "priority = 1\n"
            ),
        )
        .expect("must write unknown version state file");

        let store = RegistrySourceStore::new(&root);
        let err = store
            .list_sources()
            .expect_err("must reject unknown source state schema version");
        let rendered = format!("{err:#}");
        assert!(
            rendered.contains("unsupported source state version"),
            "expected explicit unsupported version error, got: {err}"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_defaults_enabled_to_true_when_missing_in_loaded_state() {
        let root = test_registry_root();
        fs::create_dir_all(&root).expect("must create state root");
        fs::write(
            root.join("sources.toml"),
            concat!(
                "version = 1\n",
                "\n",
                "[[sources]]\n",
                "name = \"official\"\n",
                "kind = \"git\"\n",
                "location = \"https://example.com/official.git\"\n",
                "fingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n",
                "priority = 1\n"
            ),
        )
        .expect("must write state file without enabled flag");

        let store = RegistrySourceStore::new(&root);
        store
            .add_source(source_record("mirror", 2))
            .expect("must add source");

        let content = fs::read_to_string(root.join("sources.toml")).expect("must read state file");
        assert!(
            content.contains(
                "name = \"official\"\nkind = \"git\"\nlocation = \"https://example.com/official.git\"\nfingerprint_sha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\nenabled = true\npriority = 1\n"
            ),
            "expected missing enabled flag to default to true when persisted\n{content}"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn source_store_first_write_from_empty_uses_version_one() {
        let root = test_registry_root();
        let store = RegistrySourceStore::new(&root);

        store
            .add_source(source_record("official", 0))
            .expect("must add first source");

        let content = fs::read_to_string(root.join("sources.toml")).expect("must read state file");
        assert!(
            content.starts_with("version = 1\n"),
            "expected first write to persist version 1\n{content}"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn update_filesystem_source_writes_ready_snapshot() {
        let root = test_registry_root();
        let source_root = filesystem_source_fixture();
        let store = RegistrySourceStore::new(&root);

        let registry_pub =
            fs::read(source_root.join("registry.pub")).expect("must read registry pub");
        store
            .add_source(filesystem_source_record(
                "local",
                source_root
                    .to_str()
                    .expect("filesystem source path must be valid UTF-8"),
                sha256_hex_bytes(&registry_pub),
                0,
            ))
            .expect("must add source");

        let results = store.update_sources(&[]).expect("must update source");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "local");
        assert_eq!(results[0].status, SourceUpdateStatus::Updated);

        let cache_root = root.join("cache").join("local");
        assert!(cache_root.join("registry.pub").exists());
        assert!(cache_root.join("index").exists());

        let snapshot_path = cache_root.join("snapshot.json");
        let snapshot_content =
            fs::read_to_string(&snapshot_path).expect("must write snapshot.json");
        let snapshot: serde_json::Value =
            serde_json::from_str(&snapshot_content).expect("must parse snapshot.json");
        assert_eq!(snapshot["source"], "local");
        assert_eq!(snapshot["status"], "ready");
        assert_eq!(snapshot["manifest_count"], 1);
        assert_eq!(snapshot["snapshot_id"], results[0].snapshot_id.as_str());

        let _ = fs::remove_dir_all(&source_root);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn update_filesystem_source_fails_on_fingerprint_mismatch() {
        let root = test_registry_root();
        let source_root = filesystem_source_fixture();
        let store = RegistrySourceStore::new(&root);

        store
            .add_source(filesystem_source_record(
                "local",
                source_root
                    .to_str()
                    .expect("filesystem source path must be valid UTF-8"),
                "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
                0,
            ))
            .expect("must add source");

        let results = store
            .update_sources(&[])
            .expect("update API must report per-source failure");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, SourceUpdateStatus::Failed);
        assert!(results[0]
            .error
            .as_deref()
            .expect("must include error message")
            .contains("source-key-fingerprint-mismatch"));
        assert!(!root.join("cache").join("local").exists());

        let _ = fs::remove_dir_all(&source_root);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn update_filesystem_source_accepts_uppercase_configured_fingerprint() {
        let root = test_registry_root();
        let source_root = filesystem_source_fixture();
        let store = RegistrySourceStore::new(&root);

        let registry_pub =
            fs::read(source_root.join("registry.pub")).expect("must read registry pub");
        store
            .add_source(filesystem_source_record(
                "local",
                source_root
                    .to_str()
                    .expect("filesystem source path must be valid UTF-8"),
                sha256_hex_bytes(&registry_pub).to_uppercase(),
                0,
            ))
            .expect("must add source");

        let results = store.update_sources(&[]).expect("must update source");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, SourceUpdateStatus::Updated);

        let _ = fs::remove_dir_all(&source_root);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn update_filesystem_source_preserves_existing_cache_on_failure() {
        let root = test_registry_root();
        let source_root = filesystem_source_fixture();
        let store = RegistrySourceStore::new(&root);

        let registry_pub =
            fs::read(source_root.join("registry.pub")).expect("must read registry pub");
        let fingerprint = sha256_hex_bytes(&registry_pub);
        store
            .add_source(filesystem_source_record(
                "local",
                source_root
                    .to_str()
                    .expect("filesystem source path must be valid UTF-8"),
                fingerprint,
                0,
            ))
            .expect("must add source");

        let first = store
            .update_sources(&[])
            .expect("must update source initially");
        assert_eq!(first[0].status, SourceUpdateStatus::Updated);

        fs::remove_file(
            source_root
                .join("index")
                .join("ripgrep")
                .join("14.1.0.toml.sig"),
        )
        .expect("must remove signature to force verification failure");

        let second = store
            .update_sources(&[])
            .expect("update API must report per-source failure");
        assert_eq!(second[0].status, SourceUpdateStatus::Failed);

        let cached_signature = root
            .join("cache")
            .join("local")
            .join("index")
            .join("ripgrep")
            .join("14.1.0.toml.sig");
        assert!(
            cached_signature.exists(),
            "must preserve previous verified cache on update failure"
        );

        let _ = fs::remove_dir_all(&source_root);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn update_filesystem_source_reports_updated_when_manifest_changes_with_same_key() {
        let root = test_registry_root();
        let source_root = filesystem_source_fixture();
        let store = RegistrySourceStore::new(&root);

        let registry_pub =
            fs::read(source_root.join("registry.pub")).expect("must read registry pub");
        let fingerprint = sha256_hex_bytes(&registry_pub);
        store
            .add_source(filesystem_source_record(
                "local",
                source_root
                    .to_str()
                    .expect("filesystem source path must be valid UTF-8"),
                fingerprint,
                0,
            ))
            .expect("must add source");

        let first = store
            .update_sources(&[])
            .expect("first update must succeed");
        assert_eq!(first[0].status, SourceUpdateStatus::Updated);

        rewrite_signed_manifest_with_extra_field(
            &source_root
                .join("index")
                .join("ripgrep")
                .join("14.1.0.toml"),
            &signing_key(),
            "description = \"updated\"\n",
        );

        let second = store
            .update_sources(&[])
            .expect("second update must succeed");
        assert_eq!(second[0].status, SourceUpdateStatus::Updated);

        let _ = fs::remove_dir_all(&source_root);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn update_git_source_clones_and_records_snapshot_id() {
        let root = test_registry_root();
        let source_root = git_source_fixture();
        let store = RegistrySourceStore::new(&root);

        let registry_pub =
            fs::read(source_root.join("registry.pub")).expect("must read registry pub");
        let source_location = git_fixture_location(&source_root);
        store
            .add_source(git_source_record(
                "origin",
                &source_location,
                sha256_hex_bytes(&registry_pub),
                0,
            ))
            .expect("must add source");

        let expected_snapshot_id = git_head_short(&source_root);
        let results = store.update_sources(&[]).expect("must update source");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, SourceUpdateStatus::Updated);
        assert_eq!(results[0].snapshot_id, expected_snapshot_id);

        let cache_root = root.join("cache").join("origin");
        assert!(cache_root.join("registry.pub").exists());
        assert!(cache_root.join("index").exists());
        assert_eq!(git_head_short(&cache_root), expected_snapshot_id);

        let _ = fs::remove_dir_all(&source_root);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn update_git_source_fetches_new_commit() {
        let root = test_registry_root();
        let source_root = git_source_fixture();
        let store = RegistrySourceStore::new(&root);

        let registry_pub =
            fs::read(source_root.join("registry.pub")).expect("must read registry pub");
        let source_location = git_fixture_location(&source_root);
        store
            .add_source(git_source_record(
                "origin",
                &source_location,
                sha256_hex_bytes(&registry_pub),
                0,
            ))
            .expect("must add source");

        let first = store
            .update_sources(&[])
            .expect("first update must succeed");
        assert_eq!(first[0].status, SourceUpdateStatus::Updated);

        rewrite_signed_manifest_with_extra_field(
            &source_root
                .join("index")
                .join("ripgrep")
                .join("14.1.0.toml"),
            &signing_key(),
            "description = \"updated\"\n",
        );
        git_commit_all(&source_root, "update ripgrep manifest");

        let expected_snapshot_id = git_head_short(&source_root);
        let second = store
            .update_sources(&[])
            .expect("second update must succeed");
        assert_eq!(second[0].status, SourceUpdateStatus::Updated);
        assert_eq!(second[0].snapshot_id, expected_snapshot_id);

        let cached_manifest = fs::read_to_string(
            root.join("cache")
                .join("origin")
                .join("index")
                .join("ripgrep")
                .join("14.1.0.toml"),
        )
        .expect("must read cached manifest");
        assert!(cached_manifest.contains("description = \"updated\""));

        let _ = fs::remove_dir_all(&source_root);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn update_git_source_returns_up_to_date_when_revision_unchanged() {
        let root = test_registry_root();
        let source_root = git_source_fixture();
        let store = RegistrySourceStore::new(&root);

        let registry_pub =
            fs::read(source_root.join("registry.pub")).expect("must read registry pub");
        let source_location = git_fixture_location(&source_root);
        store
            .add_source(git_source_record(
                "origin",
                &source_location,
                sha256_hex_bytes(&registry_pub),
                0,
            ))
            .expect("must add source");

        let first = store
            .update_sources(&[])
            .expect("first update must succeed");
        assert_eq!(first[0].status, SourceUpdateStatus::Updated);

        let second = store
            .update_sources(&[])
            .expect("second update must succeed");
        assert_eq!(second[0].status, SourceUpdateStatus::UpToDate);
        assert_eq!(second[0].snapshot_id, first[0].snapshot_id);

        let _ = fs::remove_dir_all(&source_root);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn git_snapshot_id_derives_source_prefixed_fixed_width_prefix_from_full_sha() {
        let full_sha = "0123456789abcdef0123456789abcdef01234567";
        let snapshot_id =
            derive_snapshot_id_from_full_git_sha(full_sha).expect("must derive snapshot id");
        assert_eq!(snapshot_id, "git:0123456789abcdef");
        assert_eq!(snapshot_id.len(), 20);
    }

    #[test]
    fn git_snapshot_id_derivation_rejects_short_sha() {
        let err = derive_snapshot_id_from_full_git_sha("0123456789abcde")
            .expect_err("must reject sha values shorter than sixteen characters");
        assert!(err.to_string().contains("too short"));
    }

    #[test]
    fn rollback_replace_error_includes_restore_failure_context() {
        let replace_err = anyhow::anyhow!("replace failed");
        let restore_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");

        let err = combine_replace_restore_errors(
            "local",
            Path::new("/tmp/cache/local"),
            Path::new("/tmp/cache/.local-backup"),
            replace_err,
            restore_err,
        );

        let rendered = format!("{err:#}");
        assert!(rendered.contains("failed replacing cache"));
        assert!(rendered.contains("failed restoring backup"));
        assert!(rendered.contains("denied"));
    }

    #[test]
    fn update_unknown_source_returns_source_not_found() {
        let root = test_registry_root();
        let source_root = filesystem_source_fixture();
        let store = RegistrySourceStore::new(&root);

        let registry_pub =
            fs::read(source_root.join("registry.pub")).expect("must read registry pub");
        store
            .add_source(filesystem_source_record(
                "local",
                source_root
                    .to_str()
                    .expect("filesystem source path must be valid UTF-8"),
                sha256_hex_bytes(&registry_pub),
                0,
            ))
            .expect("must add source");

        let err = store
            .update_sources(&[String::from("missing")])
            .expect_err("must reject unknown source names");
        assert!(err.to_string().contains("source-not-found"));

        let _ = fs::remove_dir_all(&source_root);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn search_names_fails_when_registry_public_key_is_missing() {
        let root = test_registry_root();
        let package_dir = root.join("index").join("ripgrep");
        fs::create_dir_all(&package_dir).expect("must create package dir");
        fs::write(
            package_dir.join("14.1.0.toml"),
            manifest_toml("ripgrep", "14.1.0"),
        )
        .expect("must write manifest");

        let index = RegistryIndex::open(&root);
        let err = index
            .search_names("rip")
            .expect_err("must fail when registry.pub is missing");
        assert!(err.to_string().contains("registry.pub"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn configured_index_package_versions_prefers_higher_priority_source() {
        let state_root = test_registry_root();
        let store = RegistrySourceStore::new(&state_root);

        store
            .add_source(source_record("fallback", 10))
            .expect("must add fallback source");
        store
            .add_source(source_record("preferred", 0))
            .expect("must add preferred source");

        let fallback_key = SigningKey::from_bytes(&[11u8; 32]);
        let preferred_key = SigningKey::from_bytes(&[13u8; 32]);
        write_ready_snapshot_cache(&state_root, "fallback", &fallback_key, &["14.0.0"]);
        write_ready_snapshot_cache(&state_root, "preferred", &preferred_key, &["14.1.0"]);

        let index = ConfiguredRegistryIndex::open(&state_root).expect("must open configured index");
        let manifests = index
            .package_versions("ripgrep")
            .expect("must read package from highest precedence source");

        let versions: Vec<String> = manifests
            .iter()
            .map(|manifest| manifest.version.to_string())
            .collect();
        assert_eq!(versions, vec!["14.1.0"]);

        let _ = fs::remove_dir_all(&state_root);
    }

    #[test]
    fn configured_index_package_versions_uses_name_tiebreaker() {
        let state_root = test_registry_root();
        let store = RegistrySourceStore::new(&state_root);

        store
            .add_source(source_record("beta", 1))
            .expect("must add beta source");
        store
            .add_source(source_record("alpha", 1))
            .expect("must add alpha source");

        let beta_key = SigningKey::from_bytes(&[17u8; 32]);
        let alpha_key = SigningKey::from_bytes(&[19u8; 32]);
        write_ready_snapshot_cache(&state_root, "beta", &beta_key, &["14.0.0"]);
        write_ready_snapshot_cache(&state_root, "alpha", &alpha_key, &["14.2.0"]);

        let index = ConfiguredRegistryIndex::open(&state_root).expect("must open configured index");
        let manifests = index
            .package_versions("ripgrep")
            .expect("must apply source-name tie-breaker");

        let versions: Vec<String> = manifests
            .iter()
            .map(|manifest| manifest.version.to_string())
            .collect();
        assert_eq!(versions, vec!["14.2.0"]);

        let _ = fs::remove_dir_all(&state_root);
    }

    #[test]
    fn configured_index_search_names_deduplicates_across_sources() {
        let state_root = test_registry_root();
        let store = RegistrySourceStore::new(&state_root);

        store
            .add_source(source_record("one", 0))
            .expect("must add first source");
        store
            .add_source(source_record("two", 1))
            .expect("must add second source");

        let first_key = SigningKey::from_bytes(&[23u8; 32]);
        let second_key = SigningKey::from_bytes(&[29u8; 32]);
        write_ready_snapshot_cache(&state_root, "one", &first_key, &["14.1.0"]);
        write_ready_snapshot_cache(&state_root, "two", &second_key, &["14.0.0"]);

        let index = ConfiguredRegistryIndex::open(&state_root).expect("must open configured index");
        let names = index
            .search_names("rip")
            .expect("must deduplicate package names");
        assert_eq!(names, vec!["ripgrep"]);

        let _ = fs::remove_dir_all(&state_root);
    }

    #[test]
    fn configured_index_fails_when_no_ready_snapshot_exists() {
        let state_root = test_registry_root();
        let store = RegistrySourceStore::new(&state_root);

        store
            .add_source(source_record("official", 0))
            .expect("must add source");

        let err = ConfiguredRegistryIndex::open(&state_root)
            .expect_err("must fail when no enabled source has a ready snapshot");
        assert!(err.to_string().contains("no ready snapshot"));

        let _ = fs::remove_dir_all(&state_root);
    }

    #[test]
    fn configured_index_open_fails_when_sources_file_is_unreadable() {
        let state_root = test_registry_root();
        fs::create_dir_all(state_root.join("sources.toml"))
            .expect("must make sources path unreadable");

        let err = ConfiguredRegistryIndex::open(&state_root)
            .expect_err("must fail when sources state cannot be read");
        assert!(err
            .to_string()
            .contains("failed reading configured registry sources"));

        let _ = fs::remove_dir_all(&state_root);
    }

    #[test]
    fn search_names_returns_matching_package_with_valid_signed_manifests() {
        let root = test_registry_root();
        let package_dir = root.join("index").join("ripgrep");
        fs::create_dir_all(&package_dir).expect("must create package dir");

        let signing_key = signing_key();
        fs::write(root.join("registry.pub"), public_key_hex(&signing_key))
            .expect("must write registry public key");
        write_signed_manifest(&package_dir, &signing_key, "14.1.0");

        let index = RegistryIndex::open(&root);
        let names = index
            .search_names("rip")
            .expect("must load matching package names");
        assert_eq!(names, vec!["ripgrep"]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn package_versions_fails_when_registry_public_key_is_missing() {
        let root = test_registry_root();
        let package_dir = root.join("index").join("ripgrep");
        fs::create_dir_all(&package_dir).expect("must create package dir");
        let manifest_path = package_dir.join("14.1.0.toml");
        fs::write(&manifest_path, manifest_toml("ripgrep", "14.1.0")).expect("must write manifest");

        let index = RegistryIndex::open(&root);
        let err = index
            .package_versions("ripgrep")
            .expect_err("must fail when registry.pub is missing");
        assert!(err.to_string().contains("registry.pub"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn package_versions_fails_when_manifest_signature_is_missing() {
        let root = test_registry_root();
        let package_dir = root.join("index").join("ripgrep");
        fs::create_dir_all(&package_dir).expect("must create package dir");
        let manifest_path = package_dir.join("14.1.0.toml");
        fs::write(&manifest_path, manifest_toml("ripgrep", "14.1.0")).expect("must write manifest");

        let signing_key = signing_key();
        fs::write(root.join("registry.pub"), public_key_hex(&signing_key))
            .expect("must write registry public key");

        let index = RegistryIndex::open(&root);
        let err = index
            .package_versions("ripgrep")
            .expect_err("must fail when signature sidecar is missing");
        assert!(err.to_string().contains(".sig"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn package_versions_fails_when_manifest_signature_is_invalid() {
        let root = test_registry_root();
        let package_dir = root.join("index").join("ripgrep");
        fs::create_dir_all(&package_dir).expect("must create package dir");

        let manifest = manifest_toml("ripgrep", "14.1.0");
        let manifest_path = package_dir.join("14.1.0.toml");
        fs::write(&manifest_path, manifest.as_bytes()).expect("must write manifest");

        let signing_key = signing_key();
        fs::write(root.join("registry.pub"), public_key_hex(&signing_key))
            .expect("must write registry public key");

        fs::write(manifest_path.with_extension("toml.sig"), "00")
            .expect("must write invalid signature");

        let index = RegistryIndex::open(&root);
        let err = index
            .package_versions("ripgrep")
            .expect_err("must fail when signature is invalid");
        assert!(err.to_string().contains("signature"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn package_versions_succeeds_with_valid_signatures_and_descending_sort() {
        let root = test_registry_root();
        let package_dir = root.join("index").join("ripgrep");
        fs::create_dir_all(&package_dir).expect("must create package dir");

        let signing_key = signing_key();
        fs::write(root.join("registry.pub"), public_key_hex(&signing_key))
            .expect("must write registry public key");

        write_signed_manifest(&package_dir, &signing_key, "14.0.0");
        write_signed_manifest(&package_dir, &signing_key, "14.1.0");

        let index = RegistryIndex::open(&root);
        let manifests = index
            .package_versions("ripgrep")
            .expect("must load manifests");

        let versions: Vec<String> = manifests
            .iter()
            .map(|manifest| manifest.version.to_string())
            .collect();
        assert_eq!(versions, vec!["14.1.0", "14.0.0"]);

        let _ = fs::remove_dir_all(&root);
    }

    fn write_signed_manifest(
        package_dir: &std::path::Path,
        signing_key: &SigningKey,
        version: &str,
    ) {
        let manifest_path = package_dir.join(format!("{version}.toml"));
        let manifest = manifest_toml("ripgrep", version);
        fs::write(&manifest_path, manifest.as_bytes()).expect("must write manifest");

        let signature = signing_key.sign(manifest.as_bytes());
        fs::write(
            manifest_path.with_extension("toml.sig"),
            hex::encode(signature.to_bytes()),
        )
        .expect("must write signature sidecar");
    }

    fn rewrite_signed_manifest_with_extra_field(
        manifest_path: &Path,
        signing_key: &SigningKey,
        extra_field: &str,
    ) {
        let mut manifest = fs::read_to_string(manifest_path).expect("must read manifest");
        manifest.push_str(extra_field);
        fs::write(manifest_path, manifest.as_bytes()).expect("must rewrite manifest");

        let signature = signing_key.sign(manifest.as_bytes());
        fs::write(
            manifest_path.with_extension("toml.sig"),
            hex::encode(signature.to_bytes()),
        )
        .expect("must rewrite signature sidecar");
    }

    fn manifest_toml(name: &str, version: &str) -> String {
        format!(
            r#"name = "{name}"
version = "{version}"
"#
        )
    }

    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn public_key_hex(signing_key: &SigningKey) -> String {
        hex::encode(signing_key.verifying_key().to_bytes())
    }

    fn source_record(name: &str, priority: u32) -> RegistrySourceRecord {
        RegistrySourceRecord {
            name: name.to_string(),
            kind: RegistrySourceKind::Git,
            location: format!("https://example.com/{name}.git"),
            fingerprint_sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .to_string(),
            enabled: true,
            priority,
        }
    }

    fn git_source_record(
        name: &str,
        location: &str,
        fingerprint_sha256: String,
        priority: u32,
    ) -> RegistrySourceRecord {
        RegistrySourceRecord {
            name: name.to_string(),
            kind: RegistrySourceKind::Git,
            location: location.to_string(),
            fingerprint_sha256,
            enabled: true,
            priority,
        }
    }

    fn filesystem_source_record(
        name: &str,
        location: &str,
        fingerprint_sha256: String,
        priority: u32,
    ) -> RegistrySourceRecord {
        RegistrySourceRecord {
            name: name.to_string(),
            kind: RegistrySourceKind::Filesystem,
            location: location.to_string(),
            fingerprint_sha256,
            enabled: true,
            priority,
        }
    }

    fn filesystem_source_fixture() -> PathBuf {
        let root = test_registry_root();
        let package_dir = root.join("index").join("ripgrep");
        fs::create_dir_all(&package_dir).expect("must create package dir");

        let signing_key = signing_key();
        let public_key_hex = public_key_hex(&signing_key);
        fs::write(root.join("registry.pub"), public_key_hex.as_bytes())
            .expect("must write registry public key");
        write_signed_manifest(&package_dir, &signing_key, "14.1.0");

        root
    }

    fn git_source_fixture() -> PathBuf {
        let root = filesystem_source_fixture();
        git_run(&root, &["init"]);
        git_run(&root, &["add", "."]);
        git_commit_all(&root, "initial registry snapshot");
        root
    }

    fn git_fixture_location(path: &Path) -> String {
        #[cfg(windows)]
        {
            path.to_string_lossy().replace('\\', "/")
        }

        #[cfg(not(windows))]
        {
            path.to_string_lossy().to_string()
        }
    }

    fn git_head_short(repo_root: &Path) -> String {
        let output = Command::new("git")
            .arg("rev-parse")
            .arg("HEAD")
            .current_dir(repo_root)
            .output()
            .expect("git must run rev-parse");
        assert!(
            output.status.success(),
            "git rev-parse failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let full_sha = String::from_utf8(output.stdout)
            .expect("git rev-parse output must be UTF-8")
            .trim()
            .to_string();
        derive_snapshot_id_from_full_git_sha(&full_sha)
            .expect("git rev-parse output must be a valid full sha")
    }

    fn git_commit_all(repo_root: &Path, message: &str) {
        git_run(repo_root, &["add", "."]);
        git_run(
            repo_root,
            &[
                "-c",
                "user.name=Crosspack Tests",
                "-c",
                "user.email=crosspack-tests@example.com",
                "commit",
                "-m",
                message,
            ],
        );
    }

    fn git_run(repo_root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo_root)
            .output()
            .expect("git command must execute");
        assert!(
            output.status.success(),
            "git command failed: git {}\nstdout:\n{}\nstderr:\n{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn sha256_hex_bytes(bytes: &[u8]) -> String {
        crosspack_security::sha256_hex(bytes)
    }

    fn write_ready_snapshot_cache(
        state_root: &Path,
        source_name: &str,
        signing_key: &SigningKey,
        versions: &[&str],
    ) {
        let cache_root = state_root.join("cache").join(source_name);
        let package_dir = cache_root.join("index").join("ripgrep");
        fs::create_dir_all(&package_dir).expect("must create package directory in cache");
        fs::write(cache_root.join("registry.pub"), public_key_hex(signing_key))
            .expect("must write registry key for cache source");

        for version in versions {
            write_signed_manifest(&package_dir, signing_key, version);
        }

        let snapshot = serde_json::json!({
            "version": 1,
            "source": source_name,
            "snapshot_id": format!("fs:{source_name}"),
            "updated_at_unix": 1,
            "manifest_count": versions.len(),
            "status": "ready"
        });
        fs::write(
            cache_root.join("snapshot.json"),
            serde_json::to_string_pretty(&snapshot).expect("must serialize snapshot"),
        )
        .expect("must write snapshot metadata");
    }

    static TEST_REGISTRY_ROOT_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_registry_root() -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let counter = TEST_REGISTRY_ROOT_COUNTER.fetch_add(1, Ordering::SeqCst);
        path.push(format!(
            "crosspack-registry-tests-{}-{}-{}",
            std::process::id(),
            nanos,
            counter
        ));
        path
    }
}
