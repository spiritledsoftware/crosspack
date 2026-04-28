#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct InstalledPackageIdentity {
    pub profile: String,
    pub target: Option<String>,
    pub package: String,
}

impl InstalledPackageIdentity {
    pub fn from_legacy_receipt(receipt: &crate::InstallReceipt) -> Self {
        Self {
            profile: "default".to_string(),
            target: receipt.target.clone(),
            package: receipt.name.clone(),
        }
    }

    pub fn state_key(&self) -> String {
        let target = self.target.as_deref().unwrap_or("host");
        format!("{}--{}--{}", self.profile, target, self.package)
    }
}
