use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::types::{
    HostPlatform, IntegrationActivationPlan, IntegrationActivationScope, IntegrationAdapterKind,
    IntegrationAppliedState, IntegrationReasonCode, NativeServiceAction,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationAdapterOutcome {
    pub reason_code: IntegrationReasonCode,
    pub applied_state: IntegrationAppliedState,
    pub rollback: Vec<ActivationRollbackEntry>,
}

impl ActivationAdapterOutcome {
    fn ok() -> Self {
        Self {
            reason_code: IntegrationReasonCode::Ok,
            applied_state: IntegrationAppliedState::Enabled,
            rollback: Vec::new(),
        }
    }

    fn conflict() -> Self {
        Self {
            reason_code: IntegrationReasonCode::HostPathConflict,
            applied_state: IntegrationAppliedState::Failed,
            rollback: Vec::new(),
        }
    }

    fn escalation_required() -> Self {
        Self {
            reason_code: IntegrationReasonCode::EscalationRequired,
            applied_state: IntegrationAppliedState::Unsupported,
            rollback: Vec::new(),
        }
    }

    fn service(reason_code: IntegrationReasonCode, applied_state: IntegrationAppliedState) -> Self {
        Self {
            reason_code,
            applied_state,
            rollback: Vec::new(),
        }
    }
}

fn owner_matches_rollback_expectation(
    expected: Option<&ActivationOwner>,
    actual: Option<&ActivationOwner>,
) -> bool {
    match (expected, actual) {
        (Some(expected), Some(actual)) => expected == actual,
        (Some(_), None) => true,
        (None, _) => true,
    }
}

fn parse_crosspack_windows_shim(contents: &str) -> Option<String> {
    let mut lines = contents.lines();
    let first = lines.next()?.trim_end_matches('\r');
    if !first.eq_ignore_ascii_case("@echo off") {
        return None;
    }
    let second = lines.next()?.trim_end_matches('\r').trim();
    let rest = second.strip_prefix('"')?;
    let (target, suffix) = rest.split_once('"')?;
    if suffix.trim() != "%*" || target.is_empty() {
        return None;
    }
    Some(target.to_string())
}

fn restore_precondition_matches(
    fs: &impl ActivationFilesystem,
    entry: &ActivationRollbackEntry,
    restore_shim: bool,
) -> bool {
    if entry.expected_current_absent {
        return fs.entry(&entry.path).is_none();
    }
    if restore_shim {
        let Some(expected_target) = entry.expected_current_shim_target.as_deref() else {
            return false;
        };
        return matches!(
            fs.entry(&entry.path),
            Some(ActivationFsEntry::WindowsShim { target, owner })
                if target == expected_target
                    && owner_matches_rollback_expectation(
                        entry.expected_current_owner.as_ref(),
                        owner.as_ref(),
                    )
        );
    }

    let Some(expected_target) = entry.expected_current_symlink_target.as_deref() else {
        return false;
    };
    matches!(
        fs.entry(&entry.path),
        Some(ActivationFsEntry::Symlink { target, owner })
            if target == expected_target
                && owner_matches_rollback_expectation(
                    entry.expected_current_owner.as_ref(),
                    owner.as_ref(),
                )
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeCommandResult {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl NativeCommandResult {
    pub fn success(stdout: impl Into<String>, stderr: impl Into<String>) -> Self {
        Self {
            status: 0,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    fn failed(status: i32, stdout: impl Into<String>, stderr: impl Into<String>) -> Self {
        Self {
            status,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    fn succeeded(&self) -> bool {
        self.status == 0
    }
}

pub trait ActivationCommandExecutor {
    fn run(&mut self, program: &str, args: &[String]) -> NativeCommandResult;
}

type ActivationCommand = (String, Vec<String>);
type ServiceDisableCommands = (Vec<ActivationCommand>, Vec<ActivationCommand>);

#[derive(Debug, Default)]
pub struct SystemActivationCommandExecutor;

impl ActivationCommandExecutor for SystemActivationCommandExecutor {
    fn run(&mut self, program: &str, args: &[String]) -> NativeCommandResult {
        match Command::new(program).args(args).output() {
            Ok(output) => NativeCommandResult {
                status: output.status.code().unwrap_or(-1),
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                NativeCommandResult::failed(127, "", err.to_string())
            }
            Err(err) => NativeCommandResult::failed(-1, "", err.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivationRollbackEntry {
    pub operation: ActivationRollbackOperation,
    pub path: String,
    pub previous_symlink_target: Option<String>,
    pub previous_shim_target: Option<String>,
    pub previous_owner: Option<ActivationOwner>,
    #[serde(default)]
    pub created_symlink_target: Option<String>,
    #[serde(default)]
    pub created_shim_target: Option<String>,
    #[serde(default)]
    pub created_owner: Option<ActivationOwner>,
    #[serde(default)]
    pub expected_current_symlink_target: Option<String>,
    #[serde(default)]
    pub expected_current_shim_target: Option<String>,
    #[serde(default)]
    pub expected_current_owner: Option<ActivationOwner>,
    #[serde(default)]
    pub expected_current_absent: bool,
    pub created_parent_dirs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActivationRollbackOperation {
    RemoveCreatedSymlink,
    RestoreOwnedSymlink,
    RemoveCreatedWindowsShim,
    RestoreOwnedWindowsShim,
    RemoveCreatedServiceMetadata,
    RestoreOwnedServiceMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MemoryActivationFileEntry {
    File(Vec<u8>),
    Symlink {
        target: String,
        owner: Option<ActivationOwner>,
    },
    WindowsShim {
        target: String,
        owner: Option<ActivationOwner>,
    },
    ServiceMetadata {
        source: String,
        owner: Option<ActivationOwner>,
    },
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationFsEntry {
    File,
    Symlink {
        target: String,
        owner: Option<ActivationOwner>,
    },
    WindowsShim {
        target: String,
        owner: Option<ActivationOwner>,
    },
    ServiceMetadata {
        source: String,
        owner: Option<ActivationOwner>,
    },
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivationOwner {
    pub package_state_key: String,
    pub package: String,
    pub integration_key: String,
}

pub trait ActivationFilesystem {
    fn platform(&self) -> HostPlatform;
    fn symlink_supported(&self) -> bool;
    fn entry(&self, path: &str) -> Option<ActivationFsEntry>;
    fn create_parent_dirs_after_preflight(&mut self, path: &str) -> Option<Vec<String>>;
    fn write_owned_symlink_for(
        &mut self,
        path: &str,
        target: &str,
        package_state_key: &str,
        package: &str,
        integration_key: &str,
    ) -> bool;
    fn write_owned_shim_for(
        &mut self,
        path: &str,
        target: &str,
        package_state_key: &str,
        package: &str,
        integration_key: &str,
    ) -> bool;
    fn write_owned_service_metadata_for(
        &mut self,
        _path: &str,
        _source: &str,
        _package_state_key: &str,
        _package: &str,
        _integration_key: &str,
    ) -> bool {
        false
    }
    fn remove_entry(&mut self, path: &str) -> bool;
}

#[derive(Debug, Clone)]
pub struct RealActivationFs {
    platform: HostPlatform,
    symlink_supported: bool,
    owners: BTreeMap<String, ActivationOwner>,
}

impl RealActivationFs {
    pub fn new(
        platform: HostPlatform,
        owners: impl IntoIterator<Item = (String, ActivationOwner)>,
    ) -> Self {
        Self {
            platform,
            symlink_supported: true,
            owners: owners.into_iter().collect(),
        }
    }

    pub fn with_symlink_support(mut self, supported: bool) -> Self {
        self.symlink_supported = supported;
        self
    }

    fn owner_for_path(&self, path: &str, target: Option<&str>) -> Option<ActivationOwner> {
        let owner = self.owners.get(path)?.clone();
        match target {
            Some(target) if !target.is_empty() => Some(owner),
            _ => None,
        }
    }
}

fn temp_replace_path(path: &str) -> Option<PathBuf> {
    let path = Path::new(path);
    let file_name = path.file_name()?.to_string_lossy();
    Some(path.with_file_name(format!(
        ".{file_name}.crosspack-replace-{}",
        std::process::id()
    )))
}

fn path_exists_or_symlink(path: &str) -> bool {
    Path::new(path).exists() || fs::symlink_metadata(path).is_ok()
}

fn rename_temp_over_path(temp_path: &Path, path: &str) -> bool {
    if fs::rename(temp_path, path).is_ok() {
        return true;
    }
    if path_exists_or_symlink(path) {
        let _ = fs::remove_file(path);
        return fs::rename(temp_path, path).is_ok();
    }
    false
}

#[derive(Debug, Clone)]
pub struct MemoryActivationFs {
    platform: HostPlatform,
    symlink_supported: bool,
    entries: BTreeMap<String, MemoryActivationFileEntry>,
}

impl MemoryActivationFs {
    pub fn new(platform: HostPlatform) -> Self {
        let mut entries = BTreeMap::new();
        if platform != HostPlatform::Windows {
            entries.insert("/".to_string(), MemoryActivationFileEntry::Directory);
        }
        Self {
            platform,
            symlink_supported: true,
            entries,
        }
    }

    pub fn with_symlink_support(mut self, supported: bool) -> Self {
        self.symlink_supported = supported;
        self
    }

    pub fn write_file(&mut self, path: &str, contents: &[u8]) {
        self.entries.insert(
            path.to_string(),
            MemoryActivationFileEntry::File(contents.to_vec()),
        );
    }

    pub fn write_symlink(&mut self, path: &str, target: &str) {
        self.write_symlink_entry(path, target, None);
    }

    pub fn write_owned_symlink(&mut self, path: &str, target: &str) {
        self.write_owned_symlink_for(
            path,
            target,
            "default--host--core--docker-compose",
            "docker-compose",
            "docker_cli_plugin:compose",
        );
    }

    pub fn write_owned_symlink_for(
        &mut self,
        path: &str,
        target: &str,
        package_state_key: &str,
        package: &str,
        integration_key: &str,
    ) {
        self.write_symlink_entry(
            path,
            target,
            Some(ActivationOwner {
                package_state_key: package_state_key.to_string(),
                package: package.to_string(),
                integration_key: integration_key.to_string(),
            }),
        );
    }

    pub fn exists(&self, path: &str) -> bool {
        self.entries.contains_key(path)
    }

    pub fn is_dir(&self, path: &str) -> bool {
        matches!(
            self.entries.get(path),
            Some(MemoryActivationFileEntry::Directory)
        )
    }

    pub fn is_file(&self, path: &str) -> bool {
        matches!(
            self.entries.get(path),
            Some(MemoryActivationFileEntry::File(_))
        )
    }

    pub fn symlink_target(&self, path: &str) -> Option<String> {
        match self.entries.get(path) {
            Some(MemoryActivationFileEntry::Symlink { target, .. }) => Some(target.clone()),
            _ => None,
        }
    }

    pub fn shim_target(&self, path: &str) -> Option<String> {
        match self.entries.get(path) {
            Some(MemoryActivationFileEntry::WindowsShim { target, .. }) => Some(target.clone()),
            _ => None,
        }
    }

    pub fn service_metadata_source(&self, path: &str) -> Option<String> {
        match self.entries.get(path) {
            Some(MemoryActivationFileEntry::ServiceMetadata { source, .. }) => Some(source.clone()),
            _ => None,
        }
    }

    fn write_symlink_entry(&mut self, path: &str, target: &str, owner: Option<ActivationOwner>) {
        self.entries.insert(
            path.to_string(),
            MemoryActivationFileEntry::Symlink {
                target: target.to_string(),
                owner,
            },
        );
    }

    fn write_owned_shim_for(
        &mut self,
        path: &str,
        target: &str,
        package_state_key: &str,
        package: &str,
        integration_key: &str,
    ) {
        self.entries.insert(
            path.to_string(),
            MemoryActivationFileEntry::WindowsShim {
                target: target.to_string(),
                owner: Some(ActivationOwner {
                    package_state_key: package_state_key.to_string(),
                    package: package.to_string(),
                    integration_key: integration_key.to_string(),
                }),
            },
        );
    }

    fn write_service_metadata_for(
        &mut self,
        path: &str,
        source: &str,
        package_state_key: &str,
        package: &str,
        integration_key: &str,
    ) {
        self.entries.insert(
            path.to_string(),
            MemoryActivationFileEntry::ServiceMetadata {
                source: source.to_string(),
                owner: Some(ActivationOwner {
                    package_state_key: package_state_key.to_string(),
                    package: package.to_string(),
                    integration_key: integration_key.to_string(),
                }),
            },
        );
    }

    fn create_parent_dirs_after_preflight(&mut self, path: &str) -> Option<Vec<String>> {
        let created = self.parent_dirs_to_create(path)?;
        for parent in &created {
            self.entries
                .insert(parent.clone(), MemoryActivationFileEntry::Directory);
        }
        Some(created)
    }

    fn parent_dirs_to_create(&self, path: &str) -> Option<Vec<String>> {
        let mut parents = Vec::new();
        let mut current = parent_path(self.platform, path);
        while let Some(parent) = current {
            if parent.is_empty() {
                break;
            }
            parents.push(parent.to_string());
            if parent == "/" || parent.ends_with(":\\") {
                break;
            }
            current = parent_path(self.platform, parent);
        }

        let mut created = Vec::new();
        for parent in parents.into_iter().rev() {
            match self.entries.get(&parent) {
                Some(MemoryActivationFileEntry::Directory) => {}
                Some(_) => return None,
                None => created.push(parent),
            }
        }
        Some(created)
    }
}

impl ActivationFilesystem for MemoryActivationFs {
    fn platform(&self) -> HostPlatform {
        self.platform
    }

    fn symlink_supported(&self) -> bool {
        self.symlink_supported
    }

    fn entry(&self, path: &str) -> Option<ActivationFsEntry> {
        match self.entries.get(path) {
            Some(MemoryActivationFileEntry::File(_)) => Some(ActivationFsEntry::File),
            Some(MemoryActivationFileEntry::Symlink { target, owner }) => {
                Some(ActivationFsEntry::Symlink {
                    target: target.clone(),
                    owner: owner.clone(),
                })
            }
            Some(MemoryActivationFileEntry::WindowsShim { target, owner }) => {
                Some(ActivationFsEntry::WindowsShim {
                    target: target.clone(),
                    owner: owner.clone(),
                })
            }
            Some(MemoryActivationFileEntry::ServiceMetadata { source, owner }) => {
                Some(ActivationFsEntry::ServiceMetadata {
                    source: source.clone(),
                    owner: owner.clone(),
                })
            }
            Some(MemoryActivationFileEntry::Directory) => Some(ActivationFsEntry::Directory),
            None => None,
        }
    }

    fn create_parent_dirs_after_preflight(&mut self, path: &str) -> Option<Vec<String>> {
        MemoryActivationFs::create_parent_dirs_after_preflight(self, path)
    }

    fn write_owned_symlink_for(
        &mut self,
        path: &str,
        target: &str,
        package_state_key: &str,
        package: &str,
        integration_key: &str,
    ) -> bool {
        MemoryActivationFs::write_owned_symlink_for(
            self,
            path,
            target,
            package_state_key,
            package,
            integration_key,
        );
        true
    }

    fn write_owned_shim_for(
        &mut self,
        path: &str,
        target: &str,
        package_state_key: &str,
        package: &str,
        integration_key: &str,
    ) -> bool {
        MemoryActivationFs::write_owned_shim_for(
            self,
            path,
            target,
            package_state_key,
            package,
            integration_key,
        );
        true
    }

    fn write_owned_service_metadata_for(
        &mut self,
        path: &str,
        source: &str,
        package_state_key: &str,
        package: &str,
        integration_key: &str,
    ) -> bool {
        MemoryActivationFs::write_service_metadata_for(
            self,
            path,
            source,
            package_state_key,
            package,
            integration_key,
        );
        true
    }

    fn remove_entry(&mut self, path: &str) -> bool {
        self.entries.remove(path).is_some()
    }
}

impl ActivationFilesystem for RealActivationFs {
    fn platform(&self) -> HostPlatform {
        self.platform
    }

    fn symlink_supported(&self) -> bool {
        self.symlink_supported
    }

    fn entry(&self, path: &str) -> Option<ActivationFsEntry> {
        let metadata = fs::symlink_metadata(path).ok()?;
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(path).ok()?.display().to_string();
            return Some(ActivationFsEntry::Symlink {
                owner: self.owner_for_path(path, Some(&target)),
                target,
            });
        }
        if metadata.is_dir() {
            return Some(ActivationFsEntry::Directory);
        }
        if let Some(owner) = self.owners.get(path).cloned() {
            if let Ok(contents) = fs::read_to_string(path) {
                if let Some(target) = parse_crosspack_windows_shim(&contents) {
                    return Some(ActivationFsEntry::WindowsShim {
                        owner: Some(owner),
                        target,
                    });
                }
            }
        }
        Some(ActivationFsEntry::File)
    }

    fn create_parent_dirs_after_preflight(&mut self, path: &str) -> Option<Vec<String>> {
        let parent = parent_path(self.platform, path)?;
        fs::create_dir_all(parent).ok()?;
        Some(Vec::new())
    }

    fn write_owned_symlink_for(
        &mut self,
        path: &str,
        target: &str,
        package_state_key: &str,
        package: &str,
        integration_key: &str,
    ) -> bool {
        let Some(temp_path) = temp_replace_path(path) else {
            return false;
        };
        let Some(temp_path_str) = temp_path.to_str() else {
            return false;
        };
        let _ = fs::remove_file(&temp_path);
        let created = create_symlink(target, temp_path_str).is_ok()
            && rename_temp_over_path(&temp_path, path);
        if !created {
            let _ = fs::remove_file(&temp_path);
        }
        if created {
            self.owners.insert(
                path.to_string(),
                ActivationOwner {
                    package_state_key: package_state_key.to_string(),
                    package: package.to_string(),
                    integration_key: integration_key.to_string(),
                },
            );
        }
        created
    }

    fn write_owned_shim_for(
        &mut self,
        path: &str,
        target: &str,
        package_state_key: &str,
        package: &str,
        integration_key: &str,
    ) -> bool {
        let contents = format!("@echo off\r\n\"{target}\" %*\r\n");
        let Some(temp_path) = temp_replace_path(path) else {
            return false;
        };
        let _ = fs::remove_file(&temp_path);
        let written =
            fs::write(&temp_path, contents).is_ok() && rename_temp_over_path(&temp_path, path);
        if !written {
            let _ = fs::remove_file(&temp_path);
        }
        if written {
            self.owners.insert(
                path.to_string(),
                ActivationOwner {
                    package_state_key: package_state_key.to_string(),
                    package: package.to_string(),
                    integration_key: integration_key.to_string(),
                },
            );
        }
        written
    }

    fn remove_entry(&mut self, path: &str) -> bool {
        self.owners.remove(path);
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_dir() => false,
            Ok(_) => fs::remove_file(path).is_ok(),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
            Err(_) => false,
        }
    }
}

#[cfg(unix)]
fn create_symlink(target: &str, path: &str) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, path)
}

#[cfg(windows)]
fn create_symlink(target: &str, path: &str) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, path)
}

fn restore_owned_symlink_after_failed_write(
    fs: &mut impl ActivationFilesystem,
    path: &str,
    target: &str,
    owner: &ActivationOwner,
) -> bool {
    fs.create_parent_dirs_after_preflight(path).is_some()
        && fs.write_owned_symlink_for(
            path,
            target,
            &owner.package_state_key,
            &owner.package,
            &owner.integration_key,
        )
}

fn restore_owned_shim_after_failed_write(
    fs: &mut impl ActivationFilesystem,
    path: &str,
    target: &str,
    owner: &ActivationOwner,
) -> bool {
    fs.create_parent_dirs_after_preflight(path).is_some()
        && fs.write_owned_shim_for(
            path,
            target,
            &owner.package_state_key,
            &owner.package,
            &owner.integration_key,
        )
}

pub fn apply_docker_cli_plugin_plan(
    fs: &mut impl ActivationFilesystem,
    plan: &IntegrationActivationPlan,
) -> ActivationAdapterOutcome {
    if fs.platform() == HostPlatform::Windows && !fs.symlink_supported() {
        return ActivationAdapterOutcome::escalation_required();
    }

    let existing = fs.entry(&plan.host_path);
    let expected_owner = owner_for_plan(plan);
    match existing {
        Some(ActivationFsEntry::Symlink { target, owner })
            if target == plan.source_path && owner.as_ref() == Some(&expected_owner) =>
        {
            ActivationAdapterOutcome::ok()
        }
        Some(ActivationFsEntry::Symlink { target, owner })
            if owner.as_ref() == Some(&expected_owner) =>
        {
            let Some(previous_owner) = owner.clone() else {
                return ActivationAdapterOutcome::conflict();
            };
            let Some(created_parent_dirs) = fs.create_parent_dirs_after_preflight(&plan.host_path)
            else {
                return ActivationAdapterOutcome::conflict();
            };
            if !fs.write_owned_symlink_for(
                &plan.host_path,
                &plan.source_path,
                &plan.package_state_key,
                &plan.package,
                &plan.integration_key,
            ) {
                restore_owned_symlink_after_failed_write(
                    fs,
                    &plan.host_path,
                    &target,
                    &previous_owner,
                );
                return ActivationAdapterOutcome::conflict();
            }
            ActivationAdapterOutcome {
                reason_code: IntegrationReasonCode::Ok,
                applied_state: IntegrationAppliedState::Enabled,
                rollback: vec![ActivationRollbackEntry {
                    operation: ActivationRollbackOperation::RestoreOwnedSymlink,
                    path: plan.host_path.clone(),
                    previous_symlink_target: Some(target),
                    previous_shim_target: None,
                    previous_owner: Some(previous_owner),
                    created_symlink_target: None,
                    created_shim_target: None,
                    created_owner: None,
                    expected_current_symlink_target: Some(plan.source_path.clone()),
                    expected_current_shim_target: None,
                    expected_current_owner: Some(owner_for_plan(plan)),
                    expected_current_absent: false,
                    created_parent_dirs,
                }],
            }
        }
        Some(_) => ActivationAdapterOutcome::conflict(),
        None => {
            let Some(created_parent_dirs) = fs.create_parent_dirs_after_preflight(&plan.host_path)
            else {
                return ActivationAdapterOutcome::conflict();
            };
            if !fs.write_owned_symlink_for(
                &plan.host_path,
                &plan.source_path,
                &plan.package_state_key,
                &plan.package,
                &plan.integration_key,
            ) {
                return ActivationAdapterOutcome::conflict();
            }
            ActivationAdapterOutcome {
                reason_code: IntegrationReasonCode::Ok,
                applied_state: IntegrationAppliedState::Enabled,
                rollback: vec![ActivationRollbackEntry {
                    operation: ActivationRollbackOperation::RemoveCreatedSymlink,
                    path: plan.host_path.clone(),
                    previous_symlink_target: None,
                    previous_shim_target: None,
                    previous_owner: None,
                    created_symlink_target: Some(plan.source_path.clone()),
                    created_shim_target: None,
                    created_owner: Some(owner_for_plan(plan)),
                    expected_current_symlink_target: None,
                    expected_current_shim_target: None,
                    expected_current_owner: None,
                    expected_current_absent: false,
                    created_parent_dirs,
                }],
            }
        }
    }
}

pub fn disable_docker_cli_plugin_plan(
    fs: &mut impl ActivationFilesystem,
    plan: &IntegrationActivationPlan,
) -> ActivationAdapterOutcome {
    let expected_owner = owner_for_plan(plan);
    match fs.entry(&plan.host_path) {
        Some(ActivationFsEntry::Symlink { target, owner })
            if owner.as_ref() == Some(&expected_owner) =>
        {
            if target != plan.source_path {
                return ActivationAdapterOutcome::conflict();
            }
            let rollback = ActivationRollbackEntry {
                operation: ActivationRollbackOperation::RestoreOwnedSymlink,
                path: plan.host_path.clone(),
                previous_symlink_target: Some(target),
                previous_shim_target: None,
                previous_owner: owner,
                created_symlink_target: None,
                created_shim_target: None,
                created_owner: None,
                expected_current_symlink_target: None,
                expected_current_shim_target: None,
                expected_current_owner: None,
                expected_current_absent: true,
                created_parent_dirs: Vec::new(),
            };
            if !fs.remove_entry(&plan.host_path) {
                return ActivationAdapterOutcome::conflict();
            }
            ActivationAdapterOutcome {
                reason_code: IntegrationReasonCode::Ok,
                applied_state: IntegrationAppliedState::Stopped,
                rollback: vec![rollback],
            }
        }
        Some(_) => ActivationAdapterOutcome::conflict(),
        None => ActivationAdapterOutcome::service(
            IntegrationReasonCode::Ok,
            IntegrationAppliedState::Stopped,
        ),
    }
}

pub fn apply_path_plugin_plan(
    fs: &mut impl ActivationFilesystem,
    plan: &IntegrationActivationPlan,
) -> ActivationAdapterOutcome {
    if !destination_is_under_source_prefix(fs.platform(), &plan.host_path, &plan.source_path) {
        return ActivationAdapterOutcome::conflict();
    }

    let existing = fs.entry(&plan.host_path);
    let expected_owner = owner_for_plan(plan);
    match (fs.platform(), existing) {
        (
            HostPlatform::Linux | HostPlatform::Macos,
            Some(ActivationFsEntry::Symlink { target, owner }),
        ) if target == plan.source_path && owner.as_ref() == Some(&expected_owner) => {
            ActivationAdapterOutcome::ok()
        }
        (
            HostPlatform::Linux | HostPlatform::Macos,
            Some(ActivationFsEntry::Symlink { target, owner }),
        ) if owner.as_ref() == Some(&expected_owner) => {
            let Some(previous_owner) = owner.clone() else {
                return ActivationAdapterOutcome::conflict();
            };
            let Some(created_parent_dirs) = fs.create_parent_dirs_after_preflight(&plan.host_path)
            else {
                return ActivationAdapterOutcome::conflict();
            };
            if !fs.write_owned_symlink_for(
                &plan.host_path,
                &plan.source_path,
                &plan.package_state_key,
                &plan.package,
                &plan.integration_key,
            ) {
                restore_owned_symlink_after_failed_write(
                    fs,
                    &plan.host_path,
                    &target,
                    &previous_owner,
                );
                return ActivationAdapterOutcome::conflict();
            }
            ActivationAdapterOutcome {
                reason_code: IntegrationReasonCode::Ok,
                applied_state: IntegrationAppliedState::Enabled,
                rollback: vec![ActivationRollbackEntry {
                    operation: ActivationRollbackOperation::RestoreOwnedSymlink,
                    path: plan.host_path.clone(),
                    previous_symlink_target: Some(target),
                    previous_shim_target: None,
                    previous_owner: Some(previous_owner),
                    created_symlink_target: None,
                    created_shim_target: None,
                    created_owner: None,
                    expected_current_symlink_target: Some(plan.source_path.clone()),
                    expected_current_shim_target: None,
                    expected_current_owner: Some(owner_for_plan(plan)),
                    expected_current_absent: false,
                    created_parent_dirs,
                }],
            }
        }
        (HostPlatform::Windows, Some(ActivationFsEntry::WindowsShim { target, owner }))
            if target == plan.source_path && owner.as_ref() == Some(&expected_owner) =>
        {
            ActivationAdapterOutcome::ok()
        }
        (HostPlatform::Windows, Some(ActivationFsEntry::WindowsShim { target, owner }))
            if owner.as_ref() == Some(&expected_owner) =>
        {
            let Some(previous_owner) = owner.clone() else {
                return ActivationAdapterOutcome::conflict();
            };
            let Some(created_parent_dirs) = fs.create_parent_dirs_after_preflight(&plan.host_path)
            else {
                return ActivationAdapterOutcome::conflict();
            };
            if !fs.write_owned_shim_for(
                &plan.host_path,
                &plan.source_path,
                &plan.package_state_key,
                &plan.package,
                &plan.integration_key,
            ) {
                restore_owned_shim_after_failed_write(
                    fs,
                    &plan.host_path,
                    &target,
                    &previous_owner,
                );
                return ActivationAdapterOutcome::conflict();
            }
            ActivationAdapterOutcome {
                reason_code: IntegrationReasonCode::Ok,
                applied_state: IntegrationAppliedState::Enabled,
                rollback: vec![ActivationRollbackEntry {
                    operation: ActivationRollbackOperation::RestoreOwnedWindowsShim,
                    path: plan.host_path.clone(),
                    previous_symlink_target: None,
                    previous_shim_target: Some(target),
                    previous_owner: Some(previous_owner),
                    created_symlink_target: None,
                    created_shim_target: None,
                    created_owner: None,
                    expected_current_symlink_target: None,
                    expected_current_shim_target: Some(plan.source_path.clone()),
                    expected_current_owner: Some(owner_for_plan(plan)),
                    expected_current_absent: false,
                    created_parent_dirs,
                }],
            }
        }
        (_, Some(_)) => ActivationAdapterOutcome::conflict(),
        (HostPlatform::Linux | HostPlatform::Macos, None) => {
            let Some(created_parent_dirs) = fs.create_parent_dirs_after_preflight(&plan.host_path)
            else {
                return ActivationAdapterOutcome::conflict();
            };
            if !fs.write_owned_symlink_for(
                &plan.host_path,
                &plan.source_path,
                &plan.package_state_key,
                &plan.package,
                &plan.integration_key,
            ) {
                return ActivationAdapterOutcome::conflict();
            }
            ActivationAdapterOutcome {
                reason_code: IntegrationReasonCode::Ok,
                applied_state: IntegrationAppliedState::Enabled,
                rollback: vec![ActivationRollbackEntry {
                    operation: ActivationRollbackOperation::RemoveCreatedSymlink,
                    path: plan.host_path.clone(),
                    previous_symlink_target: None,
                    previous_shim_target: None,
                    previous_owner: None,
                    created_symlink_target: Some(plan.source_path.clone()),
                    created_shim_target: None,
                    created_owner: Some(owner_for_plan(plan)),
                    expected_current_symlink_target: None,
                    expected_current_shim_target: None,
                    expected_current_owner: None,
                    expected_current_absent: false,
                    created_parent_dirs,
                }],
            }
        }
        (HostPlatform::Windows, None) => {
            let Some(created_parent_dirs) = fs.create_parent_dirs_after_preflight(&plan.host_path)
            else {
                return ActivationAdapterOutcome::conflict();
            };
            if !fs.write_owned_shim_for(
                &plan.host_path,
                &plan.source_path,
                &plan.package_state_key,
                &plan.package,
                &plan.integration_key,
            ) {
                return ActivationAdapterOutcome::conflict();
            }
            ActivationAdapterOutcome {
                reason_code: IntegrationReasonCode::Ok,
                applied_state: IntegrationAppliedState::Enabled,
                rollback: vec![ActivationRollbackEntry {
                    operation: ActivationRollbackOperation::RemoveCreatedWindowsShim,
                    path: plan.host_path.clone(),
                    previous_symlink_target: None,
                    previous_shim_target: None,
                    previous_owner: None,
                    created_symlink_target: None,
                    created_shim_target: Some(plan.source_path.clone()),
                    created_owner: Some(owner_for_plan(plan)),
                    expected_current_symlink_target: None,
                    expected_current_shim_target: None,
                    expected_current_owner: None,
                    expected_current_absent: false,
                    created_parent_dirs,
                }],
            }
        }
    }
}

pub fn disable_path_plugin_plan(
    fs: &mut impl ActivationFilesystem,
    plan: &IntegrationActivationPlan,
) -> ActivationAdapterOutcome {
    if !destination_is_under_source_prefix(fs.platform(), &plan.host_path, &plan.source_path) {
        return ActivationAdapterOutcome::conflict();
    }

    let expected_owner = owner_for_plan(plan);
    match fs.entry(&plan.host_path) {
        Some(ActivationFsEntry::Symlink { target, owner })
            if fs.platform() != HostPlatform::Windows
                && owner.as_ref() == Some(&expected_owner) =>
        {
            if target != plan.source_path {
                return ActivationAdapterOutcome::conflict();
            }
            let rollback = ActivationRollbackEntry {
                operation: ActivationRollbackOperation::RestoreOwnedSymlink,
                path: plan.host_path.clone(),
                previous_symlink_target: Some(target),
                previous_shim_target: None,
                previous_owner: owner,
                created_symlink_target: None,
                created_shim_target: None,
                created_owner: None,
                expected_current_symlink_target: None,
                expected_current_shim_target: None,
                expected_current_owner: None,
                expected_current_absent: true,
                created_parent_dirs: Vec::new(),
            };
            if !fs.remove_entry(&plan.host_path) {
                return ActivationAdapterOutcome::conflict();
            }
            ActivationAdapterOutcome {
                reason_code: IntegrationReasonCode::Ok,
                applied_state: IntegrationAppliedState::Stopped,
                rollback: vec![rollback],
            }
        }
        Some(ActivationFsEntry::WindowsShim { target, owner })
            if fs.platform() == HostPlatform::Windows
                && owner.as_ref() == Some(&expected_owner) =>
        {
            if target != plan.source_path {
                return ActivationAdapterOutcome::conflict();
            }
            let rollback = ActivationRollbackEntry {
                operation: ActivationRollbackOperation::RestoreOwnedWindowsShim,
                path: plan.host_path.clone(),
                previous_symlink_target: None,
                previous_shim_target: Some(target),
                previous_owner: owner,
                created_symlink_target: None,
                created_shim_target: None,
                created_owner: None,
                expected_current_symlink_target: None,
                expected_current_shim_target: None,
                expected_current_owner: None,
                expected_current_absent: true,
                created_parent_dirs: Vec::new(),
            };
            if !fs.remove_entry(&plan.host_path) {
                return ActivationAdapterOutcome::conflict();
            }
            ActivationAdapterOutcome {
                reason_code: IntegrationReasonCode::Ok,
                applied_state: IntegrationAppliedState::Stopped,
                rollback: vec![rollback],
            }
        }
        Some(_) => ActivationAdapterOutcome::conflict(),
        None => ActivationAdapterOutcome::service(
            IntegrationReasonCode::Ok,
            IntegrationAppliedState::Stopped,
        ),
    }
}

#[allow(clippy::collapsible_match)]
pub fn replay_activation_rollback_entry_with_fs(
    fs: &mut impl ActivationFilesystem,
    entry: &ActivationRollbackEntry,
) -> ActivationAdapterOutcome {
    match entry.operation {
        ActivationRollbackOperation::RemoveCreatedSymlink => {
            let Some(expected_target) = entry.created_symlink_target.as_deref() else {
                return ActivationAdapterOutcome::conflict();
            };
            match fs.entry(&entry.path) {
                Some(ActivationFsEntry::Symlink { target, owner })
                    if target == expected_target
                        && owner_matches_rollback_expectation(
                            entry.created_owner.as_ref(),
                            owner.as_ref(),
                        ) =>
                {
                    if !fs.remove_entry(&entry.path) {
                        return ActivationAdapterOutcome::conflict();
                    }
                }
                Some(_) => return ActivationAdapterOutcome::conflict(),
                None => {}
            }
            ActivationAdapterOutcome::service(
                IntegrationReasonCode::Ok,
                IntegrationAppliedState::Stopped,
            )
        }
        ActivationRollbackOperation::RemoveCreatedWindowsShim => {
            let Some(expected_target) = entry.created_shim_target.as_deref() else {
                return ActivationAdapterOutcome::conflict();
            };
            match fs.entry(&entry.path) {
                Some(ActivationFsEntry::WindowsShim { target, owner })
                    if target == expected_target
                        && owner_matches_rollback_expectation(
                            entry.created_owner.as_ref(),
                            owner.as_ref(),
                        ) =>
                {
                    if !fs.remove_entry(&entry.path) {
                        return ActivationAdapterOutcome::conflict();
                    }
                }
                Some(_) => return ActivationAdapterOutcome::conflict(),
                None => {}
            }
            ActivationAdapterOutcome::service(
                IntegrationReasonCode::Ok,
                IntegrationAppliedState::Stopped,
            )
        }
        ActivationRollbackOperation::RestoreOwnedSymlink => {
            let Some(target) = entry.previous_symlink_target.as_deref() else {
                return ActivationAdapterOutcome::conflict();
            };
            let Some(owner) = entry.previous_owner.as_ref() else {
                return ActivationAdapterOutcome::conflict();
            };
            if !restore_precondition_matches(fs, entry, false) {
                return ActivationAdapterOutcome::conflict();
            }
            if fs.create_parent_dirs_after_preflight(&entry.path).is_none() {
                return ActivationAdapterOutcome::conflict();
            }
            if !fs.write_owned_symlink_for(
                &entry.path,
                target,
                &owner.package_state_key,
                &owner.package,
                &owner.integration_key,
            ) {
                return ActivationAdapterOutcome::conflict();
            }
            ActivationAdapterOutcome::service(
                IntegrationReasonCode::Ok,
                IntegrationAppliedState::Enabled,
            )
        }
        ActivationRollbackOperation::RestoreOwnedWindowsShim => {
            let Some(target) = entry.previous_shim_target.as_deref() else {
                return ActivationAdapterOutcome::conflict();
            };
            let Some(owner) = entry.previous_owner.as_ref() else {
                return ActivationAdapterOutcome::conflict();
            };
            if !restore_precondition_matches(fs, entry, true) {
                return ActivationAdapterOutcome::conflict();
            }
            if fs.create_parent_dirs_after_preflight(&entry.path).is_none() {
                return ActivationAdapterOutcome::conflict();
            }
            if !fs.write_owned_shim_for(
                &entry.path,
                target,
                &owner.package_state_key,
                &owner.package,
                &owner.integration_key,
            ) {
                return ActivationAdapterOutcome::conflict();
            }
            ActivationAdapterOutcome::service(
                IntegrationReasonCode::Ok,
                IntegrationAppliedState::Enabled,
            )
        }
        ActivationRollbackOperation::RemoveCreatedServiceMetadata => {
            match fs.entry(&entry.path) {
                Some(ActivationFsEntry::ServiceMetadata { source, owner })
                    if entry
                        .created_symlink_target
                        .as_ref()
                        .is_none_or(|expected| expected == &source)
                        && owner_matches_rollback_expectation(
                            entry.created_owner.as_ref(),
                            owner.as_ref(),
                        ) =>
                {
                    if !fs.remove_entry(&entry.path) {
                        return ActivationAdapterOutcome::conflict();
                    }
                }
                Some(_) => return ActivationAdapterOutcome::conflict(),
                None => {}
            }
            ActivationAdapterOutcome::service(
                IntegrationReasonCode::Ok,
                IntegrationAppliedState::Stopped,
            )
        }
        ActivationRollbackOperation::RestoreOwnedServiceMetadata => {
            let Some(source) = entry.previous_symlink_target.as_deref() else {
                return ActivationAdapterOutcome::conflict();
            };
            let Some(owner) = entry.previous_owner.as_ref() else {
                return ActivationAdapterOutcome::conflict();
            };
            match fs.entry(&entry.path) {
                Some(ActivationFsEntry::ServiceMetadata {
                    source: current_source,
                    owner: current_owner,
                }) if entry
                    .expected_current_symlink_target
                    .as_ref()
                    .is_none_or(|expected| expected == &current_source)
                    && owner_matches_rollback_expectation(
                        entry.expected_current_owner.as_ref(),
                        current_owner.as_ref(),
                    ) => {}
                None if entry.expected_current_symlink_target.is_none() => {}
                _ => return ActivationAdapterOutcome::conflict(),
            }
            if !fs.write_owned_service_metadata_for(
                &entry.path,
                source,
                &owner.package_state_key,
                &owner.package,
                &owner.integration_key,
            ) {
                return ActivationAdapterOutcome::conflict();
            }
            ActivationAdapterOutcome::service(
                IntegrationReasonCode::Ok,
                IntegrationAppliedState::Stopped,
            )
        }
    }
}

pub fn apply_integration_plan_with_fs(
    fs: &mut impl ActivationFilesystem,
    plan: &IntegrationActivationPlan,
) -> ActivationAdapterOutcome {
    match plan.kind.as_str() {
        "docker_cli_plugin" => apply_docker_cli_plugin_plan(fs, plan),
        "path_plugin" => apply_path_plugin_plan(fs, plan),
        _ => ActivationAdapterOutcome::service(
            IntegrationReasonCode::UnsupportedHost,
            IntegrationAppliedState::Unsupported,
        ),
    }
}

pub fn disable_integration_plan_with_fs(
    fs: &mut impl ActivationFilesystem,
    plan: &IntegrationActivationPlan,
) -> ActivationAdapterOutcome {
    match plan.kind.as_str() {
        "docker_cli_plugin" => disable_docker_cli_plugin_plan(fs, plan),
        "path_plugin" => disable_path_plugin_plan(fs, plan),
        _ => ActivationAdapterOutcome::service(
            IntegrationReasonCode::UnsupportedHost,
            IntegrationAppliedState::Unsupported,
        ),
    }
}

pub fn apply_integration_plan(
    platform: HostPlatform,
    plan: &IntegrationActivationPlan,
    activation_records: &[crate::IntegrationActivationRecord],
) -> ActivationAdapterOutcome {
    let mut fs = real_activation_fs_from_records(platform, activation_records);
    apply_integration_plan_with_fs(&mut fs, plan)
}

pub fn disable_integration_plan(
    platform: HostPlatform,
    plan: &IntegrationActivationPlan,
    activation_records: &[crate::IntegrationActivationRecord],
) -> ActivationAdapterOutcome {
    let mut fs = real_activation_fs_from_records(platform, activation_records);
    disable_integration_plan_with_fs(&mut fs, plan)
}

fn real_activation_fs_from_records(
    platform: HostPlatform,
    records: &[crate::IntegrationActivationRecord],
) -> RealActivationFs {
    let owners = records
        .iter()
        .filter(|record| {
            record.reason_code == IntegrationReasonCode::Ok
                && matches!(
                    record.applied_state,
                    IntegrationAppliedState::Installed
                        | IntegrationAppliedState::Enabled
                        | IntegrationAppliedState::Running
                )
        })
        .filter_map(|record| {
            let path = record.host_path.clone()?;
            Some((
                path,
                ActivationOwner {
                    package_state_key: record.package_state_key.clone(),
                    package: record.package.clone(),
                    integration_key: record.integration_key.clone(),
                },
            ))
        });
    RealActivationFs::new(platform, owners)
}

pub fn apply_service_plan(
    executor: &mut impl ActivationCommandExecutor,
    plan: &IntegrationActivationPlan,
) -> ActivationAdapterOutcome {
    let mut fs = MemoryActivationFs::new(platform_for_service_plan(plan));
    apply_service_plan_with_fs(&mut fs, executor, plan)
}

pub fn apply_service_plan_with_fs(
    fs: &mut MemoryActivationFs,
    executor: &mut impl ActivationCommandExecutor,
    plan: &IntegrationActivationPlan,
) -> ActivationAdapterOutcome {
    if windows_admin_required(plan) {
        return ActivationAdapterOutcome::escalation_required();
    }

    let mut rollback = match install_service_metadata(fs, plan) {
        Ok(rollback) => rollback,
        Err(reason_code) => {
            return ActivationAdapterOutcome::service(reason_code, IntegrationAppliedState::Failed);
        }
    };

    let Some(commands) = service_apply_commands(plan) else {
        return ActivationAdapterOutcome::service(
            IntegrationReasonCode::UnsupportedHost,
            IntegrationAppliedState::Unsupported,
        );
    };

    if let Err(result) = run_service_commands(executor, &commands) {
        let mut outcome = command_failure_outcome(result);
        outcome.rollback = rollback;
        return outcome;
    }
    let mut outcome = status_service_plan(executor, plan);
    outcome.rollback.append(&mut rollback);
    outcome
}

pub fn disable_service_plan(
    executor: &mut impl ActivationCommandExecutor,
    plan: &IntegrationActivationPlan,
) -> ActivationAdapterOutcome {
    let mut fs = MemoryActivationFs::new(platform_for_service_plan(plan));
    disable_service_plan_with_fs(&mut fs, executor, plan)
}

pub fn disable_service_plan_with_fs(
    fs: &mut MemoryActivationFs,
    executor: &mut impl ActivationCommandExecutor,
    plan: &IntegrationActivationPlan,
) -> ActivationAdapterOutcome {
    if windows_admin_required(plan) {
        return ActivationAdapterOutcome::escalation_required();
    }

    let Some((before_remove, after_remove)) = service_disable_commands(plan) else {
        return ActivationAdapterOutcome::service(
            IntegrationReasonCode::UnsupportedHost,
            IntegrationAppliedState::Unsupported,
        );
    };

    if let Err(result) = run_service_commands(executor, &before_remove) {
        return command_failure_outcome(result);
    }
    let mut rollback = remove_service_metadata(fs, plan);
    if let Err(result) = run_service_commands(executor, &after_remove) {
        let mut outcome = command_failure_outcome(result);
        outcome.rollback = rollback;
        return outcome;
    }
    let mut outcome = ActivationAdapterOutcome::service(
        IntegrationReasonCode::Ok,
        IntegrationAppliedState::Stopped,
    );
    outcome.rollback.append(&mut rollback);
    outcome
}

pub fn status_service_plan(
    executor: &mut impl ActivationCommandExecutor,
    plan: &IntegrationActivationPlan,
) -> ActivationAdapterOutcome {
    if windows_admin_required(plan) {
        return ActivationAdapterOutcome::escalation_required();
    }

    let Some((program, args)) = service_status_command(plan) else {
        return ActivationAdapterOutcome::service(
            IntegrationReasonCode::UnsupportedHost,
            IntegrationAppliedState::Unsupported,
        );
    };
    let result = executor.run(&program, &args);
    if let Some(applied_state) =
        parse_service_status(plan.adapter.clone(), &result.stdout, &result.stderr)
    {
        return ActivationAdapterOutcome::service(IntegrationReasonCode::Ok, applied_state);
    }
    if !result.succeeded() {
        return command_failure_outcome(result);
    }

    command_failure_outcome(result)
}

pub fn run_service_action_plan(
    executor: &mut impl ActivationCommandExecutor,
    plan: &IntegrationActivationPlan,
    action: NativeServiceAction,
) -> ActivationAdapterOutcome {
    match action {
        NativeServiceAction::Status => {
            let Some((program, args)) = service_status_command(plan) else {
                return ActivationAdapterOutcome::service(
                    IntegrationReasonCode::UnsupportedHost,
                    IntegrationAppliedState::Unsupported,
                );
            };
            let result = executor.run(&program, &args);
            if let Some(applied_state) =
                parse_service_status(plan.adapter.clone(), &result.stdout, &result.stderr)
            {
                if applied_state == IntegrationAppliedState::Unsupported {
                    return ActivationAdapterOutcome::service(
                        IntegrationReasonCode::UnsupportedHost,
                        applied_state,
                    );
                }
                return ActivationAdapterOutcome::service(IntegrationReasonCode::Ok, applied_state);
            }
            command_failure_outcome(result)
        }
        NativeServiceAction::Start | NativeServiceAction::Stop | NativeServiceAction::Restart => {
            let Some(commands) = service_action_commands(plan, action) else {
                return ActivationAdapterOutcome::service(
                    IntegrationReasonCode::UnsupportedHost,
                    IntegrationAppliedState::Unsupported,
                );
            };
            if let Err(result) = run_service_commands(executor, &commands) {
                return command_failure_outcome(result);
            }
            let applied_state = match action {
                NativeServiceAction::Stop => IntegrationAppliedState::Stopped,
                _ => IntegrationAppliedState::Running,
            };
            ActivationAdapterOutcome::service(IntegrationReasonCode::Ok, applied_state)
        }
    }
}

fn run_service_commands(
    executor: &mut impl ActivationCommandExecutor,
    commands: &[(String, Vec<String>)],
) -> Result<(), NativeCommandResult> {
    for (program, args) in commands {
        let result = executor.run(program, args);
        if !result.succeeded() {
            return Err(result);
        }
    }
    Ok(())
}

fn install_service_metadata(
    fs: &mut MemoryActivationFs,
    plan: &IntegrationActivationPlan,
) -> Result<Vec<ActivationRollbackEntry>, IntegrationReasonCode> {
    if !matches!(
        plan.adapter,
        IntegrationAdapterKind::SystemdUser | IntegrationAdapterKind::LaunchdUser
    ) {
        return Ok(Vec::new());
    }

    let expected_owner = owner_for_plan(plan);
    match fs.entries.get(&plan.host_path).cloned() {
        Some(MemoryActivationFileEntry::ServiceMetadata { source, owner })
            if source == plan.source_path && owner.as_ref() == Some(&expected_owner) =>
        {
            Ok(Vec::new())
        }
        Some(MemoryActivationFileEntry::ServiceMetadata { source, owner })
            if owner.as_ref() == Some(&expected_owner) =>
        {
            fs.write_service_metadata_for(
                &plan.host_path,
                &plan.source_path,
                &plan.package_state_key,
                &plan.package,
                &plan.integration_key,
            );
            Ok(vec![ActivationRollbackEntry {
                operation: ActivationRollbackOperation::RestoreOwnedServiceMetadata,
                path: plan.host_path.clone(),
                previous_symlink_target: Some(source),
                previous_shim_target: None,
                previous_owner: owner,
                created_symlink_target: None,
                created_shim_target: None,
                created_owner: None,
                expected_current_symlink_target: Some(plan.source_path.clone()),
                expected_current_shim_target: None,
                expected_current_owner: Some(owner_for_plan(plan)),
                expected_current_absent: false,
                created_parent_dirs: Vec::new(),
            }])
        }
        Some(_) => Err(IntegrationReasonCode::HostPathConflict),
        None => {
            if plan.adapter == IntegrationAdapterKind::LaunchdUser {
                let Some(created_parent_dirs) =
                    fs.create_parent_dirs_after_preflight(&plan.host_path)
                else {
                    return Err(IntegrationReasonCode::HostPathConflict);
                };
                fs.write_service_metadata_for(
                    &plan.host_path,
                    &plan.source_path,
                    &plan.package_state_key,
                    &plan.package,
                    &plan.integration_key,
                );
                Ok(vec![ActivationRollbackEntry {
                    operation: ActivationRollbackOperation::RemoveCreatedServiceMetadata,
                    path: plan.host_path.clone(),
                    previous_symlink_target: None,
                    previous_shim_target: None,
                    previous_owner: None,
                    created_symlink_target: Some(plan.source_path.clone()),
                    created_shim_target: None,
                    created_owner: Some(owner_for_plan(plan)),
                    expected_current_symlink_target: None,
                    expected_current_shim_target: None,
                    expected_current_owner: None,
                    expected_current_absent: false,
                    created_parent_dirs,
                }])
            } else {
                fs.write_service_metadata_for(
                    &plan.host_path,
                    &plan.source_path,
                    &plan.package_state_key,
                    &plan.package,
                    &plan.integration_key,
                );
                Ok(vec![ActivationRollbackEntry {
                    operation: ActivationRollbackOperation::RemoveCreatedServiceMetadata,
                    path: plan.host_path.clone(),
                    previous_symlink_target: None,
                    previous_shim_target: None,
                    previous_owner: None,
                    created_symlink_target: Some(plan.source_path.clone()),
                    created_shim_target: None,
                    created_owner: Some(owner_for_plan(plan)),
                    expected_current_symlink_target: None,
                    expected_current_shim_target: None,
                    expected_current_owner: None,
                    expected_current_absent: false,
                    created_parent_dirs: Vec::new(),
                }])
            }
        }
    }
}

fn remove_service_metadata(
    fs: &mut MemoryActivationFs,
    plan: &IntegrationActivationPlan,
) -> Vec<ActivationRollbackEntry> {
    if !matches!(
        plan.adapter,
        IntegrationAdapterKind::SystemdUser | IntegrationAdapterKind::LaunchdUser
    ) {
        return Vec::new();
    }

    let Some(MemoryActivationFileEntry::ServiceMetadata { source, owner }) =
        fs.entries.remove(&plan.host_path)
    else {
        return Vec::new();
    };

    vec![ActivationRollbackEntry {
        operation: ActivationRollbackOperation::RestoreOwnedServiceMetadata,
        path: plan.host_path.clone(),
        previous_symlink_target: Some(source),
        previous_shim_target: None,
        previous_owner: owner,
        created_symlink_target: None,
        created_shim_target: None,
        created_owner: None,
        expected_current_symlink_target: None,
        expected_current_shim_target: None,
        expected_current_owner: None,
        expected_current_absent: true,
        created_parent_dirs: Vec::new(),
    }]
}

fn command_failure_outcome(result: NativeCommandResult) -> ActivationAdapterOutcome {
    if result.status == 127 || command_output_contains(&result, "not found") {
        ActivationAdapterOutcome::service(
            IntegrationReasonCode::AdapterToolMissing,
            IntegrationAppliedState::Unsupported,
        )
    } else {
        ActivationAdapterOutcome::service(
            IntegrationReasonCode::NativeCommandFailed,
            IntegrationAppliedState::Failed,
        )
    }
}

fn command_output_contains(result: &NativeCommandResult, needle: &str) -> bool {
    result.stdout.to_ascii_lowercase().contains(needle)
        || result.stderr.to_ascii_lowercase().contains(needle)
}

fn service_apply_commands(plan: &IntegrationActivationPlan) -> Option<Vec<(String, Vec<String>)>> {
    let service_name = service_name(plan)?;
    match plan.adapter {
        IntegrationAdapterKind::SystemdUser => Some(vec![
            command("systemctl", &["--user", "link", &plan.source_path]),
            command("systemctl", &["--user", "daemon-reload"]),
            command("systemctl", &["--user", "enable", &service_name]),
            command("systemctl", &["--user", "start", &service_name]),
        ]),
        IntegrationAdapterKind::LaunchdUser => {
            let target = format!("gui/current/{service_name}");
            Some(vec![
                command("launchctl", &["bootstrap", "gui/current", &plan.host_path]),
                command("launchctl", &["enable", &target]),
                command("launchctl", &["kickstart", "-k", &target]),
            ])
        }
        IntegrationAdapterKind::WindowsServiceUser => Some(vec![
            command(
                "crosspack-service-user",
                &["install", &service_name, &plan.source_path],
            ),
            command("crosspack-service-user", &["enable", &service_name]),
            command("crosspack-service-user", &["start", &service_name]),
        ]),
        _ => None,
    }
}

fn service_disable_commands(plan: &IntegrationActivationPlan) -> Option<ServiceDisableCommands> {
    let service_name = service_name(plan)?;
    match plan.adapter {
        IntegrationAdapterKind::SystemdUser => Some((
            vec![
                command("systemctl", &["--user", "stop", &service_name]),
                command("systemctl", &["--user", "disable", &service_name]),
                command("systemctl", &["--user", "reset-failed", &service_name]),
            ],
            vec![command("systemctl", &["--user", "daemon-reload"])],
        )),
        IntegrationAdapterKind::LaunchdUser => {
            let target = format!("gui/current/{service_name}");
            Some((
                vec![
                    command("launchctl", &["bootout", &target]),
                    command("launchctl", &["disable", &target]),
                ],
                Vec::new(),
            ))
        }
        IntegrationAdapterKind::WindowsServiceUser => Some((
            vec![
                command("crosspack-service-user", &["stop", &service_name]),
                command("crosspack-service-user", &["disable", &service_name]),
                command("crosspack-service-user", &["remove", &service_name]),
            ],
            Vec::new(),
        )),
        _ => None,
    }
}

fn service_status_command(plan: &IntegrationActivationPlan) -> Option<(String, Vec<String>)> {
    let service_name = service_name(plan)?;
    match plan.adapter {
        IntegrationAdapterKind::SystemdUser => {
            Some(command("systemctl", &["--user", "status", &service_name]))
        }
        IntegrationAdapterKind::LaunchdUser => Some(command(
            "launchctl",
            &["print", &format!("gui/current/{service_name}")],
        )),
        IntegrationAdapterKind::WindowsServiceUser => Some(command(
            "crosspack-service-user",
            &["status", &service_name],
        )),
        _ => None,
    }
}

fn service_action_commands(
    plan: &IntegrationActivationPlan,
    action: NativeServiceAction,
) -> Option<Vec<(String, Vec<String>)>> {
    let service_name = service_name(plan)?;
    match (plan.adapter.clone(), action) {
        (IntegrationAdapterKind::SystemdUser, NativeServiceAction::Start) => Some(vec![command(
            "systemctl",
            &["--user", "start", &service_name],
        )]),
        (IntegrationAdapterKind::SystemdUser, NativeServiceAction::Stop) => Some(vec![command(
            "systemctl",
            &["--user", "stop", &service_name],
        )]),
        (IntegrationAdapterKind::SystemdUser, NativeServiceAction::Restart) => Some(vec![command(
            "systemctl",
            &["--user", "restart", &service_name],
        )]),
        (IntegrationAdapterKind::LaunchdUser, NativeServiceAction::Start) => Some(vec![command(
            "launchctl",
            &["kickstart", "-k", &format!("gui/current/{service_name}")],
        )]),
        (IntegrationAdapterKind::LaunchdUser, NativeServiceAction::Stop) => Some(vec![command(
            "launchctl",
            &["bootout", &format!("gui/current/{service_name}")],
        )]),
        (IntegrationAdapterKind::LaunchdUser, NativeServiceAction::Restart) => Some(vec![command(
            "launchctl",
            &["kickstart", "-k", &format!("gui/current/{service_name}")],
        )]),
        (IntegrationAdapterKind::WindowsServiceUser, NativeServiceAction::Start) => {
            Some(vec![command(
                "crosspack-service-user",
                &["start", &service_name],
            )])
        }
        (IntegrationAdapterKind::WindowsServiceUser, NativeServiceAction::Stop) => {
            Some(vec![command(
                "crosspack-service-user",
                &["stop", &service_name],
            )])
        }
        (IntegrationAdapterKind::WindowsServiceUser, NativeServiceAction::Restart) => Some(vec![
            command("crosspack-service-user", &["stop", &service_name]),
            command("crosspack-service-user", &["start", &service_name]),
        ]),
        (_, NativeServiceAction::Status) => unreachable!(),
        _ => None,
    }
}

fn command(program: &str, args: &[&str]) -> (String, Vec<String>) {
    (
        program.to_string(),
        args.iter().map(|arg| (*arg).to_string()).collect(),
    )
}

fn windows_admin_required(plan: &IntegrationActivationPlan) -> bool {
    plan.adapter == IntegrationAdapterKind::WindowsServiceUser
        && plan.scope == IntegrationActivationScope::System
}

fn service_name(plan: &IntegrationActivationPlan) -> Option<String> {
    match plan.adapter {
        IntegrationAdapterKind::SystemdUser => plan
            .host_path
            .strip_prefix("systemd-user:")
            .map(str::to_string),
        IntegrationAdapterKind::LaunchdUser => file_name(&plan.host_path)
            .and_then(|name| name.strip_suffix(".plist"))
            .map(str::to_string),
        IntegrationAdapterKind::WindowsServiceUser => plan
            .host_path
            .strip_prefix("windows-service-user:")
            .map(str::to_string),
        _ => None,
    }
}

fn file_name(path: &str) -> Option<&str> {
    path.rsplit(['/', '\\'])
        .next()
        .filter(|value| !value.is_empty())
}

fn parse_service_status(
    adapter: IntegrationAdapterKind,
    stdout: &str,
    stderr: &str,
) -> Option<IntegrationAppliedState> {
    let output = format!("{stdout}\n{stderr}").to_ascii_lowercase();
    match adapter {
        IntegrationAdapterKind::SystemdUser => {
            if output.contains("loaded: not-found")
                || output.contains("could not be found")
                || output.contains("not-found")
            {
                Some(IntegrationAppliedState::Unsupported)
            } else if output.contains("active: active (running)")
                || output.contains("active (running)")
            {
                Some(IntegrationAppliedState::Running)
            } else if output.contains("active: failed") || output.contains("failed (") {
                Some(IntegrationAppliedState::Failed)
            } else if output.contains("active: inactive") || output.contains("inactive (dead)") {
                Some(IntegrationAppliedState::Stopped)
            } else {
                None
            }
        }
        IntegrationAdapterKind::LaunchdUser => {
            if output.contains("state = running") {
                Some(IntegrationAppliedState::Running)
            } else if output.contains("input/output error") {
                Some(IntegrationAppliedState::Unsupported)
            } else if output.contains("last exit code = 1") || output.contains("failed") {
                Some(IntegrationAppliedState::Failed)
            } else if output.contains("state = exited") || output.contains("could not find service")
            {
                Some(IntegrationAppliedState::Stopped)
            } else {
                None
            }
        }
        IntegrationAdapterKind::WindowsServiceUser => {
            if output.contains("1060")
                || output.contains("does not exist as an installed service")
                || output.contains("unsupported")
            {
                Some(IntegrationAppliedState::Unsupported)
            } else if windows_status_has_state(&output, "stopped") || output.contains("not running")
            {
                Some(IntegrationAppliedState::Stopped)
            } else if windows_status_has_state(&output, "running") {
                Some(IntegrationAppliedState::Running)
            } else if windows_status_has_state(&output, "failed") || output.contains("state=failed")
            {
                Some(IntegrationAppliedState::Failed)
            } else if output.contains("state=running") {
                Some(IntegrationAppliedState::Running)
            } else if output.contains("state=stopped") || output.contains("stopped") {
                Some(IntegrationAppliedState::Stopped)
            } else {
                None
            }
        }
        _ => Some(IntegrationAppliedState::Unsupported),
    }
}

fn windows_status_has_state(output: &str, state: &str) -> bool {
    output.lines().any(|line| {
        let line = line.trim();
        line.starts_with("state") && line.split_whitespace().any(|token| token == state)
    })
}

fn platform_for_service_plan(plan: &IntegrationActivationPlan) -> HostPlatform {
    match plan.adapter {
        IntegrationAdapterKind::LaunchdUser => HostPlatform::Macos,
        IntegrationAdapterKind::WindowsServiceUser => HostPlatform::Windows,
        _ => HostPlatform::Linux,
    }
}

fn owner_for_plan(plan: &IntegrationActivationPlan) -> ActivationOwner {
    ActivationOwner {
        package_state_key: plan.package_state_key.clone(),
        package: plan.package.clone(),
        integration_key: plan.integration_key.clone(),
    }
}

fn destination_is_under_source_prefix(
    platform: HostPlatform,
    host_path: &str,
    source_path: &str,
) -> bool {
    let Some((prefix, _)) = source_path.split_once(source_integration_marker(platform)) else {
        return false;
    };
    if prefix.is_empty() {
        return false;
    }
    let sep = separator(platform);
    let bin_dir = format!("{prefix}{sep}bin");
    let Some(leaf) = host_path.strip_prefix(&(bin_dir + sep)) else {
        return false;
    };
    if leaf.is_empty() || leaf.contains(['/', '\\']) {
        return false;
    }
    platform != HostPlatform::Windows || leaf.ends_with(".cmd")
}

fn source_integration_marker(platform: HostPlatform) -> &'static str {
    match platform {
        HostPlatform::Linux | HostPlatform::Macos => "/share/integrations/",
        HostPlatform::Windows => "\\share\\integrations\\",
    }
}

fn separator(platform: HostPlatform) -> &'static str {
    match platform {
        HostPlatform::Linux | HostPlatform::Macos => "/",
        HostPlatform::Windows => "\\",
    }
}

fn parent_path(platform: HostPlatform, path: &str) -> Option<&str> {
    let index = match platform {
        HostPlatform::Windows => path.rfind(['\\', '/']),
        HostPlatform::Linux | HostPlatform::Macos => path.rfind('/'),
    }?;

    if index == 0 {
        return Some("/");
    }
    if platform == HostPlatform::Windows && index == 2 && path.as_bytes().get(1) == Some(&b':') {
        return Some(&path[..=index]);
    }
    Some(&path[..index])
}
