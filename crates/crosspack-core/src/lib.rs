use std::collections::BTreeMap;

use anyhow::Context;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Artifact {
    pub target: String,
    pub url: String,
    pub sha256: String,
    pub size: Option<u64>,
    pub signature: Option<String>,
    pub archive: Option<String>,
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
    use super::PackageManifest;

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
}
