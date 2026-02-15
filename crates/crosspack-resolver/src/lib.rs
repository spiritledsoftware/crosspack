use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use anyhow::{anyhow, Result};
use crosspack_core::PackageManifest;
use semver::VersionReq;

#[derive(Debug, Clone)]
pub struct RootRequirement {
    pub name: String,
    pub requirement: VersionReq,
}

#[derive(Debug, Clone)]
pub struct ResolvedGraph {
    pub manifests: BTreeMap<String, PackageManifest>,
    pub install_order: Vec<String>,
}

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

fn search<F>(
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

fn selected_satisfies_constraints(
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

fn topo_order(selected: &BTreeMap<String, PackageManifest>) -> Result<Vec<String>> {
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crosspack_core::PackageManifest;
    use semver::VersionReq;

    use crate::{
        resolve_dependency_graph, resolve_dependency_graph_with_installed,
        select_highest_compatible, RootRequirement,
    };

    #[test]
    fn selects_latest_matching_version() {
        let one = manifest(
            r#"
name = "tool"
version = "1.2.0"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/tool-1.2.0.tar.zst"
sha256 = "abc"
"#,
        );

        let two = manifest(
            r#"
name = "tool"
version = "1.3.0"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/tool-1.3.0.tar.zst"
sha256 = "def"
"#,
        );

        let req = VersionReq::parse("^1.0").expect("req should parse");
        let manifests = vec![one, two];
        let resolved = select_highest_compatible(&manifests, &req).expect("must resolve");

        assert_eq!(resolved.version.to_string(), "1.3.0");
    }

    #[test]
    fn resolves_transitive_dependencies_in_dependency_first_order() {
        let mut available = BTreeMap::new();
        available.insert(
            "app".to_string(),
            vec![manifest(
                r#"
name = "app"
version = "1.0.0"
[dependencies]
lib = "^1"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/app-1.0.0.tar.zst"
sha256 = "app"
"#,
            )],
        );
        available.insert(
            "lib".to_string(),
            vec![manifest(
                r#"
name = "lib"
version = "1.2.0"
[dependencies]
zlib = "^2"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/lib-1.2.0.tar.zst"
sha256 = "lib"
"#,
            )],
        );
        available.insert(
            "zlib".to_string(),
            vec![manifest(
                r#"
name = "zlib"
version = "2.1.0"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/zlib-2.1.0.tar.zst"
sha256 = "zlib"
"#,
            )],
        );

        let roots = vec![RootRequirement {
            name: "app".to_string(),
            requirement: VersionReq::STAR,
        }];
        let graph = resolve_dependency_graph(&roots, &BTreeMap::new(), |name| {
            Ok(available.get(name).cloned().unwrap_or_default())
        })
        .expect("must resolve graph");

        assert_eq!(graph.install_order, vec!["zlib", "lib", "app"]);
    }

    #[test]
    fn applies_pin_to_transitive_dependency_constraints() {
        let mut available = BTreeMap::new();
        available.insert(
            "app".to_string(),
            vec![manifest(
                r#"
name = "app"
version = "1.0.0"
[dependencies]
lib = "^1"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/app-1.0.0.tar.zst"
sha256 = "app"
"#,
            )],
        );
        available.insert(
            "lib".to_string(),
            vec![
                manifest(
                    r#"
name = "lib"
version = "1.5.0"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/lib-1.5.0.tar.zst"
sha256 = "a"
"#,
                ),
                manifest(
                    r#"
name = "lib"
version = "1.2.0"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/lib-1.2.0.tar.zst"
sha256 = "b"
"#,
                ),
            ],
        );

        let roots = vec![RootRequirement {
            name: "app".to_string(),
            requirement: VersionReq::STAR,
        }];
        let mut pins = BTreeMap::new();
        pins.insert("lib".to_string(), VersionReq::parse("<1.3.0").expect("pin"));

        let graph = resolve_dependency_graph(&roots, &pins, |name| {
            Ok(available.get(name).cloned().unwrap_or_default())
        })
        .expect("must resolve graph");
        assert_eq!(
            graph
                .manifests
                .get("lib")
                .expect("lib selected")
                .version
                .to_string(),
            "1.2.0"
        );
    }

    #[test]
    fn fails_on_missing_dependency_package() {
        let mut available = BTreeMap::new();
        available.insert(
            "app".to_string(),
            vec![manifest(
                r#"
name = "app"
version = "1.0.0"
[dependencies]
missing = "^1"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/app-1.0.0.tar.zst"
sha256 = "app"
"#,
            )],
        );

        let roots = vec![RootRequirement {
            name: "app".to_string(),
            requirement: VersionReq::STAR,
        }];
        let err = resolve_dependency_graph(&roots, &BTreeMap::new(), |name| {
            Ok(available.get(name).cloned().unwrap_or_default())
        })
        .expect_err("must fail");

        assert!(err.to_string().contains("missing"));
    }

    #[test]
    fn fails_on_pin_conflict() {
        let mut available = BTreeMap::new();
        available.insert(
            "app".to_string(),
            vec![manifest(
                r#"
name = "app"
version = "1.0.0"
[dependencies]
lib = "^2"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/app-1.0.0.tar.zst"
sha256 = "app"
"#,
            )],
        );
        available.insert(
            "lib".to_string(),
            vec![manifest(
                r#"
name = "lib"
version = "2.1.0"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/lib-2.1.0.tar.zst"
sha256 = "lib"
"#,
            )],
        );

        let roots = vec![RootRequirement {
            name: "app".to_string(),
            requirement: VersionReq::STAR,
        }];
        let mut pins = BTreeMap::new();
        pins.insert("lib".to_string(), VersionReq::parse("<2.0.0").expect("pin"));

        let err = resolve_dependency_graph(&roots, &pins, |name| {
            Ok(available.get(name).cloned().unwrap_or_default())
        })
        .expect_err("must fail");
        assert!(err.to_string().contains("pin"));
    }

    #[test]
    fn fails_on_cycle() {
        let mut available = BTreeMap::new();
        available.insert(
            "a".to_string(),
            vec![manifest(
                r#"
name = "a"
version = "1.0.0"
[dependencies]
b = "^1"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/a-1.0.0.tar.zst"
sha256 = "a"
"#,
            )],
        );
        available.insert(
            "b".to_string(),
            vec![manifest(
                r#"
name = "b"
version = "1.0.0"
[dependencies]
a = "^1"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/b-1.0.0.tar.zst"
sha256 = "b"
"#,
            )],
        );

        let roots = vec![RootRequirement {
            name: "a".to_string(),
            requirement: VersionReq::STAR,
        }];
        let err = resolve_dependency_graph(&roots, &BTreeMap::new(), |name| {
            Ok(available.get(name).cloned().unwrap_or_default())
        })
        .expect_err("must fail");
        assert!(err.to_string().contains("cycle"));
    }

    #[test]
    fn resolves_multi_root_global_graph() {
        let mut available = BTreeMap::new();
        available.insert(
            "tool-a".to_string(),
            vec![manifest(
                r#"
name = "tool-a"
version = "1.0.0"
[dependencies]
shared = "^1"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/tool-a-1.0.0.tar.zst"
sha256 = "a"
"#,
            )],
        );
        available.insert(
            "tool-b".to_string(),
            vec![manifest(
                r#"
name = "tool-b"
version = "1.0.0"
[dependencies]
shared = ">=1.2.0, <2.0.0"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/tool-b-1.0.0.tar.zst"
sha256 = "b"
"#,
            )],
        );
        available.insert(
            "shared".to_string(),
            vec![
                manifest(
                    r#"
name = "shared"
version = "1.3.0"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/shared-1.3.0.tar.zst"
sha256 = "shared13"
"#,
                ),
                manifest(
                    r#"
name = "shared"
version = "1.1.0"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/shared-1.1.0.tar.zst"
sha256 = "shared11"
"#,
                ),
            ],
        );

        let roots = vec![
            RootRequirement {
                name: "tool-a".to_string(),
                requirement: VersionReq::STAR,
            },
            RootRequirement {
                name: "tool-b".to_string(),
                requirement: VersionReq::STAR,
            },
        ];

        let graph = resolve_dependency_graph(&roots, &BTreeMap::new(), |name| {
            Ok(available.get(name).cloned().unwrap_or_default())
        })
        .expect("must resolve graph");

        assert_eq!(
            graph
                .manifests
                .get("shared")
                .expect("shared selected")
                .version
                .to_string(),
            "1.3.0"
        );
        assert_eq!(graph.install_order, vec!["shared", "tool-a", "tool-b"]);
    }

    #[test]
    fn prefers_direct_package_name_over_capability_provider_candidates() {
        let mut available = BTreeMap::new();
        available.insert(
            "app".to_string(),
            vec![manifest(
                r#"
name = "app"
version = "1.0.0"
[dependencies]
compiler = "*"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/app-1.0.0.tar.zst"
sha256 = "app"
"#,
            )],
        );
        available.insert(
            "compiler".to_string(),
            vec![
                manifest(
                    r#"
name = "gcc"
version = "2.0.0"
provides = ["compiler"]
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/gcc-2.0.0.tar.zst"
sha256 = "gcc"
"#,
                ),
                manifest(
                    r#"
name = "compiler"
version = "1.0.0"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/compiler-1.0.0.tar.zst"
sha256 = "compiler"
"#,
                ),
            ],
        );

        let roots = vec![RootRequirement {
            name: "app".to_string(),
            requirement: VersionReq::STAR,
        }];

        let graph = resolve_dependency_graph(&roots, &BTreeMap::new(), |name| {
            Ok(available.get(name).cloned().unwrap_or_default())
        })
        .expect("must resolve graph");

        assert_eq!(
            graph
                .manifests
                .get("compiler")
                .expect("compiler dependency must be selected")
                .name,
            "compiler"
        );
    }

    #[test]
    fn fails_when_selected_packages_conflict() {
        let mut available = BTreeMap::new();
        available.insert(
            "app".to_string(),
            vec![manifest(
                r#"
name = "app"
version = "1.0.0"
[dependencies]
foo = "*"
bar = "*"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/app-1.0.0.tar.zst"
sha256 = "app"
"#,
            )],
        );
        available.insert(
            "foo".to_string(),
            vec![manifest(
                r#"
name = "foo"
version = "1.0.0"
[conflicts]
bar = "*"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/foo-1.0.0.tar.zst"
sha256 = "foo"
"#,
            )],
        );
        available.insert(
            "bar".to_string(),
            vec![manifest(
                r#"
name = "bar"
version = "1.0.0"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/bar-1.0.0.tar.zst"
sha256 = "bar"
"#,
            )],
        );

        let roots = vec![RootRequirement {
            name: "app".to_string(),
            requirement: VersionReq::STAR,
        }];

        let err = resolve_dependency_graph(&roots, &BTreeMap::new(), |name| {
            Ok(available.get(name).cloned().unwrap_or_default())
        })
        .expect_err("conflicting graph must be rejected");

        assert!(
            err.to_string()
                .contains("no compatible dependency graph found"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn fails_when_selected_package_conflicts_with_installed_state() {
        let mut available = BTreeMap::new();
        available.insert(
            "app".to_string(),
            vec![manifest(
                r#"
name = "app"
version = "1.0.0"
[dependencies]
foo = "*"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/app-1.0.0.tar.zst"
sha256 = "app"
"#,
            )],
        );
        available.insert(
            "foo".to_string(),
            vec![manifest(
                r#"
name = "foo"
version = "1.0.0"
[conflicts]
bar = "*"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/foo-1.0.0.tar.zst"
sha256 = "foo"
"#,
            )],
        );

        let mut installed = BTreeMap::new();
        installed.insert(
            "bar".to_string(),
            manifest(
                r#"
name = "bar"
version = "1.0.0"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/bar-1.0.0.tar.zst"
sha256 = "bar"
"#,
            ),
        );

        let roots = vec![RootRequirement {
            name: "app".to_string(),
            requirement: VersionReq::STAR,
        }];

        let err =
            resolve_dependency_graph_with_installed(&roots, &BTreeMap::new(), &installed, |name| {
                Ok(available.get(name).cloned().unwrap_or_default())
            })
            .expect_err("installed-state conflict must be rejected");

        assert!(
            err.to_string()
                .contains("no compatible dependency graph found"),
            "unexpected error: {err}"
        );
    }

    fn manifest(raw: &str) -> PackageManifest {
        PackageManifest::from_toml_str(raw).expect("manifest must parse")
    }
}
