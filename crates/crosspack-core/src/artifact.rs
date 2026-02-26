use anyhow::anyhow;
use serde::{Deserialize, Serialize};

use crate::archive::ArchiveType;
use crate::gui::ArtifactGuiApp;

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
    #[serde(default)]
    pub gui_apps: Vec<ArtifactGuiApp>,
}

impl Artifact {
    pub fn archive_type(&self) -> anyhow::Result<ArchiveType> {
        if let Some(archive) = &self.archive {
            return ArchiveType::parse(archive).ok_or_else(|| {
                anyhow!(
                    "unsupported archive type '{archive}' for target '{}'; supported: zip, tar.gz, tar.zst, bin, msi, dmg, appimage, exe, pkg, msix, appx",
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
