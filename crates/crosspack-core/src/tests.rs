use semver::VersionReq;

use super::*;

fn all_archive_types() -> [ArchiveType; 12] {
    [
        ArchiveType::Zip,
        ArchiveType::TarGz,
        ArchiveType::TarXz,
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
description = "Fast line-oriented search tool"
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
    assert_eq!(
        parsed.description.as_deref(),
        Some("Fast line-oriented search tool")
    );
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
fn parse_manifest_without_description_defaults_to_none() {
    let content = r#"
name = "jq"
version = "1.7.1"

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/jq-1.7.1.tar.gz"
sha256 = "abc123"
"#;

    let parsed = PackageManifest::from_toml_str(content).expect("manifest should parse");
    assert_eq!(parsed.description, None);
    assert_eq!(parsed.source_build, None);
    assert!(parsed.services.is_empty());
}

#[test]
fn parse_manifest_normalizes_leading_zero_version_components() {
    let content = r#"
name = "helix"
version = "25.07.1"
"#;

    let parsed = PackageManifest::from_toml_str(content).expect("manifest should parse");
    assert_eq!(parsed.version.to_string(), "25.7.1");
}

#[test]
fn parse_manifest_with_declared_services() {
    let content = r#"
name = "demo"
version = "1.2.3"

[[services]]
name = "demo"

[[services]]
name = "demo-worker"
native_id = "demo-worker@main"
"#;

    let parsed = PackageManifest::from_toml_str(content).expect("manifest should parse");
    assert_eq!(parsed.services.len(), 2);
    assert_eq!(parsed.services[0].name, "demo");
    assert_eq!(parsed.services[0].native_id, None);
    assert_eq!(parsed.services[1].name, "demo-worker");
    assert_eq!(
        parsed.services[1].native_id.as_deref(),
        Some("demo-worker@main")
    );
}

#[test]
fn parse_manifest_with_typed_integrations() {
    let content = r#"
name = "demo"
version = "1.2.3"

[[integrations]]
kind = "docker_cli_plugin"
name = "compose"
source = "docker-compose"

[[integrations]]
kind = "path_plugin"
host = "kubectl"
name = "ctx"
source = "kubectl-ctx"

[[integrations]]
kind = "service"
name = "demo"
source = "services/demo.service"
enable = false
"#;

    let parsed = PackageManifest::from_toml_str(content).expect("manifest should parse");
    assert_eq!(parsed.integrations.len(), 3);
    assert_eq!(parsed.integrations[0].kind(), "docker_cli_plugin");
    assert_eq!(parsed.integrations[1].kind(), "path_plugin");
    assert_eq!(parsed.integrations[2].kind(), "service");
}

#[test]
fn parse_manifest_with_man_page_glob_integration() {
    let content = r#"
name = "delta"
version = "0.18.2"

[[integrations]]
kind = "man_page"
section = "1"
source = "share/man/man1/*.1"
platforms = ["linux", "macos"]

[[integrations]]
kind = "man_page"
name = "delta"
section = "5"
source = "man/delta.5.gz"
"#;

    let parsed = PackageManifest::from_toml_str(content).expect("manifest should parse");
    assert_eq!(parsed.integrations.len(), 2);
    assert_eq!(parsed.integrations[0].kind(), "man_page");
    assert_eq!(parsed.integrations[0].source(), "share/man/man1/*.1");
    assert_eq!(parsed.integrations[1].kind(), "man_page");
    assert_eq!(parsed.integrations[1].source(), "man/delta.5.gz");
}

#[test]
fn man_page_integration_rejects_invalid_sections_enable_and_mismatched_source() {
    for (name, snippet) in [
        (
            "invalid section",
            r#"
[[integrations]]
kind = "man_page"
name = "delta"
section = "10"
source = "man/delta.10"
"#,
        ),
        (
            "glob with explicit name",
            r#"
[[integrations]]
kind = "man_page"
name = "delta"
section = "1"
source = "man/*.1"
"#,
        ),
        (
            "mismatched source",
            r#"
[[integrations]]
kind = "man_page"
name = "delta"
section = "1"
source = "man/delta.5"
"#,
        ),
    ] {
        let content = format!(
            r#"
name = "delta"
version = "0.18.2"
{snippet}
"#
        );
        PackageManifest::from_toml_str(&content).expect_err(name);
    }
}

fn shell_init_manifest(package: &str, args: &str) -> String {
    format!(
        r#"
name = "{package}"
version = "1.0.0"

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/{package}.tar.gz"
sha256 = "abc123"

[[artifacts.binaries]]
name = "{package}"
path = "{package}"

[[shell_init]]
name = "{package}"
binary = "{package}"
strategy = "eval_stdout"
bash = {args}
zsh = {args}
fish = {args}
powershell = {args}
"#
    )
}

#[test]
fn shell_init_manifest_accepts_common_eval_stdout_tools() {
    for (package, args) in [
        ("starship", r#"["init", "bash"]"#),
        ("direnv", r#"["hook", "bash"]"#),
        ("mise", r#"["activate", "bash"]"#),
        ("zoxide", r#"["init", "bash"]"#),
        ("atuin", r#"["init", "bash"]"#),
    ] {
        let parsed = PackageManifest::from_toml_str(&shell_init_manifest(package, args))
            .expect("shell init manifest should parse");
        assert_eq!(parsed.shell_init.len(), 1);
        assert_eq!(parsed.shell_init[0].strategy, ShellInitStrategy::EvalStdout);
        assert_eq!(parsed.shell_init[0].binary, package);
    }
}

#[test]
fn shell_init_manifest_rejects_raw_script_like_fields() {
    let content = shell_init_manifest("starship", r#"["init", "bash"]"#).replace(
        r#"powershell = ["init", "bash"]"#,
        "script = \"eval $(starship init bash)\"",
    );
    PackageManifest::from_toml_str(&content).expect_err("unknown raw script field must fail");
}

#[test]
fn shell_init_manifest_rejects_missing_binary_unknown_strategy_and_unsafe_args() {
    let missing_binary = shell_init_manifest("starship", r#"["init", "bash"]"#)
        .replace("binary = \"starship\"", "binary = \"missing\"");
    PackageManifest::from_toml_str(&missing_binary).expect_err("undeclared binary must fail");

    let unknown_strategy = shell_init_manifest("starship", r#"["init", "bash"]"#)
        .replace("strategy = \"eval_stdout\"", "strategy = \"raw_script\"");
    PackageManifest::from_toml_str(&unknown_strategy).expect_err("unknown strategy must fail");

    let unsafe_args = shell_init_manifest("starship", r#"["init/bash"]"#);
    PackageManifest::from_toml_str(&unsafe_args).expect_err("unsafe args must fail");

    let whitespace_args = shell_init_manifest("starship", r#"["init", "ba sh"]"#);
    PackageManifest::from_toml_str(&whitespace_args).expect_err("whitespace args must fail");
}

#[test]
fn shell_init_manifest_rejects_missing_shell_fields() {
    let content = r#"
name = "starship"
version = "1.0.0"

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/starship.tar.gz"
sha256 = "abc123"

[[artifacts.binaries]]
name = "starship"
path = "starship"

[[shell_init]]
name = "starship"
binary = "starship"
strategy = "eval_stdout"
"#;
    PackageManifest::from_toml_str(content).expect_err("missing shell fields must fail");
}

#[test]
fn shell_init_manifest_rejects_binary_missing_from_any_artifact() {
    let content = r#"
name = "starship"
version = "1.0.0"

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/starship-linux.tar.gz"
sha256 = "abc123"

[[artifacts.binaries]]
name = "starship"
path = "starship"

[[artifacts]]
target = "aarch64-apple-darwin"
url = "https://example.test/starship-macos.tar.gz"
sha256 = "def456"

[[artifacts.binaries]]
name = "starship-alt"
path = "starship"

[[shell_init]]
name = "starship"
binary = "starship"
strategy = "eval_stdout"
bash = ["init", "bash"]
"#;
    PackageManifest::from_toml_str(content)
        .expect_err("shell init binary must exist in every artifact");
}

#[test]
fn integration_service_platform_sources_accepts_fields() {
    let content = r#"
name = "demo"
version = "1.2.3"

[[integrations]]
kind = "service"
name = "demo"
linux_systemd_user = "services/demo.service"
macos_launch_agent = "launchd/com.example.demo.plist"
windows_service = "windows/demo-service.xml"
enable = true
"#;

    let parsed = PackageManifest::from_toml_str(content).expect("manifest should parse");
    assert_eq!(parsed.integrations.len(), 1);
    assert_eq!(parsed.integrations[0].kind(), "service");
    match &parsed.integrations[0] {
        PackageIntegration::Service {
            linux_systemd_user,
            macos_launch_agent,
            windows_service,
            enable,
            ..
        } => {
            assert_eq!(linux_systemd_user.as_deref(), Some("services/demo.service"));
            assert_eq!(
                macos_launch_agent.as_deref(),
                Some("launchd/com.example.demo.plist")
            );
            assert_eq!(windows_service.as_deref(), Some("windows/demo-service.xml"));
            assert!(*enable);
        }
        other => panic!("expected service integration, got {other:?}"),
    }
}

#[test]
fn integration_service_platform_sources_accepts_source_alias() {
    let content = r#"
name = "demo"
version = "1.2.3"

[[integrations]]
kind = "service"
name = "demo"
source = "services/demo.service"
"#;

    let parsed = PackageManifest::from_toml_str(content).expect("manifest should parse");
    match &parsed.integrations[0] {
        PackageIntegration::Service {
            linux_systemd_user, ..
        } => assert_eq!(linux_systemd_user.as_deref(), Some("services/demo.service")),
        other => panic!("expected service integration, got {other:?}"),
    }
}

#[test]
fn integration_service_platform_sources_docker_and_path_reject_enable() {
    let docker = r#"
name = "demo"
version = "1.2.3"

[[integrations]]
kind = "docker_cli_plugin"
name = "compose"
source = "docker-compose"
enable = true
"#;
    let path = r#"
name = "demo"
version = "1.2.3"

[[integrations]]
kind = "path_plugin"
host = "kubectl"
name = "ctx"
source = "kubectl-ctx"
enable = true
"#;

    PackageManifest::from_toml_str(docker).expect_err("docker integration enable must fail");
    PackageManifest::from_toml_str(path).expect_err("path integration enable must fail");
}

#[test]
fn integration_service_platform_sources_docker_and_path_reject_unknown_fields() {
    let docker = r#"
name = "demo"
version = "1.2.3"

[[integrations]]
kind = "docker_cli_plugin"
name = "compose"
source = "docker-compose"
macos_launch_agent = "launchd/com.example.compose.plist"
"#;
    let path = r#"
name = "demo"
version = "1.2.3"

[[integrations]]
kind = "path_plugin"
host = "kubectl"
name = "ctx"
source = "kubectl-ctx"
windows_service = "windows/ctx.xml"
"#;

    PackageManifest::from_toml_str(docker).expect_err("docker unknown field must fail");
    PackageManifest::from_toml_str(path).expect_err("path unknown field must fail");
}

#[test]
fn integration_service_platform_sources_generic_source_uses_linux_only() {
    let content = r#"
name = "demo"
version = "1.2.3"

[[integrations]]
kind = "service"
name = "mac-only"
macos_launch_agent = "launchd/com.example.demo.plist"

[[integrations]]
kind = "service"
name = "win-only"
windows_service = "windows/demo-service.xml"

[[integrations]]
kind = "service"
name = "linux"
linux_systemd_user = "services/demo.service"
"#;

    let parsed = PackageManifest::from_toml_str(content).expect("manifest should parse");
    assert_eq!(parsed.integrations[0].source(), "");
    assert_eq!(parsed.integrations[1].source(), "");
    assert_eq!(parsed.integrations[2].source(), "services/demo.service");
}

#[test]
fn integration_service_platform_sources_rejects_missing_service_sources() {
    let content = r#"
name = "demo"
version = "1.2.3"

[[integrations]]
kind = "service"
name = "demo"
enable = true
"#;

    let err = PackageManifest::from_toml_str(content)
        .expect_err("service integration without any source must fail");
    assert!(
        err.to_string().contains("service integration") && err.to_string().contains("source"),
        "unexpected error: {err}"
    );
}

#[test]
fn integration_service_platform_sources_rejects_unsafe_paths() {
    for (field, value) in [
        ("linux_systemd_user", ""),
        ("linux_systemd_user", "."),
        ("linux_systemd_user", ".."),
        ("linux_systemd_user", "/tmp/demo.service"),
        ("linux_systemd_user", "../demo.service"),
        ("macos_launch_agent", r"launchd\com.example.demo.plist"),
        ("macos_launch_agent", r"C:\demo.xml"),
        ("macos_launch_agent", "C:/demo.xml"),
        ("macos_launch_agent", r"\\server\share\demo.plist"),
        ("macos_launch_agent", "."),
        ("macos_launch_agent", ""),
        ("windows_service", "services//demo.service"),
        ("windows_service", "services/./demo.service"),
        ("windows_service", r"..\demo-service.xml"),
        ("windows_service", r"windows\demo-service.xml"),
        ("windows_service", "windows/demo\tservice.xml"),
        ("windows_service", "windows/demo\nservice.xml"),
        ("windows_service", "windows/demo\u{1f}service.xml"),
    ] {
        let value = if value.contains('\u{1f}') {
            "\"windows/demo\\u001fservice.xml\"".to_string()
        } else {
            format!("{value:?}")
        };
        let content = format!(
            r#"
name = "demo"
version = "1.2.3"

[[integrations]]
kind = "service"
name = "demo"
{field} = {value}
"#
        );

        let err = PackageManifest::from_toml_str(&content)
            .expect_err("unsafe platform service source must fail");
        let chain = err
            .chain()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(
            chain.contains("integration source path") && chain.contains(field),
            "unexpected error for {field}={value}: {chain}"
        );
    }
}

#[test]
fn parse_manifest_rejects_duplicate_integration_ownership() {
    let content = r#"
name = "demo"
version = "1.0.0"

[[integrations]]
kind = "docker_cli_plugin"
name = "compose"
source = "docker-compose"

[[integrations]]
kind = "docker_cli_plugin"
name = "compose"
source = "compose-v2"
"#;

    let err = PackageManifest::from_toml_str(content)
        .expect_err("duplicate integration ownership must fail");
    assert!(err
        .to_string()
        .contains("duplicate integration declaration"));
}

#[test]
fn parse_manifest_rejects_unsafe_integration_source_path() {
    let content = r#"
name = "demo"
version = "1.0.0"

[[integrations]]
kind = "path_plugin"
host = "kubectl"
name = "ctx"
source = "../kubectl-ctx"
"#;

    let err =
        PackageManifest::from_toml_str(content).expect_err("unsafe integration source must fail");
    assert!(err.to_string().contains("integration source path"));
}

#[test]
fn parse_manifest_rejects_duplicate_declared_service_names() {
    let content = r#"
name = "demo"
version = "1.0.0"

[[services]]
name = "demo"

[[services]]
name = "demo"
"#;

    let err =
        PackageManifest::from_toml_str(content).expect_err("duplicate service name must fail");
    assert!(err.to_string().contains("duplicate service declaration"));
}

#[test]
fn parse_manifest_rejects_invalid_declared_service_name() {
    let content = r#"
name = "demo"
version = "1.0.0"

[[services]]
name = "Demo Service"
"#;

    let err =
        PackageManifest::from_toml_str(content).expect_err("invalid service declaration must fail");
    assert!(err.to_string().contains("invalid service name"));
}

#[test]
fn parse_manifest_rejects_declared_service_name_with_at_sign() {
    let content = r#"
name = "demo"
version = "1.0.0"

[[services]]
name = "demo@main"
"#;

    let err = PackageManifest::from_toml_str(content)
        .expect_err("service name with '@' must fail package-token validation");
    assert!(err.to_string().contains("invalid service name"));
}

#[test]
fn parse_manifest_rejects_invalid_declared_service_native_id() {
    let content = r#"
name = "demo"
version = "1.0.0"

[[services]]
name = "demo"
native_id = "demo service"
"#;

    let err =
        PackageManifest::from_toml_str(content).expect_err("invalid native service id must fail");
    assert!(err.to_string().contains("invalid native service id"));
}

#[test]
fn parse_manifest_with_source_build_section() {
    let content = r#"
name = "demo"
version = "1.2.3"

[source_build]
url = "https://example.test/demo-1.2.3.tar.gz"
archive_sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
build_system = "cargo"
build_commands = ["cargo", "build", "--release"]
install_commands = ["cargo", "install", "--path", "."]
"#;

    let parsed = PackageManifest::from_toml_str(content).expect("manifest should parse");
    let source_build = parsed
        .source_build
        .expect("source_build metadata should be present");
    assert_eq!(source_build.url, "https://example.test/demo-1.2.3.tar.gz");
    assert_eq!(
        source_build.archive_sha256,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(source_build.build_system, "cargo");
    assert_eq!(
        source_build.build_commands,
        vec!["cargo", "build", "--release"]
    );
    assert_eq!(
        source_build.install_commands,
        vec!["cargo", "install", "--path", "."]
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
    assert_eq!(ArchiveType::parse("txz"), Some(ArchiveType::TarXz));
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
        ArchiveType::infer_from_url("https://example.test/pkg.tar.xz"),
        Some(ArchiveType::TarXz)
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
