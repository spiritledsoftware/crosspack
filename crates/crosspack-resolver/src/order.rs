use std::collections::{BTreeMap, BTreeSet, HashSet};

use anyhow::{anyhow, Result};
use crosspack_core::PackageManifest;

pub(crate) fn topo_order(selected: &BTreeMap<String, PackageManifest>) -> Result<Vec<String>> {
    let mut deps: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut reverse: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut in_degree: BTreeMap<String, usize> = BTreeMap::new();

    for name in selected.keys() {
        deps.insert(name.clone(), BTreeSet::new());
        reverse.insert(name.clone(), BTreeSet::new());
        in_degree.insert(name.clone(), 0);
    }

    for (name, manifest) in selected {
        for dep_name in manifest.dependencies.keys() {
            if !selected.contains_key(dep_name) {
                continue;
            }
            deps.entry(name.clone())
                .or_default()
                .insert(dep_name.clone());
            reverse
                .entry(dep_name.clone())
                .or_default()
                .insert(name.clone());
        }
    }

    for (name, dependency_set) in &deps {
        in_degree.insert(name.clone(), dependency_set.len());
    }

    let mut ready: BTreeSet<String> = in_degree
        .iter()
        .filter_map(|(name, degree)| (*degree == 0).then_some(name.clone()))
        .collect();
    let mut ordered = Vec::new();

    while let Some(next) = ready.pop_first() {
        ordered.push(next.clone());
        if let Some(children) = reverse.get(&next) {
            for child in children {
                if let Some(degree) = in_degree.get_mut(child) {
                    *degree = degree.saturating_sub(1);
                    if *degree == 0 {
                        ready.insert(child.clone());
                    }
                }
            }
        }
    }

    if ordered.len() != selected.len() {
        let ordered_set: HashSet<&str> = ordered.iter().map(String::as_str).collect();
        let mut cycle_nodes = selected
            .keys()
            .filter(|name| !ordered_set.contains(name.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        cycle_nodes.sort();
        return Err(anyhow!(
            "dependency cycle detected involving: {}",
            cycle_nodes.join(", ")
        ));
    }

    Ok(ordered)
}
