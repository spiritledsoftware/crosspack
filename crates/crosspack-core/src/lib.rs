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
pub struct Artifact {
    pub target: String,
    pub url: String,
    pub sha256: String,
    pub size: Option<u64>,
    pub signature: Option<String>,
    pub archive: Option<String>,
    pub strip_components: Option<u32>,
    pub artifact_root: Option<String>,
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
    pub dependencies: BTreeMap<String, VersionReq>,
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
}

impl PackageManifest {
    pub fn from_toml_str(input: &str) -> anyhow::Result<Self> {
        toml::from_str(input).context("failed to parse crosspack manifest")
    }
}

#[cfg(test)]
mod tests {
    use super::{ArchiveType, PackageManifest};

    #[test]
    fn parse_manifest() {
        let content = r#"
name = "ripgrep"
version = "14.1.0"
license = "MIT"

[dependencies]
zlib = ">=1.2.0, <2.0.0"

[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/ripgrep-14.1.0-x86_64-unknown-linux-gnu.tar.zst"
sha256 = "abc123"
"#;

        let parsed = PackageManifest::from_toml_str(content).expect("manifest should parse");
        assert_eq!(parsed.name, "ripgrep");
        assert_eq!(parsed.version.to_string(), "14.1.0");
        assert!(parsed.dependencies.contains_key("zlib"));
        assert_eq!(parsed.artifacts.len(), 1);
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
