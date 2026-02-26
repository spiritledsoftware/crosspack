use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::{
    parse_source_state_file, read_snapshot_state, select_update_sources, sort_sources,
    update_source, validate_source_fingerprint, validate_source_name, RegistrySourceRecord,
    RegistrySourceStateFile, RegistrySourceWithSnapshotState, SourceUpdateResult,
    SourceUpdateStatus,
};

#[derive(Debug, Clone)]
pub struct RegistrySourceStore {
    pub(crate) state_root: PathBuf,
}

impl RegistrySourceStore {
    pub fn new(state_root: impl Into<PathBuf>) -> Self {
        Self {
            state_root: state_root.into(),
        }
    }

    pub fn add_source(&self, source: RegistrySourceRecord) -> Result<()> {
        validate_source_name(&source.name)?;
        validate_source_fingerprint(&source.fingerprint_sha256)?;

        let mut state = self.load_state()?;
        if state
            .sources
            .iter()
            .any(|existing| existing.name == source.name)
        {
            anyhow::bail!("source '{}' already exists", source.name);
        }

        state.sources.push(source);
        sort_sources(&mut state.sources);
        self.save_state(&state)
    }

    pub fn list_sources(&self) -> Result<Vec<RegistrySourceRecord>> {
        let mut state = self.load_state()?;
        sort_sources(&mut state.sources);
        Ok(state.sources)
    }

    pub fn list_sources_with_snapshot_state(&self) -> Result<Vec<RegistrySourceWithSnapshotState>> {
        let mut state = self.load_state()?;
        sort_sources(&mut state.sources);

        let mut listed = Vec::with_capacity(state.sources.len());
        for source in state.sources {
            let cache_root = self.state_root.join("cache").join(&source.name);
            let snapshot = read_snapshot_state(&cache_root);
            listed.push(RegistrySourceWithSnapshotState { source, snapshot });
        }
        Ok(listed)
    }

    pub fn remove_source(&self, name: &str) -> Result<()> {
        let mut state = self.load_state()?;
        let before = state.sources.len();
        state.sources.retain(|source| source.name != name);
        if state.sources.len() == before {
            anyhow::bail!("source '{}' not found", name);
        }

        sort_sources(&mut state.sources);
        self.save_state(&state)
    }

    pub fn remove_source_with_cache_purge(&self, name: &str, purge_cache: bool) -> Result<()> {
        self.remove_source(name)?;
        if purge_cache {
            let cache_path = self.state_root.join("cache").join(name);
            if cache_path.exists() {
                fs::remove_dir_all(&cache_path).with_context(|| {
                    format!("failed purging source cache: {}", cache_path.display())
                })?;
            }
        }
        Ok(())
    }

    pub fn update_sources(&self, target_names: &[String]) -> Result<Vec<SourceUpdateResult>> {
        let state = self.load_state()?;
        let selected = select_update_sources(&state.sources, target_names)?;

        let mut results = Vec::with_capacity(selected.len());
        for source in selected {
            match update_source(self, &source) {
                Ok((status, snapshot_id)) => results.push(SourceUpdateResult {
                    name: source.name,
                    status,
                    snapshot_id,
                    error: None,
                }),
                Err(err) => results.push(SourceUpdateResult {
                    name: source.name,
                    status: SourceUpdateStatus::Failed,
                    snapshot_id: String::new(),
                    error: Some(format!("{err:#}")),
                }),
            }
        }

        Ok(results)
    }

    fn sources_file_path(&self) -> PathBuf {
        self.state_root.join("sources.toml")
    }

    fn load_state(&self) -> Result<RegistrySourceStateFile> {
        let path = self.sources_file_path();
        if !path.exists() {
            return Ok(RegistrySourceStateFile::default());
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed reading source state: {}", path.display()))?;
        let mut state = parse_source_state_file(&content)
            .with_context(|| format!("failed parsing source state: {}", path.display()))?;
        sort_sources(&mut state.sources);
        Ok(state)
    }

    fn save_state(&self, state: &RegistrySourceStateFile) -> Result<()> {
        fs::create_dir_all(&self.state_root).with_context(|| {
            format!(
                "failed creating source state root: {}",
                self.state_root.display()
            )
        })?;

        let path = self.sources_file_path();
        let mut state = state.clone();
        sort_sources(&mut state.sources);
        let content = toml::to_string(&state)
            .with_context(|| format!("failed serializing source state: {}", path.display()))?;
        fs::write(&path, content)
            .with_context(|| format!("failed writing source state: {}", path.display()))
    }
}
