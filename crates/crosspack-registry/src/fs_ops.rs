use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use crosspack_security::sha256_hex;

pub(crate) fn copy_source_to_temp(
    source_path: &Path,
    staged_root: &Path,
    source_name: &str,
) -> Result<()> {
    if !source_path.exists() {
        anyhow::bail!(
            "source-sync-failed: source '{}' path does not exist: {}",
            source_name,
            source_path.display()
        );
    }

    copy_dir_recursive(source_path, staged_root).with_context(|| {
        format!(
            "source-sync-failed: source '{}' failed copying from {}",
            source_name,
            source_path.display()
        )
    })
}

pub(crate) fn copy_dir_recursive(source_root: &Path, destination_root: &Path) -> Result<()> {
    if !source_root.is_dir() {
        anyhow::bail!(
            "source location is not a directory: {}",
            source_root.display()
        );
    }

    if destination_root.exists() {
        fs::remove_dir_all(destination_root).with_context(|| {
            format!(
                "failed clearing temp directory {}",
                destination_root.display()
            )
        })?;
    }
    fs::create_dir_all(destination_root).with_context(|| {
        format!(
            "failed creating temp directory {}",
            destination_root.display()
        )
    })?;

    let mut queue: VecDeque<(PathBuf, PathBuf)> = VecDeque::new();
    queue.push_back((source_root.to_path_buf(), destination_root.to_path_buf()));

    while let Some((from_dir, to_dir)) = queue.pop_front() {
        for entry in fs::read_dir(&from_dir)
            .with_context(|| format!("failed reading source directory {}", from_dir.display()))?
        {
            let entry = entry?;
            let from_path = entry.path();
            let to_path = to_dir.join(entry.file_name());
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                fs::create_dir_all(&to_path)
                    .with_context(|| format!("failed creating directory {}", to_path.display()))?;
                queue.push_back((from_path, to_path));
            } else if file_type.is_file() {
                fs::copy(&from_path, &to_path).with_context(|| {
                    format!(
                        "failed copying file from {} to {}",
                        from_path.display(),
                        to_path.display()
                    )
                })?;
            }
        }
    }

    Ok(())
}

pub(crate) fn validate_staged_registry_layout(staged_root: &Path, source_name: &str) -> Result<()> {
    let registry_pub = staged_root.join("registry.pub");
    if !registry_pub.is_file() {
        anyhow::bail!(
            "source-snapshot-missing: source '{}' missing registry.pub in {}",
            source_name,
            staged_root.display()
        );
    }

    let index_root = staged_root.join("index");
    if !index_root.is_dir() {
        anyhow::bail!(
            "source-snapshot-missing: source '{}' missing index/ in {}",
            source_name,
            staged_root.display()
        );
    }

    Ok(())
}

pub(crate) fn count_manifest_files(index_root: &Path) -> Result<u64> {
    let mut count = 0_u64;
    let mut queue: VecDeque<PathBuf> = VecDeque::new();
    queue.push_back(index_root.to_path_buf());

    while let Some(dir) = queue.pop_front() {
        for entry in fs::read_dir(&dir)
            .with_context(|| format!("failed reading index directory {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                queue.push_back(path);
            } else if file_type.is_file()
                && path.extension().and_then(|value| value.to_str()) == Some("toml")
            {
                count += 1;
            }
        }
    }

    Ok(count)
}

pub(crate) fn compute_filesystem_snapshot_id(staged_root: &Path) -> Result<String> {
    let mut file_paths = collect_relative_file_paths(staged_root)?;
    file_paths.sort();

    let mut snapshot_input = Vec::new();
    for relative_path in file_paths {
        let normalized_path = normalize_path_for_snapshot(&relative_path);
        let file_bytes = fs::read(staged_root.join(&relative_path)).with_context(|| {
            format!(
                "source-sync-failed: failed reading staged file for snapshot {}",
                staged_root.join(&relative_path).display()
            )
        })?;
        let file_digest = sha256_hex(&file_bytes);

        snapshot_input.extend_from_slice(normalized_path.as_bytes());
        snapshot_input.push(0);
        snapshot_input.extend_from_slice(file_digest.as_bytes());
        snapshot_input.push(0);
    }

    Ok(format!("fs:{}", sha256_hex(&snapshot_input)))
}

pub(crate) fn collect_relative_file_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let mut queue: VecDeque<PathBuf> = VecDeque::new();
    queue.push_back(root.to_path_buf());

    while let Some(dir) = queue.pop_front() {
        for entry in fs::read_dir(&dir)
            .with_context(|| format!("failed reading staged directory {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            let file_type = entry.file_type()?;

            if file_type.is_dir() {
                queue.push_back(path);
            } else if file_type.is_file() {
                let relative_path = path.strip_prefix(root).with_context(|| {
                    format!(
                        "failed deriving staged relative path {} from {}",
                        path.display(),
                        root.display()
                    )
                })?;
                paths.push(relative_path.to_path_buf());
            }
        }
    }

    Ok(paths)
}

pub(crate) fn normalize_path_for_snapshot(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub(crate) fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}
