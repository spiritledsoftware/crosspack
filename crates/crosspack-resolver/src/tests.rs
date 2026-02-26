use std::collections::BTreeMap;

use crosspack_core::PackageManifest;
use semver::VersionReq;

use super::*;

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
