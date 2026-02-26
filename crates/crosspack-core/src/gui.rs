use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactGuiFileAssociation {
    pub mime_type: String,
    #[serde(default)]
    pub extensions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactGuiProtocol {
    pub scheme: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactGuiApp {
    pub app_id: String,
    pub display_name: String,
    pub exec: String,
    pub icon: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub file_associations: Vec<ArtifactGuiFileAssociation>,
    #[serde(default)]
    pub protocols: Vec<ArtifactGuiProtocol>,
}
