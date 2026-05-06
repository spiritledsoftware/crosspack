mod archive;
mod artifact;
mod gui;
mod manifest;

pub use archive::ArchiveType;
pub use artifact::{Artifact, ArtifactBinary, ArtifactCompletion, ArtifactCompletionShell};
pub use gui::{ArtifactGuiApp, ArtifactGuiFileAssociation, ArtifactGuiProtocol};
pub use manifest::{
    PackageIntegration, PackageManifest, PackageShellInit, ServiceDeclaration, ShellInitStrategy,
    SourceBuildMetadata,
};

#[cfg(test)]
mod tests;
