use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use std::path::Path;

#[allow(dead_code)]
pub(crate) fn write_file_atomically(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;

    let tmp = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&tmp)
            .with_context(|| format!("failed to create temp file: {}", tmp.display()))?;
        file.write_all(contents)
            .with_context(|| format!("failed to write temp file: {}", tmp.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync temp file: {}", tmp.display()))?;
    }
    fs::rename(&tmp, path).with_context(|| {
        format!(
            "failed to atomically replace {} with {}",
            path.display(),
            tmp.display()
        )
    })?;
    Ok(())
}
