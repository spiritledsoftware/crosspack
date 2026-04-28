use std::collections::{BTreeMap, BTreeSet};

use semver::Version;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanOperation {
    Install,
    Upgrade,
    Uninstall,
    BundleApply,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPlan {
    pub operation: PlanOperation,
    pub target: Option<String>,
    pub packages: Vec<PlannedPackage>,
    pub removals: Vec<PlannedRemoval>,
    pub replacements: Vec<PlannedReplacement>,
    pub transitions: Vec<PlannedTransition>,
    pub provider_substitutions: Vec<ProviderSubstitution>,
    pub conflicts: Vec<ConflictConstraint>,
    pub risk_flags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedPackage {
    pub name: String,
    pub version: String,
    pub target: String,
    pub install_reason: String,
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedRemoval {
    pub name: String,
    pub version: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedReplacement {
    pub removed_name: String,
    pub removed_version: String,
    pub replacement_name: String,
    pub replacement_version: String,
    pub requirement: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedTransition {
    pub name: String,
    pub from_version: String,
    pub to_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSubstitution {
    pub capability: String,
    pub provider: String,
    pub provider_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictConstraint {
    pub selected: String,
    pub selected_version: String,
    pub conflicts_with: String,
    pub requirement: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPackageSummary {
    pub name: String,
    pub version: String,
    pub dependencies: Vec<String>,
    pub install_reason: String,
}

pub fn plan_from_resolved_graph(
    operation: PlanOperation,
    target: Option<String>,
    graph: &crate::types::ResolvedGraph,
) -> InstallPlan {
    let packages = graph
        .install_order
        .iter()
        .filter_map(|name| graph.manifests.get(name))
        .map(|manifest| PlannedPackage {
            name: manifest.name.clone(),
            version: manifest.version.to_string(),
            target: target
                .clone()
                .or_else(|| {
                    manifest
                        .artifacts
                        .first()
                        .map(|artifact| artifact.target.clone())
                })
                .unwrap_or_default(),
            install_reason: "dependency".to_string(),
            dependencies: manifest
                .dependencies
                .iter()
                .map(|(name, req)| format!("{name}@{req}"))
                .collect(),
        })
        .collect();

    InstallPlan {
        operation,
        target,
        packages,
        removals: Vec::new(),
        replacements: Vec::new(),
        transitions: Vec::new(),
        provider_substitutions: Vec::new(),
        conflicts: Vec::new(),
        risk_flags: Vec::new(),
    }
}

pub fn plan_from_resolved_graph_with_installed(
    operation: PlanOperation,
    target: Option<String>,
    graph: &crate::types::ResolvedGraph,
    installed: &[InstalledPackageSummary],
    root_names: &[String],
) -> InstallPlan {
    let mut packages = Vec::new();
    let mut removals = BTreeMap::<String, PlannedRemoval>::new();
    let mut replacements = BTreeMap::<String, PlannedReplacement>::new();
    let mut transitions = Vec::new();

    for name in &graph.install_order {
        let Some(manifest) = graph.manifests.get(name) else {
            continue;
        };
        let installed_summary = installed
            .iter()
            .find(|summary| summary.name == manifest.name);
        let new_version = manifest.version.to_string();
        if installed_summary.is_none() {
            packages.push(PlannedPackage {
                name: manifest.name.clone(),
                version: new_version.clone(),
                target: target
                    .clone()
                    .or_else(|| {
                        manifest
                            .artifacts
                            .first()
                            .map(|artifact| artifact.target.clone())
                    })
                    .unwrap_or_default(),
                install_reason: planned_install_reason(manifest, installed, root_names),
                dependencies: manifest
                    .dependencies
                    .iter()
                    .map(|(name, req)| format!("{name}@{req}"))
                    .collect(),
            });
        }

        if let Some(summary) = installed_summary {
            if summary.version != new_version {
                transitions.push(PlannedTransition {
                    name: manifest.name.clone(),
                    from_version: summary.version.clone(),
                    to_version: new_version.clone(),
                });
            }
        }

        for replacement in replacement_matches(manifest, installed) {
            removals.insert(
                replacement.name.clone(),
                PlannedRemoval {
                    name: replacement.name.clone(),
                    version: replacement.version.clone(),
                    reason: "replacement".to_string(),
                },
            );
            if let Some(requirement) = manifest.replaces.get(&replacement.name) {
                replacements.insert(
                    replacement.name.clone(),
                    PlannedReplacement {
                        removed_name: replacement.name,
                        removed_version: replacement.version,
                        replacement_name: manifest.name.clone(),
                        replacement_version: new_version.clone(),
                        requirement: requirement.to_string(),
                    },
                );
            }
        }
    }

    packages.sort_by(|left, right| left.name.cmp(&right.name));
    transitions.sort_by(|left, right| left.name.cmp(&right.name));
    let removals = removals.into_values().collect::<Vec<_>>();
    let replacements = replacements.into_values().collect::<Vec<_>>();
    let provider_substitutions = provider_substitutions(graph, root_names);
    let conflicts = conflict_constraints(graph);
    let risk_flags = plan_risk_flags(&packages, &removals, &replacements, &transitions);

    InstallPlan {
        operation,
        target,
        packages,
        removals,
        replacements,
        transitions,
        provider_substitutions,
        conflicts,
        risk_flags,
    }
}

fn planned_install_reason(
    manifest: &crosspack_core::PackageManifest,
    installed: &[InstalledPackageSummary],
    root_names: &[String],
) -> String {
    if root_names.iter().any(|root| root == &manifest.name) {
        return "root".to_string();
    }

    if installed.iter().any(|summary| {
        summary.install_reason == "root" && manifest.replaces.contains_key(&summary.name)
    }) {
        return "root".to_string();
    }

    installed
        .iter()
        .find(|summary| summary.name == manifest.name)
        .map(|summary| summary.install_reason.clone())
        .unwrap_or_else(|| "dependency".to_string())
}

fn replacement_matches(
    manifest: &crosspack_core::PackageManifest,
    installed: &[InstalledPackageSummary],
) -> Vec<InstalledPackageSummary> {
    let mut matches = installed
        .iter()
        .filter_map(|summary| {
            let requirement = manifest.replaces.get(&summary.name)?;
            let version = Version::parse(&summary.version).ok()?;
            requirement.matches(&version).then_some(summary.clone())
        })
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| left.name.cmp(&right.name));
    matches
}

fn provider_substitutions(
    graph: &crate::types::ResolvedGraph,
    root_names: &[String],
) -> Vec<ProviderSubstitution> {
    let mut requested_tokens = root_names.iter().cloned().collect::<BTreeSet<_>>();
    for manifest in graph.manifests.values() {
        requested_tokens.extend(manifest.dependencies.keys().cloned());
    }

    let mut substitutions = Vec::new();
    for capability in requested_tokens {
        let mut candidates = graph
            .manifests
            .values()
            .filter(|manifest| {
                manifest.name == capability
                    || manifest
                        .provides
                        .iter()
                        .any(|provided| provided == &capability)
            })
            .collect::<Vec<_>>();

        if candidates
            .iter()
            .any(|candidate| candidate.name == capability)
        {
            continue;
        }

        candidates.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| right.version.cmp(&left.version))
        });
        substitutions.extend(candidates.into_iter().map(|manifest| ProviderSubstitution {
            capability: capability.clone(),
            provider: manifest.name.clone(),
            provider_version: manifest.version.to_string(),
        }));
    }

    substitutions
}

fn conflict_constraints(graph: &crate::types::ResolvedGraph) -> Vec<ConflictConstraint> {
    let mut constraints = graph
        .manifests
        .values()
        .flat_map(|manifest| {
            manifest
                .conflicts
                .iter()
                .map(|(name, requirement)| ConflictConstraint {
                    selected: manifest.name.clone(),
                    selected_version: manifest.version.to_string(),
                    conflicts_with: name.clone(),
                    requirement: requirement.to_string(),
                })
        })
        .collect::<Vec<_>>();
    constraints.sort_by(|left, right| {
        left.selected
            .cmp(&right.selected)
            .then_with(|| left.conflicts_with.cmp(&right.conflicts_with))
    });
    constraints
}

fn plan_risk_flags(
    packages: &[PlannedPackage],
    removals: &[PlannedRemoval],
    replacements: &[PlannedReplacement],
    transitions: &[PlannedTransition],
) -> Vec<String> {
    let mut risk_flags = BTreeSet::new();
    if !packages.is_empty() {
        risk_flags.insert("adds".to_string());
    }
    if !removals.is_empty() {
        risk_flags.insert("removals".to_string());
    }
    if !replacements.is_empty() {
        risk_flags.insert("replacements".to_string());
    }
    if !transitions.is_empty() {
        risk_flags.insert("version-transitions".to_string());
    }
    let mut mutating_packages = packages
        .iter()
        .map(|package| package.name.clone())
        .chain(transitions.iter().map(|transition| transition.name.clone()))
        .collect::<BTreeSet<_>>();
    mutating_packages.extend(
        replacements
            .iter()
            .map(|replacement| replacement.replacement_name.clone()),
    );
    if mutating_packages.len() > 1 {
        risk_flags.insert("multi-package-transaction".to_string());
    }
    if risk_flags.is_empty() {
        risk_flags.insert("none".to_string());
    }
    risk_flags.into_iter().collect()
}
