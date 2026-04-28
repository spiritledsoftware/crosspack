use std::collections::BTreeMap;

use crosspack_core::PackageManifest;
use semver::VersionReq;

use super::*;

#[test]
fn install_plan_constructor_preserves_package_details() {
    let plan = InstallPlan {
        operation: PlanOperation::Install,
        target: Some("x86_64-unknown-linux-gnu".to_string()),
        packages: vec![PlannedPackage {
            name: "ripgrep".to_string(),
            version: "14.1.1".to_string(),
            target: "x86_64-unknown-linux-gnu".to_string(),
            install_reason: "requested".to_string(),
            dependencies: vec!["pcre2".to_string()],
        }],
        removals: Vec::new(),
        replacements: Vec::new(),
        transitions: Vec::new(),
        provider_substitutions: Vec::new(),
        conflicts: Vec::new(),
        risk_flags: vec!["downloads_executable".to_string()],
    };

    assert_eq!(
        plan,
        InstallPlan {
            operation: PlanOperation::Install,
            target: Some("x86_64-unknown-linux-gnu".to_string()),
            packages: vec![PlannedPackage {
                name: "ripgrep".to_string(),
                version: "14.1.1".to_string(),
                target: "x86_64-unknown-linux-gnu".to_string(),
                install_reason: "requested".to_string(),
                dependencies: vec!["pcre2".to_string()],
            }],
            removals: Vec::new(),
            replacements: Vec::new(),
            transitions: Vec::new(),
            provider_substitutions: Vec::new(),
            conflicts: Vec::new(),
            risk_flags: vec!["downloads_executable".to_string()],
        }
    );
}

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

    let plan = plan_from_resolved_graph(PlanOperation::Install, None, &graph);
    assert!(
        plan.packages
            .iter()
            .any(|package| package.name == "compiler"),
        "direct package should be visible in install plan: {plan:?}"
    );
}

#[test]
fn selects_lexicographically_smallest_provider_on_version_tie() {
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
name = "llvm"
version = "2.0.0"
provides = ["compiler"]
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/llvm-2.0.0.tar.zst"
sha256 = "llvm"
"#,
            ),
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
            .expect("provider for compiler must be selected")
            .name,
        "gcc"
    );

    let plan = plan_from_resolved_graph(PlanOperation::Install, None, &graph);
    assert!(
        plan.packages.iter().any(|package| package.name == "gcc"),
        "selected provider should be visible in install plan: {plan:?}"
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

#[test]
fn install_plan_represents_selected_graph_conflict_evidence() {
    let foo = manifest(
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
    );
    let bar = manifest(
        r#"
name = "bar"
version = "1.0.0"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/bar-1.0.0.tar.zst"
sha256 = "bar"
"#,
    );
    let graph = ResolvedGraph {
        manifests: BTreeMap::from([("foo".to_string(), foo), ("bar".to_string(), bar)]),
        install_order: vec!["bar".to_string(), "foo".to_string()],
    };

    let plan = plan_from_resolved_graph_with_installed(
        PlanOperation::Install,
        Some("x86_64-unknown-linux-gnu".to_string()),
        &graph,
        &[],
        &["foo".to_string()],
    );

    assert_eq!(
        plan.conflicts,
        vec![ConflictConstraint {
            selected: "foo".to_string(),
            selected_version: "1.0.0".to_string(),
            conflicts_with: "bar".to_string(),
            requirement: "*".to_string(),
        }]
    );
}

#[test]
fn install_plan_represents_installed_conflict_evidence() {
    let foo = manifest(
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
    );
    let graph = ResolvedGraph {
        manifests: BTreeMap::from([("foo".to_string(), foo)]),
        install_order: vec!["foo".to_string()],
    };
    let installed = vec![InstalledPackageSummary {
        name: "bar".to_string(),
        version: "1.0.0".to_string(),
        dependencies: Vec::new(),
        install_reason: "root".to_string(),
    }];

    let plan = plan_from_resolved_graph_with_installed(
        PlanOperation::Install,
        Some("x86_64-unknown-linux-gnu".to_string()),
        &graph,
        &installed,
        &["foo".to_string()],
    );

    assert!(
        plan.conflicts
            .iter()
            .any(|conflict| conflict.conflicts_with == "bar" && conflict.requirement == "*"),
        "installed conflict should be visible as plan evidence: {plan:?}"
    );
}

#[test]
fn install_plan_represents_replacement_removal_and_root_preservation() {
    let clang = manifest(
        r#"
name = "clang"
version = "18.0.0"
[replaces]
old-cc = "<2.0.0"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/clang-18.0.0.tar.zst"
sha256 = "clang"
"#,
    );
    let graph = ResolvedGraph {
        manifests: BTreeMap::from([("clang".to_string(), clang)]),
        install_order: vec!["clang".to_string()],
    };
    let installed = vec![InstalledPackageSummary {
        name: "old-cc".to_string(),
        version: "1.5.0".to_string(),
        dependencies: Vec::new(),
        install_reason: "root".to_string(),
    }];

    let plan = plan_from_resolved_graph_with_installed(
        PlanOperation::Install,
        Some("x86_64-unknown-linux-gnu".to_string()),
        &graph,
        &installed,
        &[],
    );

    assert_eq!(
        plan.removals,
        vec![PlannedRemoval {
            name: "old-cc".to_string(),
            version: "1.5.0".to_string(),
            reason: "replacement".to_string(),
        }]
    );
    assert_eq!(
        plan.replacements,
        vec![PlannedReplacement {
            removed_name: "old-cc".to_string(),
            removed_version: "1.5.0".to_string(),
            replacement_name: "clang".to_string(),
            replacement_version: "18.0.0".to_string(),
            requirement: "<2.0.0".to_string(),
        }]
    );
    assert_eq!(plan.packages[0].install_reason, "root");
}

fn manifest(raw: &str) -> PackageManifest {
    PackageManifest::from_toml_str(raw).expect("manifest must parse")
}
