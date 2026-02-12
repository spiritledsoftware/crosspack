use crosspack_core::PackageManifest;
use semver::VersionReq;

pub fn select_highest_compatible<'a>(
    candidates: &'a [PackageManifest],
    requirement: &VersionReq,
) -> Option<&'a PackageManifest> {
    candidates
        .iter()
        .filter(|m| requirement.matches(&m.version))
        .max_by(|a, b| a.version.cmp(&b.version))
}

#[cfg(test)]
mod tests {
    use crosspack_core::PackageManifest;
    use semver::VersionReq;

    use crate::select_highest_compatible;

    #[test]
    fn selects_latest_matching_version() {
        let one = PackageManifest::from_toml_str(
            r#"
name = "tool"
version = "1.2.0"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/tool-1.2.0.tar.zst"
sha256 = "abc"
"#,
        )
        .expect("manifest must parse");

        let two = PackageManifest::from_toml_str(
            r#"
name = "tool"
version = "1.3.0"
[[artifacts]]
target = "x86_64-unknown-linux-gnu"
url = "https://example.test/tool-1.3.0.tar.zst"
sha256 = "def"
"#,
        )
        .expect("manifest must parse");

        let req = VersionReq::parse("^1.0").expect("req should parse");
        let manifests = vec![one, two];
        let resolved = select_highest_compatible(&manifests, &req).expect("must resolve");

        assert_eq!(resolved.version.to_string(), "1.3.0");
    }
}
