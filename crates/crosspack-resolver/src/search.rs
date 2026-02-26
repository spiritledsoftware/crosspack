use std::collections::{BTreeMap, HashMap};

use anyhow::{anyhow, Result};
use crosspack_core::PackageManifest;
use semver::VersionReq;

use crate::constraints::selected_satisfies_constraints;

pub(crate) fn search<F>(
    constraints: &mut BTreeMap<String, Vec<VersionReq>>,
    pins: &BTreeMap<String, VersionReq>,
    installed: &BTreeMap<String, PackageManifest>,
    selected: &mut BTreeMap<String, PackageManifest>,
    versions_cache: &mut HashMap<String, Vec<PackageManifest>>,
    load_versions: &mut F,
) -> Result<bool>
where
    F: FnMut(&str) -> Result<Vec<PackageManifest>>,
{
    if let Some(next) = constraints
        .keys()
        .find(|name| !selected.contains_key(*name))
        .cloned()
    {
        let candidates =
            matching_candidates(&next, constraints, pins, versions_cache, load_versions)?;

        for candidate in candidates {
            selected.insert(next.clone(), candidate.clone());

            let mut added_constraints: Vec<(String, usize)> = Vec::new();
            for (dep_name, dep_req) in &candidate.dependencies {
                let list = constraints.entry(dep_name.clone()).or_default();
                list.push(dep_req.clone());
                added_constraints.push((dep_name.clone(), list.len()));
            }

            let consistent = selected_satisfies_constraints(selected, constraints, pins, installed);
            if consistent
                && search(
                    constraints,
                    pins,
                    installed,
                    selected,
                    versions_cache,
                    load_versions,
                )?
            {
                return Ok(true);
            }

            for (dep_name, old_len) in added_constraints {
                if let Some(list) = constraints.get_mut(&dep_name) {
                    list.truncate(old_len.saturating_sub(1));
                }
            }
            constraints.retain(|_, reqs| !reqs.is_empty());
            selected.remove(&next);
        }

        return Ok(false);
    }

    Ok(selected_satisfies_constraints(
        selected,
        constraints,
        pins,
        installed,
    ))
}

fn matching_candidates<F>(
    name: &str,
    constraints: &BTreeMap<String, Vec<VersionReq>>,
    pins: &BTreeMap<String, VersionReq>,
    versions_cache: &mut HashMap<String, Vec<PackageManifest>>,
    load_versions: &mut F,
) -> Result<Vec<PackageManifest>>
where
    F: FnMut(&str) -> Result<Vec<PackageManifest>>,
{
    if !versions_cache.contains_key(name) {
        let versions = load_versions(name)?;
        versions_cache.insert(name.to_string(), versions);
    }

    let versions = versions_cache
        .get(name)
        .ok_or_else(|| anyhow!("internal resolver cache error for package '{name}'"))?;
    if versions.is_empty() {
        return Err(anyhow!(
            "package '{name}' was not found in the registry index"
        ));
    }

    let package_reqs = constraints.get(name).cloned().unwrap_or_default();
    let pin_req = pins.get(name);

    let matched: Vec<PackageManifest> = versions
        .iter()
        .filter(|manifest| {
            package_reqs
                .iter()
                .all(|req| req.matches(&manifest.version))
        })
        .filter(|manifest| {
            pin_req
                .map(|pin| pin.matches(&manifest.version))
                .unwrap_or(true)
        })
        .cloned()
        .collect();

    let has_direct_match = matched.iter().any(|manifest| manifest.name == name);
    let mut selected = if has_direct_match {
        matched
            .into_iter()
            .filter(|manifest| manifest.name == name)
            .collect::<Vec<_>>()
    } else {
        matched
            .into_iter()
            .filter(|manifest| manifest.provides.iter().any(|provided| provided == name))
            .collect::<Vec<_>>()
    };

    selected.sort_by(|a, b| b.version.cmp(&a.version).then_with(|| a.name.cmp(&b.name)));

    if selected.is_empty() {
        let req_desc = if package_reqs.is_empty() {
            "*".to_string()
        } else {
            package_reqs
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" && ")
        };
        if let Some(pin) = pin_req {
            return Err(anyhow!(
                "no matching version for '{name}' with constraints [{req_desc}] and pin {pin}"
            ));
        }
        return Err(anyhow!(
            "no matching version for '{name}' with constraints [{req_desc}]"
        ));
    }

    Ok(selected)
}
