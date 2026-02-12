use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

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

    pub fn package_dir(&self, name: &str, version: &str) -> PathBuf {
        self.pkgs_dir().join(name).join(version)
    }

    pub fn ensure_base_dirs(&self) -> Result<()> {
        for dir in [
            self.pkgs_dir(),
            self.bin_dir(),
            self.state_dir(),
            self.cache_dir(),
        ] {
            std::fs::create_dir_all(&dir)
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
