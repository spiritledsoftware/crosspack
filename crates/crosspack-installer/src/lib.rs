mod artifact;
mod exposure;
mod fs_utils;
mod layout;
mod native;
mod pins;
mod receipts;
mod transactions;
mod types;
mod uninstall;

pub use artifact::install_from_artifact;
pub use exposure::{
    bin_path, clear_gui_exposure_state, expose_binary, expose_completion, expose_gui_app,
    exposed_completion_path, gui_asset_path, projected_exposed_completion_path,
    projected_gui_assets, read_all_gui_exposure_states, read_gui_exposure_state,
    remove_exposed_binary, remove_exposed_completion, remove_exposed_gui_asset,
    write_gui_exposure_state,
};
pub use fs_utils::remove_file_if_exists;
pub use layout::{default_user_prefix, PrefixLayout};
pub use native::{
    clear_gui_native_state, clear_native_sidecar_state, read_all_gui_native_states,
    read_all_native_sidecar_states, read_gui_native_state, read_native_sidecar_state,
    register_native_gui_app_best_effort, remove_native_gui_registration_best_effort,
    remove_package_native_gui_registrations_best_effort, run_package_native_uninstall_actions,
    write_gui_native_state, write_native_sidecar_state,
};
pub use pins::{read_all_pins, read_pin, remove_pin, write_pin};
pub use receipts::{read_install_receipts, write_install_receipt};
pub use transactions::{
    append_transaction_journal_entry, clear_active_transaction, current_unix_timestamp,
    read_active_transaction, read_transaction_metadata, set_active_transaction,
    update_transaction_status, write_transaction_metadata,
};
pub use types::{
    ArtifactInstallOptions, GuiExposureAsset, GuiNativeRegistrationRecord,
    InstallInteractionPolicy, InstallMode, InstallReason, InstallReceipt, NativeSidecarState,
    NativeUninstallAction, TransactionJournalEntry, TransactionMetadata, UninstallResult,
    UninstallStatus,
};
pub use uninstall::{
    uninstall_blocked_by_roots_with_dependency_overrides,
    uninstall_blocked_by_roots_with_dependency_overrides_and_ignored_roots, uninstall_package,
    uninstall_package_with_dependency_overrides,
    uninstall_package_with_dependency_overrides_and_ignored_roots,
};

#[cfg(test)]
mod tests;
