use std::collections::BTreeMap;

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
