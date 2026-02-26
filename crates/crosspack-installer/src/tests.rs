use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::anyhow;
use crosspack_core::{ArchiveType, ArtifactCompletionShell, ArtifactGuiApp};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::artifact::{
    build_appx_unpack_command, build_dmg_attach_command, build_dmg_detach_command,
    build_exe_extract_command, build_msi_admin_extract_command, build_msix_unpack_command,
    build_pkg_copy_command, build_pkg_expand_command, copy_dmg_payload, discover_pkg_payload_roots,
    stage_appimage_payload, stage_appx_payload_with_runner, stage_bin_payload,
    stage_dmg_payload_with_hooks, stage_exe_payload_with_runner, stage_msix_payload_with_runner,
    stage_pkg_payload_with_hooks, strip_rel_components,
};
use crate::native::{
    macos_registration_destination_candidates, macos_registration_source_path,
    parse_native_sidecar_state, project_linux_user_applications_dir,
    project_macos_user_applications_dir, project_windows_start_menu_programs_dir,
    register_macos_application_symlink_with_creator,
    register_macos_native_gui_registration_with_executor_and_creator,
    register_native_gui_app_best_effort_with_executor, select_macos_registration_destination,
    MACOS_LSREGISTER_PATH,
};
use crate::receipts::parse_receipt;

#[test]
fn parse_old_receipt_shape() {
    let raw = "name=fd\nversion=10.2.0\ninstalled_at_unix=123\n";
    let receipt = parse_receipt(raw).expect("must parse");
    assert_eq!(receipt.name, "fd");
    assert_eq!(receipt.version, "10.2.0");
    assert!(receipt.dependencies.is_empty());
    assert_eq!(receipt.install_status, "installed");
    assert!(receipt.target.is_none());
    assert!(receipt.snapshot_id.is_none());
    assert!(receipt.exposed_completions.is_empty());
    assert_eq!(receipt.install_reason, InstallReason::Root);
}

#[test]
fn parse_new_receipt_shape() {
    let raw = "name=fd\nversion=10.2.0\ndependency=zlib@2.1.0\ndependency=pcre2@10.44.0\ntarget=x86_64-unknown-linux-gnu\nartifact_url=https://example.test/fd.tgz\nartifact_sha256=abc\ncache_path=/tmp/fd.tgz\nexposed_bin=fd\nexposed_bin=fdfind\nexposed_completion=packages/bash/fd--completions--fd.bash\nsnapshot_id=git:5f1b3d8a1f2a4d0e\ninstall_reason=dependency\ninstall_status=installed\ninstalled_at_unix=123\n";
    let receipt = parse_receipt(raw).expect("must parse");
    assert_eq!(receipt.dependencies, vec!["zlib@2.1.0", "pcre2@10.44.0"]);
    assert_eq!(receipt.target.as_deref(), Some("x86_64-unknown-linux-gnu"));
    assert_eq!(receipt.artifact_sha256.as_deref(), Some("abc"));
    assert_eq!(receipt.exposed_bins, vec!["fd", "fdfind"]);
    assert_eq!(
        receipt.exposed_completions,
        vec!["packages/bash/fd--completions--fd.bash"]
    );
    assert_eq!(receipt.snapshot_id.as_deref(), Some("git:5f1b3d8a1f2a4d0e"));
    assert_eq!(receipt.install_reason, InstallReason::Dependency);
}

#[test]
fn receipt_round_trip_with_install_mode_native() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    write_install_receipt(
        &layout,
        &InstallReceipt {
            name: "zed".to_string(),
            version: "0.150.0".to_string(),
            dependencies: vec!["ripgrep@14.0.0".to_string()],
            target: Some("x86_64-unknown-linux-gnu".to_string()),
            artifact_url: Some("https://example.test/zed.tar.zst".to_string()),
            artifact_sha256: Some("abc123".to_string()),
            cache_path: Some("/tmp/zed.tar.zst".to_string()),
            exposed_bins: vec!["zed".to_string()],
            exposed_completions: Vec::new(),
            snapshot_id: Some("git:deadbeef".to_string()),
            install_mode: InstallMode::Native,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 123,
        },
    )
    .expect("must write receipt");

    let receipts = read_install_receipts(&layout).expect("must read receipts");
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].install_mode, InstallMode::Native);

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn receipt_defaults_install_mode_managed_for_legacy() {
    let raw = "name=fd\nversion=10.2.0\ninstalled_at_unix=123\n";
    let receipt = parse_receipt(raw).expect("must parse");
    assert_eq!(receipt.install_mode, InstallMode::Managed);
}

#[test]
fn receipt_unknown_install_mode_falls_back_to_managed() {
    let raw = "name=fd\nversion=10.2.0\ninstall_mode=native-v2\ninstalled_at_unix=123\n";
    let receipt = parse_receipt(raw).expect("must parse unknown install mode tokens");
    assert_eq!(receipt.install_mode, InstallMode::Managed);
}

#[test]
fn transaction_paths_match_spec_layout() {
    let layout = test_layout();
    assert_eq!(
        layout.transactions_dir(),
        layout.state_dir().join("transactions")
    );
    assert_eq!(
        layout.transaction_active_path(),
        layout.state_dir().join("transactions").join("active")
    );
    assert_eq!(
        layout.transaction_metadata_path("tx-1"),
        layout.state_dir().join("transactions").join("tx-1.json")
    );
    assert_eq!(
        layout.transaction_journal_path("tx-1"),
        layout.state_dir().join("transactions").join("tx-1.journal")
    );
    assert_eq!(
        layout.transaction_staging_path("tx-1"),
        layout
            .state_dir()
            .join("transactions")
            .join("staging")
            .join("tx-1")
    );
}

#[test]
fn write_transaction_metadata_and_active_file() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let metadata = TransactionMetadata {
        version: 1,
        txid: "tx-1771001234-000042".to_string(),
        operation: "upgrade".to_string(),
        status: "applying".to_string(),
        started_at_unix: 1_771_001_234,
        snapshot_id: Some("git:5f1b3d8a1f2a4d0e".to_string()),
    };

    let metadata_path =
        write_transaction_metadata(&layout, &metadata).expect("must write transaction metadata");
    set_active_transaction(&layout, &metadata.txid).expect("must write active transaction");

    let metadata_raw = fs::read_to_string(metadata_path).expect("must read metadata file");
    assert!(metadata_raw.contains("\"txid\": \"tx-1771001234-000042\""));
    assert!(metadata_raw.contains("\"operation\": \"upgrade\""));
    assert!(metadata_raw.contains("\"status\": \"applying\""));
    assert!(metadata_raw.contains("\"snapshot_id\": \"git:5f1b3d8a1f2a4d0e\""));

    let active_raw =
        fs::read_to_string(layout.transaction_active_path()).expect("must read active file");
    assert_eq!(active_raw.trim(), "tx-1771001234-000042");

    clear_active_transaction(&layout).expect("must clear active transaction");
    assert!(!layout.transaction_active_path().exists());

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn read_transaction_metadata_round_trip() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let metadata = TransactionMetadata {
        version: 1,
        txid: "tx-meta-1".to_string(),
        operation: "upgrade".to_string(),
        status: "applying".to_string(),
        started_at_unix: 1_771_001_240,
        snapshot_id: Some("git:abc123".to_string()),
    };

    write_transaction_metadata(&layout, &metadata).expect("must write metadata");
    let loaded = read_transaction_metadata(&layout, "tx-meta-1")
        .expect("must read metadata")
        .expect("metadata should exist");

    assert_eq!(loaded, metadata);

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn read_transaction_metadata_rejects_truncated_quoted_value() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let txid = "tx-corrupt-quote";
    let raw = "{\n  \"version\": 1,\n  \"txid\": \",\n  \"operation\": \"install\",\n  \"status\": \"planning\",\n  \"started_at_unix\": 1771001250\n}\n";
    fs::write(layout.transaction_metadata_path(txid), raw)
        .expect("must write malformed metadata file");

    let err = read_transaction_metadata(&layout, txid)
        .expect_err("truncated quoted value should be recoverable parse error");
    let err_text = format!("{err:#}");
    assert!(
        err_text.contains("invalid quoted transaction metadata value for field: txid"),
        "unexpected error: {err_text}"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn update_transaction_status_rewrites_metadata_status() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let metadata = TransactionMetadata {
        version: 1,
        txid: "tx-status-1".to_string(),
        operation: "install".to_string(),
        status: "planning".to_string(),
        started_at_unix: 1_771_001_250,
        snapshot_id: None,
    };

    write_transaction_metadata(&layout, &metadata).expect("must write metadata");
    update_transaction_status(&layout, "tx-status-1", "applying").expect("must update status");

    let loaded = read_transaction_metadata(&layout, "tx-status-1")
        .expect("must read metadata")
        .expect("metadata should exist");
    assert_eq!(loaded.status, "applying");

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn read_active_transaction_round_trip() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    assert!(read_active_transaction(&layout)
        .expect("must read active transaction")
        .is_none());

    set_active_transaction(&layout, "tx-abc").expect("must write active transaction");
    assert_eq!(
        read_active_transaction(&layout)
            .expect("must read active transaction")
            .as_deref(),
        Some("tx-abc")
    );

    clear_active_transaction(&layout).expect("must clear active transaction");
    assert!(read_active_transaction(&layout)
        .expect("must read active transaction")
        .is_none());

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn set_active_transaction_rejects_when_marker_already_exists() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    set_active_transaction(&layout, "tx-first").expect("must claim first active marker");

    let err = set_active_transaction(&layout, "tx-second")
        .expect_err("second active claim should fail atomically");
    assert!(
        err.to_string()
            .contains("active transaction marker already exists (txid=tx-first)"),
        "unexpected error: {err}"
    );

    assert_eq!(
        read_active_transaction(&layout)
            .expect("must read active marker")
            .as_deref(),
        Some("tx-first"),
        "first active marker should remain intact"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn append_transaction_journal_entries_in_order() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    append_transaction_journal_entry(
        &layout,
        "tx-1",
        &TransactionJournalEntry {
            seq: 1,
            step: "backup_receipt".to_string(),
            state: "done".to_string(),
            path: Some("state/installed/tool.receipt.bak".to_string()),
        },
    )
    .expect("must append first entry");

    append_transaction_journal_entry(
        &layout,
        "tx-1",
        &TransactionJournalEntry {
            seq: 2,
            step: "remove_package_dir".to_string(),
            state: "done".to_string(),
            path: Some("pkgs/tool/1.0.0".to_string()),
        },
    )
    .expect("must append second entry");

    let journal_raw =
        fs::read_to_string(layout.transaction_journal_path("tx-1")).expect("must read journal");
    let lines: Vec<&str> = journal_raw.lines().collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(
        lines[0],
        "{\"seq\":1,\"step\":\"backup_receipt\",\"state\":\"done\",\"path\":\"state/installed/tool.receipt.bak\"}"
    );
    assert_eq!(
        lines[1],
        "{\"seq\":2,\"step\":\"remove_package_dir\",\"state\":\"done\",\"path\":\"pkgs/tool/1.0.0\"}"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn expose_and_remove_binary_round_trip() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let package_dir = layout.package_dir("demo", "1.0.0");
    fs::create_dir_all(&package_dir).expect("must create package dir");
    fs::write(package_dir.join("demo"), b"#!/bin/sh\n").expect("must write binary");

    expose_binary(&layout, &package_dir, "demo", "demo").expect("must expose binary");

    let exposed_path = bin_path(&layout, "demo");
    assert!(exposed_path.exists());

    remove_exposed_binary(&layout, "demo").expect("must remove binary");
    assert!(!exposed_path.exists());

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn expose_binary_accepts_flattened_macos_app_bundle_exec_path() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let package_dir = layout.package_dir("neovide", "0.14.0");
    fs::create_dir_all(package_dir.join("Contents").join("MacOS"))
        .expect("must create app executable dir");
    fs::write(
        package_dir.join("Contents").join("MacOS").join("neovide"),
        b"#!/bin/sh\n",
    )
    .expect("must write app executable");

    expose_binary(
        &layout,
        &package_dir,
        "neovide",
        "Neovide.app/Contents/MacOS/neovide",
    )
    .expect("must expose binary for flattened app bundle path");

    assert!(bin_path(&layout, "neovide").exists());

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn expose_binary_does_not_strip_non_app_bundle_prefixes() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let package_dir = layout.package_dir("demo", "1.0.0");
    fs::create_dir_all(package_dir.join("bin")).expect("must create bin dir");
    fs::write(package_dir.join("bin").join("demo"), b"#!/bin/sh\n").expect("must write binary");

    let err = expose_binary(&layout, &package_dir, "demo", "prefix/bin/demo")
        .expect_err("non-app bundle path should not be rewritten");
    assert!(
        err.to_string()
            .contains("declared binary path 'prefix/bin/demo' was not found in install root"),
        "unexpected error: {err}"
    );
    assert!(
        !bin_path(&layout, "demo").exists(),
        "binary should not be exposed for non-app path rewrite"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn expose_and_remove_completion_round_trip() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let package_dir = layout.package_dir("zoxide", "1.0.0");
    fs::create_dir_all(package_dir.join("completions")).expect("must create completion dir");
    fs::write(
        package_dir.join("completions").join("zoxide.bash"),
        b"# bash completion\n",
    )
    .expect("must write completion file");

    let exposed = expose_completion(
        &layout,
        &package_dir,
        "zoxide",
        ArtifactCompletionShell::Bash,
        "completions/zoxide.bash",
    )
    .expect("must expose completion");
    assert_eq!(
        exposed,
        projected_exposed_completion_path(
            "zoxide",
            ArtifactCompletionShell::Bash,
            "completions/zoxide.bash",
        )
        .expect("must project completion path")
    );
    let exposed_path = exposed_completion_path(&layout, &exposed)
        .expect("must resolve exposed completion storage path");
    assert!(exposed_path.exists());

    remove_exposed_completion(&layout, &exposed).expect("must remove completion");
    assert!(!exposed_path.exists());

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn expose_completion_rejects_invalid_relative_path() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let package_dir = layout.package_dir("zoxide", "1.0.0");
    fs::create_dir_all(&package_dir).expect("must create package dir");

    let err = expose_completion(
        &layout,
        &package_dir,
        "zoxide",
        ArtifactCompletionShell::Zsh,
        "../outside/_zoxide",
    )
    .expect_err("path traversal should be rejected");
    assert!(err.to_string().contains("must not include '..'"));

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn expose_gui_app_and_state_round_trip() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let package_dir = layout.package_dir("zed", "1.0.0");
    fs::create_dir_all(&package_dir).expect("must create package dir");
    fs::write(package_dir.join("zed"), b"#!/bin/sh\n").expect("must write gui app exec");

    let app = ArtifactGuiApp {
        app_id: "dev.zed.Zed".to_string(),
        display_name: "Zed".to_string(),
        exec: "zed".to_string(),
        icon: None,
        categories: vec!["Development".to_string()],
        file_associations: vec![crosspack_core::ArtifactGuiFileAssociation {
            mime_type: "text/plain".to_string(),
            extensions: vec![".txt".to_string()],
        }],
        protocols: vec![crosspack_core::ArtifactGuiProtocol {
            scheme: "zed".to_string(),
        }],
    };

    let assets = expose_gui_app(&layout, &package_dir, "zed", &app).expect("must expose gui app");
    assert!(
        assets.iter().any(|asset| asset.key == "app:dev.zed.zed"),
        "launcher ownership key must be present"
    );

    for asset in &assets {
        let path = gui_asset_path(&layout, &asset.rel_path).expect("must resolve gui path");
        assert!(
            path.exists(),
            "gui asset path should exist: {}",
            path.display()
        );
    }

    write_gui_exposure_state(&layout, "zed", &assets).expect("must write gui state");
    let loaded = read_gui_exposure_state(&layout, "zed").expect("must read gui state");
    assert_eq!(loaded, assets);

    for asset in &assets {
        remove_exposed_gui_asset(&layout, asset).expect("must remove gui asset");
    }
    clear_gui_exposure_state(&layout, "zed").expect("must remove gui state file");

    assert!(
        read_gui_exposure_state(&layout, "zed")
            .expect("must read removed gui state")
            .is_empty(),
        "gui state should be empty after clear"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn native_gui_state_round_trip() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let records = vec![
        GuiNativeRegistrationRecord {
            key: "app:dev.zed.zed".to_string(),
            kind: "desktop-entry".to_string(),
            path: "/tmp/dev.zed.zed.desktop".to_string(),
        },
        GuiNativeRegistrationRecord {
            key: "protocol:zed".to_string(),
            kind: "protocol-handler".to_string(),
            path: "HKCU\\Software\\Classes\\zed".to_string(),
        },
    ];

    write_gui_native_state(&layout, "zed", &records).expect("must write native gui state");
    let loaded = read_gui_native_state(&layout, "zed").expect("must read native gui state");
    assert_eq!(loaded, records);
}

#[test]
fn native_state_round_trip_for_uninstall_actions() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let state = NativeSidecarState {
        uninstall_actions: vec![
            NativeUninstallAction {
                key: "app:dev.zed.zed".to_string(),
                kind: "desktop-entry".to_string(),
                path: "/tmp/dev.zed.zed.desktop".to_string(),
            },
            NativeUninstallAction {
                key: "protocol:zed".to_string(),
                kind: "registry-key".to_string(),
                path: "HKCU\\Software\\Classes\\zed".to_string(),
            },
        ],
    };

    write_native_sidecar_state(&layout, "zed", &state)
        .expect("must write native uninstall sidecar state");
    let loaded = read_native_sidecar_state(&layout, "zed").expect("must read native sidecar state");
    assert_eq!(loaded, state);
}

#[test]
fn native_sidecar_legacy_record_rows_are_still_read() {
    let raw = "version=1\nrecord=app:dev.zed.zed\tdesktop-entry\t/tmp/dev.zed.zed.desktop\nrecord=protocol:zed\tregistry-key\tHKCU\\Software\\Classes\\zed\n";
    let state = parse_native_sidecar_state(raw).expect("must parse legacy record rows");

    assert_eq!(
        state.uninstall_actions,
        vec![
            NativeUninstallAction {
                key: "app:dev.zed.zed".to_string(),
                kind: "desktop-entry".to_string(),
                path: "/tmp/dev.zed.zed.desktop".to_string(),
            },
            NativeUninstallAction {
                key: "protocol:zed".to_string(),
                kind: "registry-key".to_string(),
                path: "HKCU\\Software\\Classes\\zed".to_string(),
            },
        ]
    );
}

#[test]
fn native_sidecar_unsupported_version_rejects() {
    let raw =
        "version=42\nuninstall_action=app:dev.zed.zed\tdesktop-entry\t/tmp/dev.zed.zed.desktop\n";
    let err = parse_native_sidecar_state(raw).expect_err("unsupported version must fail");
    assert!(
        err.to_string()
            .contains("unsupported native sidecar version: 42"),
        "unexpected error: {err}"
    );
}

#[test]
fn native_sidecar_malformed_uninstall_action_row_rejects() {
    let raw = "version=1\nuninstall_action=app:dev.zed.zed\tdesktop-entry\n";
    let err = parse_native_sidecar_state(raw)
        .expect_err("malformed uninstall_action row should fail parsing");
    assert!(
        err.to_string()
            .contains("invalid native uninstall action row format"),
        "unexpected error: {err}"
    );
}

#[test]
fn native_sidecar_malformed_line_without_equals_rejects() {
    let raw =
        "version=1\nuninstall_action\tapp:dev.zed.zed\tdesktop-entry\t/tmp/dev.zed.zed.desktop\n";
    let err = parse_native_sidecar_state(raw)
        .expect_err("suspicious malformed sidecar line should fail parsing");
    assert!(
        err.to_string()
            .contains("invalid native sidecar row format"),
        "unexpected error: {err}"
    );
}

#[test]
fn native_gui_state_read_missing_returns_empty() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let records = read_gui_native_state(&layout, "missing").expect("must read missing state");
    assert!(records.is_empty());
}

#[test]
fn native_gui_state_clear_removes_state_file() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    write_gui_native_state(
        &layout,
        "zed",
        &[GuiNativeRegistrationRecord {
            key: "app:dev.zed.zed".to_string(),
            kind: "desktop-entry".to_string(),
            path: "/tmp/dev.zed.zed.desktop".to_string(),
        }],
    )
    .expect("must write native gui state");
    clear_gui_native_state(&layout, "zed").expect("must clear native gui state");

    assert!(!layout.gui_native_state_path("zed").exists());
}

#[test]
fn write_gui_exposure_state_rejects_tab_delimiter_characters() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let err = write_gui_exposure_state(
        &layout,
        "demo",
        &[GuiExposureAsset {
            key: "app:demo\tbad".to_string(),
            rel_path: "launchers/demo.command".to_string(),
        }],
    )
    .expect_err("tab-delimited values should be rejected");
    assert!(
        err.to_string().contains("must not contain"),
        "unexpected error: {err}"
    );
}

#[test]
fn register_native_gui_linux_projects_user_desktop_path() {
    let home = Path::new("/home/tester");
    assert_eq!(
        project_linux_user_applications_dir(home),
        PathBuf::from("/home/tester/.local/share/applications")
    );
}

#[test]
fn register_native_gui_windows_projects_start_menu_path() {
    let appdata = Path::new(r"C:\Users\tester\AppData\Roaming");
    assert_eq!(
        project_windows_start_menu_programs_dir(appdata),
        PathBuf::from(r"C:\Users\tester\AppData\Roaming")
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
    );
}

#[test]
fn register_native_gui_macos_projects_user_applications_path() {
    let home = Path::new("/Users/tester");
    assert_eq!(
        project_macos_user_applications_dir(home),
        PathBuf::from("/Users/tester/Applications")
    );
}

#[test]
fn macos_registration_destination_candidates_prioritize_system_then_user() {
    let home = Path::new("/Users/tester");
    let candidates =
        macos_registration_destination_candidates(home, std::ffi::OsStr::new("Demo.app"));

    assert_eq!(
        candidates,
        [
            PathBuf::from("/Applications/Demo.app"),
            PathBuf::from("/Users/tester/Applications/Demo.app"),
        ]
    );
}

#[test]
fn macos_registration_source_prefers_app_bundle_root() {
    let install_root = Path::new("/Users/tester/.crosspack/pkgs/neovide/0.15.2");
    let source_path = install_root.join("Neovide.app/Contents/MacOS/neovide");
    assert_eq!(
        macos_registration_source_path(install_root, &source_path),
        install_root.join("Neovide.app")
    );
}

#[test]
fn macos_registration_source_falls_back_to_binary_path_when_no_bundle() {
    let install_root = Path::new("/Users/tester/.crosspack/pkgs/demo/1.0.0");
    let source_path = install_root.join("demo");
    assert_eq!(
        macos_registration_source_path(install_root, &source_path),
        source_path
    );
}

#[test]
fn register_native_gui_macos_bundle_source_deploys_directory_copy_not_symlink() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let root = layout.prefix().join("macos-bundle-register-test");
    let source_bundle = root.join("staged").join("Demo.app");
    let source_binary = source_bundle.join("Contents").join("MacOS").join("demo");
    fs::create_dir_all(source_binary.parent().expect("must have parent"))
        .expect("must create source bundle dirs");
    fs::write(&source_binary, b"#!/bin/sh\n").expect("must write source bundle binary");

    let system_target = root.join("system-applications").join("Demo.app");
    let user_target = root.join("user-applications").join("Demo.app");
    let projected_assets = vec![GuiExposureAsset {
        key: "app:dev.demo.App".to_string(),
        rel_path: "launchers/dev-demo.command".to_string(),
    }];

    let mut symlink_calls = 0usize;
    let (_records, warnings) = register_macos_native_gui_registration_with_executor_and_creator(
        &projected_assets,
        &source_bundle,
        [system_target.clone(), user_target],
        &[],
        &mut |_command, _context| Ok(()),
        |_source, _destination| {
            symlink_calls += 1;
            Ok(())
        },
    );

    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    assert_eq!(
        symlink_calls, 0,
        "bundle deployment must not use symlink registration"
    );
    let deployed_metadata =
        fs::symlink_metadata(&system_target).expect("deployed bundle path should exist");
    assert!(
        deployed_metadata.is_dir(),
        "deployed bundle path should be a directory"
    );
    assert!(
        !deployed_metadata.file_type().is_symlink(),
        "deployed bundle path must not be a symlink"
    );
    assert!(
        system_target.join("Contents/MacOS/demo").exists(),
        "bundle payload should be copied to deployed destination"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn register_native_gui_macos_bundle_source_runs_lsregister_for_deployed_bundle_path() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let root = layout.prefix().join("macos-bundle-lsregister-test");
    let source_bundle = root.join("staged").join("Demo.app");
    let source_binary = source_bundle.join("Contents").join("MacOS").join("demo");
    fs::create_dir_all(source_binary.parent().expect("must have parent"))
        .expect("must create source bundle dirs");
    fs::write(&source_binary, b"#!/bin/sh\n").expect("must write source bundle binary");

    let system_target = root.join("system-applications").join("Demo.app");
    let user_target = root.join("user-applications").join("Demo.app");
    let projected_assets = vec![GuiExposureAsset {
        key: "app:dev.demo.App".to_string(),
        rel_path: "launchers/dev-demo.command".to_string(),
    }];

    let mut observed_program = String::new();
    let mut observed_args: Vec<String> = Vec::new();
    let (_records, warnings) = register_macos_native_gui_registration_with_executor_and_creator(
        &projected_assets,
        &source_bundle,
        [system_target.clone(), user_target],
        &[],
        &mut |command, _context| {
            observed_program = command.get_program().to_string_lossy().into_owned();
            observed_args = command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
            Ok(())
        },
        |_source, _destination| Ok(()),
    );

    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    assert_eq!(observed_program, MACOS_LSREGISTER_PATH);
    assert_eq!(
        observed_args,
        vec!["-f".to_string(), system_target.display().to_string()]
    );
    assert!(
        Path::new(&observed_args[1]).exists(),
        "lsregister should run against the deployed bundle path"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn uninstall_native_macos_bundle_registration_persists_bundle_copy_kind() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let root = layout.prefix().join("macos-bundle-kind-persist-test");
    let source_bundle = root.join("staged").join("Demo.app");
    let source_binary = source_bundle.join("Contents").join("MacOS").join("demo");
    fs::create_dir_all(source_binary.parent().expect("must have parent"))
        .expect("must create source bundle dirs");
    fs::write(&source_binary, b"#!/bin/sh\n").expect("must write source bundle binary");

    let system_target = root.join("system-applications").join("Demo.app");
    let user_target = root.join("user-applications").join("Demo.app");
    let projected_assets = vec![GuiExposureAsset {
        key: "app:dev.demo.App".to_string(),
        rel_path: "launchers/dev-demo.command".to_string(),
    }];

    let (records, warnings) = register_macos_native_gui_registration_with_executor_and_creator(
        &projected_assets,
        &source_bundle,
        [system_target.clone(), user_target],
        &[],
        &mut |_command, _context| Ok(()),
        |_source, _destination| Ok(()),
    );

    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    assert!(records
        .iter()
        .all(|record| record.kind == "applications-bundle-copy"));
    write_gui_native_state(&layout, "demo", &records).expect("must persist native state");
    let loaded = read_gui_native_state(&layout, "demo").expect("must reload native state");
    assert!(loaded
        .iter()
        .all(|record| record.kind == "applications-bundle-copy"));

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn register_native_gui_macos_non_bundle_source_keeps_symlink_registration_behavior() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let root = layout.prefix().join("macos-non-bundle-register-test");
    let source_path = root.join("staged").join("demo");
    fs::create_dir_all(source_path.parent().expect("must have parent"))
        .expect("must create source parent");
    fs::write(&source_path, b"#!/bin/sh\n").expect("must write source executable");

    let system_target = root.join("system-applications").join("demo");
    let user_target = root.join("user-applications").join("demo");
    let projected_assets = vec![GuiExposureAsset {
        key: "app:dev.demo.App".to_string(),
        rel_path: "launchers/dev-demo.command".to_string(),
    }];

    let mut symlink_invocations = Vec::new();
    let (records, warnings) = register_macos_native_gui_registration_with_executor_and_creator(
        &projected_assets,
        &source_path,
        [system_target.clone(), user_target],
        &[],
        &mut |_command, _context| Ok(()),
        |source, destination| {
            symlink_invocations.push((source.to_path_buf(), destination.to_path_buf()));
            fs::write(destination, b"simulated-symlink")
        },
    );

    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    assert_eq!(
        symlink_invocations,
        vec![(source_path.clone(), system_target.clone())],
        "non-.app registration should continue using symlink writer path"
    );
    assert_eq!(records.len(), projected_assets.len());
    assert!(records
        .iter()
        .all(|record| record.kind == "applications-symlink"));
    assert!(records
        .iter()
        .all(|record| record.path == system_target.display().to_string()));
    assert_eq!(
        fs::read(&system_target).expect("simulated symlink destination should exist"),
        b"simulated-symlink"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn macos_registration_destination_prefers_system_when_safe() {
    let layout = test_layout();
    let root = layout.prefix().join("macos-destination-test");
    let app_name = "Demo.app";
    let system_target = root.join("system-applications").join(app_name);
    let user_target = root.join("user-applications").join(app_name);

    let (selected, warnings) =
        select_macos_registration_destination([system_target.clone(), user_target], &[]);

    assert_eq!(selected.as_ref(), Some(&system_target));
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn macos_registration_destination_falls_back_to_user_when_system_unavailable() {
    let layout = test_layout();
    let root = layout.prefix().join("macos-destination-test");
    let app_name = "Demo.app";
    let blocked_parent = root.join("blocked-parent");
    fs::create_dir_all(&root).expect("must create test root");
    fs::write(&blocked_parent, b"blocked").expect("must create blocking file");

    let system_target = blocked_parent.join(app_name);
    let user_target = root.join("user-applications").join(app_name);

    let (selected, warnings) =
        select_macos_registration_destination([system_target, user_target.clone()], &[]);

    assert_eq!(selected.as_ref(), Some(&user_target));
    assert!(warnings
        .iter()
        .any(|warning| { warning.contains("failed to prepare macOS applications dir") }));

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn macos_registration_destination_refuses_unmanaged_existing_target() {
    let layout = test_layout();
    let root = layout.prefix().join("macos-destination-test");
    let app_name = "Demo.app";
    let system_target = root.join("system-applications").join(app_name);
    let blocked_parent = root.join("blocked-parent");

    fs::create_dir_all(system_target.parent().expect("must have parent"))
        .expect("must create system parent");
    fs::create_dir_all(&system_target).expect("must seed unmanaged app bundle");
    fs::write(&blocked_parent, b"blocked").expect("must create blocking file");

    let user_target = blocked_parent.join(app_name);
    let (selected, warnings) =
        select_macos_registration_destination([system_target.clone(), user_target], &[]);

    assert!(selected.is_none(), "unmanaged target must be skipped");
    assert!(warnings
        .iter()
        .any(|warning| { warning.contains("refusing to overwrite unmanaged macOS app bundle") }));

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn macos_registration_destination_allows_existing_target_when_previously_managed() {
    let layout = test_layout();
    let root = layout.prefix().join("macos-destination-test");
    let app_name = "Demo.app";
    let system_target = root.join("system-applications").join(app_name);
    let user_target = root.join("user-applications").join(app_name);

    fs::create_dir_all(system_target.parent().expect("must have parent"))
        .expect("must create system parent");
    fs::create_dir_all(&system_target).expect("must seed managed app bundle");

    let previous_records = [GuiNativeRegistrationRecord {
        key: "app:dev.demo.App".to_string(),
        kind: "applications-symlink".to_string(),
        path: system_target.display().to_string(),
    }];

    let (selected, warnings) = select_macos_registration_destination(
        [system_target.clone(), user_target],
        &previous_records,
    );

    assert_eq!(selected.as_ref(), Some(&system_target));
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn macos_registration_destination_falls_back_to_user_when_system_write_fails() {
    let layout = test_layout();
    let root = layout.prefix().join("macos-destination-test");
    let app_name = "Demo.app";
    let source_path = root.join("staged").join(app_name);
    let system_target = root.join("system-applications").join(app_name);
    let user_target = root.join("user-applications").join(app_name);

    fs::create_dir_all(source_path.parent().expect("must have parent"))
        .expect("must create source parent");
    fs::write(&source_path, b"demo-app").expect("must seed source bundle path");

    let mut attempts = Vec::new();
    let (selected, warnings) = register_macos_application_symlink_with_creator(
        &source_path,
        [system_target.clone(), user_target.clone()],
        &[],
        |source, destination| {
            attempts.push(destination.to_path_buf());
            if destination == system_target.as_path() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "simulated permission denied",
                ));
            }
            let _ = source;
            fs::write(destination, b"simulated-link")
        },
    );

    assert_eq!(selected.as_ref(), Some(&user_target));
    assert_eq!(attempts, vec![system_target, user_target.clone()]);
    assert!(warnings
        .iter()
        .any(|warning| warning.contains("simulated permission denied")));
    assert!(
        user_target.exists(),
        "fallback destination should be written"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn macos_registration_destination_fallback_respects_unmanaged_overwrite_guard() {
    let layout = test_layout();
    let root = layout.prefix().join("macos-destination-test");
    let app_name = "Demo.app";
    let source_path = root.join("staged").join(app_name);
    let system_target = root.join("system-applications").join(app_name);
    let user_target = root.join("user-applications").join(app_name);

    fs::create_dir_all(source_path.parent().expect("must have parent"))
        .expect("must create source parent");
    fs::write(&source_path, b"demo-app").expect("must seed source bundle path");
    fs::create_dir_all(&user_target).expect("must seed unmanaged fallback target");

    let mut attempts = Vec::new();
    let (selected, warnings) = register_macos_application_symlink_with_creator(
        &source_path,
        [system_target.clone(), user_target],
        &[],
        |_source, destination| {
            attempts.push(destination.to_path_buf());
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "simulated permission denied",
            ))
        },
    );

    assert!(selected.is_none(), "fallback should skip unmanaged target");
    assert_eq!(attempts, vec![system_target]);
    assert!(warnings
        .iter()
        .any(|warning| warning.contains("simulated permission denied")));
    assert!(warnings
        .iter()
        .any(|warning| { warning.contains("refusing to overwrite unmanaged macOS app bundle") }));

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn register_native_gui_returns_warnings_without_error_on_command_failure() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let install_root = layout.package_dir("demo", "1.0.0");
    fs::create_dir_all(&install_root).expect("must create install root");
    fs::write(install_root.join("demo"), b"#!/bin/sh\n").expect("must write executable");

    let app = ArtifactGuiApp {
        app_id: "dev.demo.App".to_string(),
        display_name: "Demo".to_string(),
        exec: "demo".to_string(),
        icon: None,
        categories: vec!["Utility".to_string()],
        file_associations: Vec::new(),
        protocols: vec![crosspack_core::ArtifactGuiProtocol {
            scheme: "demo".to_string(),
        }],
    };

    let (_records, warnings) = register_native_gui_app_best_effort_with_executor(
        "demo",
        &app,
        &install_root,
        &[],
        |_command, _context| Err(anyhow!("simulated command failure")),
    )
    .expect("command failures should become warnings");

    assert!(
        !warnings.is_empty(),
        "native registration failures should produce warning output"
    );
    assert!(
        warnings.iter().any(|warning| {
            warning.contains("simulated command failure")
                || warning.contains("native GUI registration warning")
        }),
        "expected command-failure or adapter warning line"
    );
}

#[test]
fn remove_package_native_gui_registrations_preserves_state_when_cleanup_warns() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    write_gui_native_state(
        &layout,
        "demo",
        &[GuiNativeRegistrationRecord {
            key: "app:demo".to_string(),
            kind: "unknown-kind".to_string(),
            path: "/tmp/demo".to_string(),
        }],
    )
    .expect("must seed native state");

    let warnings = remove_package_native_gui_registrations_best_effort(&layout, "demo")
        .expect("must remove native registrations");
    assert!(!warnings.is_empty());
    assert!(layout.gui_native_state_path("demo").exists());
}

#[test]
fn strip_components_behavior() {
    let p = Path::new("top/inner/bin/tool");
    assert_eq!(
        strip_rel_components(p, 1).expect("must exist"),
        Path::new("inner/bin/tool")
    );
    assert!(strip_rel_components(p, 4).is_none());
}

#[test]
fn install_from_artifact_rejects_native_installer_when_escalation_policy_forbids_it() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let artifact_path = layout.prefix().join("demo.exe");
    fs::write(&artifact_path, b"dummy exe").expect("must write artifact");

    let err = install_from_artifact(
        &layout,
        "demo",
        "1.0.0",
        &artifact_path,
        ArchiveType::Exe,
        ArtifactInstallOptions {
            strip_components: 0,
            artifact_root: None,
            install_mode: InstallMode::Native,
            interaction_policy: InstallInteractionPolicy {
                allow_prompt_escalation: false,
                allow_non_prompt_escalation: false,
            },
        },
    )
    .expect_err("native installer should be blocked when escalation is disallowed");

    assert!(
        err.to_string()
            .contains("native installer mode requires escalation but policy forbids it"),
        "unexpected error: {err}"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[cfg(not(windows))]
#[test]
fn install_from_artifact_rejects_msi_on_non_windows_host() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let artifact_path = layout.prefix().join("demo.msi");
    fs::write(&artifact_path, b"dummy msi").expect("must write artifact");

    let err = install_from_artifact(
        &layout,
        "demo",
        "1.0.0",
        &artifact_path,
        ArchiveType::Msi,
        ArtifactInstallOptions {
            strip_components: 0,
            artifact_root: None,
            install_mode: InstallMode::Native,
            interaction_policy: InstallInteractionPolicy::default(),
        },
    )
    .expect_err("msi should be rejected on non-Windows host");
    assert!(
        err.to_string()
            .contains("MSI artifacts are supported only on Windows hosts"),
        "unexpected error: {err}"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[cfg(not(target_os = "macos"))]
#[test]
fn install_from_artifact_rejects_dmg_on_non_macos_host() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let artifact_path = layout.prefix().join("demo.dmg");
    fs::write(&artifact_path, b"dummy dmg").expect("must write artifact");

    let err = install_from_artifact(
        &layout,
        "demo",
        "1.0.0",
        &artifact_path,
        ArchiveType::Dmg,
        ArtifactInstallOptions {
            strip_components: 0,
            artifact_root: None,
            install_mode: InstallMode::Managed,
            interaction_policy: InstallInteractionPolicy::default(),
        },
    )
    .expect_err("dmg should be rejected on non-macOS host");
    assert!(
        err.to_string()
            .contains("DMG artifacts are supported only on macOS hosts"),
        "unexpected error: {err}"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[cfg(not(windows))]
#[test]
fn install_from_artifact_rejects_exe_on_non_windows_host() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let artifact_path = layout.prefix().join("demo.exe");
    fs::write(&artifact_path, b"dummy exe").expect("must write artifact");

    let err = install_from_artifact(
        &layout,
        "demo",
        "1.0.0",
        &artifact_path,
        ArchiveType::Exe,
        ArtifactInstallOptions {
            strip_components: 0,
            artifact_root: None,
            install_mode: InstallMode::Native,
            interaction_policy: InstallInteractionPolicy::default(),
        },
    )
    .expect_err("exe should be rejected on non-Windows host");
    assert!(
        err.to_string()
            .contains("EXE artifacts are supported only on Windows hosts"),
        "unexpected error: {err}"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[cfg(not(target_os = "macos"))]
#[test]
fn install_from_artifact_rejects_pkg_on_non_macos_host() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let artifact_path = layout.prefix().join("demo.pkg");
    fs::write(&artifact_path, b"dummy pkg").expect("must write artifact");

    let err = install_from_artifact(
        &layout,
        "demo",
        "1.0.0",
        &artifact_path,
        ArchiveType::Pkg,
        ArtifactInstallOptions {
            strip_components: 0,
            artifact_root: None,
            install_mode: InstallMode::Native,
            interaction_policy: InstallInteractionPolicy::default(),
        },
    )
    .expect_err("pkg should be rejected on non-macOS host");
    assert!(
        err.to_string()
            .contains("PKG artifacts are supported only on macOS hosts"),
        "unexpected error: {err}"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn installer_dispatch_does_not_support_deb_or_rpm_archive_types() {
    assert!(
        ArchiveType::parse("deb").is_none(),
        "DEB should be unsupported in installer dispatch"
    );
    assert!(
        ArchiveType::parse("rpm").is_none(),
        "RPM should be unsupported in installer dispatch"
    );
    assert!(
        ArchiveType::infer_from_url("https://example.test/demo.deb").is_none(),
        "DEB URL inference should be unsupported in installer dispatch"
    );
    assert!(
        ArchiveType::infer_from_url("https://example.test/demo.rpm").is_none(),
        "RPM URL inference should be unsupported in installer dispatch"
    );
}

#[cfg(not(windows))]
#[test]
fn install_from_artifact_rejects_msix_on_non_windows_host() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let artifact_path = layout.prefix().join("demo.msix");
    fs::write(&artifact_path, b"dummy msix").expect("must write artifact");

    let err = install_from_artifact(
        &layout,
        "demo",
        "1.0.0",
        &artifact_path,
        ArchiveType::Msix,
        ArtifactInstallOptions {
            strip_components: 0,
            artifact_root: None,
            install_mode: InstallMode::Native,
            interaction_policy: InstallInteractionPolicy::default(),
        },
    )
    .expect_err("msix should be rejected on non-Windows host");
    assert!(
        err.to_string()
            .contains("MSIX artifacts are supported only on Windows hosts"),
        "unexpected error: {err}"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[cfg(not(windows))]
#[test]
fn install_from_artifact_rejects_appx_on_non_windows_host() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let artifact_path = layout.prefix().join("demo.appx");
    fs::write(&artifact_path, b"dummy appx").expect("must write artifact");

    let err = install_from_artifact(
        &layout,
        "demo",
        "1.0.0",
        &artifact_path,
        ArchiveType::Appx,
        ArtifactInstallOptions {
            strip_components: 0,
            artifact_root: None,
            install_mode: InstallMode::Native,
            interaction_policy: InstallInteractionPolicy::default(),
        },
    )
    .expect_err("appx should be rejected on non-Windows host");
    assert!(
        err.to_string()
            .contains("APPX artifacts are supported only on Windows hosts"),
        "unexpected error: {err}"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[cfg(windows)]
#[test]
fn install_from_artifact_reports_exe_extraction_failure_on_windows_host() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let artifact_path = layout.prefix().join("demo.exe");
    fs::write(&artifact_path, b"dummy exe").expect("must write artifact");

    let err = install_from_artifact(
        &layout,
        "demo",
        "1.0.0",
        &artifact_path,
        ArchiveType::Exe,
        ArtifactInstallOptions {
            strip_components: 0,
            artifact_root: None,
            install_mode: InstallMode::Native,
            interaction_policy: InstallInteractionPolicy::default(),
        },
    )
    .expect_err("exe staging should fail deterministic extraction on Windows host");
    assert!(
        err.to_string()
            .contains("failed to stage EXE artifact via deterministic extraction"),
        "unexpected error: {err}"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[cfg(target_os = "macos")]
#[test]
fn install_from_artifact_reports_pkg_extraction_failure_on_macos_host() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let artifact_path = layout.prefix().join("demo.pkg");
    fs::write(&artifact_path, b"dummy pkg").expect("must write artifact");

    let err = install_from_artifact(
        &layout,
        "demo",
        "1.0.0",
        &artifact_path,
        ArchiveType::Pkg,
        ArtifactInstallOptions {
            strip_components: 0,
            artifact_root: None,
            install_mode: InstallMode::Native,
            interaction_policy: InstallInteractionPolicy::default(),
        },
    )
    .expect_err("pkg staging should fail deterministic extraction on macOS host");
    assert!(
        err.to_string()
            .contains("failed to stage PKG artifact via deterministic extraction"),
        "unexpected error: {err}"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[cfg(windows)]
#[test]
fn install_from_artifact_reports_msix_extraction_failure_on_windows_host() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let artifact_path = layout.prefix().join("demo.msix");
    fs::write(&artifact_path, b"dummy msix").expect("must write artifact");

    let err = install_from_artifact(
        &layout,
        "demo",
        "1.0.0",
        &artifact_path,
        ArchiveType::Msix,
        ArtifactInstallOptions {
            strip_components: 0,
            artifact_root: None,
            install_mode: InstallMode::Native,
            interaction_policy: InstallInteractionPolicy::default(),
        },
    )
    .expect_err("msix staging should fail deterministic extraction on Windows host");
    assert!(
        err.to_string()
            .contains("failed to stage MSIX artifact via deterministic extraction"),
        "unexpected error: {err}"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[cfg(windows)]
#[test]
fn install_from_artifact_reports_appx_extraction_failure_on_windows_host() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let artifact_path = layout.prefix().join("demo.appx");
    fs::write(&artifact_path, b"dummy appx").expect("must write artifact");

    let err = install_from_artifact(
        &layout,
        "demo",
        "1.0.0",
        &artifact_path,
        ArchiveType::Appx,
        ArtifactInstallOptions {
            strip_components: 0,
            artifact_root: None,
            install_mode: InstallMode::Native,
            interaction_policy: InstallInteractionPolicy::default(),
        },
    )
    .expect_err("appx staging should fail deterministic extraction on Windows host");
    assert!(
        err.to_string()
            .contains("failed to stage APPX artifact via deterministic extraction"),
        "unexpected error: {err}"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[cfg(target_os = "linux")]
#[test]
fn install_from_artifact_rejects_appimage_with_strip_components() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let artifact_path = layout.prefix().join("demo.AppImage");
    fs::write(&artifact_path, b"dummy appimage").expect("must write artifact");

    let err = install_from_artifact(
        &layout,
        "demo",
        "1.0.0",
        &artifact_path,
        ArchiveType::AppImage,
        ArtifactInstallOptions {
            strip_components: 1,
            artifact_root: None,
            install_mode: InstallMode::Managed,
            interaction_policy: InstallInteractionPolicy::default(),
        },
    )
    .expect_err("appimage strip_components should be rejected");
    assert!(
        err.to_string()
            .contains("strip_components must be 0 for AppImage artifacts"),
        "unexpected error: {err}"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[cfg(not(target_os = "linux"))]
#[test]
fn install_from_artifact_rejects_appimage_on_non_linux_host() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let artifact_path = layout.prefix().join("demo.AppImage");
    fs::write(&artifact_path, b"dummy appimage").expect("must write artifact");

    let err = install_from_artifact(
        &layout,
        "demo",
        "1.0.0",
        &artifact_path,
        ArchiveType::AppImage,
        ArtifactInstallOptions {
            strip_components: 0,
            artifact_root: None,
            install_mode: InstallMode::Managed,
            interaction_policy: InstallInteractionPolicy::default(),
        },
    )
    .expect_err("appimage installs should be rejected on non-Linux hosts");
    assert!(
        err.to_string()
            .contains("AppImage artifacts are supported only on Linux hosts"),
        "unexpected error: {err}"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[cfg(target_os = "linux")]
#[test]
fn stage_appimage_copies_payload_into_raw_dir() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let artifact_path = layout.prefix().join("demo.AppImage");
    fs::write(&artifact_path, b"appimage payload").expect("must write artifact");
    let raw_dir = layout.prefix().join("raw");
    fs::create_dir_all(&raw_dir).expect("must create raw dir");

    stage_appimage_payload(&artifact_path, &raw_dir, 0, None).expect("must stage appimage payload");

    let staged = raw_dir.join("artifact.appimage");
    assert!(staged.exists(), "staged payload should exist");
    assert_eq!(
        fs::read(&staged).expect("must read staged payload"),
        b"appimage payload"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[cfg(target_os = "linux")]
#[test]
fn stage_appimage_sets_executable_permissions_on_unix() {
    use std::os::unix::fs::PermissionsExt;

    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let artifact_path = layout.prefix().join("demo.AppImage");
    fs::write(&artifact_path, b"appimage payload").expect("must write artifact");
    let raw_dir = layout.prefix().join("raw");
    fs::create_dir_all(&raw_dir).expect("must create raw dir");

    stage_appimage_payload(&artifact_path, &raw_dir, 0, None).expect("must stage appimage payload");

    let mode = fs::metadata(raw_dir.join("artifact.appimage"))
        .expect("must stat staged payload")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o755);

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn install_from_artifact_rejects_bin_with_strip_components() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let artifact_path = layout.prefix().join("demo.bin");
    fs::write(&artifact_path, b"dummy bin").expect("must write artifact");

    let err = install_from_artifact(
        &layout,
        "demo",
        "1.0.0",
        &artifact_path,
        ArchiveType::Bin,
        ArtifactInstallOptions {
            strip_components: 1,
            artifact_root: None,
            install_mode: InstallMode::Managed,
            interaction_policy: InstallInteractionPolicy::default(),
        },
    )
    .expect_err("bin strip_components should be rejected");
    assert!(
        err.to_string()
            .contains("strip_components must be 0 for bin artifacts"),
        "unexpected error: {err}"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn stage_bin_copies_payload_into_raw_dir() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let artifact_path = layout.prefix().join("demo.bin");
    fs::write(&artifact_path, b"bin payload").expect("must write artifact");
    let raw_dir = layout.prefix().join("raw");
    fs::create_dir_all(&raw_dir).expect("must create raw dir");

    stage_bin_payload(&artifact_path, &raw_dir, 0, None).expect("must stage bin payload");

    let staged = raw_dir.join("demo.bin");
    assert!(staged.exists(), "staged payload should exist");
    assert_eq!(
        fs::read(&staged).expect("must read staged payload"),
        b"bin payload"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[cfg(unix)]
#[test]
fn stage_bin_sets_executable_permissions_on_unix() {
    use std::os::unix::fs::PermissionsExt;

    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");
    let artifact_path = layout.prefix().join("demo.bin");
    fs::write(&artifact_path, b"bin payload").expect("must write artifact");
    let raw_dir = layout.prefix().join("raw");
    fs::create_dir_all(&raw_dir).expect("must create raw dir");

    stage_bin_payload(&artifact_path, &raw_dir, 0, None).expect("must stage bin payload");

    let mode = fs::metadata(raw_dir.join("demo.bin"))
        .expect("must stat staged payload")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o755);

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn stage_msi_builds_admin_extract_command() {
    let artifact_path = Path::new("/tmp/demo.msi");
    let raw_dir = Path::new("/tmp/raw");
    let command = build_msi_admin_extract_command(artifact_path, raw_dir);

    assert_eq!(command.get_program(), "msiexec");
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        args,
        vec![
            "/a".to_string(),
            artifact_path.display().to_string(),
            "/qn".to_string(),
            format!("TARGETDIR={}", raw_dir.display())
        ]
    );
}

#[test]
fn stage_exe_builds_extract_command_shape() {
    let artifact_path = Path::new("C:/tmp/demo.exe");
    let raw_dir = Path::new("C:/tmp/raw");
    let command = build_exe_extract_command(artifact_path, raw_dir);

    assert_eq!(command.get_program(), "7z");
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        args,
        vec![
            "x".to_string(),
            artifact_path.display().to_string(),
            format!("-o{}", raw_dir.display()),
            "-y".to_string(),
        ]
    );
}

#[test]
fn stage_msix_builds_unpack_command_shape() {
    let artifact_path = Path::new("C:/tmp/demo.msix");
    let raw_dir = Path::new("C:/tmp/raw-msix");
    let command = build_msix_unpack_command(artifact_path, raw_dir);

    assert_eq!(command.get_program(), "makeappx");
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        args,
        vec![
            "unpack".to_string(),
            "/p".to_string(),
            artifact_path.display().to_string(),
            "/d".to_string(),
            raw_dir.display().to_string(),
            "/o".to_string(),
        ]
    );
}

#[test]
fn stage_appx_builds_unpack_command_shape() {
    let artifact_path = Path::new("C:/tmp/demo.appx");
    let raw_dir = Path::new("C:/tmp/raw-appx");
    let command = build_appx_unpack_command(artifact_path, raw_dir);

    assert_eq!(command.get_program(), "makeappx");
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        args,
        vec![
            "unpack".to_string(),
            "/p".to_string(),
            artifact_path.display().to_string(),
            "/d".to_string(),
            raw_dir.display().to_string(),
            "/o".to_string(),
        ]
    );
}

#[test]
fn stage_msix_payload_with_runner_invokes_expected_command_context() {
    let artifact_path = Path::new("C:/tmp/demo.msix");
    let raw_dir = Path::new("C:/tmp/raw-msix");
    let mut observed_program = String::new();
    let mut observed_args = Vec::new();
    let mut observed_context = String::new();

    stage_msix_payload_with_runner(artifact_path, raw_dir, |command, context| {
        observed_program = command.get_program().to_string_lossy().into_owned();
        observed_args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        observed_context = context.to_string();
        Ok(())
    })
    .expect("runner should succeed");

    assert_eq!(
        observed_context,
        "failed to stage MSIX artifact via deterministic extraction"
    );
    assert_eq!(observed_program, "makeappx");
    assert_eq!(
        observed_args,
        vec![
            "unpack".to_string(),
            "/p".to_string(),
            artifact_path.display().to_string(),
            "/d".to_string(),
            raw_dir.display().to_string(),
            "/o".to_string(),
        ]
    );
}

#[test]
fn stage_appx_payload_with_runner_invokes_expected_command_context() {
    let artifact_path = Path::new("C:/tmp/demo.appx");
    let raw_dir = Path::new("C:/tmp/raw-appx");
    let mut observed_program = String::new();
    let mut observed_args = Vec::new();
    let mut observed_context = String::new();

    stage_appx_payload_with_runner(artifact_path, raw_dir, |command, context| {
        observed_program = command.get_program().to_string_lossy().into_owned();
        observed_args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        observed_context = context.to_string();
        Ok(())
    })
    .expect("runner should succeed");

    assert_eq!(
        observed_context,
        "failed to stage APPX artifact via deterministic extraction"
    );
    assert_eq!(observed_program, "makeappx");
    assert_eq!(
        observed_args,
        vec![
            "unpack".to_string(),
            "/p".to_string(),
            artifact_path.display().to_string(),
            "/d".to_string(),
            raw_dir.display().to_string(),
            "/o".to_string(),
        ]
    );
}

#[test]
fn stage_exe_uses_extract_tool_not_installer_execution() {
    let artifact_path = Path::new("C:/tmp/app.exe");
    let raw_dir = Path::new("C:/tmp/raw");
    let command = build_exe_extract_command(artifact_path, raw_dir);

    assert_ne!(command.get_program(), artifact_path.as_os_str());
}

#[test]
fn stage_exe_payload_with_runner_invokes_expected_command_context() {
    let artifact_path = Path::new("C:/tmp/demo.exe");
    let raw_dir = Path::new("C:/tmp/raw");
    let mut observed_program = String::new();
    let mut observed_args = Vec::new();
    let mut observed_context = String::new();

    stage_exe_payload_with_runner(artifact_path, raw_dir, |command, context| {
        observed_program = command.get_program().to_string_lossy().into_owned();
        observed_args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        observed_context = context.to_string();
        Ok(())
    })
    .expect("runner should succeed");

    assert_eq!(
        observed_context,
        "failed to stage EXE artifact via deterministic extraction"
    );
    assert_eq!(observed_program, "7z");
    assert_eq!(
        observed_args,
        vec![
            "x".to_string(),
            artifact_path.display().to_string(),
            format!("-o{}", raw_dir.display()),
            "-y".to_string(),
        ]
    );
}

#[test]
fn stage_exe_returns_actionable_error_when_extraction_fails() {
    let artifact_path = Path::new("C:/tmp/demo.exe");
    let raw_dir = Path::new("C:/tmp/raw");
    let err = stage_exe_payload_with_runner(artifact_path, raw_dir, |_command, _context| {
        Err(anyhow!(io::Error::new(
            io::ErrorKind::NotFound,
            "simulated missing 7z"
        )))
    })
    .expect_err("missing extraction tool should be surfaced with guidance");

    let message = err.to_string();
    assert!(
        message.contains("failed to stage EXE artifact via deterministic extraction"),
        "unexpected error: {message}"
    );
    assert!(
        message.contains("required extraction tool '7z' was not found on PATH"),
        "unexpected error: {message}"
    );
    assert!(
        message.contains("install 7-Zip CLI and ensure '7z' is available, then retry"),
        "unexpected error: {message}"
    );
}

#[test]
fn stage_pkg_builds_expand_command_shape() {
    let artifact_path = Path::new("/tmp/demo.pkg");
    let expanded_dir = Path::new("/tmp/pkg-expanded");
    let command = build_pkg_expand_command(artifact_path, expanded_dir);

    assert_eq!(command.get_program(), "pkgutil");
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        args,
        vec![
            "--expand-full".to_string(),
            artifact_path.display().to_string(),
            expanded_dir.display().to_string(),
        ]
    );
}

#[test]
fn stage_pkg_copy_and_cleanup_command_shapes_are_stable() {
    let expanded_raw_dir = Path::new("/tmp/pkg-expanded/Payload");
    let raw_dir = Path::new("/tmp/raw");

    let copy = build_pkg_copy_command(expanded_raw_dir, raw_dir);
    assert_eq!(copy.get_program(), "ditto");
    let copy_args = copy
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        copy_args,
        vec![
            expanded_raw_dir.display().to_string(),
            raw_dir.display().to_string(),
        ]
    );
}

#[test]
fn stage_pkg_orchestrates_expand_then_copy_then_cleanup() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let artifact_path = layout.prefix().join("demo.pkg");
    fs::write(&artifact_path, b"pkg").expect("must create artifact");
    let raw_dir = layout.prefix().join("raw");
    let expanded_dir = layout.prefix().join("pkg-expanded");
    let mut command_invocations = Vec::new();

    stage_pkg_payload_with_hooks(&artifact_path, &raw_dir, &expanded_dir, |command, _| {
        let mut invocation = command.get_program().to_string_lossy().into_owned();
        for arg in command.get_args() {
            invocation.push(' ');
            invocation.push_str(arg.to_string_lossy().as_ref());
        }
        command_invocations.push(invocation.clone());
        if invocation.starts_with("pkgutil --expand-full ") {
            fs::create_dir_all(expanded_dir.join("Payload"))
                .expect("must create top-level payload root");
        }
        Ok(())
    })
    .expect("stage flow should succeed");

    assert_eq!(command_invocations.len(), 2, "expand + copy must run");
    assert!(command_invocations[0].starts_with("pkgutil --expand-full "));
    assert!(command_invocations[1].starts_with("ditto "));
    assert!(
        !expanded_dir.exists(),
        "expanded dir should be removed during cleanup"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn stage_pkg_cleanup_runs_on_expand_failure() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let artifact_path = layout.prefix().join("demo.pkg");
    fs::write(&artifact_path, b"pkg").expect("must create artifact");
    let raw_dir = layout.prefix().join("raw");
    let expanded_dir = layout.prefix().join("pkg-expanded");
    fs::create_dir_all(&expanded_dir).expect("must seed expanded dir");

    let err = stage_pkg_payload_with_hooks(&artifact_path, &raw_dir, &expanded_dir, |_, _| {
        Err(anyhow!("simulated expand failure"))
    })
    .expect_err("expand failure should propagate");

    assert!(err.to_string().contains("simulated expand failure"));
    assert!(
        !expanded_dir.exists(),
        "expanded dir should be removed during cleanup"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn stage_pkg_payload_discovery_is_deterministic_for_top_level_and_nested_roots() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let expanded_dir = layout.prefix().join("pkg-expanded");
    fs::create_dir_all(expanded_dir.join("Payload")).expect("must create top-level payload");
    fs::create_dir_all(expanded_dir.join("zeta.pkg").join("Payload"))
        .expect("must create nested payload");
    fs::create_dir_all(expanded_dir.join("alpha.pkg").join("Payload"))
        .expect("must create nested payload");
    fs::create_dir_all(expanded_dir.join("ignored")).expect("must create ignored dir");

    let payload_roots =
        discover_pkg_payload_roots(&expanded_dir).expect("must discover payload roots");

    assert_eq!(
        payload_roots,
        vec![
            expanded_dir.join("Payload"),
            expanded_dir.join("alpha.pkg").join("Payload"),
            expanded_dir.join("zeta.pkg").join("Payload"),
        ]
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn stage_pkg_payload_discovery_returns_actionable_error_when_missing() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let expanded_dir = layout.prefix().join("pkg-expanded");
    fs::create_dir_all(&expanded_dir).expect("must create expanded dir");

    let err = discover_pkg_payload_roots(&expanded_dir)
        .expect_err("missing payload roots must return error");
    let message = err.to_string();
    assert!(message.contains("expanded PKG payload not found"));
    assert!(message.contains(expanded_dir.join("Payload").display().to_string().as_str()));

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn stage_pkg_cleanup_runs_on_copy_failure() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let artifact_path = layout.prefix().join("demo.pkg");
    fs::write(&artifact_path, b"pkg").expect("must create artifact");
    let raw_dir = layout.prefix().join("raw");
    let expanded_dir = layout.prefix().join("pkg-expanded");
    let mut command_invocations = Vec::new();

    let err =
        stage_pkg_payload_with_hooks(&artifact_path, &raw_dir, &expanded_dir, |command, _| {
            let mut invocation = command.get_program().to_string_lossy().into_owned();
            for arg in command.get_args() {
                invocation.push(' ');
                invocation.push_str(arg.to_string_lossy().as_ref());
            }
            command_invocations.push(invocation.clone());
            if invocation.starts_with("pkgutil --expand-full ") {
                fs::create_dir_all(expanded_dir.join("Payload"))
                    .expect("must create top-level payload root");
            }
            if invocation.starts_with("ditto ") {
                return Err(anyhow!("simulated copy failure"));
            }
            Ok(())
        })
        .expect_err("copy failure should propagate");

    assert!(err.to_string().contains("simulated copy failure"));
    assert_eq!(command_invocations.len(), 2, "expand + copy must run");
    assert!(
        !expanded_dir.exists(),
        "expanded dir should be removed during cleanup"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn stage_pkg_copies_top_level_then_nested_payloads_in_stable_order() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let artifact_path = layout.prefix().join("demo.pkg");
    fs::write(&artifact_path, b"pkg").expect("must create artifact");
    let raw_dir = layout.prefix().join("raw");
    let expanded_dir = layout.prefix().join("pkg-expanded");
    let mut copy_sources = Vec::new();

    stage_pkg_payload_with_hooks(&artifact_path, &raw_dir, &expanded_dir, |command, _| {
        if command.get_program() == "pkgutil" {
            fs::create_dir_all(expanded_dir.join("Payload"))
                .expect("must create top-level payload root");
            fs::create_dir_all(expanded_dir.join("zeta.pkg").join("Payload"))
                .expect("must create nested payload root");
            fs::create_dir_all(expanded_dir.join("alpha.pkg").join("Payload"))
                .expect("must create nested payload root");
            return Ok(());
        }

        if command.get_program() == "ditto" {
            let args = command.get_args().collect::<Vec<_>>();
            let source = args
                .first()
                .expect("ditto should have a source arg")
                .to_string_lossy()
                .into_owned();
            copy_sources.push(source);
        }
        Ok(())
    })
    .expect("copy flow should succeed");

    assert_eq!(
        copy_sources.len(),
        3,
        "top-level and two nested payload roots should be copied"
    );
    assert_eq!(
        copy_sources,
        vec![
            expanded_dir.join("Payload").display().to_string(),
            expanded_dir
                .join("alpha.pkg")
                .join("Payload")
                .display()
                .to_string(),
            expanded_dir
                .join("zeta.pkg")
                .join("Payload")
                .display()
                .to_string(),
        ]
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn stage_msix_returns_actionable_error_when_makeappx_is_missing() {
    let artifact_path = Path::new("C:/tmp/demo.msix");
    let raw_dir = Path::new("C:/tmp/raw-msix");
    let err = stage_msix_payload_with_runner(artifact_path, raw_dir, |_command, _context| {
        Err(anyhow!(io::Error::new(
            io::ErrorKind::NotFound,
            "simulated missing makeappx"
        )))
    })
    .expect_err("missing makeappx should be surfaced with guidance");

    let message = err.to_string();
    assert!(
        message.contains("failed to stage MSIX artifact via deterministic extraction"),
        "unexpected error: {message}"
    );
    assert!(
        message.contains("required extraction tool 'makeappx' was not found on PATH"),
        "unexpected error: {message}"
    );
    assert!(
        message.contains(
            "install Windows SDK App Certification Kit tools and ensure 'makeappx' is available, then retry"
        ),
        "unexpected error: {message}"
    );
}

#[test]
fn stage_appx_returns_actionable_error_when_makeappx_is_missing() {
    let artifact_path = Path::new("C:/tmp/demo.appx");
    let raw_dir = Path::new("C:/tmp/raw-appx");
    let err = stage_appx_payload_with_runner(artifact_path, raw_dir, |_command, _context| {
        Err(anyhow!(io::Error::new(
            io::ErrorKind::NotFound,
            "simulated missing makeappx"
        )))
    })
    .expect_err("missing makeappx should be surfaced with guidance");

    let message = err.to_string();
    assert!(
        message.contains("failed to stage APPX artifact via deterministic extraction"),
        "unexpected error: {message}"
    );
    assert!(
        message.contains("required extraction tool 'makeappx' was not found on PATH"),
        "unexpected error: {message}"
    );
    assert!(
        message.contains(
            "install Windows SDK App Certification Kit tools and ensure 'makeappx' is available, then retry"
        ),
        "unexpected error: {message}"
    );
}

#[test]
fn stage_dmg_attach_and_detach_command_shapes_are_stable() {
    let artifact_path = Path::new("/tmp/demo.dmg");
    let mount_point = Path::new("/tmp/mount-point");

    let attach = build_dmg_attach_command(artifact_path, mount_point);
    assert_eq!(attach.get_program(), "hdiutil");
    let attach_args = attach
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        attach_args,
        vec![
            "attach".to_string(),
            artifact_path.display().to_string(),
            "-readonly".to_string(),
            "-nobrowse".to_string(),
            "-mountpoint".to_string(),
            mount_point.display().to_string(),
        ]
    );

    let detach = build_dmg_detach_command(mount_point);
    assert_eq!(detach.get_program(), "hdiutil");
    let detach_args = detach
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        detach_args,
        vec!["detach".to_string(), mount_point.display().to_string()]
    );
}

#[test]
fn stage_dmg_detach_runs_on_copy_failure() {
    let artifact_path = Path::new("/tmp/demo.dmg");
    let raw_dir = Path::new("/tmp/raw");
    let mount_point = Path::new("/tmp/mount-point");
    let mut command_invocations = Vec::new();

    let err = stage_dmg_payload_with_hooks(
        artifact_path,
        raw_dir,
        mount_point,
        |command, _context| {
            let mut invocation = command.get_program().to_string_lossy().into_owned();
            for arg in command.get_args() {
                invocation.push(' ');
                invocation.push_str(arg.to_string_lossy().as_ref());
            }
            command_invocations.push(invocation);
            Ok(())
        },
        |_mounted, _dst| Err(anyhow!("simulated copy failure")),
    )
    .expect_err("copy failure should propagate");

    assert!(err.to_string().contains("simulated copy failure"));
    assert_eq!(command_invocations.len(), 2, "attach + detach must run");
    assert!(command_invocations[0].starts_with("hdiutil attach "));
    assert!(command_invocations[1].starts_with("hdiutil detach "));
}

#[test]
#[cfg(unix)]
fn copy_dmg_payload_skips_root_applications_symlink() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let mount_point = layout.prefix().join("mount-point");
    let raw_dir = layout.prefix().join("raw");
    fs::create_dir_all(&mount_point).expect("must create mount point");

    let app_binary = mount_point.join("Demo.app/Contents/MacOS/demo");
    fs::create_dir_all(app_binary.parent().expect("must have parent"))
        .expect("must create app bundle dirs");
    fs::write(&app_binary, b"#!/bin/sh\n").expect("must write app binary");

    let nested_dir = mount_point.join("nested");
    fs::create_dir_all(&nested_dir).expect("must create nested payload dir");

    std::os::unix::fs::symlink(Path::new("/Applications"), mount_point.join("Applications"))
        .expect("must create root Applications symlink");
    std::os::unix::fs::symlink(Path::new("../Demo.app"), nested_dir.join("Applications"))
        .expect("must create nested Applications symlink");

    copy_dmg_payload(&mount_point, &raw_dir).expect("must copy DMG payload");

    match fs::symlink_metadata(raw_dir.join("Applications")) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Ok(_) => panic!("root Applications symlink should be skipped"),
        Err(err) => panic!("unexpected root Applications metadata error: {err}"),
    }

    let copied_binary = raw_dir.join("Demo.app/Contents/MacOS/demo");
    assert!(copied_binary.exists(), "expected app bundle to be copied");

    let nested_symlink = raw_dir.join("nested/Applications");
    let nested_metadata =
        fs::symlink_metadata(&nested_symlink).expect("nested Applications entry should exist");
    assert!(
        nested_metadata.file_type().is_symlink(),
        "nested Applications symlink should be preserved"
    );
    assert_eq!(
        fs::read_link(&nested_symlink).expect("must read nested symlink target"),
        PathBuf::from("../Demo.app")
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn uninstall_removes_package_dir_and_receipt() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let package_dir = layout.package_dir("demo", "1.0.0");
    fs::create_dir_all(&package_dir).expect("must create package dir");
    fs::write(package_dir.join("demo.txt"), b"hello").expect("must create package file");
    let completion_rel_path = "packages/bash/demo--completions--demo.bash".to_string();
    let completion_path = exposed_completion_path(&layout, &completion_rel_path)
        .expect("must resolve completion storage path");
    fs::create_dir_all(
        completion_path
            .parent()
            .expect("completion path must have parent"),
    )
    .expect("must create completion parent dir");
    fs::write(&completion_path, b"# demo completion\n").expect("must create completion file");
    let gui_rel_path = "launchers/demo--demo.command".to_string();
    let gui_path = gui_asset_path(&layout, &gui_rel_path).expect("must resolve gui path");
    fs::create_dir_all(gui_path.parent().expect("gui path must have parent"))
        .expect("must create gui parent dir");
    fs::write(&gui_path, b"#!/bin/sh\n").expect("must create gui launcher file");
    write_gui_exposure_state(
        &layout,
        "demo",
        &[GuiExposureAsset {
            key: "app:demo".to_string(),
            rel_path: gui_rel_path,
        }],
    )
    .expect("must write gui state");
    let native_launcher = layout.prefix().join("native-demo.desktop");
    fs::write(&native_launcher, b"[Desktop Entry]\n").expect("must write native launcher");
    write_gui_native_state(
        &layout,
        "demo",
        &[GuiNativeRegistrationRecord {
            key: "app:demo".to_string(),
            kind: "desktop-entry".to_string(),
            path: native_launcher.display().to_string(),
        }],
    )
    .expect("must write native gui state");

    write_install_receipt(
        &layout,
        &InstallReceipt {
            name: "demo".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: Vec::new(),
            exposed_completions: vec![completion_rel_path],
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        },
    )
    .expect("must write receipt");

    let result = uninstall_package(&layout, "demo").expect("must uninstall");
    assert_eq!(result.status, UninstallStatus::Uninstalled);
    assert_eq!(result.version.as_deref(), Some("1.0.0"));
    assert!(!layout.receipt_path("demo").exists());
    assert!(!package_dir.exists());
    assert!(!completion_path.exists());
    assert!(!gui_path.exists());
    assert!(!native_launcher.exists());
    assert!(!layout.gui_state_path("demo").exists());
    assert!(!layout.gui_native_state_path("demo").exists());

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn uninstall_native_runs_native_uninstall_before_managed_cleanup() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let package_dir = layout.package_dir("demo", "1.0.0");
    fs::create_dir_all(&package_dir).expect("must create package dir");
    fs::write(package_dir.join("demo.txt"), b"hello").expect("must create package file");

    write_native_sidecar_state(
        &layout,
        "demo",
        &NativeSidecarState {
            uninstall_actions: vec![NativeUninstallAction {
                key: "app:demo".to_string(),
                kind: "desktop-entry".to_string(),
                path: package_dir.display().to_string(),
            }],
        },
    )
    .expect("must write native sidecar state");

    write_install_receipt(
        &layout,
        &InstallReceipt {
            name: "demo".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: Vec::new(),
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Native,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        },
    )
    .expect("must write receipt");

    let err = uninstall_package(&layout, "demo")
        .expect_err("native uninstall action should run before managed cleanup");
    assert!(
        err.to_string().contains("native uninstall action failed"),
        "unexpected error: {err}"
    );
    assert!(
        package_dir.exists(),
        "managed cleanup should not remove package dir after native action failure"
    );
    assert!(
        layout.receipt_path("demo").exists(),
        "managed cleanup should not remove receipt after native action failure"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn uninstall_native_treats_not_found_as_idempotent_success() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let package_dir = layout.package_dir("demo", "1.0.0");
    fs::create_dir_all(&package_dir).expect("must create package dir");
    fs::write(package_dir.join("demo.txt"), b"hello").expect("must create package file");

    let missing_native_path = layout.prefix().join("already-removed.desktop");
    write_native_sidecar_state(
        &layout,
        "demo",
        &NativeSidecarState {
            uninstall_actions: vec![NativeUninstallAction {
                key: "app:demo".to_string(),
                kind: "desktop-entry".to_string(),
                path: missing_native_path.display().to_string(),
            }],
        },
    )
    .expect("must write native sidecar state");

    write_install_receipt(
        &layout,
        &InstallReceipt {
            name: "demo".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: Vec::new(),
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Native,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        },
    )
    .expect("must write receipt");

    let result = uninstall_package(&layout, "demo")
        .expect("missing native uninstall action target should be idempotent success");
    assert_eq!(result.status, UninstallStatus::Uninstalled);
    assert!(!package_dir.exists());
    assert!(!layout.receipt_path("demo").exists());
    assert!(
        !layout.gui_native_state_path("demo").exists(),
        "managed cleanup should clear sidecar state"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn uninstall_native_removes_bundle_copy_records_recursively() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let package_dir = layout.package_dir("demo", "1.0.0");
    fs::create_dir_all(&package_dir).expect("must create package dir");
    fs::write(package_dir.join("demo.txt"), b"hello").expect("must create package file");

    let copied_bundle = layout.prefix().join("Applications").join("Demo.app");
    let copied_binary = copied_bundle.join("Contents").join("MacOS").join("demo");
    fs::create_dir_all(copied_binary.parent().expect("must have parent"))
        .expect("must create copied bundle dirs");
    fs::write(&copied_binary, b"#!/bin/sh\n").expect("must create copied bundle binary");

    write_native_sidecar_state(
        &layout,
        "demo",
        &NativeSidecarState {
            uninstall_actions: vec![NativeUninstallAction {
                key: "app:demo".to_string(),
                kind: "applications-bundle-copy".to_string(),
                path: copied_bundle.display().to_string(),
            }],
        },
    )
    .expect("must write native sidecar state");

    write_install_receipt(
        &layout,
        &InstallReceipt {
            name: "demo".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: Vec::new(),
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Native,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        },
    )
    .expect("must write receipt");

    let result = uninstall_package(&layout, "demo")
        .expect("bundle-copy native uninstall action should be removed recursively");
    assert_eq!(result.status, UninstallStatus::Uninstalled);
    assert!(!copied_bundle.exists());
    assert!(!package_dir.exists());
    assert!(!layout.receipt_path("demo").exists());

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn uninstall_native_legacy_applications_symlink_kind_preserves_app_bundle_directory() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let package_dir = layout.package_dir("demo", "1.0.0");
    fs::create_dir_all(&package_dir).expect("must create package dir");
    fs::write(package_dir.join("demo.txt"), b"hello").expect("must create package file");

    let legacy_bundle = layout.prefix().join("Applications").join("LegacyDemo.app");
    let legacy_bundle_binary = legacy_bundle.join("Contents").join("MacOS").join("demo");
    fs::create_dir_all(legacy_bundle_binary.parent().expect("must have parent"))
        .expect("must create legacy bundle dirs");
    fs::write(&legacy_bundle_binary, b"#!/bin/sh\n").expect("must create legacy bundle binary");

    write_native_sidecar_state(
        &layout,
        "demo",
        &NativeSidecarState {
            uninstall_actions: vec![NativeUninstallAction {
                key: "app:demo".to_string(),
                kind: "applications-symlink".to_string(),
                path: legacy_bundle.display().to_string(),
            }],
        },
    )
    .expect("must write native sidecar state");

    write_install_receipt(
        &layout,
        &InstallReceipt {
            name: "demo".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: Vec::new(),
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Native,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        },
    )
    .expect("must write receipt");

    let result = uninstall_package(&layout, "demo")
        .expect("legacy applications-symlink uninstall action should skip app bundle dirs");
    assert_eq!(result.status, UninstallStatus::Uninstalled);
    assert!(legacy_bundle.exists());
    assert!(!package_dir.exists());
    assert!(!layout.receipt_path("demo").exists());

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn uninstall_native_applications_symlink_kind_behavior_unchanged() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let package_dir = layout.package_dir("demo", "1.0.0");
    fs::create_dir_all(&package_dir).expect("must create package dir");
    fs::write(package_dir.join("demo.txt"), b"hello").expect("must create package file");

    let symlink_like_path = layout.prefix().join("Demo.app-link");
    fs::write(&symlink_like_path, b"simulated symlink file").expect("must write symlink-like path");
    write_native_sidecar_state(
        &layout,
        "demo",
        &NativeSidecarState {
            uninstall_actions: vec![NativeUninstallAction {
                key: "app:demo".to_string(),
                kind: "applications-symlink".to_string(),
                path: symlink_like_path.display().to_string(),
            }],
        },
    )
    .expect("must write native sidecar state");

    write_install_receipt(
        &layout,
        &InstallReceipt {
            name: "demo".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: Vec::new(),
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Native,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        },
    )
    .expect("must write receipt");

    let result = uninstall_package(&layout, "demo")
        .expect("applications-symlink native uninstall action should still succeed");
    assert_eq!(result.status, UninstallStatus::Uninstalled);
    assert!(!symlink_like_path.exists());
    assert!(!package_dir.exists());
    assert!(!layout.receipt_path("demo").exists());

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn uninstall_native_stale_cleanup_handles_bundle_copy_and_symlink_kinds() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let package_dir = layout.package_dir("demo", "1.0.0");
    fs::create_dir_all(&package_dir).expect("must create package dir");
    fs::write(package_dir.join("demo.txt"), b"hello").expect("must create package file");

    let stale_symlink_like_path = layout.prefix().join("stale-demo-link");
    fs::write(&stale_symlink_like_path, b"simulated symlink")
        .expect("must create stale symlink-like path");
    let stale_bundle_copy = layout.prefix().join("stale-applications").join("Demo.app");
    let stale_bundle_binary = stale_bundle_copy
        .join("Contents")
        .join("MacOS")
        .join("demo");
    fs::create_dir_all(stale_bundle_binary.parent().expect("must have parent"))
        .expect("must create stale bundle dirs");
    fs::write(&stale_bundle_binary, b"#!/bin/sh\n").expect("must create stale bundle binary");

    write_gui_native_state(
        &layout,
        "demo",
        &[
            GuiNativeRegistrationRecord {
                key: "app:demo".to_string(),
                kind: "applications-symlink".to_string(),
                path: stale_symlink_like_path.display().to_string(),
            },
            GuiNativeRegistrationRecord {
                key: "app:demo-bundle".to_string(),
                kind: "applications-bundle-copy".to_string(),
                path: stale_bundle_copy.display().to_string(),
            },
        ],
    )
    .expect("must write stale native gui state");

    write_install_receipt(
        &layout,
        &InstallReceipt {
            name: "demo".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: Vec::new(),
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        },
    )
    .expect("must write receipt");

    let result = uninstall_package(&layout, "demo")
        .expect("stale native cleanup should handle both bundle-copy and symlink kinds");
    assert_eq!(result.status, UninstallStatus::Uninstalled);
    assert!(!stale_symlink_like_path.exists());
    assert!(!stale_bundle_copy.exists());
    assert!(!layout.gui_native_state_path("demo").exists());

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn uninstall_native_failure_reports_action_context() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let package_dir = layout.package_dir("demo", "1.0.0");
    fs::create_dir_all(&package_dir).expect("must create package dir");
    fs::write(package_dir.join("demo.txt"), b"hello").expect("must create package file");

    let action = NativeUninstallAction {
        key: "protocol:demo".to_string(),
        kind: "unsupported-kind".to_string(),
        path: "/tmp/demo-protocol".to_string(),
    };
    write_native_sidecar_state(
        &layout,
        "demo",
        &NativeSidecarState {
            uninstall_actions: vec![action.clone()],
        },
    )
    .expect("must write native sidecar state");

    write_install_receipt(
        &layout,
        &InstallReceipt {
            name: "demo".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: Vec::new(),
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Native,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        },
    )
    .expect("must write receipt");

    let err = uninstall_package(&layout, "demo")
        .expect_err("unsupported native uninstall action kind should fail uninstall");
    let message = err.to_string();
    assert!(
        message.contains("native uninstall action failed"),
        "unexpected error: {message}"
    );
    assert!(
        message.contains(&action.key),
        "error should include action key: {message}"
    );
    assert!(
        message.contains(&action.kind),
        "error should include action kind: {message}"
    );
    assert!(
        message.contains(&action.path),
        "error should include action path: {message}"
    );

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn uninstall_is_idempotent_when_not_installed() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let result = uninstall_package(&layout, "missing").expect("must be ok");
    assert_eq!(result.status, UninstallStatus::NotInstalled);
    assert_eq!(result.version, None);

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn uninstall_cleans_stale_receipt_when_package_is_missing() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    write_install_receipt(
        &layout,
        &InstallReceipt {
            name: "demo".to_string(),
            version: "1.0.0".to_string(),
            dependencies: Vec::new(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path: None,
            exposed_bins: Vec::new(),
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason: InstallReason::Root,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        },
    )
    .expect("must write receipt");

    let result = uninstall_package(&layout, "demo").expect("must uninstall stale state");
    assert_eq!(result.status, UninstallStatus::RepairedStaleState);
    assert!(!layout.receipt_path("demo").exists());

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn uninstall_parse_failure_preserves_files() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let package_dir = layout.package_dir("demo", "1.0.0");
    fs::create_dir_all(&package_dir).expect("must create package dir");
    let receipt_path = layout.receipt_path("demo");
    fs::write(&receipt_path, b"name=demo\nversion=1.0.0\n").expect("must write malformed");

    let err = uninstall_package(&layout, "demo").expect_err("must fail on malformed receipt");
    assert!(err.to_string().contains("failed to parse install receipt"));
    assert!(receipt_path.exists());
    assert!(package_dir.exists());

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn uninstall_blocks_when_required_by_remaining_root() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    write_receipt(
        &layout,
        "app",
        "1.0.0",
        &["shared@1.0.0"],
        InstallReason::Root,
        None,
    );
    write_receipt(
        &layout,
        "shared",
        "1.0.0",
        &[],
        InstallReason::Dependency,
        None,
    );

    let result = uninstall_package(&layout, "shared").expect("must evaluate dependencies");
    assert_eq!(result.status, UninstallStatus::BlockedByDependents);
    assert_eq!(result.blocked_by_roots, vec!["app"]);
    assert!(layout.receipt_path("shared").exists());

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn uninstall_with_dependency_overrides_allows_planned_root_transition() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    write_receipt(
        &layout,
        "app",
        "1.0.0",
        &["shared@1.0.0"],
        InstallReason::Root,
        None,
    );
    write_receipt(
        &layout,
        "shared",
        "1.0.0",
        &[],
        InstallReason::Dependency,
        None,
    );

    let dependency_overrides =
        HashMap::from([("app".to_string(), vec!["replacement".to_string()])]);
    let result =
        uninstall_package_with_dependency_overrides(&layout, "shared", &dependency_overrides)
            .expect("planned dependency override should allow uninstall");

    assert_eq!(result.status, UninstallStatus::Uninstalled);
    assert!(!layout.receipt_path("shared").exists());
    assert!(layout.receipt_path("app").exists());

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn uninstall_with_dependency_overrides_keeps_transitive_edges_for_planned_packages() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    write_receipt(
        &layout,
        "app",
        "1.0.0",
        &["legacy@1.0.0"],
        InstallReason::Root,
        None,
    );
    write_receipt(
        &layout,
        "legacy",
        "1.0.0",
        &["lib@1.0.0"],
        InstallReason::Dependency,
        None,
    );
    write_receipt(
        &layout,
        "lib",
        "1.0.0",
        &[],
        InstallReason::Dependency,
        None,
    );

    let dependency_overrides = HashMap::from([
        ("app".to_string(), vec!["new".to_string()]),
        ("new".to_string(), vec!["lib".to_string()]),
    ]);
    let result =
        uninstall_package_with_dependency_overrides(&layout, "legacy", &dependency_overrides)
            .expect("planned transitive overrides should preserve shared dependencies");

    assert_eq!(result.status, UninstallStatus::Uninstalled);
    assert!(
        result.pruned_dependencies.is_empty(),
        "shared lib must not be pruned when planned graph still requires it"
    );
    assert!(!layout.receipt_path("legacy").exists());
    assert!(layout.receipt_path("app").exists());
    assert!(layout.receipt_path("lib").exists());

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn uninstall_prunes_orphans_when_root_removed() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    write_receipt(
        &layout,
        "app",
        "1.0.0",
        &["shared@1.0.0"],
        InstallReason::Root,
        None,
    );
    write_receipt(
        &layout,
        "shared",
        "1.0.0",
        &[],
        InstallReason::Dependency,
        None,
    );

    let result = uninstall_package(&layout, "app").expect("must uninstall root and orphan");
    assert_eq!(result.status, UninstallStatus::Uninstalled);
    assert_eq!(result.pruned_dependencies, vec!["shared"]);
    assert!(!layout.receipt_path("app").exists());
    assert!(!layout.receipt_path("shared").exists());

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn uninstall_keeps_shared_dependency_for_other_root() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    write_receipt(
        &layout,
        "app-a",
        "1.0.0",
        &["shared@1.0.0"],
        InstallReason::Root,
        None,
    );
    write_receipt(
        &layout,
        "app-b",
        "1.0.0",
        &["shared@1.0.0"],
        InstallReason::Root,
        None,
    );
    write_receipt(
        &layout,
        "shared",
        "1.0.0",
        &[],
        InstallReason::Dependency,
        None,
    );

    let result = uninstall_package(&layout, "app-a").expect("must uninstall app-a only");
    assert_eq!(result.status, UninstallStatus::Uninstalled);
    assert!(result.pruned_dependencies.is_empty());
    assert!(!layout.receipt_path("app-a").exists());
    assert!(layout.receipt_path("app-b").exists());
    assert!(layout.receipt_path("shared").exists());

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn uninstall_prunes_unreferenced_cache_paths() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let cache_path = layout
        .cache_dir()
        .join("artifacts")
        .join("shared")
        .join("1.0.0")
        .join("x86_64-unknown-linux-gnu")
        .join("artifact.tar.zst");
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent).expect("must create cache dir");
    }
    fs::write(&cache_path, b"artifact").expect("must create cache file");

    write_receipt(
        &layout,
        "app",
        "1.0.0",
        &["shared@1.0.0"],
        InstallReason::Root,
        None,
    );
    write_receipt(
        &layout,
        "shared",
        "1.0.0",
        &[],
        InstallReason::Dependency,
        Some(cache_path.to_string_lossy().to_string()),
    );

    let result = uninstall_package(&layout, "app").expect("must uninstall root and orphan");
    assert_eq!(result.pruned_dependencies, vec!["shared"]);
    assert!(!cache_path.exists());

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn uninstall_keeps_cache_when_still_referenced() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let cache_path = layout
        .cache_dir()
        .join("artifacts")
        .join("shared")
        .join("1.0.0")
        .join("x86_64-unknown-linux-gnu")
        .join("artifact.tar.zst");
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent).expect("must create cache dir");
    }
    fs::write(&cache_path, b"artifact").expect("must create cache file");

    write_receipt(
        &layout,
        "app-a",
        "1.0.0",
        &["shared@1.0.0"],
        InstallReason::Root,
        None,
    );
    write_receipt(
        &layout,
        "app-b",
        "1.0.0",
        &["shared@1.0.0"],
        InstallReason::Root,
        None,
    );
    write_receipt(
        &layout,
        "shared",
        "1.0.0",
        &[],
        InstallReason::Dependency,
        Some(cache_path.to_string_lossy().to_string()),
    );

    uninstall_package(&layout, "app-a").expect("must uninstall only app-a");
    assert!(cache_path.exists());

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn uninstall_skips_pruning_cache_path_outside_artifacts_dir() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let outside_cache_path = layout.prefix().join("outside-cache-file");
    fs::write(&outside_cache_path, b"artifact").expect("must create outside cache file");

    write_receipt(
        &layout,
        "app",
        "1.0.0",
        &["shared@1.0.0"],
        InstallReason::Root,
        None,
    );
    write_receipt(
        &layout,
        "shared",
        "1.0.0",
        &[],
        InstallReason::Dependency,
        Some(outside_cache_path.to_string_lossy().to_string()),
    );

    let result = uninstall_package(&layout, "app").expect("must ignore unsafe cache prune");
    assert_eq!(result.pruned_dependencies, vec!["shared"]);
    assert!(outside_cache_path.exists());
    assert!(!layout.receipt_path("app").exists());
    assert!(!layout.receipt_path("shared").exists());

    let _ = fs::remove_dir_all(layout.prefix());
}

fn write_receipt(
    layout: &PrefixLayout,
    name: &str,
    version: &str,
    dependencies: &[&str],
    install_reason: InstallReason,
    cache_path: Option<String>,
) {
    let package_dir = layout.package_dir(name, version);
    fs::create_dir_all(&package_dir).expect("must create package dir");
    write_install_receipt(
        layout,
        &InstallReceipt {
            name: name.to_string(),
            version: version.to_string(),
            dependencies: dependencies.iter().map(|v| (*v).to_string()).collect(),
            target: None,
            artifact_url: None,
            artifact_sha256: None,
            cache_path,
            exposed_bins: Vec::new(),
            exposed_completions: Vec::new(),
            snapshot_id: None,
            install_mode: InstallMode::Managed,
            install_reason,
            install_status: "installed".to_string(),
            installed_at_unix: 1,
        },
    )
    .expect("must write receipt");
}

static TEST_LAYOUT_COUNTER: AtomicU64 = AtomicU64::new(0);

fn build_test_layout_path(nanos: u128) -> PathBuf {
    let mut path = std::env::temp_dir();
    let sequence = TEST_LAYOUT_COUNTER.fetch_add(1, Ordering::Relaxed);
    path.push(format!(
        "crosspack-installer-tests-{}-{}-{}",
        std::process::id(),
        nanos,
        sequence
    ));
    path
}

#[test]
fn build_test_layout_path_disambiguates_same_timestamp_calls() {
    let first = build_test_layout_path(42);
    let second = build_test_layout_path(42);
    assert_ne!(
        first, second,
        "installer test layout paths must remain unique when timestamp granularity is coarse"
    );
}

fn test_layout() -> PrefixLayout {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    PrefixLayout::new(build_test_layout_path(nanos))
}

#[test]
fn pin_round_trip() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    write_pin(&layout, "ripgrep", "^14").expect("must write pin");
    let pin = read_pin(&layout, "ripgrep").expect("must read pin");
    assert_eq!(pin.as_deref(), Some("^14"));

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn pin_overwrite_replaces_old_value() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    write_pin(&layout, "ripgrep", "^13").expect("must write pin");
    write_pin(&layout, "ripgrep", "^14").expect("must overwrite pin");
    let pin = read_pin(&layout, "ripgrep").expect("must read pin");
    assert_eq!(pin.as_deref(), Some("^14"));

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn remove_pin_returns_false_when_missing() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    let removed = remove_pin(&layout, "missing").expect("must handle missing");
    assert!(!removed);

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn remove_pin_returns_true_when_existing() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    write_pin(&layout, "ripgrep", "^14").expect("must write pin");
    let removed = remove_pin(&layout, "ripgrep").expect("must remove existing");
    assert!(removed);
    let pin = read_pin(&layout, "ripgrep").expect("must read pin");
    assert!(pin.is_none());

    let _ = fs::remove_dir_all(layout.prefix());
}

#[test]
fn read_all_pins_reads_multiple_pin_files() {
    let layout = test_layout();
    layout.ensure_base_dirs().expect("must create dirs");

    write_pin(&layout, "ripgrep", "^14").expect("pin ripgrep");
    write_pin(&layout, "fd", "^10").expect("pin fd");

    let pins = read_all_pins(&layout).expect("must read pins");
    assert_eq!(pins.get("ripgrep").map(String::as_str), Some("^14"));
    assert_eq!(pins.get("fd").map(String::as_str), Some("^10"));

    let _ = fs::remove_dir_all(layout.prefix());
}
