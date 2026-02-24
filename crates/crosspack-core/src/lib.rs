use std::collections::BTreeMap;

use anyhow::{anyhow, Context};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveType {
    Zip,
    TarGz,
    TarZst,
}

impl ArchiveType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::TarGz => "tar.gz",
            Self::TarZst => "tar.zst",
        }
    }

    pub fn cache_extension(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::TarGz => "tar.gz",
            Self::TarZst => "tar.zst",
        }
    }

    pub fn parse(input: &str) -> Option<Self> {
        match input.trim().to_ascii_lowercase().as_str() {
            "zip" => Some(Self::Zip),
            "tar.gz" | "tgz" => Some(Self::TarGz),
            "tar.zst" | "tzst" => Some(Self::TarZst),
            _ => None,
        }
    }

    pub fn infer_from_url(url: &str) -> Option<Self> {
        let lower = url.to_ascii_lowercase();
        if lower.ends_with(".zip") {
            return Some(Self::Zip);
        }
        if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
            return Some(Self::TarGz);
        }
        if lower.ends_with(".tar.zst") || lower.ends_with(".tzst") {
            return Some(Self::TarZst);
        }
        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactBinary {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactCompletionShell {
    Bash,
    Zsh,
    Fish,
    Powershell,
}

impl ArtifactCompletionShell {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bash => "bash",
            Self::Zsh => "zsh",
            Self::Fish => "fish",
            Self::Powershell => "powershell",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactCompletion {
    pub shell: ArtifactCompletionShell,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Artifact {
    pub target: String,
    pub url: String,
    pub sha256: String,
    pub size: Option<u64>,
    pub signature: Option<String>,
    pub archive: Option<String>,
    pub strip_components: Option<u32>,
    pub artifact_root: Option<String>,
    #[serde(default)]
    pub binaries: Vec<ArtifactBinary>,
    #[serde(default)]
    pub completions: Vec<ArtifactCompletion>,
}

impl Artifact {
    pub fn archive_type(&self) -> anyhow::Result<ArchiveType> {
        if let Some(archive) = &self.archive {
            return ArchiveType::parse(archive).ok_or_else(|| {
                anyhow!(
                    "unsupported archive type '{archive}' for target '{}'; supported: zip, tar.gz, tar.zst",
                    self.target
                )
            });
        }

        ArchiveType::infer_from_url(&self.url).ok_or_else(|| {
            anyhow!(
                "could not infer archive type from URL '{}' for target '{}'; set artifact.archive explicitly",
                self.url,
                self.target
            )
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageManifest {
    pub name: String,
    pub version: Version,
    pub license: Option<String>,
    pub homepage: Option<String>,
    #[serde(default)]
    pub provides: Vec<String>,
    #[serde(default)]
    pub conflicts: BTreeMap<String, VersionReq>,
    #[serde(default)]
    pub replaces: BTreeMap<String, VersionReq>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, VersionReq>,
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
}

impl PackageManifest {
    pub fn from_toml_str(input: &str) -> anyhow::Result<Self> {
        let manifest: Self = toml::from_str(input).context("failed to parse crosspack manifest")?;
        if manifest.conflicts.contains_key(&manifest.name) {
            return Err(anyhow!(
                "manifest '{}' conflicts with itself",
                manifest.name
            ));
        }
        if manifest.replaces.contains_key(&manifest.name) {
            return Err(anyhow!("manifest '{}' replaces itself", manifest.name));
        }
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::{ArchiveType, ArtifactCompletionShell, PackageManifest};
    use semver::VersionReq;

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

        let err =
            PackageManifest::from_toml_str(content).expect_err("invalid shell token must fail");
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
        assert_eq!(ArchiveType::parse("rar"), None);
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
            ArchiveType::infer_from_url("https://example.test/pkg.zip"),
            Some(ArchiveType::Zip)
        );
        assert_eq!(
            ArchiveType::infer_from_url("https://example.test/pkg"),
            None
        );
    }
}
