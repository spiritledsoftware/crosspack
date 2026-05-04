mod artifact;
mod atomic_write;
mod durable;
mod exposure;
mod fs_utils;
mod identity;
mod installed_state;
mod layout;
mod native;
mod pins;
mod receipts;
mod transaction_coordinator;
mod transactions;
mod types;
mod uninstall;

pub use artifact::{
    install_from_artifact, install_from_artifact_to_dir, install_from_source_archive,
    install_from_source_archive_to_dir,
};
pub use exposure::{
    bin_path, clear_gui_exposure_state, clear_integration_state, expose_binary, expose_completion,
    expose_gui_app, expose_integration, exposed_completion_path, gui_asset_path,
    projected_exposed_completion_path, projected_gui_assets, projected_integration,
    read_all_gui_exposure_states, read_all_integration_states, read_gui_exposure_state,
    read_integration_state, remove_exposed_binary, remove_exposed_completion,
    remove_exposed_gui_asset, remove_exposed_integration, write_gui_exposure_state,
    write_identity_gui_exposure_state, write_identity_integration_state, write_integration_state,
};
pub use fs_utils::remove_file_if_exists;
pub use identity::{InstalledPackageIdentity, InstalledPackageSelector};
pub use installed_state::{
    clear_installed_package_state_document, find_installed_states_by_package_name,
    read_all_installed_package_states, read_installed_package_state,
    resolve_installed_package_selector, write_installed_package_state,
    InstalledPackageSelectionAmbiguity, InstalledPackageState,
};
pub use layout::{default_user_prefix, PrefixLayout};
pub use native::{
    clear_gui_native_state, clear_native_sidecar_state, read_all_gui_native_states,
    read_all_native_sidecar_states, read_gui_native_state, read_native_sidecar_state,
    register_native_gui_app_best_effort, remove_native_gui_registration_best_effort,
    remove_package_native_gui_registrations_best_effort, run_native_service_action,
    run_package_native_uninstall_actions, write_gui_native_state, write_identity_gui_native_state,
    write_native_sidecar_state,
};
pub use pins::{
    read_all_pins, read_identity_pin, read_pin, remove_pin, write_identity_pin, write_pin,
};
pub use receipts::{
    clear_declared_services_state, parse_identity_receipt, read_all_declared_services_states,
    read_declared_services_state, read_identity_install_receipt, read_identity_install_receipts,
    read_install_receipts, write_declared_services_state, write_identity_declared_services_state,
    write_identity_install_receipt, write_install_receipt, IdentityInstallReceipt,
};
#[cfg(test)]
pub(crate) use transaction_coordinator::TransactionBeginCrashHook;
pub use transaction_coordinator::{StartedTransaction, TransactionCoordinator};
pub use transactions::{
    append_transaction_journal_entry, clear_active_transaction, current_unix_timestamp,
    read_active_transaction, read_active_transaction_marker, read_transaction_journal_entries,
    read_transaction_metadata, set_active_transaction, update_transaction_status,
    write_transaction_metadata, ActiveTransactionMarker,
};
pub use types::{
    ArtifactInstallOptions, GuiExposureAsset, GuiNativeRegistrationRecord,
    InstallInteractionPolicy, InstallMode, InstallReason, InstallReceipt, IntegrationProjection,
    NativeServiceAction, NativeServiceOutcome, NativeSidecarState, NativeUninstallAction,
    TransactionJournalEntry, TransactionMetadata, TransactionRecoveryAction,
    TransactionRepairReason, TransactionStatus, UninstallResult, UninstallStatus,
};
pub use uninstall::{
    uninstall_blocked_by_roots_with_dependency_overrides,
    uninstall_blocked_by_roots_with_dependency_overrides_and_ignored_roots, uninstall_package,
    uninstall_package_identity, uninstall_package_with_dependency_overrides,
    uninstall_package_with_dependency_overrides_and_ignored_roots,
};

#[cfg(test)]
mod tests;
