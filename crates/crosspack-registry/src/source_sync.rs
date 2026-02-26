use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use crosspack_security::sha256_hex;

use crate::{
    compute_filesystem_snapshot_id, copy_source_to_temp, count_manifest_files,
    git_head_snapshot_id, read_snapshot_id, run_git_clone, run_git_command, unique_suffix,
    validate_staged_registry_layout, write_snapshot_file, RegistryIndex, RegistrySourceKind,
    RegistrySourceRecord, RegistrySourceStore, SourceUpdateStatus,
};

pub(crate) fn update_source(
    store: &RegistrySourceStore,
    source: &RegistrySourceRecord,
) -> Result<(SourceUpdateStatus, String)> {
    match source.kind {
        RegistrySourceKind::Filesystem => update_filesystem_source(store, source),
        RegistrySourceKind::Git => update_git_source(store, source),
    }
}

fn update_filesystem_source(
    store: &RegistrySourceStore,
    source: &RegistrySourceRecord,
) -> Result<(SourceUpdateStatus, String)> {
    let staged_root = store
        .state_root
        .join(format!("tmp-{}-{}", source.name, unique_suffix()));

    let source_path = PathBuf::from(&source.location);
    if let Err(err) = copy_source_to_temp(&source_path, &staged_root, &source.name) {
        let _ = fs::remove_dir_all(&staged_root);
        return Err(err);
    }

    let snapshot_id = match compute_filesystem_snapshot_id(&staged_root) {
        Ok(snapshot_id) => snapshot_id,
        Err(err) => {
            let _ = fs::remove_dir_all(&staged_root);
            return Err(err);
        }
    };

    finalize_staged_source_update(store, source, staged_root, snapshot_id)
}

fn update_git_source(
    store: &RegistrySourceStore,
    source: &RegistrySourceRecord,
) -> Result<(SourceUpdateStatus, String)> {
    let staged_root = store
        .state_root
        .join(format!("tmp-{}-{}", source.name, unique_suffix()));
    let destination = store.state_root.join("cache").join(&source.name);

    let prepare_result = if destination.exists() {
        copy_source_to_temp(&destination, &staged_root, &source.name).and_then(|_| {
            run_git_command(
                &staged_root,
                &["fetch", "--prune", "--", source.location.as_str()],
                &source.name,
            )?;
            run_git_command(
                &staged_root,
                &["reset", "--hard", "FETCH_HEAD"],
                &source.name,
            )?;
            Ok(())
        })
    } else {
        run_git_clone(&source.location, &staged_root, &source.name)
    };

    if let Err(err) = prepare_result {
        let _ = fs::remove_dir_all(&staged_root);
        return Err(err);
    }

    let snapshot_id = match git_head_snapshot_id(&staged_root, &source.name) {
        Ok(snapshot_id) => snapshot_id,
        Err(err) => {
            let _ = fs::remove_dir_all(&staged_root);
            return Err(err);
        }
    };

    finalize_staged_source_update(store, source, staged_root, snapshot_id)
}

fn finalize_staged_source_update(
    store: &RegistrySourceStore,
    source: &RegistrySourceRecord,
    staged_root: PathBuf,
    snapshot_id: String,
) -> Result<(SourceUpdateStatus, String)> {
    let pipeline_result = (|| -> Result<(String, u64, Option<String>)> {
        validate_staged_registry_layout(&staged_root, &source.name)?;

        let registry_pub_path = staged_root.join("registry.pub");
        let registry_pub_raw = fs::read(&registry_pub_path).with_context(|| {
            format!(
                "source-sync-failed: source '{}' failed reading {}",
                source.name,
                registry_pub_path.display()
            )
        })?;
        let actual_fingerprint = sha256_hex(&registry_pub_raw);
        if !actual_fingerprint.eq_ignore_ascii_case(&source.fingerprint_sha256) {
            anyhow::bail!(
                "source-key-fingerprint-mismatch: source '{}' expected {}, got {}",
                source.name,
                source.fingerprint_sha256,
                actual_fingerprint
            );
        }

        verify_metadata_signature_policy(&staged_root, &source.name)?;

        let manifest_count = count_manifest_files(&staged_root.join("index"))?;
        let existing_snapshot_id = read_snapshot_id(
            &store
                .state_root
                .join("cache")
                .join(&source.name)
                .join("snapshot.json"),
        );
        Ok((snapshot_id, manifest_count, existing_snapshot_id))
    })();

    if let Err(err) = pipeline_result {
        let _ = fs::remove_dir_all(&staged_root);
        return Err(err);
    }

    let (snapshot_id, manifest_count, existing_snapshot_id) = pipeline_result?;
    let cache_root = store.state_root.join("cache");
    fs::create_dir_all(&cache_root).with_context(|| {
        format!(
            "source-sync-failed: source '{}' failed creating cache root {}",
            source.name,
            cache_root.display()
        )
    })?;
    let destination = cache_root.join(&source.name);
    let backup = cache_root.join(format!(".{}-backup-{}", source.name, unique_suffix()));
    let had_existing = destination.exists();

    if had_existing {
        fs::rename(&destination, &backup).with_context(|| {
            format!(
                "source-sync-failed: source '{}' failed backing up cache {}",
                source.name,
                destination.display()
            )
        })?;
    }

    if let Err(err) = fs::rename(&staged_root, &destination).with_context(|| {
        format!(
            "source-sync-failed: source '{}' failed replacing cache {}",
            source.name,
            destination.display()
        )
    }) {
        if had_existing {
            if let Err(restore_err) = fs::rename(&backup, &destination) {
                return Err(combine_replace_restore_errors(
                    &source.name,
                    &destination,
                    &backup,
                    err,
                    restore_err,
                ));
            }
        }
        return Err(err);
    }

    if let Err(err) = write_snapshot_file(&destination, &source.name, &snapshot_id, manifest_count)
    {
        let _ = fs::remove_dir_all(&destination);
        if had_existing {
            if let Err(restore_err) = fs::rename(&backup, &destination) {
                return Err(combine_replace_restore_errors(
                    &source.name,
                    &destination,
                    &backup,
                    err,
                    restore_err,
                ));
            }
        }
        return Err(err);
    }

    if had_existing {
        let _ = fs::remove_dir_all(&backup);
    }

    let status = if existing_snapshot_id.as_deref() == Some(snapshot_id.as_str()) {
        SourceUpdateStatus::UpToDate
    } else {
        SourceUpdateStatus::Updated
    };
    Ok((status, snapshot_id))
}

pub(crate) fn verify_metadata_signature_policy(
    staged_root: &Path,
    source_name: &str,
) -> Result<()> {
    let index_root = staged_root.join("index");
    for entry in fs::read_dir(&index_root).with_context(|| {
        format!(
            "source-metadata-invalid: source '{}' failed reading index {}",
            source_name,
            index_root.display()
        )
    })? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let package = entry.file_name().to_string_lossy().to_string();
        RegistryIndex::open(staged_root)
            .package_versions(&package)
            .with_context(|| {
                format!(
                    "source-metadata-invalid: source '{}' package '{}' failed signature validation",
                    source_name, package
                )
            })?;
    }

    Ok(())
}

pub(crate) fn combine_replace_restore_errors(
    source_name: &str,
    destination: &Path,
    backup: &Path,
    replace_err: anyhow::Error,
    restore_err: std::io::Error,
) -> anyhow::Error {
    anyhow::anyhow!(
        "source-sync-failed: source '{}' failed replacing cache {}: {:#}; failed restoring backup {}: {}",
        source_name,
        destination.display(),
        replace_err,
        backup.display(),
        restore_err
    )
}
