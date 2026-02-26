mod fs_ops;
mod git_ops;
mod registry_index;
mod snapshot_state;
mod source_state;
mod source_store;
mod source_sync;
mod source_types;

pub use registry_index::{ConfiguredRegistryIndex, RegistryIndex};
pub use source_store::RegistrySourceStore;
pub use source_types::{
    RegistrySourceKind, RegistrySourceRecord, RegistrySourceSnapshotState,
    RegistrySourceWithSnapshotState, RegistrySourceWithSnapshotStatus, SourceUpdateResult,
    SourceUpdateStatus,
};

pub(crate) use fs_ops::{
    compute_filesystem_snapshot_id, copy_source_to_temp, count_manifest_files,
    current_unix_timestamp, unique_suffix, validate_staged_registry_layout,
};
pub(crate) use git_ops::{git_head_snapshot_id, run_git_clone, run_git_command};
pub(crate) use snapshot_state::{
    read_snapshot_id, read_snapshot_state, source_has_ready_snapshot, write_snapshot_file,
};
pub(crate) use source_state::{
    parse_source_state_file, select_update_sources, sort_sources, validate_source_fingerprint,
    validate_source_name, RegistrySourceStateFile,
};
pub(crate) use source_sync::update_source;

#[cfg(test)]
pub(crate) use git_ops::derive_snapshot_id_from_full_git_sha;
#[cfg(test)]
pub(crate) use source_sync::combine_replace_restore_errors;

#[cfg(test)]
mod tests;
