use std::collections::BTreeMap;

use crosspack_core::PackageManifest;
use semver::VersionReq;

pub(crate) fn selected_satisfies_constraints(
    selected: &BTreeMap<String, PackageManifest>,
    constraints: &BTreeMap<String, Vec<VersionReq>>,
    pins: &BTreeMap<String, VersionReq>,
    installed: &BTreeMap<String, PackageManifest>,
) -> bool {
    for (name, manifest) in selected {
        if let Some(reqs) = constraints.get(name) {
            if !reqs.iter().all(|req| req.matches(&manifest.version)) {
                return false;
            }
        }
        if let Some(pin) = pins.get(name) {
            if !pin.matches(&manifest.version) {
                return false;
            }
        }
    }

    let selected_manifests: Vec<&PackageManifest> = selected.values().collect();
    for (index, left) in selected_manifests.iter().enumerate() {
        for right in selected_manifests.iter().skip(index + 1) {
            if manifests_conflict(left, right) {
                return false;
            }
        }
    }

    for selected_manifest in selected.values() {
        for (installed_name, installed_manifest) in installed {
            if selected.contains_key(installed_name) {
                continue;
            }
            if manifests_conflict(selected_manifest, installed_manifest) {
                return false;
            }
        }
    }

    true
}

fn manifests_conflict(left: &PackageManifest, right: &PackageManifest) -> bool {
    left.conflicts
        .get(&right.name)
        .map(|req| req.matches(&right.version))
        .unwrap_or(false)
        || right
            .conflicts
            .get(&left.name)
            .map(|req| req.matches(&left.version))
            .unwrap_or(false)
}
