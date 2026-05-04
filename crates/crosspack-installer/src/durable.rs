use anyhow::{Context, Result};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) fn write_file_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;

    let tmp = temporary_sibling_path(path);
    let write_result = (|| -> Result<()> {
        {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)
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
        sync_directory(parent)?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&tmp);
    }

    write_result
}

pub(crate) fn append_line(path: &Path, line: &str) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open append file: {}", path.display()))?;
    file.write_all(line.as_bytes())
        .with_context(|| format!("failed to append file: {}", path.display()))?;
    file.write_all(b"\n")
        .with_context(|| format!("failed to append file newline: {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to sync append file: {}", path.display()))?;
    sync_directory(parent)?;
    Ok(())
}

pub(crate) fn remove_file_if_exists_durable(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return Err(err).with_context(|| format!("failed to remove file: {}", path.display()));
        }
    }

    if let Some(parent) = path.parent() {
        sync_directory(parent)?;
    }
    Ok(())
}

pub(crate) fn sync_directory(path: &Path) -> Result<()> {
    let dir = match fs::File::open(path) {
        Ok(dir) => dir,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Ok(()),
    };

    let _ = dir.sync_all();
    Ok(())
}

fn temporary_sibling_path(path: &Path) -> PathBuf {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("durable");
    path.with_file_name(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        counter
    ))
}
