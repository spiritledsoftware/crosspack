use std::collections::{BTreeMap, HashMap};

use anyhow::{anyhow, Result};
use crosspack_core::PackageManifest;
use semver::VersionReq;

use crate::order::topo_order;
use crate::search::search;
use crate::types::{ResolvedGraph, RootRequirement};

pub fn select_highest_compatible<'a>(
    candidates: &'a [PackageManifest],
    requirement: &VersionReq,
) -> Option<&'a PackageManifest> {
    candidates
        .iter()
        .filter(|m| requirement.matches(&m.version))
        .max_by(|a, b| a.version.cmp(&b.version))
}

pub fn resolve_dependency_graph<F>(
    roots: &[RootRequirement],
    pins: &BTreeMap<String, VersionReq>,
    load_versions: F,
) -> Result<ResolvedGraph>
where
    F: FnMut(&str) -> Result<Vec<PackageManifest>>,
{
    resolve_dependency_graph_with_installed(roots, pins, &BTreeMap::new(), load_versions)
}

pub fn resolve_dependency_graph_with_installed<F>(
    roots: &[RootRequirement],
    pins: &BTreeMap<String, VersionReq>,
    installed: &BTreeMap<String, PackageManifest>,
    mut load_versions: F,
) -> Result<ResolvedGraph>
where
    F: FnMut(&str) -> Result<Vec<PackageManifest>>,
{
    let mut constraints: BTreeMap<String, Vec<VersionReq>> = BTreeMap::new();
    for root in roots {
        constraints
            .entry(root.name.clone())
            .or_default()
            .push(root.requirement.clone());
    }

    let mut versions_cache: HashMap<String, Vec<PackageManifest>> = HashMap::new();
    let mut selected: BTreeMap<String, PackageManifest> = BTreeMap::new();

    if !search(
        &mut constraints,
        pins,
        installed,
        &mut selected,
        &mut versions_cache,
        &mut load_versions,
    )? {
        return Err(anyhow!("no compatible dependency graph found"));
    }

    let install_order = topo_order(&selected)?;
    Ok(ResolvedGraph {
        manifests: selected,
        install_order,
    })
}
