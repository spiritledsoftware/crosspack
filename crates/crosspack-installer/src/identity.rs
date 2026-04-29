#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct InstalledPackageIdentity {
    pub profile: String,
    pub target: Option<String>,
    pub source_namespace: String,
    pub source_provenance: Option<String>,
    pub package: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPackageSelector {
    pub package: String,
    pub target: Option<String>,
    pub profile: Option<String>,
    pub source_namespace: Option<String>,
}

impl InstalledPackageIdentity {
    pub fn from_legacy_receipt(receipt: &crate::InstallReceipt) -> Self {
        Self {
            profile: "default".to_string(),
            target: receipt.target.clone(),
            source_namespace: "default".to_string(),
            source_provenance: Some("unknown".to_string()),
            package: receipt.name.clone(),
        }
    }

    pub fn target_label(&self) -> &str {
        self.target.as_deref().unwrap_or("host")
    }

    pub fn source_namespace_label(&self) -> &str {
        &self.source_namespace
    }

    pub fn source_provenance_label(&self) -> &str {
        self.source_provenance.as_deref().unwrap_or("unknown")
    }

    pub fn state_key(&self) -> String {
        format!(
            "{}--{}--{}--{}",
            self.profile,
            self.target_label(),
            self.source_namespace_label(),
            self.package
        )
    }

    pub fn legacy_state_key(&self) -> String {
        format!(
            "{}--{}--{}",
            self.profile,
            self.target_label(),
            self.package
        )
    }

    pub fn selector_display(&self) -> String {
        format!(
            "{} --target {} --profile {} --source {}",
            self.package,
            self.target_label(),
            self.profile,
            self.source_namespace_label()
        )
    }
}

impl InstalledPackageSelector {
    pub fn matches(&self, identity: &InstalledPackageIdentity) -> bool {
        identity.package == self.package
            && self
                .target
                .as_deref()
                .is_none_or(|target| identity.target_label() == target)
            && self
                .profile
                .as_deref()
                .is_none_or(|profile| identity.profile == profile)
            && self
                .source_namespace
                .as_deref()
                .is_none_or(|source| identity.source_namespace_label() == source)
    }
}
