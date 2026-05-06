use anyhow::{Context, Result};
use crosspack_core::{ArchiveType, ArtifactCompletionShell};
use std::fs;
use std::path::{Path, PathBuf};

use crate::InstalledPackageIdentity;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefixLayout {
    prefix: PathBuf,
}
impl PrefixLayout {
    pub fn new(prefix: impl Into<PathBuf>) -> Self {
        Self {
            prefix: prefix.into(),
        }
    }

    pub fn prefix(&self) -> &Path {
        &self.prefix
    }

    pub fn pkgs_dir(&self) -> PathBuf {
        self.prefix.join("pkgs")
    }

    pub fn bin_dir(&self) -> PathBuf {
        self.prefix.join("bin")
    }

    pub fn state_dir(&self) -> PathBuf {
        self.prefix.join("state")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.prefix.join("cache")
    }

    pub fn share_dir(&self) -> PathBuf {
        self.prefix.join("share")
    }

    pub fn completions_dir(&self) -> PathBuf {
        self.share_dir().join("completions")
    }

    pub fn package_completions_dir(&self) -> PathBuf {
        self.completions_dir().join("packages")
    }

    pub fn package_completions_shell_dir(&self, shell: ArtifactCompletionShell) -> PathBuf {
        self.package_completions_dir().join(shell.as_str())
    }

    pub fn shell_init_dir(&self) -> PathBuf {
        self.share_dir().join("shell").join("init")
    }

    pub fn shell_init_shell_dir(&self, shell: ArtifactCompletionShell) -> PathBuf {
        self.shell_init_dir().join(shell.as_str())
    }

    pub fn man_dir(&self) -> PathBuf {
        self.share_dir().join("man")
    }

    pub fn gui_dir(&self) -> PathBuf {
        self.share_dir().join("gui")
    }

    pub fn gui_launchers_dir(&self) -> PathBuf {
        self.gui_dir().join("launchers")
    }

    pub fn gui_handlers_dir(&self) -> PathBuf {
        self.gui_dir().join("handlers")
    }

    pub fn integrations_dir(&self) -> PathBuf {
        self.share_dir().join("integrations")
    }

    pub fn artifacts_cache_dir(&self) -> PathBuf {
        self.cache_dir().join("artifacts")
    }

    pub fn tmp_state_dir(&self) -> PathBuf {
        self.state_dir().join("tmp")
    }

    pub fn installed_state_dir(&self) -> PathBuf {
        self.state_dir().join("installed")
    }

    pub fn pins_dir(&self) -> PathBuf {
        self.state_dir().join("pins")
    }

    pub fn pin_path(&self, name: &str) -> PathBuf {
        self.pins_dir().join(format!("{name}.pin"))
    }

    pub fn identity_pin_path(&self, identity: &InstalledPackageIdentity) -> PathBuf {
        self.pins_dir()
            .join(format!("{}.pin", identity.state_key()))
    }

    pub fn package_dir(&self, name: &str, version: &str) -> PathBuf {
        self.pkgs_dir().join(name).join(version)
    }

    pub fn identity_pkgs_dir(&self) -> PathBuf {
        self.pkgs_dir().join("identities").join("v1")
    }

    pub fn identity_package_dir(
        &self,
        identity: &InstalledPackageIdentity,
        version: &str,
    ) -> PathBuf {
        self.identity_pkgs_dir()
            .join(&identity.profile)
            .join(identity.target_label())
            .join(identity.source_namespace_label())
            .join(&identity.package)
            .join(version)
    }

    pub fn receipt_path(&self, name: &str) -> PathBuf {
        self.installed_state_dir().join(format!("{name}.receipt"))
    }

    pub fn identity_receipt_path(&self, identity: &InstalledPackageIdentity) -> PathBuf {
        self.installed_state_dir()
            .join(format!("{}.receipt", identity.state_key()))
    }

    pub fn installed_state_document_path(&self, package_name: &str) -> PathBuf {
        self.installed_state_dir()
            .join(format!("{package_name}.state.json"))
    }

    pub fn installed_identity_state_document_path(
        &self,
        identity: &InstalledPackageIdentity,
    ) -> PathBuf {
        self.installed_state_dir()
            .join(format!("{}.state.json", identity.state_key()))
    }

    pub fn installed_legacy_identity_state_document_path(
        &self,
        identity: &InstalledPackageIdentity,
    ) -> PathBuf {
        self.installed_state_dir()
            .join(format!("{}.state.json", identity.legacy_state_key()))
    }

    pub fn gui_state_path(&self, name: &str) -> PathBuf {
        self.installed_state_dir().join(format!("{name}.gui"))
    }

    pub fn identity_gui_state_path(&self, identity: &InstalledPackageIdentity) -> PathBuf {
        self.installed_state_dir()
            .join(format!("{}.gui", identity.state_key()))
    }

    pub fn gui_native_state_path(&self, name: &str) -> PathBuf {
        self.installed_state_dir()
            .join(format!("{name}.gui-native"))
    }

    pub fn identity_gui_native_state_path(&self, identity: &InstalledPackageIdentity) -> PathBuf {
        self.installed_state_dir()
            .join(format!("{}.gui-native", identity.state_key()))
    }

    pub fn declared_services_state_path(&self, name: &str) -> PathBuf {
        self.installed_state_dir().join(format!("{name}.services"))
    }

    pub fn identity_declared_services_state_path(
        &self,
        identity: &InstalledPackageIdentity,
    ) -> PathBuf {
        self.installed_state_dir()
            .join(format!("{}.services", identity.state_key()))
    }

    pub fn integration_state_path(&self, name: &str) -> PathBuf {
        self.installed_state_dir()
            .join(format!("{name}.integrations"))
    }

    pub fn identity_integration_state_path(&self, identity: &InstalledPackageIdentity) -> PathBuf {
        self.installed_state_dir()
            .join(format!("{}.integrations", identity.state_key()))
    }

    pub fn shell_init_state_path(&self, name: &str) -> PathBuf {
        self.installed_state_dir()
            .join(format!("{name}.shell-init"))
    }

    pub fn identity_shell_init_state_path(&self, identity: &InstalledPackageIdentity) -> PathBuf {
        self.installed_state_dir()
            .join(format!("{}.shell-init", identity.state_key()))
    }

    pub fn integration_activation_state_path(&self) -> PathBuf {
        self.installed_state_dir().join("integrations.activation")
    }

    pub fn transactions_dir(&self) -> PathBuf {
        self.state_dir().join("transactions")
    }

    pub fn transactions_staging_dir(&self) -> PathBuf {
        self.transactions_dir().join("staging")
    }

    pub fn transaction_active_path(&self) -> PathBuf {
        self.transactions_dir().join("active")
    }

    pub fn transaction_metadata_path(&self, txid: &str) -> PathBuf {
        self.transactions_dir().join(format!("{txid}.json"))
    }

    pub fn transaction_journal_path(&self, txid: &str) -> PathBuf {
        self.transactions_dir().join(format!("{txid}.journal"))
    }

    pub fn transaction_staging_path(&self, txid: &str) -> PathBuf {
        self.transactions_staging_dir().join(txid)
    }

    pub fn artifact_cache_path(
        &self,
        name: &str,
        version: &str,
        target: &str,
        archive_type: ArchiveType,
    ) -> PathBuf {
        self.artifacts_cache_dir()
            .join(name)
            .join(version)
            .join(target)
            .join(format!("artifact.{}", archive_type.cache_extension()))
    }

    pub fn ensure_base_dirs(&self) -> Result<()> {
        for dir in [
            self.pkgs_dir(),
            self.identity_pkgs_dir(),
            self.bin_dir(),
            self.state_dir(),
            self.cache_dir(),
            self.share_dir(),
            self.completions_dir(),
            self.package_completions_dir(),
            self.shell_init_dir(),
            self.man_dir(),
            self.gui_dir(),
            self.gui_launchers_dir(),
            self.gui_handlers_dir(),
            self.integrations_dir(),
            self.artifacts_cache_dir(),
            self.tmp_state_dir(),
            self.installed_state_dir(),
            self.pins_dir(),
            self.transactions_dir(),
            self.transactions_staging_dir(),
        ] {
            fs::create_dir_all(&dir)
                .with_context(|| format!("failed to create {}", dir.display()))?;
        }
        Ok(())
    }
}

pub fn default_user_prefix() -> Result<PathBuf> {
    if cfg!(windows) {
        let app_data = std::env::var("LOCALAPPDATA")
            .context("LOCALAPPDATA is not set; cannot resolve Windows user prefix")?;
        return Ok(PathBuf::from(app_data).join("Crosspack"));
    }

    let home = std::env::var("HOME").context("HOME is not set; cannot resolve user prefix")?;
    Ok(PathBuf::from(home).join(".crosspack"))
}
