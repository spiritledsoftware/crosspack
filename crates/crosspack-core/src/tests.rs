use semver::VersionReq;

use super::*;

fn all_archive_types() -> [ArchiveType; 11] {
    [
        ArchiveType::Zip,
        ArchiveType::TarGz,
        ArchiveType::TarZst,
        ArchiveType::Bin,
        ArchiveType::Msi,
        ArchiveType::Dmg,
        ArchiveType::AppImage,
        ArchiveType::Exe,
        ArchiveType::Pkg,
        ArchiveType::Msix,
        ArchiveType::Appx,
    ]
}

#[test]
fn parse_manifest() {
    let content = r#"
name = "ripgrep"
version = "14.1.0"
license = "MIT"
provides = ["ripgrep", "rg"]

[conflicts]
grep = "<2.0.0"

[replaces]
ripgrep-legacy = "^1.0"

[dependencies]
zlib = ">=1.2.0, <2.0.0"

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/ripgrep-14.1.0-x86_64-unknown-linux-gnu.tar.zst"
sha256 = "abc123"

[[artifacts.binaries]]
name = "rg"
path = "ripgrep"

[[artifacts.completions]]
shell = "bash"
path = "completions/rg.bash"
"#;

    let parsed = PackageManifest::from_toml_str(content).expect("manifest should parse");
    assert_eq!(parsed.name, "ripgrep");
    assert_eq!(parsed.version.to_string(), "14.1.0");
    assert_eq!(parsed.provides, vec!["ripgrep", "rg"]);
    assert_eq!(
        parsed.conflicts.get("grep"),
        Some(&VersionReq::parse("<2.0.0").expect("valid version req"))
    );
    assert_eq!(
        parsed.replaces.get("ripgrep-legacy"),
        Some(&VersionReq::parse("^1.0").expect("valid version req"))
    );
    assert!(parsed.dependencies.contains_key("zlib"));
    assert_eq!(parsed.artifacts.len(), 1);
    assert_eq!(parsed.artifacts[0].binaries.len(), 1);
    assert_eq!(parsed.artifacts[0].binaries[0].name, "rg");
    assert_eq!(parsed.artifacts[0].binaries[0].path, "ripgrep");
    assert_eq!(parsed.artifacts[0].completions.len(), 1);
    assert_eq!(
        parsed.artifacts[0].completions[0].shell,
        ArtifactCompletionShell::Bash
    );
    assert_eq!(
        parsed.artifacts[0].completions[0].path,
        "completions/rg.bash"
    );
}

#[test]
fn parse_manifest_with_multiple_artifact_completions() {
    let content = r#"
name = "zoxide"
version = "0.9.0"

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/zoxide-0.9.0.tar.gz"
sha256 = "abc123"

[[artifacts.completions]]
shell = "bash"
path = "completions/zoxide.bash"

[[artifacts.completions]]
shell = "zsh"
path = "completions/_zoxide"

[[artifacts.completions]]
shell = "fish"
path = "completions/zoxide.fish"

[[artifacts.completions]]
shell = "powershell"
path = "completions/_zoxide.ps1"
"#;

    let parsed = PackageManifest::from_toml_str(content).expect("manifest should parse");
    let completions = &parsed.artifacts[0].completions;
    assert_eq!(completions.len(), 4);
    assert_eq!(completions[0].shell, ArtifactCompletionShell::Bash);
    assert_eq!(completions[1].shell, ArtifactCompletionShell::Zsh);
    assert_eq!(completions[2].shell, ArtifactCompletionShell::Fish);
    assert_eq!(completions[3].shell, ArtifactCompletionShell::Powershell);
}

#[test]
fn parse_manifest_with_gui_apps() {
    let content = r#"
name = "zed"
version = "0.190.5"

[[artifacts]]
target = "x86_64-apple-darwin"
url = "https://example.test/zed-macos.tar.gz"
sha256 = "abc123"

[[artifacts.gui_apps]]
app_id = "dev.zed.Zed"
display_name = "Zed"
exec = "Zed.app"
icon = "resources/zed.icns"
categories = ["Development", "IDE"]

[[artifacts.gui_apps.file_associations]]
mime_type = "text/plain"
extensions = [".txt", ".md"]

[[artifacts.gui_apps.protocols]]
scheme = "zed"
"#;

    let parsed = PackageManifest::from_toml_str(content).expect("manifest should parse");
    assert_eq!(parsed.artifacts[0].gui_apps.len(), 1);
    let gui = &parsed.artifacts[0].gui_apps[0];
    assert_eq!(gui.app_id, "dev.zed.Zed");
    assert_eq!(gui.display_name, "Zed");
    assert_eq!(gui.exec, "Zed.app");
    assert_eq!(gui.categories, vec!["Development", "IDE"]);
    assert_eq!(gui.file_associations.len(), 1);
    assert_eq!(gui.protocols.len(), 1);
}

#[test]
fn parse_manifest_rejects_duplicate_gui_app_id_per_artifact() {
    let content = r#"
name = "demo"
version = "1.0.0"

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/demo.tar.gz"
sha256 = "abc123"

[[artifacts.gui_apps]]
app_id = "demo.app"
display_name = "Demo"
exec = "demo"

[[artifacts.gui_apps]]
app_id = "demo.app"
display_name = "Demo 2"
exec = "demo2"
"#;

    let err = PackageManifest::from_toml_str(content).expect_err("duplicate gui app id must fail");
    assert!(err.to_string().contains("duplicate gui app declaration"));
}

#[test]
fn parse_manifest_rejects_invalid_gui_protocol_scheme() {
    let content = r#"
name = "demo"
version = "1.0.0"

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/demo.tar.gz"
sha256 = "abc123"

[[artifacts.gui_apps]]
app_id = "demo.app"
display_name = "Demo"
exec = "demo"

[[artifacts.gui_apps.protocols]]
scheme = "1bad"
"#;

    let err =
        PackageManifest::from_toml_str(content).expect_err("invalid protocol scheme must fail");
    assert!(
        err.to_string().contains("invalid gui protocol scheme"),
        "unexpected error: {err}"
    );
}

#[test]
fn parse_manifest_rejects_invalid_completion_shell_token() {
    let content = r#"
name = "zoxide"
version = "0.9.0"

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/zoxide-0.9.0.tar.gz"
sha256 = "abc123"

[[artifacts.completions]]
shell = "elvish"
path = "completions/zoxide.elvish"
"#;

    let err = PackageManifest::from_toml_str(content).expect_err("invalid shell token must fail");
    let chain = err
        .chain()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        chain.contains("unknown variant") && chain.contains("elvish"),
        "unexpected error chain: {chain}"
    );
}

#[test]
fn reject_self_conflict() {
    let content = r#"
name = "ripgrep"
version = "14.1.0"

[conflicts]
ripgrep = "*"
"#;

    let err = PackageManifest::from_toml_str(content).expect_err("manifest should be rejected");
    assert!(
        err.to_string().contains("conflicts with itself"),
        "unexpected error: {err}"
    );
}

#[test]
fn reject_self_replace() {
    let content = r#"
name = "ripgrep"
version = "14.1.0"

[replaces]
ripgrep = "*"
"#;

    let err = PackageManifest::from_toml_str(content).expect_err("manifest should be rejected");
    assert!(
        err.to_string().contains("replaces itself"),
        "unexpected error: {err}"
    );
}

#[test]
fn archive_type_from_manifest_value() {
    assert_eq!(ArchiveType::parse("zip"), Some(ArchiveType::Zip));
    assert_eq!(ArchiveType::parse("tgz"), Some(ArchiveType::TarGz));
    assert_eq!(ArchiveType::parse("tar.zst"), Some(ArchiveType::TarZst));
    assert_eq!(ArchiveType::parse("bin"), Some(ArchiveType::Bin));
    assert_eq!(ArchiveType::parse("msi"), Some(ArchiveType::Msi));
    assert_eq!(ArchiveType::parse("dmg"), Some(ArchiveType::Dmg));
    assert_eq!(ArchiveType::parse("appimage"), Some(ArchiveType::AppImage));
    assert_eq!(ArchiveType::parse("rar"), None);
}

#[test]
fn archive_type_parse_supports_exe_pkg_msix_appx() {
    assert_eq!(
        ArchiveType::parse("exe").map(|kind| kind.as_str()),
        Some("exe")
    );
    assert_eq!(
        ArchiveType::parse("pkg").map(|kind| kind.as_str()),
        Some("pkg")
    );
    assert_eq!(
        ArchiveType::parse("msix").map(|kind| kind.as_str()),
        Some("msix")
    );
    assert_eq!(
        ArchiveType::parse("appx").map(|kind| kind.as_str()),
        Some("appx")
    );
    assert_eq!(
        ArchiveType::parse("bin").map(|kind| kind.as_str()),
        Some("bin")
    );
}

#[test]
fn archive_type_parse_rejects_deb_rpm() {
    assert_eq!(ArchiveType::parse("deb"), None);
    assert_eq!(ArchiveType::parse("rpm"), None);
}

#[test]
fn archive_type_from_url() {
    assert_eq!(
        ArchiveType::infer_from_url("https://example.test/pkg.tar.gz"),
        Some(ArchiveType::TarGz)
    );
    assert_eq!(
        ArchiveType::infer_from_url("https://example.test/pkg.tzst"),
        Some(ArchiveType::TarZst)
    );
    assert_eq!(
        ArchiveType::infer_from_url("https://example.test/pkg.bin"),
        Some(ArchiveType::Bin)
    );
    assert_eq!(
        ArchiveType::infer_from_url("https://example.test/pkg.zip"),
        Some(ArchiveType::Zip)
    );
    assert_eq!(
        ArchiveType::infer_from_url("https://example.test/pkg.msi"),
        Some(ArchiveType::Msi)
    );
    assert_eq!(
        ArchiveType::infer_from_url("https://example.test/pkg.dmg"),
        Some(ArchiveType::Dmg)
    );
    assert_eq!(
        ArchiveType::infer_from_url("https://example.test/pkg.appimage"),
        Some(ArchiveType::AppImage)
    );
    assert_eq!(
        ArchiveType::infer_from_url("https://example.test/pkg"),
        Some(ArchiveType::Bin)
    );
    assert_eq!(
        ArchiveType::infer_from_url("https://example.test/path/"),
        None
    );
}

#[test]
fn archive_type_as_str_parse_round_trip_for_all_variants() {
    for kind in all_archive_types() {
        assert_eq!(ArchiveType::parse(kind.as_str()), Some(kind));
    }
}

#[test]
fn archive_type_cache_extension_consistent_with_as_str() {
    for kind in all_archive_types() {
        assert_eq!(kind.cache_extension(), kind.as_str());
    }
}

#[test]
fn archive_type_infer_from_url_supports_exe_pkg_msix_appx() {
    assert_eq!(
        ArchiveType::infer_from_url("https://example.test/pkg.exe").map(|kind| kind.as_str()),
        Some("exe")
    );
    assert_eq!(
        ArchiveType::infer_from_url("https://example.test/pkg.pkg").map(|kind| kind.as_str()),
        Some("pkg")
    );
    assert_eq!(
        ArchiveType::infer_from_url("https://example.test/pkg.msix").map(|kind| kind.as_str()),
        Some("msix")
    );
    assert_eq!(
        ArchiveType::infer_from_url("https://example.test/pkg.appx").map(|kind| kind.as_str()),
        Some("appx")
    );
}

#[test]
fn archive_type_infer_from_url_rejects_deb_rpm() {
    assert_eq!(
        ArchiveType::infer_from_url("https://example.test/pkg.deb"),
        None
    );
    assert_eq!(
        ArchiveType::infer_from_url("https://example.test/pkg.rpm"),
        None
    );
}

#[test]
fn archive_type_error_message_excludes_deb_rpm() {
    let artifact = Artifact {
        target: "x86_64-unknown-linux-gnu".to_string(),
        url: "https://example.test/pkg.unknown".to_string(),
        sha256: "abc123".to_string(),
        size: None,
        signature: None,
        archive: Some("unknown".to_string()),
        strip_components: None,
        artifact_root: None,
        binaries: vec![],
        completions: vec![],
        gui_apps: vec![],
    };

    let err = artifact
        .archive_type()
        .expect_err("unknown archive type should fail");
    let msg = err.to_string();
    assert!(!msg.contains("deb"), "error should not mention deb: {msg}");
    assert!(!msg.contains("rpm"), "error should not mention rpm: {msg}");
}

#[test]
fn manifest_allows_gui_package_with_exe_installer_artifact_kind() {
    let content = r#"
name = "zed"
version = "0.190.5"

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/zed-installer.exe"
archive = "exe"
sha256 = "abc123"

[[artifacts.gui_apps]]
app_id = "dev.zed.Zed"
display_name = "Zed"
exec = "zed.exe"
"#;

    let parsed = PackageManifest::from_toml_str(content).expect("manifest should parse");
    assert_eq!(
        parsed.artifacts[0]
            .archive_type()
            .expect("archive type")
            .as_str(),
        "exe"
    );
}
