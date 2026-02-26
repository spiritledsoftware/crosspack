use anyhow::{Context, Result};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Component, PathBuf};

use crate::exposure::{
    clear_gui_exposure_state, read_gui_exposure_state, remove_exposed_binary,
    remove_exposed_completion, remove_exposed_gui_asset,
};
use crate::fs_utils::remove_file_if_exists;
use crate::native::{
    clear_native_sidecar_state, remove_package_native_gui_registrations_best_effort,
    run_package_native_uninstall_actions,
};
use crate::receipts::read_install_receipts;
use crate::{
    InstallMode, InstallReason, InstallReceipt, PrefixLayout, UninstallResult, UninstallStatus,
};

pub fn uninstall_package(layout: &PrefixLayout, name: &str) -> Result<UninstallResult> {
    uninstall_package_with_dependency_overrides(layout, name, &HashMap::new())
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
    let receipts = read_install_receipts(layout)?;
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
