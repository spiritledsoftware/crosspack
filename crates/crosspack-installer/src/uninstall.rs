use anyhow::{Context, Result};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Component, PathBuf};

use crate::exposure::{
    clear_gui_exposure_state, clear_integration_state, read_gui_exposure_state,
    read_integration_state, remove_exposed_binary, remove_exposed_completion,
    remove_exposed_gui_asset, remove_exposed_integration,
};
use crate::fs_utils::remove_file_if_exists;
use crate::native::{
    clear_native_sidecar_state, remove_package_native_gui_registrations_best_effort,
    run_identity_native_uninstall_actions, run_package_native_uninstall_actions,
};
use crate::receipts::clear_declared_services_state;
use crate::{
    clear_installed_package_state_document, disable_integration_plan,
    read_all_installed_package_states, read_identity_install_receipt,
    read_integration_activation_state, write_integration_activation_state, HostPlatform,
    InstallMode, InstallReason, InstallReceipt, InstalledPackageIdentity, InstalledPackageState,
    IntegrationActivationPlan, IntegrationAppliedState, IntegrationDesiredState,
    IntegrationProjection, IntegrationReasonCode, PrefixLayout, UninstallResult, UninstallStatus,
};

pub fn uninstall_package(layout: &PrefixLayout, name: &str) -> Result<UninstallResult> {
    uninstall_package_with_dependency_overrides(layout, name, &HashMap::new())
}

pub fn uninstall_package_identity(
    layout: &PrefixLayout,
    identity: &InstalledPackageIdentity,
) -> Result<UninstallResult> {
    let states = read_all_installed_package_states(layout)?;
    let state = states.iter().find(|state| &state.identity == identity);
    let receipt = if let Some(state) = state.as_ref() {
        state.receipt.clone()
    } else if let Some(identity_receipt) = read_identity_install_receipt(layout, identity)? {
        identity_receipt.receipt
    } else {
        return Ok(UninstallResult {
            name: identity.package.clone(),
            version: None,
            status: UninstallStatus::NotInstalled,
            pruned_dependencies: Vec::new(),
            blocked_by_roots: Vec::new(),
        });
    };

    let nodes = identity_uninstall_nodes(&states, identity, &receipt);
    let node_map = nodes
        .iter()
        .cloned()
        .map(|node| (node.key.clone(), node))
        .collect::<HashMap<_, _>>();
    let target_key = identity.state_key();
    let dependencies = identity_dependency_map(&nodes);
    let remaining_roots = collect_remaining_identity_roots(&nodes, &target_key);
    let reachable = reachable_packages(&remaining_roots, &dependencies);

    if reachable.contains(&target_key) {
        let mut blocked_by_roots = remaining_roots
            .iter()
            .filter(|root| package_reachable(root, &target_key, &dependencies))
            .filter_map(|root| node_map.get(root))
            .map(|node| node.package.clone())
            .collect::<Vec<_>>();
        blocked_by_roots.sort();
        blocked_by_roots.dedup();
        return Ok(UninstallResult {
            name: receipt.name,
            version: Some(receipt.version),
            status: UninstallStatus::BlockedByDependents,
            pruned_dependencies: Vec::new(),
            blocked_by_roots,
        });
    }

    let target_closure = reachable_packages(std::slice::from_ref(&target_key), &dependencies);
    let mut pruned_dependency_keys = target_closure
        .iter()
        .filter(|entry| *entry != &target_key)
        .filter(|entry| !reachable.contains(entry.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    pruned_dependency_keys.sort();
    let mut pruned_dependencies = pruned_dependency_keys
        .iter()
        .filter_map(|key| node_map.get(key))
        .map(|node| node.package.clone())
        .collect::<Vec<_>>();
    pruned_dependencies.sort();
    pruned_dependencies.dedup();

    let mut removal_keys = Vec::with_capacity(pruned_dependency_keys.len() + 1);
    removal_keys.push(target_key.clone());
    removal_keys.extend(pruned_dependency_keys.iter().cloned());
    let removal_key_set: HashSet<&str> = removal_keys.iter().map(String::as_str).collect();

    let mut target_status = UninstallStatus::RepairedStaleState;
    let mut removed_cache_paths = Vec::new();
    for removal_key in &removal_keys {
        let Some(removal_node) = node_map.get(removal_key) else {
            continue;
        };

        if removal_key == &target_key {
            target_status =
                remove_identity_artifacts(layout, identity, &removal_node.receipt, state)?;
        } else if let Some(removal_identity) = &removal_node.identity {
            let _ = remove_identity_artifacts(
                layout,
                removal_identity,
                &removal_node.receipt,
                states
                    .iter()
                    .find(|state| &state.identity == removal_identity),
            )?;
        } else {
            let _ = remove_receipt_artifacts(layout, &removal_node.receipt)?;
        }
        if let Some(cache_path) = &removal_node.receipt.cache_path {
            removed_cache_paths.push(cache_path.clone());
        }
    }

    let referenced_cache_paths: HashSet<String> = node_map
        .iter()
        .filter(|(key, _)| !removal_key_set.contains(key.as_str()))
        .filter_map(|(_, node)| node.receipt.cache_path.clone())
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
        name: receipt.name,
        version: Some(receipt.version),
        status: target_status,
        pruned_dependencies,
        blocked_by_roots: Vec::new(),
    })
}

fn remove_identity_artifacts(
    layout: &PrefixLayout,
    identity: &InstalledPackageIdentity,
    receipt: &InstallReceipt,
    state: Option<&InstalledPackageState>,
) -> Result<UninstallStatus> {
    if receipt.install_mode == InstallMode::Native {
        run_identity_native_uninstall_actions(layout, identity)?;
    }

    let integrations = state
        .map(|state| state.integrations.as_slice())
        .unwrap_or(&[]);
    cleanup_activation_records_for_uninstall(
        layout,
        &receipt.name,
        Some(&identity.state_key()),
        integrations,
    )?;

    let package_dir = layout.identity_package_dir(identity, &receipt.version);
    let package_existed = package_dir.exists();
    if package_existed {
        fs::remove_dir_all(&package_dir)
            .with_context(|| format!("failed to remove package dir: {}", package_dir.display()))?;
    }

    for exposed_bin in &receipt.exposed_bins {
        remove_exposed_binary(layout, exposed_bin)?;
    }
    for exposed_completion in &receipt.exposed_completions {
        remove_exposed_completion(layout, exposed_completion)?;
    }
    if let Some(state) = state {
        for asset in &state.gui_assets {
            remove_exposed_gui_asset(layout, asset)?;
        }
        for projection in &state.integrations {
            remove_exposed_integration(layout, projection)?;
        }
    }

    remove_file_if_exists(&layout.identity_receipt_path(identity)).with_context(|| {
        format!(
            "failed to remove install receipt: {}",
            layout.identity_receipt_path(identity).display()
        )
    })?;
    remove_file_if_exists(&layout.installed_identity_state_document_path(identity)).with_context(
        || {
            format!(
                "failed to remove installed state document: {}",
                layout
                    .installed_identity_state_document_path(identity)
                    .display()
            )
        },
    )?;
    remove_file_if_exists(&layout.identity_gui_state_path(identity))?;
    remove_file_if_exists(&layout.identity_gui_native_state_path(identity))?;
    remove_file_if_exists(&layout.identity_declared_services_state_path(identity))?;
    remove_file_if_exists(&layout.identity_integration_state_path(identity))?;

    Ok(if package_existed {
        UninstallStatus::Uninstalled
    } else {
        UninstallStatus::RepairedStaleState
    })
}

#[derive(Clone)]
struct IdentityUninstallNode {
    key: String,
    package: String,
    identity: Option<InstalledPackageIdentity>,
    receipt: InstallReceipt,
}

fn identity_uninstall_nodes(
    states: &[InstalledPackageState],
    target_identity: &InstalledPackageIdentity,
    target_receipt: &InstallReceipt,
) -> Vec<IdentityUninstallNode> {
    let mut nodes = states
        .iter()
        .map(|state| IdentityUninstallNode {
            key: state.identity.state_key(),
            package: state.receipt.name.clone(),
            identity: Some(state.identity.clone()),
            receipt: state.receipt.clone(),
        })
        .collect::<Vec<_>>();
    if !nodes
        .iter()
        .any(|node| node.key == target_identity.state_key())
    {
        nodes.push(IdentityUninstallNode {
            key: target_identity.state_key(),
            package: target_receipt.name.clone(),
            identity: Some(target_identity.clone()),
            receipt: target_receipt.clone(),
        });
    }
    nodes
}

fn collect_remaining_identity_roots(
    nodes: &[IdentityUninstallNode],
    target_key: &str,
) -> Vec<String> {
    let mut remaining_roots = nodes
        .iter()
        .filter(|node| node.key != target_key)
        .filter(|node| node.receipt.install_reason == InstallReason::Root)
        .map(|node| node.key.clone())
        .collect::<Vec<_>>();
    remaining_roots.sort();
    remaining_roots.dedup();
    remaining_roots
}

fn identity_dependency_map(nodes: &[IdentityUninstallNode]) -> HashMap<String, BTreeSet<String>> {
    nodes
        .iter()
        .map(|node| {
            let deps = node
                .receipt
                .dependencies
                .iter()
                .filter_map(|entry| parse_dependency_name(entry))
                .flat_map(|dep| {
                    nodes
                        .iter()
                        .filter(move |candidate| candidate.package == dep)
                        .map(|candidate| candidate.key.clone())
                })
                .collect::<BTreeSet<_>>();
            (node.key.clone(), deps)
        })
        .collect()
}

pub fn uninstall_package_with_dependency_overrides(
    layout: &PrefixLayout,
    name: &str,
    dependency_overrides: &HashMap<String, Vec<String>>,
) -> Result<UninstallResult> {
    uninstall_package_with_dependency_overrides_and_ignored_roots(
        layout,
        name,
        dependency_overrides,
        &HashSet::new(),
    )
}

pub fn uninstall_package_with_dependency_overrides_and_ignored_roots(
    layout: &PrefixLayout,
    name: &str,
    dependency_overrides: &HashMap<String, Vec<String>>,
    ignored_root_names: &HashSet<String>,
) -> Result<UninstallResult> {
    let receipts = read_all_installed_package_states(layout)?
        .into_iter()
        .map(|state| state.receipt)
        .collect::<Vec<_>>();
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
    let mut dependencies = dependency_map(&receipt_map);
    apply_dependency_overrides(&mut dependencies, dependency_overrides);

    let remaining_roots = collect_remaining_roots(&receipt_map, name, ignored_root_names);
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

pub fn uninstall_blocked_by_roots_with_dependency_overrides(
    layout: &PrefixLayout,
    name: &str,
    dependency_overrides: &HashMap<String, Vec<String>>,
) -> Result<Vec<String>> {
    uninstall_blocked_by_roots_with_dependency_overrides_and_ignored_roots(
        layout,
        name,
        dependency_overrides,
        &HashSet::new(),
    )
}

pub fn uninstall_blocked_by_roots_with_dependency_overrides_and_ignored_roots(
    layout: &PrefixLayout,
    name: &str,
    dependency_overrides: &HashMap<String, Vec<String>>,
    ignored_root_names: &HashSet<String>,
) -> Result<Vec<String>> {
    let receipts = read_all_installed_package_states(layout)?
        .into_iter()
        .map(|state| state.receipt)
        .collect::<Vec<_>>();
    let receipt_map: HashMap<String, InstallReceipt> = receipts
        .iter()
        .cloned()
        .map(|receipt| (receipt.name.clone(), receipt))
        .collect();

    if !receipt_map.contains_key(name) {
        return Ok(Vec::new());
    }

    let mut dependencies = dependency_map(&receipt_map);
    apply_dependency_overrides(&mut dependencies, dependency_overrides);

    let remaining_roots = collect_remaining_roots(&receipt_map, name, ignored_root_names);
    let reachable = reachable_packages(&remaining_roots, &dependencies);

    if !reachable.contains(name) {
        return Ok(Vec::new());
    }

    let mut blocked_by_roots = remaining_roots
        .iter()
        .filter(|root| package_reachable(root, name, &dependencies))
        .cloned()
        .collect::<Vec<_>>();
    blocked_by_roots.sort();
    blocked_by_roots.dedup();
    Ok(blocked_by_roots)
}

fn collect_remaining_roots(
    receipt_map: &HashMap<String, InstallReceipt>,
    target_name: &str,
    ignored_root_names: &HashSet<String>,
) -> Vec<String> {
    let mut remaining_roots = receipt_map
        .values()
        .filter(|receipt| receipt.name != target_name)
        .filter(|receipt| receipt.install_reason == InstallReason::Root)
        .filter(|receipt| !ignored_root_names.contains(&receipt.name))
        .map(|receipt| receipt.name.clone())
        .collect::<Vec<_>>();
    remaining_roots.sort();
    remaining_roots.dedup();
    remaining_roots
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

fn remove_receipt_artifacts(
    layout: &PrefixLayout,
    receipt: &InstallReceipt,
) -> Result<UninstallStatus> {
    if receipt.install_mode == InstallMode::Native {
        run_package_native_uninstall_actions(layout, &receipt.name)?;
        clear_native_sidecar_state(layout, &receipt.name)?;
    }

    let integration_projections = read_integration_state(layout, &receipt.name)?;
    cleanup_activation_records_for_uninstall(
        layout,
        &receipt.name,
        None,
        &integration_projections,
    )?;

    let package_dir = layout.package_dir(&receipt.name, &receipt.version);
    let package_existed = package_dir.exists();
    if package_existed {
        fs::remove_dir_all(&package_dir)
            .with_context(|| format!("failed to remove package dir: {}", package_dir.display()))?;
    }

    for exposed_bin in &receipt.exposed_bins {
        remove_exposed_binary(layout, exposed_bin)?;
    }
    for exposed_completion in &receipt.exposed_completions {
        remove_exposed_completion(layout, exposed_completion)?;
    }

    let gui_assets = read_gui_exposure_state(layout, &receipt.name)?;
    for asset in &gui_assets {
        remove_exposed_gui_asset(layout, asset)?;
    }
    clear_gui_exposure_state(layout, &receipt.name)?;
    for projection in &integration_projections {
        remove_exposed_integration(layout, projection)?;
    }
    clear_integration_state(layout, &receipt.name)?;
    if receipt.install_mode != InstallMode::Native {
        let _native_gui_warnings =
            remove_package_native_gui_registrations_best_effort(layout, &receipt.name)?;
    }

    let receipt_path = layout.receipt_path(&receipt.name);
    fs::remove_file(&receipt_path).with_context(|| {
        format!(
            "failed to remove install receipt: {}",
            receipt_path.display()
        )
    })?;
    clear_declared_services_state(layout, &receipt.name)?;
    clear_installed_package_state_document(layout, receipt)?;

    Ok(if package_existed {
        UninstallStatus::Uninstalled
    } else {
        UninstallStatus::RepairedStaleState
    })
}

fn cleanup_activation_records_for_uninstall(
    layout: &PrefixLayout,
    package: &str,
    package_state_key: Option<&str>,
    projections: &[IntegrationProjection],
) -> Result<()> {
    cleanup_activation_records_for_uninstall_with(
        layout,
        package,
        package_state_key,
        projections,
        |record, plan, existing_records| {
            let platform = current_host_platform();
            if record.kind == "service" {
                crate::ActivationAdapterOutcome {
                    reason_code: IntegrationReasonCode::UnsupportedHost,
                    applied_state: IntegrationAppliedState::Unsupported,
                    rollback: Vec::new(),
                }
            } else {
                disable_integration_plan(platform, plan, existing_records)
            }
        },
    )
}

pub(crate) fn cleanup_activation_records_for_uninstall_with(
    layout: &PrefixLayout,
    package: &str,
    package_state_key: Option<&str>,
    projections: &[IntegrationProjection],
    mut disable_record: impl FnMut(
        &crate::IntegrationActivationRecord,
        &IntegrationActivationPlan,
        &[crate::IntegrationActivationRecord],
    ) -> crate::ActivationAdapterOutcome,
) -> Result<()> {
    let mut records = read_integration_activation_state(layout)?;
    let existing_records = records.clone();
    let mut changed = false;
    let mut next_records = Vec::with_capacity(records.len());

    for mut record in records.drain(..) {
        if record.package != package {
            next_records.push(record);
            continue;
        }
        if package_state_key.is_some_and(|key| record.package_state_key != key) {
            next_records.push(record);
            continue;
        }

        let Some(plan) = activation_plan_for_uninstall_record(layout, &record, projections) else {
            record.desired_state = IntegrationDesiredState::Projected;
            record.applied_state = IntegrationAppliedState::Failed;
            record.reason_code = IntegrationReasonCode::StateMissing;
            next_records.push(record);
            changed = true;
            continue;
        };

        let outcome = disable_record(&record, &plan, &existing_records);
        if outcome.reason_code == IntegrationReasonCode::Ok
            && uninstall_activation_cleanup_verified(&record, &outcome)
        {
            changed = true;
            continue;
        }

        record.desired_state = IntegrationDesiredState::Projected;
        record.applied_state = IntegrationAppliedState::Failed;
        record.reason_code = if outcome.reason_code == IntegrationReasonCode::Ok {
            IntegrationReasonCode::HostPathConflict
        } else {
            outcome.reason_code
        };
        next_records.push(record);
        changed = true;
    }

    if changed {
        write_integration_activation_state(layout, &next_records)?;
    }
    Ok(())
}

fn uninstall_activation_cleanup_verified(
    record: &crate::IntegrationActivationRecord,
    outcome: &crate::ActivationAdapterOutcome,
) -> bool {
    record.kind != "service"
        || outcome.rollback.iter().any(|entry| {
            entry.operation == crate::ActivationRollbackOperation::RestoreOwnedServiceMetadata
        })
}

fn activation_plan_for_uninstall_record(
    layout: &PrefixLayout,
    record: &crate::IntegrationActivationRecord,
    projections: &[IntegrationProjection],
) -> Option<IntegrationActivationPlan> {
    let projection = projections
        .iter()
        .find(|projection| projection.key == record.integration_key)?;
    let host_path = record.host_path.clone()?;
    Some(IntegrationActivationPlan {
        package_state_key: record.package_state_key.clone(),
        package: record.package.clone(),
        integration_key: record.integration_key.clone(),
        kind: record.kind.clone(),
        adapter: record.adapter.clone(),
        scope: record.scope.clone(),
        desired_state: IntegrationDesiredState::Projected,
        host_path,
        source_path: layout
            .integrations_dir()
            .join(&projection.rel_path)
            .display()
            .to_string(),
    })
}

fn current_host_platform() -> HostPlatform {
    if cfg!(target_os = "windows") {
        HostPlatform::Windows
    } else if cfg!(target_os = "macos") {
        HostPlatform::Macos
    } else {
        HostPlatform::Linux
    }
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

fn apply_dependency_overrides(
    dependencies: &mut HashMap<String, BTreeSet<String>>,
    dependency_overrides: &HashMap<String, Vec<String>>,
) {
    for (package, override_dependencies) in dependency_overrides {
        let projected = override_dependencies
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        dependencies.insert(package.clone(), projected);
    }
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
